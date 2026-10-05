//! Why a note change was refused. `Display` says what went wrong and what to do.

use std::fmt;

use crate::collection::CollectionError;

#[derive(Debug, PartialEq, Eq)]
pub enum NoteError {
    /// No note (or note type) with that ID, or it is deleted.
    NotFound,
    /// A value was given for something that is not a live field of the note's note type.
    UnknownField,
    /// The note would make no cards. `cloze` says whether its note type is a cloze type.
    NoCards {
        cloze: bool,
    },
    Collection(CollectionError),
}

impl fmt::Display for NoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That note or note type no longer exists."),
            Self::UnknownField => f.write_str(
                "That field does not belong to this note type any more. Reload the note and try \
                 again.",
            ),
            Self::NoCards { cloze: true } => f.write_str(
                "This note would make no cards. Add a cloze deletion, for example {{c1::answer}}, \
                 to the text.",
            ),
            Self::NoCards { cloze: false } => f.write_str(
                "This note would make no cards. Fill in a field that the front of a card template \
                 uses.",
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for NoteError {}

impl From<CollectionError> for NoteError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for NoteError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}

impl From<crate::notetype::NoteTypeError> for NoteError {
    fn from(error: crate::notetype::NoteTypeError) -> Self {
        match error {
            crate::notetype::NoteTypeError::Collection(error) => Self::Collection(error),
            _ => Self::NotFound,
        }
    }
}

impl From<crate::deck::DeckError> for NoteError {
    fn from(error: crate::deck::DeckError) -> Self {
        match error {
            crate::deck::DeckError::Collection(error) => Self::Collection(error),
            _ => Self::NotFound,
        }
    }
}
