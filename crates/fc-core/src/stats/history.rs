//! One card's review history.

use std::collections::HashSet;

use super::StudyError;
use crate::collection::Collection;
use crate::id::Id;
use crate::scheduling::{CardState, Due, Memory, Rating, elapsed_days};
use crate::study::EventKind;

/// One answer, as a history screen shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewEntry {
    pub event: Id,
    /// Unix milliseconds.
    pub time_ms: i64,
    /// The device's UTC offset in minutes when it was answered.
    pub utc_offset: i32,
    /// The study day it belongs to.
    pub day: i64,
    pub rating: Rating,
    /// What the card was before and after. `None` if the event does not say (a newer app).
    pub state_before: Option<CardState>,
    pub state: Option<CardState>,
    /// Study days since the previous review in this list, `None` for the first.
    pub days_since_previous: Option<u32>,
    /// When the answer made the card come back.
    pub due: Option<Due>,
    pub duration_ms: u32,
    /// What FSRS remembered after the answer.
    pub memory: Option<Memory>,
    pub device: Id,
}

impl Collection {
    /// A card's reviews, oldest first, in the order the schedule folds them. Reviews that were
    /// undone are left out (`card_events` has them). A deleted card keeps its history, and
    /// restoring the card shows it again.
    pub fn card_history(&self, card: Id) -> Result<Vec<ReviewEntry>, StudyError> {
        let found: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM card WHERE id = ?1)",
            [card],
            |row| row.get(0),
        )?;
        if !found {
            return Err(StudyError::NotFound);
        }
        let events = self.card_events(card)?;
        let voided: HashSet<Id> = events
            .iter()
            .filter(|e| e.kind == EventKind::Void)
            .filter_map(|e| e.target)
            .collect();
        let mut out: Vec<ReviewEntry> = Vec::new();
        for e in events {
            if e.kind != EventKind::Review || voided.contains(&e.id) {
                continue;
            }
            let Some(rating) = e.rating else { continue };
            let due = match (e.due_day, e.due_ms) {
                (Some(day), _) => {
                    Some(Due::Days((day - e.day).clamp(0, i64::from(u32::MAX)) as u32))
                }
                (None, Some(ms)) => Some(Due::Minutes(
                    ((ms - e.time_ms + 30_000) / 60_000).clamp(0, i64::from(u32::MAX)) as u32,
                )),
                (None, None) => None,
            };
            out.push(ReviewEntry {
                event: e.id,
                time_ms: e.time_ms,
                utc_offset: e.utc_offset,
                day: e.day,
                rating,
                state_before: e.state_before,
                state: e.state,
                days_since_previous: out.last().map(|p| elapsed_days(e.day, p.day)),
                due,
                duration_ms: e.duration_ms.unwrap_or(0),
                memory: e.memory,
                device: e.device,
            });
        }
        Ok(out)
    }
}
