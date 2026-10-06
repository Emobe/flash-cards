//! Reviews per day and retention.

use rusqlite::params;

use super::{DeckFilter, StudyError};
use crate::collection::Collection;
use crate::id::Id;

/// The longest range (in days) that is filled in. A longer one starts later, so a mistaken range
/// cannot ask for a million rows of zeros.
const MAX_RANGE_DAYS: i64 = 36_525;

/// A range of study days, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayRange {
    pub from: i64,
    pub to: i64,
}

impl DayRange {
    /// The `days` study days that end on `to`.
    pub fn last(to: i64, days: u32) -> Self {
        Self {
            from: to.saturating_sub(i64::from(days.max(1)) - 1),
            to,
        }
    }
}

/// The answers of one study day. Voided answers are not counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DailyCount {
    pub day: i64,
    /// Answers by what the card was before the answer.
    pub new: u32,
    pub learning: u32,
    pub relearning: u32,
    pub review: u32,
    /// Answers of Again, whatever the state.
    pub again: u32,
    /// Answers on review cards that were not Again (what retention counts).
    pub review_passed: u32,
    /// Time spent answering, in milliseconds.
    pub time_ms: u64,
}

impl DailyCount {
    pub fn answers(&self) -> u32 {
        self.new + self.learning + self.relearning + self.review
    }
}

/// How often review cards were remembered: answers on review cards, and those that were not Again.
/// Counts, not a percentage, so ranges can be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Retention {
    pub reviews: u32,
    pub passed: u32,
}

impl Retention {
    /// The share remembered from 0 to 1, or `None` when there were no reviews.
    pub fn rate(&self) -> Option<f64> {
        (self.reviews > 0).then(|| f64::from(self.passed) / f64::from(self.reviews))
    }
}

fn clamp(range: DayRange) -> Option<DayRange> {
    (range.from <= range.to).then(|| DayRange {
        from: range.from.max(range.to.saturating_sub(MAX_RANGE_DAYS - 1)),
        to: range.to,
    })
}

impl Collection {
    /// One entry per study day of the range (days with no answers are zeros), oldest first.
    /// Without a deck every answer counts, including those of cards that were deleted since (history
    /// is never lost). With a deck, only the cards that exist now in it or inside it.
    pub fn daily_counts(
        &self,
        range: DayRange,
        deck: Option<Id>,
    ) -> Result<Vec<DailyCount>, StudyError> {
        let filter = deck.map(|d| DeckFilter::load(self, d)).transpose()?;
        let Some(range) = clamp(range) else {
            return Ok(Vec::new());
        };
        let mut days: Vec<DailyCount> = (range.from..=range.to)
            .map(|day| DailyCount {
                day,
                ..Default::default()
            })
            .collect();
        let mut statement = self.conn.prepare(
            "SELECT e.day, COALESCE(e.state_before, -1), k.deck, k.deleted, n.deleted,
                    COUNT(*), COALESCE(SUM(e.rating = 1), 0), COALESCE(SUM(e.duration_ms), 0)
             FROM card_event e
             LEFT JOIN card k ON k.id = e.card
             LEFT JOIN note n ON n.id = k.note
             WHERE e.kind = 'review' AND e.day BETWEEN ?1 AND ?2
               AND NOT EXISTS (SELECT 1 FROM card_event v WHERE v.target = e.id)
             GROUP BY e.day, COALESCE(e.state_before, -1), k.deck, k.deleted, n.deleted",
        )?;
        let rows = statement.query_map(params![range.from, range.to], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<Vec<u8>>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, u32>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        for row in rows {
            let (day, state, deck, k_deleted, n_deleted, count, again, time) = row?;
            if let Some(filter) = &filter {
                let live = k_deleted == Some(0) && n_deleted == Some(0);
                if !live || !deck.as_deref().is_some_and(|d| filter.contains(d)) {
                    continue;
                }
            }
            let entry = &mut days[(day - range.from) as usize];
            match state {
                0 => entry.new += count,
                1 => entry.learning += count,
                2 => {
                    entry.review += count;
                    entry.review_passed += count - again.min(count);
                }
                3 => entry.relearning += count,
                // A state from a newer app: not one of the four buckets.
                _ => continue,
            }
            entry.again += again;
            entry.time_ms += time.max(0) as u64;
        }
        Ok(days)
    }

    /// Retention over a range: review-card answers, and how many were not Again. Same cards as
    /// `daily_counts`.
    pub fn retention(&self, range: DayRange, deck: Option<Id>) -> Result<Retention, StudyError> {
        Ok(self
            .daily_counts(range, deck)?
            .iter()
            .fold(Retention::default(), |sum, d| Retention {
                reviews: sum.reviews + d.review,
                passed: sum.passed + d.review_passed,
            }))
    }
}
