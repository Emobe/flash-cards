//! The study queue (ADR 0007, parts 3, 4 and 5). Computed from the database on every call: the
//! schedule cache says what is due, today's events say what has been done, the registers say what is
//! hidden, and the decks and presets say how much is allowed. Nothing is stored, so a merge, a
//! restart of the web worker or a change of day needs nothing rebuilt.
//!
//! The rules, in the order the code applies them:
//!
//! - **Candidates** are live cards that are not suspended and not buried today. A new or review
//!   card is also left out when another card of its note was answered today and its preset spaces
//!   siblings. Learning cards are never held back.
//! - **Limits.** A card can be shown only while every deck on its path has room. A deck's limits
//!   count its own cards and, unless its `limits_include_subdecks` is off, everything inside it.
//!   "Done today" comes from today's events that were not voided (a new card counts when its first
//!   answer is today, a review when the card was in the review state), by the card's current deck.
//!   Learning and relearning cards are not limited. Limits are applied in card order, so the cards
//!   that would be shown first are the ones that are kept.
//! - **Order.** Learning cards that are due now, earliest first. Then reviews (by due day, then card
//!   ID) with the new cards spread evenly among them (new cards by note creation, template position
//!   and cloze number). When nothing else is left, a learning card due within 20 minutes.
//!   Otherwise the queue says when the next learning card is due, if that is today.
//! - **Counts.** `learning` is every learning or relearning card due before the end of today, so a
//!   deck list shows what is left today. `new` and `review` are the cards the queue would show.

use std::collections::HashMap;

use rusqlite::params;

use super::StudyError;
use super::answer::effective_parameters;
use super::schedule::{card_now, read_cached};
use crate::collection::Collection;
use crate::deck::{Preset, Tree, card_deck, default_deck, presets_by_id};
use crate::id::Id;
use crate::scheduling::{CardState, Due, Scheduler, Steps, elapsed_days, preview, study_day};

/// A learning card due this soon is shown early when nothing else is left (ADR 0007, part 4).
pub const LEARN_AHEAD_MS: i64 = 20 * 60_000;

/// What is left to study in a deck and the decks inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub new: u32,
    /// Learning and relearning cards due before the end of today.
    pub learning: u32,
    pub review: u32,
}

/// The counts of one deck, for the deck list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckCounts {
    pub deck: Id,
    pub counts: Counts,
}

/// What to study next.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Show this card. `previews` is when it comes back after Again, Hard, Good and Easy, before
    /// fuzz. `counts` includes this card.
    Card {
        card: Id,
        /// The deck the card is in.
        deck: Id,
        state: CardState,
        previews: [Due; 4],
        counts: Counts,
    },
    /// Nothing is due now. A learning card is due later today, at this time (Unix milliseconds).
    Waiting { until_ms: i64, counts: Counts },
    /// Nothing more to study today.
    Done { counts: Counts },
}

struct Learning {
    card: Id,
    deck: usize,
    due_ms: i64,
}

struct Review {
    card: Id,
    deck: usize,
    due_day: i64,
}

struct Fresh {
    card: Id,
    deck: usize,
    /// Notes are UUIDv7, so the note ID is creation order.
    note: Id,
    /// The template's fractional-index position.
    position: String,
    ordinal: i64,
}

/// What a card was when it was answered today.
struct Done {
    card: Id,
    note: Id,
    deck: usize,
    state_before: i64,
}

/// Everything the queue is made from, for the whole collection.
struct Queues {
    tree: Tree,
    presets: HashMap<Id, Preset>,
    today: i64,
    now_ms: i64,
    day_end_ms: i64,
    parent: Vec<Option<usize>>,
    /// The deck at `i` and everything inside it is `i..end[i]` (the tree is in tree order).
    end: Vec<usize>,
    include: Vec<bool>,
    new_limit: Vec<u32>,
    review_limit: Vec<u32>,
    /// Cards already done today that count against each deck's limit.
    base_new: Vec<u32>,
    base_review: Vec<u32>,
    /// Cards done today in each deck itself, for spreading new cards among reviews.
    done_new_here: Vec<u32>,
    done_review_here: Vec<u32>,
    learning: Vec<Vec<Learning>>,
    reviews: Vec<Vec<Review>>,
    fresh: Vec<Vec<Fresh>>,
}

