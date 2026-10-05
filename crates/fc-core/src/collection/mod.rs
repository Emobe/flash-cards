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

use crate::clock::Host;
use crate::id::Id;
use crate::sync::{SYNCED_TABLES, SyncedTable, state};

pub use error::CollectionError;
pub(crate) use migrate::{MIGRATIONS, Migration};
use migrate::{State, latest, read_state, run};

/// What a build knows about its collections: the migrations that bring one up to date, and the
/// synced tables the newest schema has. Tests use smaller schemas.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Schema {
    pub migrations: &'static [Migration],
    pub tables: &'static [SyncedTable],
}

const CURRENT: Schema = Schema {
    migrations: MIGRATIONS,
    tables: SYNCED_TABLES,
};

/// Facts about an open collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionInfo {
    /// Schema version of the collection (the app migrates it to the newest on open).
    pub schema_version: u32,
    /// Newest schema version this build understands.
    pub supported_schema_version: u32,
    /// Core version that created the collection.
    pub created_by: String,
    /// This copy's device ID (ADR 0006, section 1).
    pub device_id: Id,
    /// Features the collection needs that this build does not know. While there are any, sync
    /// stays paused and local study carries on (ADR 0006, section 10).
    pub unsupported_features: Vec<String>,
}

/// An open collection. Dropping it closes it, but `close` reports a failure to do so.
#[derive(Debug)]
pub struct Collection {
    pub(crate) conn: Connection,
    schema_version: u32,
    pub(crate) host: Host,
    pub(crate) schema: Schema,
}

impl Collection {
    /// Creates a new collection. Fails if the location already holds data.
    pub fn create(location: &str, host: Host) -> Result<Self, CollectionError> {
        Self::create_with(location, CURRENT, host)
    }

    /// Opens an existing collection and migrates it to the newest schema. Fails if there is none.
    /// If it was last opened by another installation (a copied file), it gets a new device ID.
    pub fn open(location: &str, host: Host) -> Result<Self, CollectionError> {
        Self::open_with(location, CURRENT, host)
    }

    /// Opens the collection at `location`, creating it if nothing is there yet.
    pub fn open_or_create(location: &str, host: Host) -> Result<Self, CollectionError> {
        match Self::open(location, host.clone()) {
            Err(CollectionError::NotFound) => Self::create(location, host),
            other => other,
        }
    }

    pub(crate) fn create_with(
        location: &str,
        schema: Schema,
        host: Host,
    ) -> Result<Self, CollectionError> {
        let mut conn = connect(location, true)?;
        match read_state(&conn)? {
            State::Empty => {}
            State::Collection(_) => return Err(CollectionError::AlreadyExists),
        }
        run(&mut conn, schema.migrations, &State::Empty)?;
        Self::finish(conn, schema, host)
    }

    pub(crate) fn open_with(
        location: &str,
        schema: Schema,
        host: Host,
    ) -> Result<Self, CollectionError> {
        let mut conn = connect(location, false)?;
        let opened = read_state(&conn)?;
        if matches!(opened, State::Empty) {
            return Err(CollectionError::NotFound);
        }
        run(&mut conn, schema.migrations, &opened)?;
        Self::finish(conn, schema, host)
    }

    fn finish(mut conn: Connection, schema: Schema, host: Host) -> Result<Self, CollectionError> {
        state::identify(&mut conn, &host)?;
        Ok(Self {
            conn,
            schema_version: latest(schema.migrations),
            host,
            schema,
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
            supported_schema_version: latest(self.schema.migrations),
            created_by,
            device_id: self.device_id()?,
            unsupported_features: self.unsupported_features()?,
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
