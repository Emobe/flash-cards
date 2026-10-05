//! Why a note type change was refused. `Display` says what went wrong and what to do.

use std::fmt;

use crate::collection::CollectionError;
use crate::template::TemplateProblem;

#[derive(Debug, PartialEq, Eq)]
pub enum NoteTypeError {
    /// No note type, field or template with that ID (or it belongs to another note type).
    NotFound,
    /// The name is empty after trimming.
    EmptyName,
    /// A live field or template of the same note type already has this name.
    NameTaken(String),
    /// A note type needs at least one field.
    LastField,
    /// A note type needs at least one template.
    LastTemplate,
    /// A cloze note type has exactly one template.
    ClozeTemplate,
    /// A side of a card template has a mistake. Nothing is saved.
    Template(TemplateProblem),
    Collection(CollectionError),
}

impl fmt::Display for NoteTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That note type, field or template no longer exists."),
            Self::EmptyName => f.write_str("The name is empty. Type a name."),
            Self::NameTaken(name) => write!(
                f,
                "There is already one called \"{name}\" in this note type. Choose a different name."
            ),
            Self::LastField => f.write_str(
                "A note type needs at least one field, so the last one cannot be removed. Add \
                 another field first.",
            ),
            Self::LastTemplate => f.write_str(
                "A note type needs at least one card template, so the last one cannot be removed. \
                 Add another template first.",
            ),
            Self::ClozeTemplate => f.write_str(
                "A cloze note type has exactly one card template. To use several, make a standard \
                 note type.",
            ),
            Self::Template(problem) => {
                problem.fmt(f)?;
                f.write_str(" Nothing was saved.")
            }
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for NoteTypeError {}

impl From<CollectionError> for NoteTypeError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for NoteTypeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}

impl From<NoteTypeError> for CollectionError {
    /// For code that reads note types inside a collection operation. Reading only fails in the
    /// database, so the other kinds cannot happen there.
    fn from(error: NoteTypeError) -> Self {
        match error {
            NoteTypeError::Collection(error) => error,
            other => Self::Storage(other.to_string()),
        }
    }
}
