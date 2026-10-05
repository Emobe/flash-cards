//! Why a tag change was refused. `Display` says what went wrong and what to do.

use std::fmt;

use crate::collection::CollectionError;

use super::SEPARATOR;

#[derive(Debug, PartialEq, Eq)]
pub enum TagError {
    /// A note is missing or deleted.
    NotFound,
    /// No live note has this tag.
    NoSuchTag(String),
    /// The name is empty after trimming.
    Empty,
    /// The name has a space or other whitespace in it.
    Whitespace(String),
    /// A part of the name, between two separators or at an end, is empty.
    EmptyPart(String),
    Collection(CollectionError),
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That note no longer exists."),
            Self::NoSuchTag(name) => {
                write!(f, "No note has the tag \"{name}\". Check the spelling.")
            }
            Self::Empty => f.write_str("The tag is empty. Type a tag."),
            Self::Whitespace(name) => write!(
                f,
                "The tag \"{name}\" has a space in it. Tags cannot contain spaces. Use a dash or \
                 an underscore, or \"{SEPARATOR}\" to put a tag inside another."
            ),
            Self::EmptyPart(name) => write!(
                f,
                "The tag \"{name}\" has an empty part. Put a name on both sides of every \
                 \"{SEPARATOR}\", for example lang{SEPARATOR}polish."
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for TagError {}

impl From<CollectionError> for TagError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for TagError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}
