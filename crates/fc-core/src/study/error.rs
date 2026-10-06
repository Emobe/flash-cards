//! Why answering, undoing or a study setting was refused. `Display` says what went wrong and what
//! to do.

use std::fmt;

use crate::collection::CollectionError;
use crate::deck::DeckError;
use crate::scheduling::SchedulingError;

#[derive(Debug, PartialEq, Eq)]
pub enum StudyError {
    /// No card (or, when starting a study session, deck) with that ID, or it is deleted.
    NotFound,
    /// The day can start at a whole hour from 0 to 23.
    StartHour,
    /// The card was answered again, here or on another device, after the answer to undo. Undoing
    /// the older answer would leave the newer one built on something that is gone.
    LaterAnswer,
    /// The scheduler refused the options (a desired retention outside its range). The text is for
    /// the log.
    Scheduling(String),
    /// Too few reviews to optimise the FSRS parameters. `items` is how many the history has that
    /// can be learned from (`TrainingSet::items`), `needed` how many it takes.
    NotEnoughHistory {
        items: u32,
        needed: u32,
    },
    Collection(CollectionError),
}

impl fmt::Display for StudyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That card or deck no longer exists."),
            Self::StartHour => f.write_str(
                "The day can start at a whole hour from 0 (midnight) to 23, for example 4 for 4 am.",
            ),
            Self::LaterAnswer => f.write_str(
                "That card was answered again after this answer, so it cannot be undone. \
                 Answer the card again instead.",
            ),
            Self::Scheduling(_) => f.write_str(
                "The card could not be scheduled with these deck options. Check the desired \
                 retention and try again.",
            ),
            Self::NotEnoughHistory { items, needed } => write!(
                f,
                "There is not enough review history to tune the scheduler yet: {items} of the \
                 {needed} reviews it needs. A review counts once its card is answered again on a \
                 later day. Keep studying and try again later."
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for StudyError {}

impl From<SchedulingError> for StudyError {
    fn from(error: SchedulingError) -> Self {
        Self::Scheduling(format!("{error:?}"))
    }
}

impl From<DeckError> for StudyError {
    fn from(error: DeckError) -> Self {
        match error {
            DeckError::NotFound => Self::NotFound,
            DeckError::Collection(error) => Self::Collection(error),
            other => Self::Collection(CollectionError::Storage(other.to_string())),
        }
    }
}

impl From<CollectionError> for StudyError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for StudyError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}

impl From<StudyError> for CollectionError {
    /// For code that reads study data inside a collection operation. Only the database can fail
    /// there, so the other kinds cannot happen.
    fn from(error: StudyError) -> Self {
        match error {
            StudyError::Collection(error) => error,
            other => Self::Storage(other.to_string()),
        }
    }
}
