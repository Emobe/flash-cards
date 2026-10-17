//! Set due date and reset to new, the "reschedule" bulk actions (ADR 0014, decision 5), and the void
//! that undoes them. Each is a card event that records its result, like an answer, so the fold takes
//! it as it is when it builds on the card's last event, and an older app does the same with a kind
//! it does not know. Each call is one write for every card.

use std::collections::HashSet;

use super::answer::live_card_deck;
use super::event::{CardEvent, EventKind, event, events_of};
use super::fold::{ParameterSets, rebuild_card};
use super::schedule::{Applied, Cached, after_change, read_cached, write_cached};
use super::undo::has_later_answer;
use super::{CARD_EVENT, StudyError};
use crate::collection::Collection;
use crate::id::Id;
use crate::scheduling::{CardState, MAX_INTERVAL_DAYS, day_in_range, study_day};
use crate::sync::state;

/// What a set due date or a reset wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescheduled {
    /// One event per card that changed, which undo voids.
    pub events: Vec<Id>,
    /// Cards left as they were because they are new: a set due date has no memory to build on, and
    /// a reset has nothing to do.
    pub skipped_new: usize,
}

/// What `void_events` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voided {
    /// The events now voided.
    pub voided: Vec<Id>,
    /// Events that were not voided because their card has a later event (an answer since, here or
    /// on another device).
    pub refused: usize,
}

/// An event that is not an answer, built on the card's last event.
struct Change {
    card: Id,
    previous: Option<Cached>,
    applied: Applied,
    /// The study day of the card's last answer, which the card keeps.
    last_day: Option<i64>,
}

impl Collection {
    /// Makes each card a review card due `from_days` to `to_days` study days from today (0 is
    /// today), keeping its memory and the day of its last answer, so the next answer sees the real
    /// time since then. With a range, each card's day comes from its event's ID. New cards are
    /// skipped. Refused with `DueRange` for a range outside 0 to 36,500 days or the wrong way round,
    /// and with `NotFound` if any card is deleted.
    pub fn set_due(
        &self,
        cards: &[Id],
        from_days: u32,
        to_days: u32,
    ) -> Result<Rescheduled, StudyError> {
        if from_days > to_days || to_days > MAX_INTERVAL_DAYS {
            return Err(StudyError::DueRange);
        }
        self.write_changes(cards, EventKind::SetDue, |cached, id, day| {
            let card = cached.filter(|c| c.state != CardState::New)?;
            let due = day + i64::from(day_in_range(from_days, to_days, id));
            Some((
                Applied {
                    state_before: card.state,
                    state: CardState::Review,
                    step: 0,
                    memory: card.memory,
                    due_day: Some(due),
                    due_ms: None,
                },
                Some(card.last_day),
            ))
        })
    }

    /// The cards among `cards` that are new (never answered, every answer undone, or reset), each
    /// once, in the order given: those a set due date leaves alone and a reset skips. Browse reads
    /// it before either. It does not check that the cards exist.
    pub fn new_among(&self, cards: &[Id]) -> Result<Vec<Id>, StudyError> {
        let mut statement = self
            .conn
            .prepare("SELECT card FROM card_schedule WHERE state <> 0")?;
        let studied: HashSet<Id> = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let mut seen = HashSet::new();
        Ok(cards
            .iter()
            .copied()
            .filter(|c| !studied.contains(c) && seen.insert(*c))
            .collect())
    }

    /// Makes each card new again: no memory, the first learning step. Its history stays, and so do
    /// its counts of answers and lapses. The event records the day of the last answer as a set due
    /// date does, so an app that does not know the kind keeps the same day. Cards that are already new are skipped. Refused with
    /// `NotFound` if any card is deleted.
    pub fn reset_cards(&self, cards: &[Id]) -> Result<Rescheduled, StudyError> {
        self.write_changes(cards, EventKind::Reset, |cached, _, _| {
            let card = cached.filter(|c| c.state != CardState::New)?;
            Some((
                Applied {
                    state_before: card.state,
                    state: CardState::New,
                    step: 0,
                    memory: None,
                    due_day: None,
                    due_ms: None,
                },
                Some(card.last_day),
            ))
        })
    }

