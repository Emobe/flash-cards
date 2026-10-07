//! Why a collection could not be created or opened. `Display` is a readable English sentence that
//! says what went wrong and what to do (`PRODUCT.md`), so every host can show it as it is.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionError {
    /// Nothing exists at the location, and the caller did not ask to create it.
    NotFound,
    /// The location already holds data, so creating a collection there would overwrite it.
    AlreadyExists,
    /// The file is not one of our collections (another SQLite database, or not a database).
    NotACollection,
    /// The collection was saved by a newer version of the app. It is left untouched.
    TooNew { found: u32, supported: u32 },
    /// Another window or program has the collection open.
    InUse,
    /// SQLite failed. The text is for the log, never for the user.
    Storage(String),
}

impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => {
                f.write_str("No collection exists at that location. Create one first.")
            }
            Self::AlreadyExists => {
                f.write_str("A collection already exists at that location. Open it instead.")
            }
            Self::NotACollection => {
                f.write_str("That file is not a flash card collection. Choose a different file.")
            }
            Self::TooNew { found, supported } => write!(
                f,
                "This collection was saved by a newer version of the app (storage version \
                 {found}, and this app understands up to {supported}). Update the app to open \
                 it. Nothing was changed."
            ),
            Self::InUse => f.write_str(
                "The collection is open in another window or program. Close it there and try again.",
            ),
            Self::Storage(_) => f.write_str(
                "The collection could not be read or written. Restart the app and try again.",
            ),
        }
    }
}

impl std::error::Error for CollectionError {}

impl From<rusqlite::Error> for CollectionError {
    fn from(error: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode;
        match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Self::InUse,
            Some(ErrorCode::NotADatabase) => Self::NotACollection,
            _ => Self::Storage(error.to_string()),
        }
    }
}
