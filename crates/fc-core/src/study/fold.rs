//! Folding events into the schedule cache (ADR 0007, part 9, and ADR 0006, section 4).
//!
//! A card's schedule is what its events add up to. The fold drops voided reviews, applies the rest
//! in order of `(time, ID)`, and for each takes its recorded result when it built on the event just
//! applied. An event that did not (two devices answered the same card without seeing each other) is
//! recomputed from the card as it is at that point, with the inputs it recorded and the fuzz from
//! its own ID, so every device reaches the same card. The same function serves the rebuild of the
//! whole cache and a single card after undo or, in 1.11, after a merge.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::StudyError;
use super::answer::parse_steps_text;
use super::event::{CardEvent, EventKind, events_of, from_row};
use super::schedule::{Applied, Cached, after, clear_cached, compute, write_cached};
use crate::collection::{Collection, CollectionError};
use crate::id::Id;
use crate::scheduling::{Scheduler, Steps, default_parameters, parameter_set_id};

/// Bumped whenever the fold or the state machine changes what it gives for the same events (ADR
/// 0007, Consequences). A collection whose cache was built by another version is rebuilt on open.
pub(crate) const SCHEDULE_CACHE_VERSION: u32 = 1;
const CACHE_VERSION_KEY: &str = "schedule_cache_version";

/// The parameter sets stored in the collection, by ID.
pub(crate) struct ParameterSets(HashMap<Id, Vec<f32>>);

impl ParameterSets {
    pub(crate) fn load(conn: &Connection) -> rusqlite::Result<Self> {
        let mut statement = conn.prepare("SELECT id, values_f32 FROM fsrs_parameter_set")?;
        let sets = statement
            .query_map([], |row| {
                let bytes: Vec<u8> = row.get(1)?;
                let values = bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                Ok((row.get(0)?, values))
            })?
            .collect::<Result<_, _>>()?;
        Ok(Self(sets))
    }

    /// The values a parameter set ID stands for. The defaults need no row, since their ID is known.
    fn values(&self, id: Option<Id>) -> Option<Vec<f32>> {
        match id {
            None => Some(default_parameters().to_vec()),
            Some(id) if id == parameter_set_id(default_parameters()) => {
                Some(default_parameters().to_vec())
            }
            Some(id) => self.0.get(&id).cloned(),
        }
    }
}

/// An event computed again from the card as it is when the fold reaches it. `None` if the event
/// does not have what that takes (a rating, retention, a parameter set this device does not have).
fn recompute(event: &CardEvent, cached: Option<&Cached>, sets: &ParameterSets) -> Option<Applied> {
    let rating = event.rating?;
    let values = sets.values(event.parameters)?;
    let scheduler = Scheduler::new(Some(&values), event.desired_retention? as f32).ok()?;
    let (learning, relearning) = parse_steps_text(event.steps.as_deref()?);
    let steps = Steps {
        learning: &learning,
        relearning: &relearning,
    };
    compute(
        cached,
        event.day,
        event.time_ms,
        rating,
        steps,
        &scheduler,
        event.id,
    )
    .ok()
}

/// The cache row for a card with these events (in `(time, ID)` order), or `None` if it is new.
pub(crate) fn fold(events: &[CardEvent], sets: &ParameterSets) -> Option<Cached> {
    let voided: HashSet<Id> = events
        .iter()
        .filter(|e| e.kind == EventKind::Void)
        .filter_map(|e| e.target)
        .collect();
    let mut cached: Option<Cached> = None;
    for event in events {
        if event.kind == EventKind::Void || voided.contains(&event.id) {
            continue;
        }
        let builds_on_last = event.previous == cached.as_ref().map(|c| c.last_event);
        match &event.kind {
            EventKind::Review => {
                let Some(rating) = event.rating else { continue };
                let applied = if builds_on_last {
                    event.applied()
                } else {
                    None
                }
                // Recomputed when it did not build on the last one, or when the event has no
                // recorded result. If that is not possible either, the recorded result is the best
                // there is, so a review is never lost from the card.
                .or_else(|| recompute(event, cached.as_ref(), sets))
                .or_else(|| event.applied());
                if let Some(applied) = applied {
                    cached = Some(after(
                        cached.as_ref(),
                        event.id,
                        event.day,
                        Some(rating),
                        &applied,
                    ));
                }
            }
            // A kind from a newer app counts only where it builds on the last event: its recorded
            // result is all we can know of it.
            EventKind::Unknown(_) => {
                if let Some(applied) = event.applied().filter(|_| builds_on_last) {
                    cached = Some(after(cached.as_ref(), event.id, event.day, None, &applied));
                }
            }
            EventKind::Void => {}
        }
    }
    cached
}

/// Folds one card's events and writes the result into the cache.
pub(crate) fn rebuild_card(
    conn: &Connection,
    card: Id,
    sets: &ParameterSets,
) -> rusqlite::Result<()> {
    match fold(&events_of(conn, card)?, sets) {
        Some(cached) => write_cached(conn, card, &cached),
        None => clear_cached(conn, card),
    }
}

/// Rebuilds the whole cache from the events, one card's events in memory at a time.
pub(crate) fn rebuild_all(conn: &Connection) -> rusqlite::Result<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    tx.execute("DELETE FROM card_schedule", [])?;
    let sets = ParameterSets::load(&tx)?;
    {
        let mut statement = tx.prepare(&format!(
            "SELECT {} ORDER BY card, time_ms, id",
            super::event::SELECT
        ))?;
        let mut rows = statement.query([])?;
        let mut current: Vec<CardEvent> = Vec::new();
        while let Some(row) = rows.next()? {
            let event = from_row(row)?;
            if current
                .first()
                .is_some_and(|first| first.card != event.card)
            {
                flush(&tx, &current, &sets)?;
                current.clear();
            }
            current.push(event);
        }
        flush(&tx, &current, &sets)?;
    }
    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        rusqlite::params![CACHE_VERSION_KEY, SCHEDULE_CACHE_VERSION.to_string()],
    )?;
    tx.commit()
}

fn flush(conn: &Connection, events: &[CardEvent], sets: &ParameterSets) -> rusqlite::Result<()> {
    if let (Some(first), Some(cached)) = (events.first(), fold(events, sets)) {
        write_cached(conn, first.card, &cached)?;
    }
    Ok(())
}

/// Run on every open: a cache built by another version of the fold is rebuilt. A collection from
/// before the schedule existed has no cache table, and nothing to do.
pub(crate) fn check_cache(conn: &Connection) -> Result<(), CollectionError> {
    let has_cache: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'card_schedule'",
            [],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if !has_cache {
        return Ok(());
    }
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [CACHE_VERSION_KEY],
            |row| row.get(0),
        )
        .optional()?;
    if stored.as_deref() != Some(&SCHEDULE_CACHE_VERSION.to_string()) {
        rebuild_all(conn)?;
    }
    Ok(())
}

impl Collection {
    /// Throws the schedule cache away and folds it again from the card events. Nothing is written
    /// to a synced table. The result is what answering built, so this is for after a merge, a
    /// change to the fold, or a doubt.
    pub fn rebuild_schedule(&self) -> Result<(), StudyError> {
        Ok(rebuild_all(&self.conn)?)
    }
}
