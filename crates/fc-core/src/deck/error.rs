//! Why a deck or preset change was refused. `Display` says what went wrong and what to do.

use std::fmt;

use crate::collection::CollectionError;

use super::SEPARATOR;

#[derive(Debug, PartialEq, Eq)]
pub enum DeckError {
    /// No deck, options preset or card with that ID, or it is deleted.
    NotFound,
    /// The name is empty after trimming.
    EmptyName,
    /// The name contains `::`, which separates a deck from its parent in a path.
    NameHasSeparator,
    /// Another live deck or preset next to it already has this name.
    NameTaken(String),
    /// A deck cannot go inside itself or one of its own sub-decks.
    MoveIntoItself,
    /// The Default deck and the Default preset cannot be deleted.
    Default,
    /// Desired retention must be between 0.70 and 0.99.
    Retention,
    /// A daily limit is more than 9999.
    Limit,
    /// Learning steps are at most 8 whole numbers of minutes, each 1 to 1440.
    Steps,
    /// FSRS parameters are 17, 19 or 21 finite numbers, or empty for the defaults.
    Parameters,
    Collection(CollectionError),
}

impl fmt::Display for DeckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That deck, options preset or card no longer exists."),
            Self::EmptyName => f.write_str("The name is empty. Type a name."),
            Self::NameHasSeparator => write!(
                f,
                "A deck name cannot contain \"{SEPARATOR}\", which marks a deck inside another. \
                 To put a deck inside another, move it."
            ),
            Self::NameTaken(name) => write!(
                f,
                "There is already one called \"{name}\" here. Choose a different name."
            ),
            Self::MoveIntoItself => f.write_str(
                "A deck cannot be moved inside itself or one of its own sub-decks. Choose a \
                 different deck.",
            ),
            Self::Default => f.write_str(
                "The Default deck and the Default options preset cannot be deleted. You can \
                 rename them or change their options.",
            ),
            Self::Retention => f.write_str(
                "Desired retention must be between 0.70 and 0.99, for example 0.90 to remember \
                 90% of reviews.",
            ),
            Self::Limit => f.write_str("A daily limit can be at most 9999."),
            Self::Steps => f.write_str(
                "Learning steps are whole numbers of minutes from 1 to 1440, at most 8 of them, \
                 for example 1 10.",
            ),
            Self::Parameters => f.write_str(
                "FSRS parameters are 21 numbers (17 or 19 from an older version also work). \
                 Leave them empty to use the defaults.",
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DeckError {}

impl From<CollectionError> for DeckError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for DeckError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}

impl From<DeckError> for CollectionError {
    /// For code that reads decks inside a collection operation. Reading only fails in the
    /// database, so the other kinds cannot happen there.
    fn from(error: DeckError) -> Self {
        match error {
            DeckError::Collection(error) => error,
            other => Self::Storage(other.to_string()),
        }
    }
}
