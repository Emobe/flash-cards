//! Why a backup could not be written or read. `Display` says what went wrong and what to do.

use std::fmt;

use crate::collection::CollectionError;
use crate::media::MediaError;

#[derive(Debug, PartialEq, Eq)]
pub enum BackupError {
    /// Not a backup file, or one cut short. Nothing was changed.
    NotABackup,
    /// Made by a newer version of the app. Nothing was changed.
    TooNew {
        found: u32,
        supported: u32,
    },
    /// A backup whose content does not hold together (a line that is not valid, media that does
    /// not match its hash). Nothing was changed. The text is for the log.
    Damaged(String),
    /// The file is one deck, and restoring would delete the rest of the collection.
    IsADeck,
    /// The deck to export is not in the collection.
    NoSuchDeck,
    /// Reading or writing the file failed. The text is for the log.
    Io(String),
    Collection(CollectionError),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotABackup => f.write_str(
                "That file is not a flash card backup, or it was cut short. Nothing was changed. \
                 Choose a different file.",
            ),
            Self::TooNew { found, supported } => write!(
                f,
                "This backup was made by a newer version of the app (backup format {found}, and \
                 this app reads up to {supported}). Update the app to read it. Nothing was changed."
            ),
            Self::Damaged(_) => f.write_str(
                "This backup is damaged and cannot be read. Nothing was changed. Try an older \
                 backup.",
            ),
            Self::IsADeck => f.write_str(
                "This file holds one deck, not a whole collection. Restoring it would remove \
                 everything else, so import it instead.",
            ),
            Self::NoSuchDeck => f.write_str("That deck is not in the collection."),
            Self::Io(_) => f.write_str(
                "The backup file could not be read or written. Check that there is space and that \
                 the file is still there, then try again.",
            ),
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BackupError {}

impl From<CollectionError> for BackupError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for BackupError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}

impl From<MediaError> for BackupError {
    fn from(error: MediaError) -> Self {
        match error {
            MediaError::Collection(error) => Self::Collection(error),
            other => Self::Damaged(other.to_string()),
        }
    }
}

impl From<std::io::Error> for BackupError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<zip::result::ZipError> for BackupError {
    fn from(error: zip::result::ZipError) -> Self {
        use zip::result::ZipError;
        match error {
            ZipError::Io(error) => Self::Io(error.to_string()),
            // Not a zip, cut short, or an entry that does not match its checksum.
            other => Self::Damaged(other.to_string()),
        }
    }
}