/// What the queue holds for one deck and the decks inside it.
struct Scope<'a> {
    learning: Vec<&'a Learning>,
    reviews: Vec<&'a Review>,
    fresh: Vec<&'a Fresh>,
    done_new: u32,
    done_review: u32,
}

impl Scope<'_> {
    fn counts(&self) -> Counts {
        Counts {
            new: self.fresh.len() as u32,
            learning: self.learning.len() as u32,
            review: self.reviews.len() as u32,
        }
    }
}

impl Queues {
    fn load(c: &Collection) -> Result<Self, StudyError> {
        let now = c.host.clock.now();
        let start_hour = c.day_start_hour()?;
        let today = study_day(now.unix_ms, now.utc_offset_minutes, start_hour);
        let day_end_ms = (today + 1) * 86_400_000 + i64::from(start_hour) * 3_600_000
            - i64::from(now.utc_offset_minutes) * 60_000;
        let tree = Tree::load(&c.conn)?;
        let presets = presets_by_id(&c.conn, &tree)?;
        let count = tree.rows.len();
        let parent: Vec<Option<usize>> = tree
            .rows
            .iter()
            .map(|row| row.parent.and_then(|p| tree.position(p)))
            .collect();
        let end: Vec<usize> = tree
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| i + tree.subtree(row.id).len())
            .collect();
        let preset_of = |i: usize| presets.get(&tree.rows[i].preset);
        let new_limit = (0..count)
            .map(|i| preset_of(i).map_or(20, |p| p.new_per_day))
            .collect();
        let review_limit = (0..count)
            .map(|i| preset_of(i).map_or(200, |p| p.reviews_per_day))
            .collect();
        let space: Vec<bool> = (0..count)
            .map(|i| preset_of(i).is_none_or(|p| p.space_siblings))
            .collect();
        let include = tree
            .rows
            .iter()
            .map(|r| r.limits_include_subdecks)
            .collect();
        let mut queues = Self {
            presets,
            today,
            now_ms: now.unix_ms,
            day_end_ms,
            parent,
            end,
            include,
            new_limit,
            review_limit,
            base_new: vec![0; count],
            base_review: vec![0; count],
            done_new_here: vec![0; count],
            done_review_here: vec![0; count],
            learning: (0..count).map(|_| Vec::new()).collect(),
            reviews: (0..count).map(|_| Vec::new()).collect(),
            fresh: (0..count).map(|_| Vec::new()).collect(),
            tree,
        };

