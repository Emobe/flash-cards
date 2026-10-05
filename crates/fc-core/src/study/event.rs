//! Card events as stored and as read (ADR 0007, part 6). An event is immutable: it is written once
//! through `WriteTx::insert_row` and never changed. Columns a kind does not use are null.

use rusqlite::types::Value;
use rusqlite::{Connection, Row};

use super::schedule::{Applied, opt};
use super::{CARD_EVENT, StudyError};
use crate::collection::Collection;
use crate::id::Id;
use crate::scheduling::{CardState, Memory, Rating};

/// What an event records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// An answer.
    Review,
    /// Cancels the review it targets (undo). The review stays stored.
    Void,
    /// A kind this build does not know, from a newer app. It is kept as it is.
    Unknown(String),
}

impl EventKind {
    pub(crate) fn as_text(&self) -> &str {
        match self {
            Self::Review => "review",
            Self::Void => "void",
            Self::Unknown(text) => text,
        }
    }

    fn from_text(text: String) -> Self {
        match text.as_str() {
            "review" => Self::Review,
            "void" => Self::Void,
            _ => Self::Unknown(text),
        }
    }
}

/// One card event.
#[derive(Debug, Clone, PartialEq)]
pub struct CardEvent {
    pub id: Id,
    pub card: Id,
    pub kind: EventKind,
    /// Unix milliseconds. Never before the time of the event this one builds on.
    pub time_ms: i64,
    /// The UTC offset of the device when it was written, in minutes.
    pub utc_offset: i32,
    pub device: Id,
    /// The event this one built on: the card's last event on the device that wrote it.
    pub previous: Option<Id>,
    /// The study day it belongs to, worked out when it was written.
    pub day: i64,
    pub rating: Option<Rating>,
    pub duration_ms: Option<u32>,
    /// The inputs: the preset, its desired retention, the FSRS parameter set (by ID) and the steps
    /// that applied (whole minutes separated by spaces, empty for none).
    pub preset: Option<Id>,
    pub desired_retention: Option<f64>,
    pub parameters: Option<Id>,
    pub steps: Option<String>,
    /// The result.
    pub state_before: Option<CardState>,
    pub state: Option<CardState>,
    pub step: Option<u32>,
    pub memory: Option<Memory>,
    pub due_day: Option<i64>,
    pub due_ms: Option<i64>,
    /// For a void: the review it cancels.
    pub target: Option<Id>,
}

impl CardEvent {
    /// The recorded result, if the event has all of it.
    pub(crate) fn applied(&self) -> Option<Applied> {
        Some(Applied {
            state_before: self.state_before?,
            state: self.state?,
            step: self.step?,
            memory: self.memory?,
            due_day: self.due_day,
            due_ms: self.due_ms,
        })
    }

    /// All columns, for `WriteTx::insert_row`.
    pub(crate) fn values(&self) -> Vec<(&'static str, Value)> {
        let blob = |id: Id| Value::Blob(id.as_bytes().to_vec());
        let columns: [(&'static str, Value); 21] = [
            ("card", blob(self.card)),
            ("kind", Value::Text(self.kind.as_text().to_owned())),
            ("time_ms", Value::Integer(self.time_ms)),
            ("utc_offset", Value::Integer(self.utc_offset.into())),
            ("device", blob(self.device)),
            ("previous", opt(self.previous.map(blob))),
            ("day", Value::Integer(self.day)),
            ("rating", opt(self.rating.map(|r| i64::from(r.number())))),
            ("duration_ms", opt(self.duration_ms.map(i64::from))),
            ("preset", opt(self.preset.map(blob))),
            ("desired_retention", opt(self.desired_retention)),
            ("parameters", opt(self.parameters.map(blob))),
            ("steps", opt(self.steps.clone())),
            ("state_before", opt(self.state_before.map(CardState::code))),
            ("state", opt(self.state.map(CardState::code))),
            ("step", opt(self.step.map(i64::from))),
            (
                "stability",
                opt(self.memory.map(|m| f64::from(m.stability))),
            ),
            (
                "difficulty",
                opt(self.memory.map(|m| f64::from(m.difficulty))),
            ),
            ("due_day", opt(self.due_day)),
            ("due_ms", opt(self.due_ms)),
            ("target", opt(self.target.map(blob))),
        ];
        debug_assert_eq!(columns.len(), CARD_EVENT.columns.len());
        columns.into()
    }
}

/// The columns `from_row` reads, in its order.
const SELECT: &str = "id, card, kind, time_ms, utc_offset, device, previous, day, rating,
    duration_ms, preset, desired_retention, parameters, steps, state_before, state, step,
    stability, difficulty, due_day, due_ms, target FROM card_event";

fn from_row(row: &Row<'_>) -> rusqlite::Result<CardEvent> {
    let stability: Option<f64> = row.get(17)?;
    let difficulty: Option<f64> = row.get(18)?;
    Ok(CardEvent {
        id: row.get(0)?,
        card: row.get(1)?,
        kind: EventKind::from_text(row.get(2)?),
        time_ms: row.get(3)?,
        utc_offset: row.get(4)?,
        device: row.get(5)?,
        previous: row.get(6)?,
        day: row.get(7)?,
        rating: row.get::<_, Option<i64>>(8)?.and_then(Rating::from_number),
        duration_ms: row
            .get::<_, Option<i64>>(9)?
            .map(|ms| ms.clamp(0, i64::from(u32::MAX)) as u32),
        preset: row.get(10)?,
        desired_retention: row.get(11)?,
        parameters: row.get(12)?,
        steps: row.get(13)?,
        state_before: row
            .get::<_, Option<i64>>(14)?
            .and_then(CardState::from_code),
        state: row
            .get::<_, Option<i64>>(15)?
            .and_then(CardState::from_code),
        step: row
            .get::<_, Option<i64>>(16)?
            .map(|s| s.clamp(0, i64::from(u32::MAX)) as u32),
        memory: stability.zip(difficulty).map(|(s, d)| Memory {
            stability: s as f32,
            difficulty: d as f32,
        }),
        due_day: row.get(19)?,
        due_ms: row.get(20)?,
        target: row.get(21)?,
    })
}

/// A card's events in the order the fold applies them: by time, then ID.
pub(crate) fn events_of(conn: &Connection, card: Id) -> rusqlite::Result<Vec<CardEvent>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {SELECT} WHERE card = ?1 ORDER BY time_ms, id"
    ))?;
    statement.query_map([card], from_row)?.collect()
}

pub(crate) fn event(conn: &Connection, id: Id) -> rusqlite::Result<Option<CardEvent>> {
    let mut statement = conn.prepare(&format!("SELECT {SELECT} WHERE id = ?1"))?;
    let mut rows = statement.query_map([id], from_row)?;
    rows.next().transpose()
}

impl Collection {
    /// Every event of a card, oldest first, voided reviews and voids included (1.8 builds the
    /// history on this).
    pub fn card_events(&self, card: Id) -> Result<Vec<CardEvent>, StudyError> {
        Ok(events_of(&self.conn, card)?)
    }
}
