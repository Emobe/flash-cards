//! Why a media call was refused. `Display` says what went wrong and what to do.

use std::fmt;

use crate::collection::CollectionError;

#[derive(Debug, PartialEq, Eq)]
pub enum MediaError {
    /// The file has no bytes.
    Empty,
    /// No file of the collection has this name.
    NotFound(String),
    /// The file is known but its bytes are not on this device (it has not been downloaded yet).
    NoBytes(String),
    /// A note, template or style still names the file.
    StillUsed(String),
    Collection(CollectionError),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("The file is empty, so there is nothing to add."),
            Self::NotFound(name) => write!(
                f,
                "No media file is called \"{name}\". Check the name, or add the file again."
            ),
            Self::NoBytes(name) => write!(
                f,
                "The media file \"{name}\" is known, but its contents are not on this device yet. \
                 Sync to download it."
            ),
            Self::StillUsed(name) => write!(
                f,
                "The media file \"{name}\" is still used by a note, a template or a style, so it \
                 was not deleted. Remove it from there first."
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for MediaError {}

impl From<CollectionError> for MediaError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for MediaError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}