        let done = queues.load_done(c)?;
        for entry in &done {
            match entry.state_before {
                0 => {
                    queues.done_new_here[entry.deck] += 1;
                    for a in queues.applying(entry.deck).collect::<Vec<_>>() {
                        queues.base_new[a] += 1;
                    }
                }
                2 => {
                    queues.done_review_here[entry.deck] += 1;
                    for a in queues.applying(entry.deck).collect::<Vec<_>>() {
                        queues.base_review[a] += 1;
                    }
                }
                _ => {}
            }
        }
        // A sibling answered today holds back a new or review card, unless the preset says not to.
        let mut answered: HashMap<Id, (Id, bool)> = HashMap::new();
        for entry in &done {
            answered
                .entry(entry.note)
                .and_modify(|(first, several)| *several |= *first != entry.card)
                .or_insert((entry.card, false));
        }
        let held = |deck: usize, note: Id, card: Id| {
            space[deck]
                && answered
                    .get(&note)
                    .is_some_and(|(first, several)| *several || *first != card)
        };
        queues.load_cards(c, held)?;
        Ok(queues)
    }

    fn deck_index(&self, stored: &[u8]) -> usize {
        self.tree
            .position(card_deck(stored))
            .or_else(|| self.tree.position(default_deck()))
            .unwrap_or(0)
    }

    /// The decks whose limits count a card in `deck`: the deck itself, and each deck above it that
    /// includes its sub-decks.
    fn applying(&self, deck: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(deck), |&i| self.parent[i])
            .filter(move |&i| i == deck || self.include[i])
    }

    /// Today's answers that were not voided, for cards that still exist.
    fn load_done(&self, c: &Collection) -> Result<Vec<Done>, StudyError> {
        let mut statement = c.conn.prepare(
            "SELECT e.card, k.note, k.deck, e.state_before
             FROM card_event e
             JOIN card k ON k.id = e.card
             JOIN note n ON n.id = k.note
             WHERE e.day = ?1 AND e.kind = 'review' AND k.deleted = 0 AND n.deleted = 0
               AND NOT EXISTS (SELECT 1 FROM card_event v WHERE v.target = e.id)",
        )?;
        let rows = statement.query_map([self.today], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Id>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        let mut done = Vec::new();
        for row in rows {
            let (card, note, deck, state_before) = row?;
            done.push(Done {
                card,
                note,
                deck: self.deck_index(&deck),
                state_before: state_before.unwrap_or(-1),
            });
        }
        Ok(done)
    }

    fn load_cards(
        &mut self,
        c: &Collection,
        held: impl Fn(usize, Id, Id) -> bool,
    ) -> Result<(), StudyError> {
        let (today, day_end) = (self.today, self.day_end_ms);
        // Cards that have been answered: learning cards due before the end of today, and reviews
        // due today or earlier.
        let mut statement = c.conn.prepare(
            "SELECT k.id, k.note, k.deck, s.state, COALESCE(s.due_day, ?1), COALESCE(s.due_ms, 0)
             FROM card_schedule s
             JOIN card k ON k.id = s.card
             JOIN note n ON n.id = k.note
             WHERE k.deleted = 0 AND n.deleted = 0 AND k.suspended = 0 AND k.buried_until < ?1
               AND ((s.state = 2 AND COALESCE(s.due_day, ?1) <= ?1)
                 OR (s.state IN (1, 3) AND COALESCE(s.due_ms, 0) < ?2))",
        )?;
        let rows = statement.query_map(params![today, day_end], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Id>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            let (card, note, deck, state, due_day, due_ms) = row?;
            let deck = self.deck_index(&deck);
            if state == 2 {
                if !held(deck, note, card) {
                    self.reviews[deck].push(Review {
                        card,
                        deck,
                        due_day,
                    });
                }
            } else {
                self.learning[deck].push(Learning { card, deck, due_ms });
            }
        }
        // Cards never answered.
        let mut statement = c.conn.prepare(
            "SELECT k.id, k.note, k.deck, k.ordinal, COALESCE(t.position, '')
             FROM card k
             JOIN note n ON n.id = k.note
             LEFT JOIN card_schedule s ON s.card = k.id
             LEFT JOIN template t ON t.id = k.template
             WHERE k.deleted = 0 AND n.deleted = 0 AND k.suspended = 0 AND k.buried_until < ?1
               AND (s.card IS NULL OR s.state = 0)",
        )?;
        let rows = statement.query_map([today], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Id>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (card, note, deck, ordinal, position) = row?;
            let deck = self.deck_index(&deck);
            if !held(deck, note, card) {
                self.fresh[deck].push(Fresh {
                    card,
                    deck,
                    note,
                    position,
                    ordinal,
                });
            }
        }
        for list in &mut self.fresh {
            list.sort_by(|a, b| fresh_order(a, b));
        }
        for list in &mut self.reviews {
            list.sort_by_key(|r| (r.due_day, r.card));
        }
        for list in &mut self.learning {
            list.sort_by_key(|l| (l.due_ms, l.card));
        }
        Ok(())
    }

    /// Takes one more card in `deck` against `used` if every deck on its path has room.
    fn take(&self, deck: usize, used: &mut [u32], limits: &[u32]) -> bool {
        if self.applying(deck).any(|a| used[a] >= limits[a]) {
            return false;
        }
        for a in self.applying(deck).collect::<Vec<_>>() {
            used[a] += 1;
        }
        true
    }

    /// What can be studied in the deck at `top` and the decks inside it, limits applied.
    fn scope(&self, top: usize) -> Scope<'_> {
        let decks = top..self.end[top];
        let mut learning: Vec<&Learning> = decks
            .clone()
            .flat_map(|d| self.learning[d].iter())
            .collect();
        learning.sort_by_key(|l| (l.due_ms, l.card));
        let mut reviews: Vec<&Review> =
            decks.clone().flat_map(|d| self.reviews[d].iter()).collect();
        reviews.sort_by_key(|r| (r.due_day, r.card));
        let mut fresh: Vec<&Fresh> = decks.clone().flat_map(|d| self.fresh[d].iter()).collect();
        fresh.sort_by(|a, b| fresh_order(a, b));

        let mut used = self.base_review.clone();
        reviews.retain(|r| self.take(r.deck, &mut used, &self.review_limit));
        let mut used = self.base_new.clone();
        fresh.retain(|f| self.take(f.deck, &mut used, &self.new_limit));
        Scope {
            learning,
            reviews,
            fresh,
            done_new: decks.clone().map(|d| self.done_new_here[d]).sum(),
            done_review: decks.map(|d| self.done_review_here[d]).sum(),
        }
    }
}