    /// Writes one event of `kind` per card, with the result `result` makes, in one write. `result` gets the card
    /// as cached, the new event's ID and its study day, and answers `None` to skip the card.
    fn write_changes(
        &self,
        cards: &[Id],
        kind: EventKind,
        result: impl Fn(Option<&Cached>, Id, i64) -> Option<(Applied, Option<i64>)>,
    ) -> Result<Rescheduled, StudyError> {
        let mut seen = HashSet::new();
        let cards: Vec<Id> = cards.iter().copied().filter(|c| seen.insert(*c)).collect();
        for card in &cards {
            live_card_deck(&self.conn, *card)?;
        }
        let now = self.host.clock.now();
        let start_hour = self.day_start_hour()?;
        let device = state::device_id(&self.conn)?;
        let mut events: Vec<(CardEvent, Change)> = Vec::new();
        let mut skipped_new = 0;
        for card in cards {
            let cached = read_cached(&self.conn, card)?;
            // Never earlier than the event it builds on, as for an answer.
            let time_ms = match &cached {
                None => now.unix_ms,
                Some(c) => {
                    let previous = event(&self.conn, c.last_event)?.map_or(i64::MIN, |e| e.time_ms);
                    now.unix_ms.max(previous.saturating_add(1))
                }
            };
            let day = study_day(time_ms, now.utc_offset_minutes, start_hour);
            let id = state::new_id(&self.host)?;
            let Some((applied, last_day)) = result(cached.as_ref(), id, day) else {
                skipped_new += 1;
                continue;
            };
            let event = CardEvent {
                id,
                card,
                kind: kind.clone(),
                time_ms,
                utc_offset: now.utc_offset_minutes,
                device,
                previous: cached.as_ref().map(|c| c.last_event),
                day,
                rating: None,
                duration_ms: None,
                preset: None,
                desired_retention: None,
                parameters: None,
                steps: None,
                state_before: Some(applied.state_before),
                state: Some(applied.state),
                step: Some(applied.step),
                memory: applied.memory,
                due_day: applied.due_day,
                due_ms: applied.due_ms,
                target: None,
                source: None,
                last_day,
            };
            events.push((
                event,
                Change {
                    card,
                    previous: cached,
                    applied,
                    last_day,
                },
            ));
        }
        if events.is_empty() {
            return Ok(Rescheduled {
                events: Vec::new(),
                skipped_new,
            });
        }
        self.write(|w| {
            for (event, change) in &events {
                w.insert_row(CARD_EVENT.entity, event.id, event.values())?;
                let last_day = change
                    .last_day
                    .unwrap_or_else(|| change.previous.as_ref().map_or(event.day, |c| c.last_day));
                let row = after_change(
                    change.previous.as_ref(),
                    event.id,
                    last_day,
                    &change.applied,
                );
                write_cached(w.local(), change.card, &row)?;
            }
            Ok(())
        })?;
        Ok(Rescheduled {
            events: events.into_iter().map(|(e, _)| e.id).collect(),
            skipped_new,
        })
    }

    /// Voids these events (undo of a set due date or a reset, or of answers), in one write. An event
    /// whose card has a later event that is not voided is refused, as `undo_answer` refuses; one
    /// already voided, or a void itself, is skipped. Refused with `NotFound` if an event does not
    /// exist. Works for a deleted card.
    pub fn void_events(&self, ids: &[Id]) -> Result<Voided, StudyError> {
        let mut targets: Vec<CardEvent> = Vec::new();
        for id in ids {
            let found = event(&self.conn, *id)?.ok_or(StudyError::NotFound)?;
            if !targets.iter().any(|t| t.id == found.id) {
                targets.push(found);
            }
        }
        // The newest first, so voiding a card's later event lets an earlier one in the same call go.
        targets.sort_by_key(|t| std::cmp::Reverse((t.time_ms, t.id)));
        let now = self.host.clock.now();
        let start_hour = self.day_start_hour()?;
        let device = state::device_id(&self.conn)?;
        let mut new_ids = Vec::with_capacity(targets.len());
        for _ in &targets {
            new_ids.push(state::new_id(&self.host)?);
        }
        let sets = ParameterSets::load(&self.conn)?;
        let mut voided = Vec::new();
        let mut refused = 0;
        self.write(|w| {
            for (target, id) in targets.iter().zip(new_ids) {
                let conn = w.local();
                let all = events_of(conn, target.card)?;
                let already = all
                    .iter()
                    .any(|e| e.kind == EventKind::Void && e.target == Some(target.id));
                if target.kind == EventKind::Void || already {
                    continue;
                }
                if has_later_answer(&all, target) {
                    refused += 1;
                    continue;
                }
                let last = read_cached(conn, target.card)?.map(|c| c.last_event);
                let previous_time = last
                    .and_then(|l| all.iter().find(|e| e.id == l))
                    .map_or(target.time_ms, |e| e.time_ms);
                let time_ms = now.unix_ms.max(previous_time.saturating_add(1));
                let void = CardEvent {
                    id,
                    card: target.card,
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
                    source: None,
                    last_day: None,
                };
                w.insert_row(CARD_EVENT.entity, id, void.values())?;
                rebuild_card(w.local(), target.card, &sets)?;
                voided.push(target.id);
            }
            Ok(())
        })?;
        Ok(Voided { voided, refused })
    }
}
