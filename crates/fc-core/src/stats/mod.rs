//! Review history and basic statistics (step 1.8, ADR 0007 build notes).
//!
//! Everything here reads the card events (ADR 0006, section 3) and the schedule cache, and writes
//! nothing. A review that was undone (a void names it) is left out of every number. Days are study
//! days, as the event recorded them when it was written (ADR 0007, part 2), so a figure for a day
//! never changes when the device moves to another time zone or the start hour is changed.

mod counts;
mod forecast;
mod history;
mod optimise;
#[cfg(test)]
mod tests;

use std::collections::HashSet;

use crate::collection::Collection;
use crate::deck::{Tree, card_deck, default_deck};
use crate::id::Id;

use crate::study::StudyError;

pub use counts::{DailyCount, DayRange, Retention};
pub use forecast::{DueForecast, ForecastDay};
pub use history::ReviewEntry;
pub use optimise::{MIN_TRAINING_ITEMS, Optimised, TrainingSet};

/// The decks a statistic is limited to: the chosen deck and everything inside it, or `None` for
/// everything. A card that is not in any live deck of the tree reads as in the Default deck, as in
/// the queues.
pub(crate) struct DeckFilter {
    tree: Tree,
    decks: HashSet<Id>,
}

impl DeckFilter {
    pub(crate) fn load(c: &Collection, deck: Id) -> Result<Self, StudyError> {
        let tree = Tree::load(&c.conn)?;
        tree.live(deck).ok_or(StudyError::NotFound)?;
        let decks = tree
            .subtree(deck)
            .iter()
            .filter(|r| !r.deleted)
            .map(|r| r.id)
            .collect();
        Ok(Self { tree, decks })
    }

    /// Whether a card whose `deck` register is `stored` is in the chosen decks.
    pub(crate) fn contains(&self, stored: &[u8]) -> bool {
        let id = card_deck(stored);
        let id = if self.tree.position(id).is_some() {
            id
        } else {
            default_deck()
        };
        self.decks.contains(&id)
    }
}

impl Collection {
    /// Today's study day, for ranges and the forecast ("the last 30 days" is `today - 29..=today`).
    pub fn study_today(&self) -> Result<i64, StudyError> {
        self.today()
    }
}
