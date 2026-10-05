//! A collection: one SQLite database holding everything a user studies. Hosts create or open it
//! at a path or `file:` URI (a plain path on native, an OPFS URI on the web, see ADR 0003), so this
//! module never touches the file system itself.
//!
//! Schema changes go through `migrate`. A collection saved by a newer version of the app is
//! refused without being modified.

mod error;
mod migrate;
#[cfg(test)]
mod tests;

use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

pub use error::CollectionError;
use migrate::{MIGRATIONS, Migration, State, latest, read_state, run};

/// Facts about an open collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionInfo {
    /// Schema version of the collection (the app migrates it to the newest on open).
    pub schema_version: u32,
    /// Newest schema version this build understands.
    pub supported_schema_version: u32,
    /// Core version that created the collection.
    pub created_by: String,
}

/// An open collection. Dropping it closes it, but `close` reports a failure to do so.
#[derive(Debug)]
pub struct Collection {
    conn: Connection,
    schema_version: u32,
}

impl Collection {
    /// Creates a new collection. Fails if the location already holds data.
    pub fn create(location: &str) -> Result<Self, CollectionError> {
        Self::create_with(location, MIGRATIONS)
    }

    /// Opens an existing collection and migrates it to the newest schema. Fails if there is none.
    pub fn open(location: &str) -> Result<Self, CollectionError> {
        Self::open_with(location, MIGRATIONS)
    }

    /// Opens the collection at `location`, creating it if nothing is there yet.
    pub fn open_or_create(location: &str) -> Result<Self, CollectionError> {
        match Self::open(location) {
            Err(CollectionError::NotFound) => Self::create(location),
            other => other,
        }
    }

    fn create_with(location: &str, migrations: &[Migration]) -> Result<Self, CollectionError> {
        let mut conn = connect(location, true)?;
        match read_state(&conn)? {
            State::Empty => {}
            State::Collection(_) => return Err(CollectionError::AlreadyExists),
        }
        run(&mut conn, migrations, &State::Empty)?;
        Ok(Self {
            conn,
            schema_version: latest(migrations),
        })
    }

    fn open_with(location: &str, migrations: &[Migration]) -> Result<Self, CollectionError> {
        let mut conn = connect(location, false)?;
        let state = read_state(&conn)?;
        if matches!(state, State::Empty) {
            return Err(CollectionError::NotFound);
        }
        run(&mut conn, migrations, &state)?;
        Ok(Self {
            conn,
            schema_version: latest(migrations),
        })
    }

    pub fn info(&self) -> Result<CollectionInfo, CollectionError> {
        let created_by = self.conn.query_row(
            "SELECT value FROM meta WHERE key = 'created_by'",
            [],
            |row| row.get(0),
        )?;
        Ok(CollectionInfo {
            schema_version: self.schema_version,
            supported_schema_version: latest(MIGRATIONS),
            created_by,
        })
    }

    /// Closes the collection, reporting a failure that dropping it would hide.
    pub fn close(self) -> Result<(), CollectionError> {
        self.conn.close().map_err(|(_, error)| error.into())
    }
}

/// Opens the SQLite file. Without `create`, a missing file is `NotFound` instead of a new file.
fn connect(location: &str, create: bool) -> Result<Connection, CollectionError> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = Connection::open_with_flags(location, flags).map_err(|error| {
        if !create && error.sqlite_error_code() == Some(rusqlite::ErrorCode::CannotOpen) {
            CollectionError::NotFound
        } else {
            error.into()
        }
    })?;
    // Another program holding the file is reported at once as `InUse`, not waited for. Foreign
    // keys stay off: synced tables never enforce them (ADR 0006, section 6).
    conn.busy_timeout(Duration::ZERO)?;
    conn.pragma_update(None, "foreign_keys", false)?;
    Ok(conn)
}
