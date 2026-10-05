//! Why answering, undoing or a study setting was refused. `Display` says what went wrong and what
//! to do.

use std::fmt;

use crate::collection::CollectionError;
use crate::deck::DeckError;
use crate::scheduling::SchedulingError;

#[derive(Debug, PartialEq, Eq)]
pub enum StudyError {
    /// No card with that ID, or it is deleted.
    NotFound,
    /// The day can start at a whole hour from 0 to 23.
    StartHour,
    /// Undo has nothing to undo: no answer was made on this device yet, or every one is undone.
    NothingToUndo,
    /// The card was answered again, here or on another device, after the answer to undo. Undoing
    /// the older answer would leave the newer one built on something that is gone.
    LaterAnswer,
    /// The scheduler refused the options (a desired retention outside its range). The text is for
    /// the log.
    Scheduling(String),
    Collection(CollectionError),
}

impl fmt::Display for StudyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That card no longer exists."),
            Self::StartHour => f.write_str(
                "The day can start at a whole hour from 0 (midnight) to 23, for example 4 for 4 am.",
            ),
            Self::NothingToUndo => f.write_str("There is no answer to undo."),
            Self::LaterAnswer => f.write_str(
                "That card was answered again after this answer, so it cannot be undone. \
                 Answer the card again instead.",
            ),
            Self::Scheduling(_) => f.write_str(
                "The card could not be scheduled with these deck options. Check the desired \
                 retention and try again.",
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