fn fresh_order(a: &Fresh, b: &Fresh) -> std::cmp::Ordering {
    (a.note, &a.position, a.ordinal, a.card).cmp(&(b.note, &b.position, b.ordinal, b.card))
}

/// Which of the next review and the next new card comes first when new cards are spread evenly
/// among reviews. The `i`-th review of `reviews` in the day sits at `(i + 1/2) / reviews`, the
/// `j`-th new card at `(j + 1/2) / new`, and the earlier position goes first (a review on a tie).
/// Totals include what was done today, so the choice stays the same as cards are answered.
fn review_first(scope: &Scope<'_>) -> bool {
    if scope.reviews.is_empty() {
        return false;
    }
    if scope.fresh.is_empty() {
        return true;
    }
    let reviews = u64::from(scope.done_review) + scope.reviews.len() as u64;
    let new = u64::from(scope.done_new) + scope.fresh.len() as u64;
    (2 * u64::from(scope.done_review) + 1) * new <= (2 * u64::from(scope.done_new) + 1) * reviews
}

impl Collection {
    /// What is left to study in a live deck and the decks inside it.
    pub fn study_counts(&self, deck: Id) -> Result<Counts, StudyError> {
        let queues = Queues::load(self)?;
        let top = live_position(&queues, deck)?;
        Ok(queues.scope(top).counts())
    }

    /// The counts of every live deck, in deck tree order.
    pub fn deck_counts(&self) -> Result<Vec<DeckCounts>, StudyError> {
        let queues = Queues::load(self)?;
        Ok(queues
            .tree
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !row.deleted)
            .map(|(i, row)| DeckCounts {
                deck: row.id,
                counts: queues.scope(i).counts(),
            })
            .collect())
    }

    /// The card to study next in a live deck and the decks inside it, with what the four answers
    /// would do to it.
    pub fn next_card(&self, deck: Id) -> Result<Next, StudyError> {
        let queues = Queues::load(self)?;
        let top = live_position(&queues, deck)?;
        let scope = queues.scope(top);
        let counts = scope.counts();
        let due_now = scope.learning.first().filter(|l| l.due_ms <= queues.now_ms);
        let picked = if let Some(card) = due_now {
            Some((card.card, card.deck))
        } else if review_first(&scope) {
            scope.reviews.first().map(|r| (r.card, r.deck))
        } else if let Some(card) = scope.fresh.first() {
            Some((card.card, card.deck))
        } else {
            scope
                .learning
                .first()
                .filter(|l| l.due_ms <= queues.now_ms + LEARN_AHEAD_MS)
                .map(|l| (l.card, l.deck))
        };
        let Some((card, at)) = picked else {
            return Ok(match scope.learning.first() {
                Some(l) => Next::Waiting {
                    until_ms: l.due_ms,
                    counts,
                },
                None => Next::Done { counts },
            });
        };
        let row = &queues.tree.rows[at];
        let preset = queues.presets.get(&row.preset);
        let cached = read_cached(&self.conn, card)?;
        let now = card_now(cached.as_ref());
        let elapsed = cached
            .as_ref()
            .map_or(0, |c| elapsed_days(queues.today, c.last_day));
        let previews = match preset {
            Some(preset) => {
                let parameters = effective_parameters(preset);
                let scheduler = Scheduler::new(Some(&parameters), preset.desired_retention as f32)?;
                let steps = Steps {
                    learning: &preset.learning_steps,
                    relearning: &preset.relearning_steps,
                };
                preview(now, elapsed, steps, &scheduler)?.map(|outcome| outcome.due)
            }
            None => [Due::Days(1); 4],
        };
        Ok(Next::Card {
            card,
            deck: row.id,
            state: now.state,
            previews,
            counts,
        })
    }
}

fn live_position(queues: &Queues, deck: Id) -> Result<usize, StudyError> {
    queues
        .tree
        .live(deck)
        .and_then(|row| queues.tree.position(row.id))
        .ok_or(StudyError::NotFound)
}
