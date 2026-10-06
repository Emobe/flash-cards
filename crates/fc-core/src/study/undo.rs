//! Undo of the last answer (ADR 0007, part 10). Undo is a void event, so it syncs like any other
//! event and nothing is deleted: the review stays stored, and the fold leaves it out, which puts the
//! card's state, today's counts and the held-back siblings back as they were.

use rusqlite::OptionalExtension;

use super::event::{CardEvent, EventKind, event, events_of};
use super::fold::{ParameterSets, rebuild_card};
use super::schedule::{CardSchedule, read_cached};
use super::{CARD_EVENT, StudyError};
use crate::collection::Collection;
use crate::id::Id;
use crate::scheduling::{Rating, study_day};
use crate::sync::state;

/// What an undo took back.
#[derive(Debug, Clone, PartialEq)]
pub struct Undone {
    pub card: Id,
    /// The review that is now voided.
    pub event: Id,
    pub rating: Rating,
    /// The card as it is after the undo.
    pub schedule: CardSchedule,
}

impl Collection {
    /// Takes back the newest answer made on this device that is not already taken back, and
    /// returns what it was, or `None` if there is none. Calling it again goes back one more answer.
    /// Refused with `LaterAnswer` if the card was answered again after that answer (from another
    /// device, after a merge).
    pub fn undo_answer(&self) -> Result<Option<Undone>, StudyError> {
        let device = state::device_id(&self.conn)?;
        let target: Option<Id> = self
            .conn
            .query_row(
                "SELECT e.id FROM card_event e
                 WHERE e.kind = 'review' AND e.device = ?1
                   AND NOT EXISTS (SELECT 1 FROM card_event v WHERE v.kind = 'void' AND v.target = e.id)
                 ORDER BY e.time_ms DESC, e.id DESC LIMIT 1",
                [device],
                |row| row.get(0),
            )
            .optional()?;
        let Some(target) = target else {
            return Ok(None);
        };
        let target = event(&self.conn, target)?.ok_or(StudyError::NotFound)?;
        let card = target.card;
        let all = events_of(&self.conn, card)?;
        if has_later_answer(&all, &target) {
            return Err(StudyError::LaterAnswer);
        }

        let now = self.host.clock.now();
        let start_hour = self.day_start_hour()?;
        let last = read_cached(&self.conn, card)?.map(|c| c.last_event);
        let previous_time = last
            .and_then(|id| all.iter().find(|e| e.id == id))
            .map_or(target.time_ms, |e| e.time_ms);
        let time_ms = now.unix_ms.max(previous_time.saturating_add(1));
        let id = state::new_id(&self.host)?;
        let void = CardEvent {
            id,
            card,
            kind: EventKind::Void,
            time_ms,
            utc_offset: now.utc_offset_minutes,
            device,
            previous: last,
            day: study_day(time_ms, now.utc_offset_minutes, start_hour),
            rating: None,
            duration_ms: None,
            preset: None,
            desired_retention: None,
            parameters: None,
            steps: None,
            state_before: None,
            state: None,
            step: None,
            memory: None,
            due_day: None,
            due_ms: None,
            target: Some(target.id),
        };
        let sets = ParameterSets::load(&self.conn)?;
        self.write(|w| {
            w.insert_row(CARD_EVENT.entity, id, void.values())?;
            rebuild_card(w.local(), card, &sets)?;
            Ok(())
        })?;
        Ok(Some(Undone {
            card,
            event: target.id,
            rating: target.rating.unwrap_or(Rating::Good),
            schedule: self.card_schedule(card)?,
        }))
    }
}

/// Whether the card has an event, not voided and not a void, after `target` in the order of the
/// fold, or built directly on it.
fn has_later_answer(all: &[CardEvent], target: &CardEvent) -> bool {
    let voided: Vec<Id> = all
        .iter()
        .filter(|e| e.kind == EventKind::Void)
        .filter_map(|e| e.target)
        .collect();
    all.iter().any(|e| {
        e.id != target.id
            && e.kind != EventKind::Void
            && !voided.contains(&e.id)
            && ((e.time_ms, e.id) > (target.time_ms, target.id) || e.previous == Some(target.id))
    })
}
