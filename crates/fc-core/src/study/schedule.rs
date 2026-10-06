//! The schedule cache: what a card's events add up to (ADR 0007, part 9). One row in the local
//! table `card_schedule` per answered card, and no row for a new card. Answering and the fold both
//! build rows with the functions here, so there is one way to compute a result.

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

use super::StudyError;
use crate::collection::Collection;
use crate::id::Id;
use crate::scheduling::{
    CardNow, CardState, Due, Memory, Rating, Scheduler, SchedulingError, Steps, answer,
};

/// A card's schedule as the cache holds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cached {
    pub state: CardState,
    pub step: u32,
    pub memory: Memory,
    /// For a review card: the study day it is due.
    pub due_day: Option<i64>,
    /// For a learning or relearning card: when it is due, in Unix milliseconds.
    pub due_ms: Option<i64>,
    /// The study day of the last answer.
    pub last_day: i64,
    pub last_event: Id,
    pub answers: u32,
    pub lapses: u32,
}

/// What one answer does to a card: the fields a card event records as its result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Applied {
    pub state_before: CardState,
    pub state: CardState,
    pub step: u32,
    pub memory: Memory,
    pub due_day: Option<i64>,
    pub due_ms: Option<i64>,
}

/// What the scheduler sees of a card.
pub(crate) fn card_now(cached: Option<&Cached>) -> CardNow {
    match cached {
        None => CardNow::NEW,
        Some(c) => CardNow {
            state: c.state,
            step: c.step,
            memory: Some(c.memory),
        },
    }
}

/// Works out the answer. `day` is the study day of the event and `time_ms` its time, which is the
/// base of a step's due time. Pure: the same inputs give the same result on every device.
pub(crate) fn compute(
    cached: Option<&Cached>,
    day: i64,
    time_ms: i64,
    rating: Rating,
    steps: Steps<'_>,
    scheduler: &Scheduler,
    event: Id,
) -> Result<Applied, SchedulingError> {
    let elapsed = cached.map_or(0, |c| crate::scheduling::elapsed_days(day, c.last_day));
    let now = card_now(cached);
    let outcome = answer(now, elapsed, rating, steps, scheduler, event)?;
    let (due_day, due_ms) = match outcome.due {
        Due::Days(days) => (Some(day + i64::from(days)), None),
        Due::Minutes(minutes) => (None, Some(time_ms + i64::from(minutes) * 60_000)),
    };
    Ok(Applied {
        state_before: now.state,
        state: outcome.state,
        step: outcome.step,
        memory: outcome.memory,
        due_day,
        due_ms,
    })
}

/// The cache row after an event with this result. A review counts as an answer, and a lapse (a
/// review card answered Again) as a lapse. `rating` is `None` for a kind that is not a review.
pub(crate) fn after(
    cached: Option<&Cached>,
    event: Id,
    day: i64,
    rating: Option<Rating>,
    applied: &Applied,
) -> Cached {
    let (answers, lapses) = cached.map_or((0, 0), |c| (c.answers, c.lapses));
    let lapsed = applied.state_before == CardState::Review && rating == Some(Rating::Again);
    Cached {
        state: applied.state,
        step: applied.step,
        memory: applied.memory,
        due_day: applied.due_day,
        due_ms: applied.due_ms,
        last_day: day,
        last_event: event,
        answers: answers + u32::from(rating.is_some()),
        lapses: lapses + u32::from(lapsed),
    }
}

pub(crate) fn read_cached(conn: &Connection, card: Id) -> rusqlite::Result<Option<Cached>> {
    conn.query_row(
        "SELECT state, step, due_day, due_ms, stability, difficulty, last_day, last_event,
                answers, lapses
         FROM card_schedule WHERE card = ?1",
        [card],
        |row| {
            Ok(Cached {
                state: CardState::from_code(row.get(0)?).unwrap_or(CardState::New),
                step: row.get::<_, i64>(1)?.max(0) as u32,
                due_day: row.get(2)?,
                due_ms: row.get(3)?,
                memory: Memory {
                    stability: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0) as f32,
                    difficulty: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0) as f32,
                },
                last_day: row.get(6)?,
                last_event: row.get(7)?,
                answers: row.get::<_, i64>(8)?.max(0) as u32,
                lapses: row.get::<_, i64>(9)?.max(0) as u32,
            })
        },
    )
    .optional()
}

pub(crate) fn write_cached(conn: &Connection, card: Id, cached: &Cached) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO card_schedule
            (card, state, step, due_day, due_ms, stability, difficulty, last_day, last_event,
             answers, lapses)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            card,
            cached.state.code(),
            cached.step,
            cached.due_day,
            cached.due_ms,
            f64::from(cached.memory.stability),
            f64::from(cached.memory.difficulty),
            cached.last_day,
            cached.last_event,
            cached.answers,
            cached.lapses,
        ],
    )?;
    Ok(())
}

pub(crate) fn clear_cached(conn: &Connection, card: Id) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM card_schedule WHERE card = ?1", [card])?;
    Ok(())
}

/// A card's schedule, for screens and tools.
#[derive(Debug, Clone, PartialEq)]
pub struct CardSchedule {
    pub card: Id,
    /// `New` for a card that was never answered (or whose answers were all undone).
    pub state: CardState,
    /// The learning or relearning step, 0 for the first.
    pub step: u32,
    /// For a review card: the study day it is due.
    pub due_day: Option<i64>,
    /// For a learning or relearning card: when it is due, in Unix milliseconds.
    pub due_ms: Option<i64>,
    pub memory: Option<Memory>,
    /// The study day of the last answer.
    pub last_day: Option<i64>,
    /// Answers that count as reviews (not voided).
    pub answers: u32,
    pub lapses: u32,
}

impl CardSchedule {
    fn new_card(card: Id) -> Self {
        Self {
            card,
            state: CardState::New,
            step: 0,
            due_day: None,
            due_ms: None,
            memory: None,
            last_day: None,
            answers: 0,
            lapses: 0,
        }
    }
}

/// The value for a column, or null.
pub(crate) fn opt<T: Into<Value>>(value: Option<T>) -> Value {
    value.map_or(Value::Null, Into::into)
}

impl Collection {
    /// A card's schedule. A card with no answers reads as new.
    pub fn card_schedule(&self, card: Id) -> Result<CardSchedule, StudyError> {
        Ok(match read_cached(&self.conn, card)? {
            None => CardSchedule::new_card(card),
            Some(c) => CardSchedule {
                card,
                state: c.state,
                step: c.step,
                due_day: c.due_day,
                due_ms: c.due_ms,
                memory: Some(c.memory),
                last_day: Some(c.last_day),
                answers: c.answers,
                lapses: c.lapses,
            },
        })
    }
}
