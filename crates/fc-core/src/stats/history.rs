//! One card's review history: its answers, and the set due dates and resets in between.

use std::collections::HashSet;

use super::StudyError;
use crate::collection::Collection;
use crate::id::Id;
use crate::scheduling::{CardState, Due, Memory, Rating, elapsed_days};
use crate::study::{EventKind, events_with_sources};

/// What an entry of a card's history is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// An answer, with its rating.
    Answer,
    /// The due date was set by hand (ADR 0014, decision 5). Not an answer.
    SetDue,
    /// The card was reset to new. Not an answer.
    Reset,
    /// The card took the schedule of the card it replaced when its note changed type (ADR 0014,
    /// decision 6). The entries before it are that card's. Not an answer.
    Transfer,
}

/// One answer, or a set due date, reset or transfer, as a history screen shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewEntry {
    pub event: Id,
    pub kind: HistoryKind,
    /// Unix milliseconds.
    pub time_ms: i64,
    /// The device's UTC offset in minutes when it was answered.
    pub utc_offset: i32,
    /// The study day it belongs to.
    pub day: i64,
    /// `None` for an entry that is not an answer.
    pub rating: Option<Rating>,
    /// What the card was before and after. `None` if the event does not say (a newer app).
    pub state_before: Option<CardState>,
    pub state: Option<CardState>,
    /// For an answer: study days since the previous answer in this list, `None` for the first.
    /// `None` for an entry that is not an answer.
    pub days_since_previous: Option<u32>,
    /// When the answer made the card come back.
    pub due: Option<Due>,
    pub duration_ms: u32,
    /// What FSRS remembered after the answer.
    pub memory: Option<Memory>,
    pub device: Id,
}

impl Collection {
    /// A card's answers, set due dates and resets, oldest first, in the order the schedule folds
    /// them, with those of the cards it took its schedule from when its note changed type. Those
    /// that were undone are left out (`card_events` has them). A deleted card keeps its history,
    /// and restoring the card shows it again.
    pub fn card_history(&self, card: Id) -> Result<Vec<ReviewEntry>, StudyError> {
        let found: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM card WHERE id = ?1)",
            [card],
            |row| row.get(0),
        )?;
        if !found {
            return Err(StudyError::NotFound);
        }
        let events = events_with_sources(&self.conn, card)?;
        let voided: HashSet<Id> = events
            .iter()
            .filter(|e| e.kind == EventKind::Void)
            .filter_map(|e| e.target)
            .collect();
        let mut out: Vec<ReviewEntry> = Vec::new();
        let mut last_answer: Option<i64> = None;
        for e in events {
            if voided.contains(&e.id) {
                continue;
            }
            let (kind, rating) = match (&e.kind, e.rating) {
                (EventKind::Review, Some(rating)) => (HistoryKind::Answer, Some(rating)),
                (EventKind::SetDue, _) => (HistoryKind::SetDue, None),
                (EventKind::Reset, _) => (HistoryKind::Reset, None),
                (EventKind::Transfer, _) => (HistoryKind::Transfer, None),
                _ => continue,
            };
            let due = match (e.due_day, e.due_ms) {
                (Some(day), _) => {
                    Some(Due::Days((day - e.day).clamp(0, i64::from(u32::MAX)) as u32))
                }
                (None, Some(ms)) => Some(Due::Minutes(
                    ((ms - e.time_ms + 30_000) / 60_000).clamp(0, i64::from(u32::MAX)) as u32,
                )),
                (None, None) => None,
            };
            let days_since_previous = rating
                .and(last_answer)
                .map(|previous| elapsed_days(e.day, previous));
            if rating.is_some() {
                last_answer = Some(e.day);
            }
            out.push(ReviewEntry {
                event: e.id,
                kind,
                time_ms: e.time_ms,
                utc_offset: e.utc_offset,
                day: e.day,
                rating,
                state_before: e.state_before,
                state: e.state,
                days_since_previous,
                due,
                duration_ms: e.duration_ms.unwrap_or(0),
                memory: e.memory,
                device: e.device,
            });
        }
        Ok(out)
    }
}

/// Where a live card is and whether it is out of the queue, for the card info screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardDetail {
    pub card: Id,
    pub note: Id,
    pub note_type: Id,
    pub template: Id,
    /// The cloze number, or 0 for a template that is not cloze.
    pub ordinal: u32,
    pub deck: Id,
    pub suspended: bool,
    /// Buried until a later study day.
    pub buried: bool,
}

impl Collection {
    /// A live card of a live note. `NotFound` for a missing or deleted card or note.
    pub fn card_detail(&self, card: Id) -> Result<CardDetail, StudyError> {
        let today = self.today()?;
        let found = self
            .conn
            .query_row(
                "SELECT c.note, n.note_type, c.template, c.ordinal, c.deck, c.suspended,
                        c.buried_until
                 FROM card c JOIN note n ON n.id = c.note
                 WHERE c.id = ?1 AND c.deleted = 0 AND n.deleted = 0",
                [card],
                |row| {
                    Ok(CardDetail {
                        card,
                        note: row.get(0)?,
                        note_type: row.get(1)?,
                        template: row.get(2)?,
                        ordinal: row.get(3)?,
                        deck: crate::deck::card_deck(&row.get::<_, Vec<u8>>(4)?),
                        suspended: row.get::<_, i64>(5)? != 0,
                        buried: row.get::<_, i64>(6)? >= today,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StudyError::NotFound,
                other => other.into(),
            })?;
        Ok(found)
    }
}
