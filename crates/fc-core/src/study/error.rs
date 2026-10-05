//! Why answering, undoing or a study setting was refused. `Display` says what went wrong and what
//! to do.

use std::fmt;

use crate::collection::CollectionError;

#[derive(Debug, PartialEq, Eq)]
pub enum StudyError {
    /// No card with that ID, or it is deleted.
    NotFound,
    /// The day can start at a whole hour from 0 to 23.
    StartHour,
    Collection(CollectionError),
}

impl fmt::Display for StudyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That card no longer exists."),
            Self::StartHour => f.write_str(
                "The day can start at a whole hour from 0 (midnight) to 23, for example 4 for 4 am.",
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for StudyError {}

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
