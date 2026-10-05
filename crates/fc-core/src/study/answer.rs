//! Answering a card (ADR 0007, parts 1, 6 and 9): the answer becomes an immutable event, and the
//! card's row in the schedule cache is updated in the same transaction.

use rusqlite::Connection;
use rusqlite::types::Value;

use super::event::{CardEvent, EventKind};
use super::schedule::{after, compute, read_cached, write_cached};
use super::{PARAMETER_SET, StudyError};
use crate::collection::Collection;
use crate::deck::Preset;
use crate::id::Id;
use crate::scheduling::{
    CardState, Rating, Scheduler, Steps, default_parameters, parameter_bytes, parameter_set_id,
    study_day,
};
use crate::sync::state;

/// The longest time one answer can be recorded as taking. A card left open overnight does not
/// distort the statistics.
const MAX_DURATION_MS: u32 = 3_600_000;

/// What an answer did to the card.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    /// The event that records it, which undo needs.
    pub event: Id,
    pub state: CardState,
    /// For a review card: the study day it is due.
    pub due_day: Option<i64>,
    /// For a learning or relearning card: when it is due, in Unix milliseconds.
    pub due_ms: Option<i64>,
    /// Whole days until a review card is due, after fuzz.
    pub interval_days: Option<u32>,
}

pub(crate) fn join_steps(steps: &[u32]) -> String {
    steps
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Both step lists of a preset as an event records them: the learning steps, a bar, the relearning
/// steps (`1 10|10`). A concurrent event is recomputed from whatever state the card is in when the
/// fold reaches it, which can differ from the state it was answered in, so it needs both.
pub(crate) fn steps_text(preset: &Preset) -> String {
    format!(
        "{}|{}",
        join_steps(&preset.learning_steps),
        join_steps(&preset.relearning_steps)
    )
}

/// The inverse of `steps_text`. A missing or malformed part is an empty list.
pub(crate) fn parse_steps_text(text: &str) -> (Vec<u32>, Vec<u32>) {
    let (learning, relearning) = text.split_once('|').unwrap_or((text, ""));
    let parse = |part: &str| {
        part.split_whitespace()
            .filter_map(|word| word.parse().ok())
            .collect()
    };
    (parse(learning), parse(relearning))
}

/// The 21 parameters a preset uses: its own, or the defaults.
pub(crate) fn effective_parameters(preset: &Preset) -> Vec<f32> {
    if preset.fsrs_parameters.is_empty() {
        default_parameters().to_vec()
    } else {
        preset.fsrs_parameters.clone()
    }
}

impl Collection {
    /// The deck of a live card (one that is not deleted, in a note that is not deleted).
    fn live_card_deck(&self, card: Id) -> Result<Id, StudyError> {
        live_card_deck(&self.conn, card)
    }

    /// Records an answer to a card: schedules it, writes the event and updates the cache, in one
    /// transaction. `duration_ms` is how long the person took (kept up to one hour). Answering a
    /// suspended or buried card is allowed, because a merge can produce it; answering a deleted card
    /// is refused.
    pub fn answer(
        &self,
        card: Id,
        rating: Rating,
        duration_ms: u32,
    ) -> Result<Answered, StudyError> {
        let deck = self.live_card_deck(card)?;
        let preset = self.deck_preset(deck)?;
        let now = self.host.clock.now();
        let start_hour = self.day_start_hour()?;
        let cached = read_cached(&self.conn, card)?;
        // An event is never earlier than the one it builds on, so the fold always sees the order.
        let time_ms = cached.as_ref().map_or(now.unix_ms, |c| {
            let previous = event_time(&self.conn, c.last_event).unwrap_or(i64::MIN);
            now.unix_ms.max(previous.saturating_add(1))
        });
        let day = study_day(time_ms, now.utc_offset_minutes, start_hour);

        let parameters = effective_parameters(&preset);
        let scheduler = Scheduler::new(Some(&parameters), preset.desired_retention as f32)?;
        let id = state::new_id(&self.host)?;
        let steps = Steps {
            learning: &preset.learning_steps,
            relearning: &preset.relearning_steps,
        };
        let applied = compute(cached.as_ref(), day, time_ms, rating, steps, &scheduler, id)?;
        let event = CardEvent {
            id,
            card,
            kind: EventKind::Review,
            time_ms,
            utc_offset: now.utc_offset_minutes,
            device: state::device_id(&self.conn)?,
            previous: cached.as_ref().map(|c| c.last_event),
            day,
            rating: Some(rating),
            duration_ms: Some(duration_ms.min(MAX_DURATION_MS)),
            preset: Some(preset.id),
            desired_retention: Some(preset.desired_retention),
            parameters: Some(parameter_set_id(&parameters)),
            steps: Some(steps_text(&preset)),
            state_before: Some(applied.state_before),
            state: Some(applied.state),
            step: Some(applied.step),
            memory: Some(applied.memory),
            due_day: applied.due_day,
            due_ms: applied.due_ms,
            target: None,
        };
        let row = after(cached.as_ref(), id, day, Some(rating), &applied);
        self.write(|w| {
            w.insert_row(
                PARAMETER_SET.entity,
                parameter_set_id(&parameters),
                vec![("values_f32", Value::Blob(parameter_bytes(&parameters)))],
            )?;
            w.insert_row(super::CARD_EVENT.entity, id, event.values())?;
            write_cached(w.local(), card, &row)?;
            Ok(())
        })?;
        Ok(Answered {
            event: id,
            state: applied.state,
            due_day: applied.due_day,
            due_ms: applied.due_ms,
            interval_days: applied
                .due_day
                .map(|due| (due - day).clamp(0, i64::from(u32::MAX)) as u32),
        })
    }
}

fn event_time(conn: &Connection, event: Id) -> Option<i64> {
    super::event::event(conn, event)
        .ok()
        .flatten()
        .map(|e| e.time_ms)
}

/// The deck a card is in, if the card is live.
pub(crate) fn live_card_deck(conn: &Connection, card: Id) -> Result<Id, StudyError> {
    use rusqlite::OptionalExtension;
    let found: Option<(Vec<u8>, i64, Option<i64>)> = conn
        .query_row(
            "SELECT c.deck, c.deleted, n.deleted FROM card c
             LEFT JOIN note n ON n.id = c.note WHERE c.id = ?1",
            [card],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    match found {
        Some((deck, 0, Some(0))) => Ok(crate::deck::card_deck(&deck)),
        _ => Err(StudyError::NotFound),
    }
}
