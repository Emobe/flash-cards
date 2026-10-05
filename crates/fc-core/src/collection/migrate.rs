//! Schema migrations. The schema version lives in SQLite's `user_version`, and `application_id`
//! marks the file as ours. See `docs/adr/0003-web-client.md` (build notes, step 1.1a).
//!
//! # Adding a migration
//!
//! Append a `Migration` to `MIGRATIONS` with the next version number, and never edit or reorder an
//! earlier one: collections in the wild have already run it. A migration is a plain function that
//! gets the open transaction, so it can run SQL or Rust (for example to rewrite data). Every
//! pending migration runs in one transaction together with the version bump, so a failure leaves
//! the collection exactly as it was. A migration that creates a synced table calls `install_guard`
//! (see `crate::sync`), and one that rewrites synced rows needs a write path with clocks (add one
//! when the first such migration is written). Migrations are local to the device: they never change the sync
//! format (ADR 0006, section 10).

use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::error::CollectionError;
use crate::sync::{SyncedTable, install_guard};

/// Marks a database as one of ours ("FCCL").
pub(super) const APPLICATION_ID: i32 = 0x4643_4343;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Migration {
    /// Version the collection has after this migration. Versions start at 1 and have no gaps.
    pub(crate) version: u32,
    pub(crate) apply: fn(&Transaction) -> rusqlite::Result<()>,
}

pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        apply: v1,
    },
    Migration {
        version: 2,
        apply: v2,
    },
];

/// The newest version in `migrations`.
pub(super) fn latest(migrations: &[Migration]) -> u32 {
    migrations.last().map_or(0, |m| m.version)
}

/// Version 1: a local key/value table (never synced) for facts about the collection itself.
fn v1(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE meta (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        ) WITHOUT ROWID",
    )?;
    tx.execute(
        "INSERT INTO meta (key, value) VALUES ('created_by', ?1)",
        [crate::version()],
    )?;
    Ok(())
}

/// Version 2 (step 1.1b), the sync foundation (ADR 0006, section 13):
///
/// - `register_clock`: the `(hlc, device, pushed)` of every register of every synced table;
/// - `unknown_register`: registers of types or fields this build does not know, kept as received;
/// - `write_guard`: holds one row while a `WriteTx` is open, which the guard triggers check;
/// - `requirement`: the first synced table, for the collection-level `requires`.
///
/// Clock rows and the unknown store have no foreign keys: a register can arrive before its row
/// (ADR 0006, section 6). The saved HLC, device ID and installation ID go in `meta`.
fn v2(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE register_clock (
            entity_type TEXT NOT NULL,
            entity_id BLOB NOT NULL,
            field TEXT NOT NULL,
            hlc INTEGER NOT NULL,
            device BLOB NOT NULL,
            pushed INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (entity_type, entity_id, field)
        ) WITHOUT ROWID;
        CREATE INDEX register_clock_unpushed ON register_clock (hlc) WHERE pushed = 0;
        CREATE TABLE unknown_register (
            entity_type TEXT NOT NULL,
            entity_id BLOB NOT NULL,
            field TEXT NOT NULL,
            value,
            hlc INTEGER NOT NULL,
            device BLOB NOT NULL,
            PRIMARY KEY (entity_type, entity_id, field)
        ) WITHOUT ROWID;
        CREATE TABLE write_guard (id INTEGER PRIMARY KEY CHECK (id = 1));
        CREATE TABLE requirement (
            id BLOB PRIMARY KEY NOT NULL,
            feature TEXT NOT NULL DEFAULT '',
            active INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;",
    )?;
    // A frozen copy: the guard only needs the names, and a migration must not follow later edits
    // to the registry.
    install_guard(
        tx,
        &SyncedTable {
            entity: "requirement",
            table: "requirement",
            registers: &["feature", "active"],
        },
    )
}

/// What a database file is, before anything is written to it.
pub(super) enum State {
    /// No tables and no version: a new file.
    Empty,
    /// One of our collections, at this schema version.
    Collection(u32),
}

pub(super) fn read_state(conn: &Connection) -> Result<State, CollectionError> {
    let application_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if application_id == APPLICATION_ID {
        return Ok(State::Collection(version));
    }
    let tables: u32 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
    if application_id == 0 && version == 0 && tables == 0 {
        Ok(State::Empty)
    } else {
        Err(CollectionError::NotACollection)
    }
}

/// Brings a collection from `from` (0 for a new file) to the newest version, or does nothing.
/// Refuses a collection that is newer than `migrations` before writing anything.
pub(super) fn run(
    conn: &mut Connection,
    migrations: &[Migration],
    state: &State,
) -> Result<(), CollectionError> {
    let target = latest(migrations);
    let from = match state {
        State::Empty => 0,
        State::Collection(version) => *version,
    };
    if from > target {
        return Err(CollectionError::TooNew {
            found: from,
            supported: target,
        });
    }
    if from == target {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if matches!(state, State::Empty) {
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    }
    for migration in migrations.iter().filter(|m| m.version > from) {
        (migration.apply)(&tx)?;
    }
    tx.pragma_update(None, "user_version", target)?;
    tx.commit()?;
    Ok(())
}
