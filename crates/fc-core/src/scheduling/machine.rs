//! What a card does when it is answered (ADR 0007, part 1): the learning steps decide when it comes
//! back while it is learning or relearning, and FSRS sets the interval when it graduates or is
//! reviewed. FSRS sees every answer, including the ones inside a day, so the memory state is always
//! the one the optimiser will see in the history (finding 2 of the ADR).
//!
//! | State | Again | Hard | Good | Easy |
//! | --- | --- | --- | --- | --- |
//! | New or learning (step *i*) | first step | repeat step *i* | step *i*+1, or graduate after the last | graduate |
//! | Review | lapse: first relearning step | FSRS interval | FSRS interval | FSRS interval |
//! | Relearning (step *i*) | first relearning step | repeat step *i* | step *i*+1, or graduate after the last | graduate |
//!
//! Everything here is a pure function of its inputs, so an event can be recomputed on any device.

use super::{Memory, Rating, Reviewed, Scheduler, SchedulingError};
use crate::id::Id;

/// The longest interval, in days. A constant for now (ADR 0007, part 1).
pub const MAX_INTERVAL_DAYS: u32 = 36_500;

/// Where a card is in its life. The numbers are stored in card events and the schedule cache, and
/// never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardState {
    New,
    Learning,
    Review,
    Relearning,
}

impl CardState {
    pub fn code(self) -> i64 {
        match self {
            Self::New => 0,
            Self::Learning => 1,
            Self::Review => 2,
            Self::Relearning => 3,
        }
    }

    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => Self::New,
            1 => Self::Learning,
            2 => Self::Review,
            3 => Self::Relearning,
            _ => return None,
        })
    }
}

/// What the scheduler needs to know about a card before an answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardNow {
    pub state: CardState,
    /// The learning or relearning step the card is on (0 for the first).
    pub step: u32,
    /// `None` until the card has been answered.
    pub memory: Option<Memory>,
}

impl CardNow {
    pub const NEW: Self = Self {
        state: CardState::New,
        step: 0,
        memory: None,
    };
}

/// When a card comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// This many minutes from now (a learning or relearning step).
    Minutes(u32),
    /// This many study days after the day of the answer.
    Days(u32),
}

/// The card after an answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    pub state: CardState,
    pub step: u32,
    pub memory: Memory,
    pub due: Due,
}

/// The preset's steps, in minutes.
#[derive(Debug, Clone, Copy)]
pub struct Steps<'a> {
    pub learning: &'a [u32],
    pub relearning: &'a [u32],
}

/// The days a card is put off by, after the fuzz. With no `fuzz_seed` the interval is the one FSRS
/// gave (the answer buttons show these before the event, and its ID, exist).
fn graduate_days(interval_days: u32, fuzz_seed: Option<Id>) -> u32 {
    let interval = interval_days.clamp(1, MAX_INTERVAL_DAYS);
    match fuzz_seed {
        Some(id) => fuzz(interval, id),
        None => interval,
    }
}

/// Spreads an interval of whole days a little, so cards learned together do not all come back
/// together (ADR 0007, part 7). Under 3 days nothing changes. Otherwise a whole number of days
/// chosen evenly from `interval ± max(1, round(5%))`, never below 2 or above the maximum. The
/// choice comes from the 8 random bytes at the end of the event's UUIDv7, so recomputing the event
/// gives the same day.
pub fn fuzz(interval_days: u32, event_id: Id) -> u32 {
    if interval_days < 3 {
        return interval_days;
    }
    let range = ((f64::from(interval_days) * 0.05).round() as u32).max(1);
    let mut tail = [0u8; 8];
    tail.copy_from_slice(&event_id.as_bytes()[8..]);
    let pick = u64::from_be_bytes(tail) % u64::from(2 * range + 1);
    let days = u64::from(interval_days - range) + pick;
    days.clamp(2, u64::from(MAX_INTERVAL_DAYS)) as u32
}

fn outcome(
    card: CardNow,
    rating: Rating,
    reviewed: &[Reviewed; 4],
    steps: Steps<'_>,
    fuzz_seed: Option<Id>,
) -> Outcome {
    let result = reviewed[rating as usize];
    let review = |days: u32| Outcome {
        state: CardState::Review,
        step: 0,
        memory: result.memory,
        due: Due::Days(graduate_days(days, fuzz_seed)),
    };
    let list = match card.state {
        CardState::Review => {
            // Only a lapse uses steps, and only if the preset has some.
            return match steps.relearning.first() {
                Some(minutes) if rating == Rating::Again => Outcome {
                    state: CardState::Relearning,
                    step: 0,
                    memory: result.memory,
                    due: Due::Minutes(*minutes),
                },
                _ => review(result.interval_days),
            };
        }
        CardState::Relearning => steps.relearning,
        CardState::New | CardState::Learning => steps.learning,
    };
    if list.is_empty() {
        return review(result.interval_days);
    }
    // A card on a step the preset no longer has is on the last one.
    let current = (card.step as usize).min(list.len() - 1);
    let next = match rating {
        Rating::Again => Some(0),
        Rating::Hard => Some(current),
        Rating::Good => (current + 1 < list.len()).then_some(current + 1),
        Rating::Easy => None,
    };
    match next {
        Some(step) => Outcome {
            state: if card.state == CardState::Relearning {
                CardState::Relearning
            } else {
                CardState::Learning
            },
            step: step as u32,
            memory: result.memory,
            due: Due::Minutes(list[step]),
        },
        None => review(result.interval_days),
    }
}

/// The card after `rating`. `elapsed_days` is the study days since its last answer (0 for a new
/// card or the same day). `fuzz_seed` is the ID of the event that records the answer.
pub fn answer(
    card: CardNow,
    elapsed_days: u32,
    rating: Rating,
    steps: Steps<'_>,
    scheduler: &Scheduler,
    fuzz_seed: Id,
) -> Result<Outcome, SchedulingError> {
    let reviewed = scheduler.next(card.memory, elapsed_days)?;
    Ok(outcome(card, rating, &reviewed, steps, Some(fuzz_seed)))
}

/// The four outcomes (again, hard, good, easy) for the answer buttons, without fuzz.
pub fn preview(
    card: CardNow,
    elapsed_days: u32,
    steps: Steps<'_>,
    scheduler: &Scheduler,
) -> Result<[Outcome; 4], SchedulingError> {
    let reviewed = scheduler.next(card.memory, elapsed_days)?;
    Ok([Rating::Again, Rating::Hard, Rating::Good, Rating::Easy]
        .map(|rating| outcome(card, rating, &reviewed, steps, None)))
}

#[cfg(test)]
mod tests;
