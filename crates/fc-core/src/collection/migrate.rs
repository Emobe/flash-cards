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
use crate::sync::{AppendOnlyTable, SyncedTable, install_append_only_guard, install_guard};

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
    Migration {
        version: 3,
        apply: v3,
    },
    Migration {
        version: 4,
        apply: v4,
    },
    Migration {
        version: 5,
        apply: v5,
    },
    Migration {
        version: 6,
        apply: v6,
    },
    Migration {
        version: 7,
        apply: v7,
    },
    Migration {
        version: 8,
        apply: v8,
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

/// Version 3 (step 1.2): note types, their fields and their templates, and the built-in note types
/// (Basic, Basic and reversed, Cloze) with fixed IDs. See `crate::notetype`.
///
/// Every column has a default, so a row that arrives with only some of its registers is harmless
/// (ADR 0006, section 6). Rows point at their note type by ID with no foreign key.
fn v3(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE note_type (
            id BLOB PRIMARY KEY NOT NULL,
            name TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL DEFAULT 'standard',
            css TEXT NOT NULL DEFAULT '',
            sort_field BLOB,
            deleted INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE TABLE note_type_field (
            id BLOB PRIMARY KEY NOT NULL,
            note_type BLOB NOT NULL DEFAULT x'',
            name TEXT NOT NULL DEFAULT '',
            position TEXT NOT NULL DEFAULT '',
            deleted INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE INDEX note_type_field_by_note_type ON note_type_field (note_type);
        CREATE TABLE template (
            id BLOB PRIMARY KEY NOT NULL,
            note_type BLOB NOT NULL DEFAULT x'',
            name TEXT NOT NULL DEFAULT '',
            position TEXT NOT NULL DEFAULT '',
            front TEXT NOT NULL DEFAULT '',
            back TEXT NOT NULL DEFAULT '',
            deleted INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE INDEX template_by_note_type ON template (note_type);",
    )?;
    // Frozen copies, as in `v2`: the guard only needs the names.
    for (entity, registers) in [
        (
            "note_type",
            &["name", "kind", "css", "sort_field", "deleted"][..],
        ),
        (
            "note_type_field",
            &["note_type", "name", "position", "deleted"][..],
        ),
        (
            "template",
            &["note_type", "name", "position", "front", "back", "deleted"][..],
        ),
    ] {
        install_guard(
            tx,
            &SyncedTable {
                entity,
                table: entity,
                registers,
            },
        )?;
    }
    crate::notetype::builtin::seed(tx)
}

/// Version 4 (step 1.3): notes, their field values and cards. See `crate::note`.
///
/// A note's values are one row per `(note, field)`, with no `id`: the register is named by the field
/// ID (a dynamic table). Every column has a default and nothing has a foreign key, so a card can
/// arrive before its note (ADR 0006, section 6). A card's deck, suspension and scheduling are later
/// registers, added by the steps that need them.
fn v4(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE note (
            id BLOB PRIMARY KEY NOT NULL,
            note_type BLOB NOT NULL DEFAULT x'',
            deleted INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE INDEX note_by_note_type ON note (note_type);
        CREATE TABLE note_field_value (
            note BLOB NOT NULL,
            field BLOB NOT NULL,
            value TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (note, field)
        ) WITHOUT ROWID;
        CREATE TABLE card (
            id BLOB PRIMARY KEY NOT NULL,
            note BLOB NOT NULL DEFAULT x'',
            template BLOB NOT NULL DEFAULT x'',
            ordinal INTEGER NOT NULL DEFAULT 0,
            deleted INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE INDEX card_by_note ON card (note);",
    )?;
    // Frozen copies, as in `v2`: the guard only needs the names.
    for name in ["note", "note_field_value", "card"] {
        install_guard(
            tx,
            &SyncedTable {
                entity: name,
                table: name,
                registers: &[],
            },
        )?;
    }
    Ok(())
}

/// Version 5 (step 1.5): decks, option presets, and a `deck` register on cards. See `crate::deck`.
///
/// A card's `deck` is empty for the cards that exist already, which read as being in the Default
/// deck, so no card is rewritten. The Default deck and preset are seeded with fixed IDs and the
/// lowest clock. Every column has a default and nothing has a foreign key (ADR 0006, section 6).
fn v5(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE deck (
            id BLOB PRIMARY KEY NOT NULL,
            name TEXT NOT NULL DEFAULT '',
            parent BLOB NOT NULL DEFAULT x'',
            options_preset BLOB NOT NULL DEFAULT x'',
            deleted INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE TABLE options_preset (
            id BLOB PRIMARY KEY NOT NULL,
            name TEXT NOT NULL DEFAULT '',
            new_per_day INTEGER NOT NULL DEFAULT 20,
            reviews_per_day INTEGER NOT NULL DEFAULT 200,
            learning_steps TEXT NOT NULL DEFAULT '1 10',
            desired_retention REAL NOT NULL DEFAULT 0.9,
            deleted INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        ALTER TABLE card ADD COLUMN deck BLOB NOT NULL DEFAULT x'';
        CREATE INDEX card_by_deck ON card (deck);",
    )?;
    // Frozen copies, as in `v2`: the guard only needs the names.
    for name in ["deck", "options_preset"] {
        install_guard(
            tx,
            &SyncedTable {
                entity: name,
                table: name,
                registers: &[],
            },
        )?;
    }
    crate::deck::seed(tx)
}

/// Version 6 (step 1.6): note tags. See `crate::tag`.
///
/// One row per `(note, tag)`, where `tag` is the text as written and `present` is `1` or empty, so
/// removing a tag is a write and rows are never deleted. Nothing has a foreign key (ADR 0006,
/// section 6) and no existing row is rewritten.
fn v6(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE note_tag (
            note BLOB NOT NULL,
            tag TEXT NOT NULL,
            present TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (note, tag)
        ) WITHOUT ROWID;
        CREATE INDEX note_tag_by_tag ON note_tag (tag);",
    )?;
    // A frozen copy, as in `v2`: the guard only needs the name.
    install_guard(
        tx,
        &SyncedTable {
            entity: "note_tag",
            table: "note_tag",
            registers: &[],
        },
    )
}

/// Version 7 (step 1.7a): answering. See `crate::study`.
///
/// - `options_preset` gets `relearning_steps` and `fsrs_parameters`. The Default preset keeps the
///   column defaults (steps `10`, the default parameters), the same on every device, so it needs no
///   clock rows;
/// - `collection_setting`: synced settings, the first being the hour the study day starts;
/// - `card_event` and `fsrs_parameter_set`: append-only synced tables, which the database refuses to
///   change or delete;
/// - `card_schedule`: the local cache folded from card events, and `unpushed_row`, the local list of
///   append-only rows a sync has not pushed. A card with no `card_schedule` row is new.
///
/// Nothing has a foreign key and no existing row is rewritten.
fn v7(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "ALTER TABLE options_preset ADD COLUMN relearning_steps TEXT NOT NULL DEFAULT '10';
        ALTER TABLE options_preset ADD COLUMN fsrs_parameters TEXT NOT NULL DEFAULT '';
        CREATE TABLE collection_setting (
            id BLOB PRIMARY KEY NOT NULL,
            key TEXT NOT NULL DEFAULT '',
            value TEXT NOT NULL DEFAULT ''
        ) WITHOUT ROWID;
        CREATE TABLE fsrs_parameter_set (
            id BLOB PRIMARY KEY NOT NULL,
            values_f32 BLOB NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE card_event (
            id BLOB PRIMARY KEY NOT NULL,
            card BLOB NOT NULL,
            kind TEXT NOT NULL,
            time_ms INTEGER NOT NULL,
            utc_offset INTEGER NOT NULL,
            device BLOB NOT NULL,
            previous BLOB,
            day INTEGER NOT NULL,
            rating INTEGER,
            duration_ms INTEGER,
            preset BLOB,
            desired_retention REAL,
            parameters BLOB,
            steps TEXT,
            state_before INTEGER,
            state INTEGER,
            step INTEGER,
            stability REAL,
            difficulty REAL,
            due_day INTEGER,
            due_ms INTEGER,
            target BLOB
        ) WITHOUT ROWID;
        CREATE INDEX card_event_by_card ON card_event (card, time_ms);
        CREATE INDEX card_event_by_time ON card_event (time_ms);
        CREATE INDEX card_event_by_day ON card_event (day);
        CREATE INDEX card_event_by_target ON card_event (target) WHERE target IS NOT NULL;
        CREATE TABLE card_schedule (
            card BLOB PRIMARY KEY NOT NULL,
            state INTEGER NOT NULL,
            step INTEGER NOT NULL DEFAULT 0,
            due_day INTEGER,
            due_ms INTEGER,
            stability REAL,
            difficulty REAL,
            last_day INTEGER NOT NULL,
            last_event BLOB NOT NULL,
            answers INTEGER NOT NULL DEFAULT 0,
            lapses INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        CREATE INDEX card_schedule_by_due_day ON card_schedule (state, due_day);
        CREATE INDEX card_schedule_by_due_ms ON card_schedule (due_ms);
        CREATE TABLE unpushed_row (
            entity_type TEXT NOT NULL,
            row_id BLOB NOT NULL,
            PRIMARY KEY (entity_type, row_id)
        ) WITHOUT ROWID;",
    )?;
    // The Default preset's new registers hold the column defaults on every device, so they get the
    // lowest clock, already pushed, like the rest of the seed (`seed_row`).
    for field in ["relearning_steps", "fsrs_parameters"] {
        tx.execute(
            "INSERT INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES ('options_preset', ?1, ?2, 0, ?3, 1)",
            rusqlite::params![
                crate::deck::default_preset(),
                field,
                crate::id::Id::from_bytes([0; 16])
            ],
        )?;
    }
    // Frozen copies, as in `v2`: the guard only needs the names.
    install_guard(
        tx,
        &SyncedTable {
            entity: "collection_setting",
            table: "collection_setting",
            registers: &["key", "value"],
        },
    )?;
    for name in ["card_event", "fsrs_parameter_set"] {
        install_append_only_guard(
            tx,
            &AppendOnlyTable {
                entity: name,
                table: name,
                columns: &[],
            },
        )?;
    }
    Ok(())
}

/// Version 8 (step 1.7b): queues. See `crate::study::queue`.
///
/// New registers, all with column defaults, so no existing row is rewritten: `card.suspended` and
/// `card.buried_until` (a study day, 0 for none), `deck.limits_include_subdecks` (on) and
/// `options_preset.space_siblings` (on). The seeded Default deck and preset get the lowest clock for
/// the new registers, as `seed_row` gives the rest of them.
fn v8(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "ALTER TABLE card ADD COLUMN suspended INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE card ADD COLUMN buried_until INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE deck ADD COLUMN limits_include_subdecks INTEGER NOT NULL DEFAULT 1;
        ALTER TABLE options_preset ADD COLUMN space_siblings INTEGER NOT NULL DEFAULT 1;",
    )?;
    for (entity, id, field) in [
        (
            "deck",
            crate::deck::default_deck(),
            "limits_include_subdecks",
        ),
        (
            "options_preset",
            crate::deck::default_preset(),
            "space_siblings",
        ),
    ] {
        tx.execute(
            "INSERT INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES (?1, ?2, ?3, 0, ?4, 1)",
            rusqlite::params![entity, id, field, crate::id::Id::from_bytes([0; 16])],
        )?;
    }
    Ok(())
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
