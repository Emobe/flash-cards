//! Optimising the FSRS parameters from review history (ADR 0004, ADR 0007 finding 2).
//!
//! Every answer goes in, learning-step answers too, with the days since the card's previous answer
//! (0 for the same day). That is what the scheduler fed into `next_states` when it answered, so the
//! optimiser trains on what the scheduler used.
//!
//! Reading the history and training are separate calls: training takes a while and cannot be
//! cancelled, and a host should not hold the collection while it runs.

use std::collections::HashMap;

use super::StudyError;
use crate::collection::Collection;
use crate::deck::Tree;
use crate::id::Id;
use crate::scheduling::{HistoryReview, Rating, optimise_with_steps, training_item_count};

/// The fewest training items we will train on (see `TrainingSet::items`). `fsrs` returns
/// the default parameters without a word under 8 items and only fits the first-interval parameters
/// under 64, so we refuse instead of handing back numbers that mean nothing. Even 64 is a small set:
/// the result then stays close to the defaults.
pub const MIN_TRAINING_ITEMS: u32 = 64;

/// The review history of a preset's cards, ready to train on.
#[derive(Debug, Clone)]
pub struct TrainingSet {
    pub preset: Id,
    /// Cards with at least one review.
    pub cards: u32,
    /// Reviews in all.
    pub reviews: u32,
    /// What there is to learn from: each review after a card's first that comes on a later day than
    /// the one before it, or after such a review. A card answered only within one day (its learning
    /// steps) gives none, so this is often well under `reviews - cards`.
    pub items: u32,
    relearning_steps: u32,
    pub(crate) histories: Vec<Vec<HistoryReview>>,
}

/// The outcome of training.
#[derive(Debug, Clone, PartialEq)]
pub struct Optimised {
    /// The 21 parameters.
    pub parameters: Vec<f32>,
    pub cards: u32,
    pub reviews: u32,
}

impl TrainingSet {
    /// Trains. Slow for a large history, and cannot be cancelled.
    pub fn optimise(&self) -> Result<Optimised, StudyError> {
        if self.items < MIN_TRAINING_ITEMS {
            return Err(StudyError::NotEnoughHistory {
                items: self.items,
                needed: MIN_TRAINING_ITEMS,
            });
        }
        Ok(Optimised {
            parameters: optimise_with_steps(&self.histories, self.relearning_steps)?,
            cards: self.cards,
            reviews: self.reviews,
        })
    }
}

impl Collection {
    /// The history of every card that exists now in a deck that uses this preset, undone reviews
    /// left out, from the card's last reset on. A card moved to a deck with another preset takes its
    /// history with it.
    pub fn optimisation_data(&self, preset: Id) -> Result<TrainingSet, StudyError> {
        let tree = Tree::load(&self.conn)?;
        let options = self.preset(preset)?.ok_or(StudyError::NotFound)?;
        // Decks that read as using this preset (a deck whose preset is gone uses the Default one).
        let uses: HashMap<Id, bool> = tree
            .rows
            .iter()
            .filter(|r| !r.deleted)
            .map(|r| (r.id, r.preset == preset))
            .collect();
        // A reset starts the card again (ADR 0014, decision 5): only the answers after its last
        // reset are trained on.
        let mut statement = self.conn.prepare(
            "SELECT e.card, k.deck, e.day, e.rating, e.kind = 'reset'
             FROM card_event e
             JOIN card k ON k.id = e.card
             JOIN note n ON n.id = k.note
             WHERE (e.kind = 'reset' OR (e.kind = 'review' AND e.rating IS NOT NULL))
               AND k.deleted = 0 AND n.deleted = 0
               AND NOT EXISTS (SELECT 1 FROM card_event v WHERE v.target = e.id)
             ORDER BY e.card, e.time_ms, e.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?;
        let mut histories: Vec<Vec<HistoryReview>> = Vec::new();
        let mut current: Option<Id> = None;
        // The day of the card's first answer in its history (after its last reset).
        let mut first: Option<i64> = None;
        let mut skipping = false;
        for row in rows {
            let (card, stored, day, rating, reset) = row?;
            if current != Some(card) {
                let deck = crate::deck::card_deck(&stored);
                let deck = if tree.position(deck).is_some() {
                    deck
                } else {
                    crate::deck::default_deck()
                };
                skipping = !uses.get(&deck).copied().unwrap_or(false);
                current = Some(card);
                first = None;
                if !skipping {
                    histories.push(Vec::new());
                }
            }
            if skipping {
                continue;
            }
            let history = histories.last_mut().expect("a history was started");
            if reset {
                history.clear();
                first = None;
                continue;
            }
            let Some(rating) = rating.and_then(Rating::from_number) else {
                continue;
            };
            let first = *first.get_or_insert(day);
            history.push(HistoryReview {
                day: (day - first).clamp(0, i64::from(u32::MAX)) as u32,
                rating,
            });
        }
        histories.retain(|h| !h.is_empty());
        let reviews = histories.iter().map(|h| h.len() as u32).sum();
        let items = training_item_count(&histories);
        Ok(TrainingSet {
            preset,
            cards: histories.len() as u32,
            reviews,
            items,
            relearning_steps: options.relearning_steps.len() as u32,
            histories,
        })
    }

    /// Reads the history and trains in one call. Changes nothing: store the result with
    /// `set_preset_options` and `PresetChange::fsrs_parameters`. Hosts that must not hold the
    /// collection while training call `optimisation_data` and then `TrainingSet::optimise`.
    pub fn optimise_preset(&self, preset: Id) -> Result<Optimised, StudyError> {
        self.optimisation_data(preset)?.optimise()
    }
}
