//! Why a search or a saved search was refused. `Display` says what went wrong and what to do.

use std::fmt;

use crate::collection::CollectionError;

#[derive(Debug, PartialEq, Eq)]
pub enum SearchError {
    /// The query could not be read. `position` is the character where the problem is, counted from
    /// 0, so a screen can point at it.
    Syntax {
        position: usize,
        problem: String,
    },
    /// A word followed by a colon is the name of a field, and no note type has a field called
    /// that. `suggestion` is the filter it looks like a misspelling of.
    UnknownField {
        name: String,
        suggestion: Option<&'static str>,
    },
    /// No saved search with that ID, or it was deleted.
    NotFound,
    /// A saved search needs a name.
    EmptyName,
    /// Another saved search has this name (ignoring case).
    NameTaken(String),
    /// The stored sort or the one given is not a sort. Holds the text.
    UnknownSort(String),
    Collection(CollectionError),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { position, problem } => {
                write!(f, "{problem} (at character {})", position + 1)
            }
            Self::UnknownField { name, suggestion } => {
                write!(f, "No note type has a field called \"{name}\".")?;
                if let Some(suggestion) = suggestion {
                    write!(f, " Did you mean the filter \"{suggestion}:\"?")?;
                }
                f.write_str(
                    " To look for the text itself, put it in quotes, for example \"name:text\".",
                )
            }
            Self::NotFound => f.write_str("That saved search no longer exists."),
            Self::EmptyName => f.write_str("The saved search has no name. Type a name."),
            Self::NameTaken(name) => write!(
                f,
                "There is already a saved search called \"{name}\". Choose another name."
            ),
            Self::UnknownSort(text) => write!(
                f,
                "\"{text}\" is not a way to sort. Use one of: {}, optionally followed by \
                 :asc or :desc.",
                super::SortKey::ALL
                    .iter()
                    .map(|key| key.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<CollectionError> for SearchError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for SearchError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}
