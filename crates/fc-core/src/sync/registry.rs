//! The list of synced tables, and the database-level guard that keeps writes on the write path.

use rusqlite::Transaction;

use super::requires;
use crate::deck;
use crate::note;
use crate::notetype;
use crate::study;
use crate::tag;

/// A table whose fields merge between devices, one register per field.
#[derive(Debug, Clone, Copy)]
pub struct SyncedTable {
    /// The entity type in sync data. Part of the sync format: never renamed (ADR 0006, section 10).
    pub entity: &'static str,
    /// The SQLite table. Its columns are `id` plus one per register.
    pub table: &'static str,
    /// Register names, which are also the column names. Part of the sync format.
    pub registers: &'static [&'static str],
}

/// Every synced table, in the newest schema.
pub const SYNCED_TABLES: &[SyncedTable] = &[
    requires::TABLE,
    notetype::NOTE_TYPE,
    notetype::FIELD,
    notetype::TEMPLATE,
    note::NOTE,
    note::CARD,
    deck::DECK,
    deck::PRESET,
    study::SETTING,
];

/// A table of registers whose names are not fixed: one row per `(owner, key)`, where the key is an
/// ID chosen by the data (a note's field values are keyed by field ID, ADR 0006, section 1). In sync
/// data the register's entity is `entity`, the entity ID is the owner and the register name is the
/// key as UUID text. Its columns are exactly `owner`, `key` and `value`, and it gets the same guard
/// triggers as a [`SyncedTable`]. Written only through [`WriteTx::set_value`](super::WriteTx).
#[derive(Debug, Clone, Copy)]
pub struct DynamicTable {
    /// The entity type in sync data. Its fixed registers are in a [`SyncedTable`] of the same name,
    /// unless `text_key` is set.
    pub entity: &'static str,
    pub table: &'static str,
    pub owner: &'static str,
    pub key: &'static str,
    pub value: &'static str,
    /// The key is text chosen by the person (a tag), not an ID. The register name in sync data is
    /// the text as written, and the entity has no fixed registers.
    pub text_key: bool,
}

/// Every dynamic table, in the newest schema.
pub const DYNAMIC_TABLES: &[DynamicTable] = &[note::NOTE_VALUE, tag::NOTE_TAG];

/// A table of immutable rows that sync by union (ADR 0006, section 3): card events, and the FSRS
/// parameter sets they point at. A row has an `id` and typed columns, not registers, so there is no
/// clock per column and nothing to merge: two devices that have the same ID have the same row.
/// Written only through [`WriteTx::insert_row`](super::WriteTx), and never changed or deleted.
#[derive(Debug, Clone, Copy)]
pub struct AppendOnlyTable {
    /// The entity type in sync data. Part of the sync format: never renamed (ADR 0006, section 10).
    pub entity: &'static str,
    pub table: &'static str,
    /// Every column except `id`. Part of the sync format.
    pub columns: &'static [&'static str],
}

/// Every append-only table, in the newest schema.
pub const APPEND_ONLY_TABLES: &[AppendOnlyTable] = &[study::CARD_EVENT, study::PARAMETER_SET];

/// Tables that exist only on this device and never sync. `card_schedule` is the cache folded from
/// card events (ADR 0007, part 9), and `unpushed_row` lists the append-only rows a sync has not
/// pushed yet.
pub const LOCAL_TABLES: &[&str] = &[
    "meta",
    "register_clock",
    "unknown_register",
    "write_guard",
    "card_schedule",
    "unpushed_row",
];

/// One row in `write_guard` exists only while a `WriteTx` is open.
pub(super) const GUARD_TABLE: &str = "write_guard";

fn trigger_names(table: &str) -> [String; 3] {
    ["insert", "update", "delete"].map(|op| format!("{table}_guard_{op}"))
}

/// Makes the database refuse writes to `table` outside a `WriteTx`: an INSERT or UPDATE needs the
/// guard row, and a DELETE is never allowed (ADR 0006, section 5: only a purge may delete, and it
/// is designed in step 1.11). Call it in the migration that creates the table.
pub fn install_guard(tx: &Transaction, table: &SyncedTable) -> rusqlite::Result<()> {
    install_guard_on(tx, table.table)
}

/// The guard for an [`AppendOnlyTable`]: an INSERT needs the guard row, and no row is ever changed
/// or deleted.
pub fn install_append_only_guard(
    tx: &Transaction,
    table: &AppendOnlyTable,
) -> rusqlite::Result<()> {
    let [insert, update, delete] = trigger_names(table.table);
    let name = table.table;
    tx.execute_batch(&format!(
        "CREATE TRIGGER {insert} BEFORE INSERT ON {name}
           WHEN NOT EXISTS (SELECT 1 FROM {GUARD_TABLE})
           BEGIN SELECT RAISE(ABORT, 'synced table written outside the write path'); END;
         CREATE TRIGGER {update} BEFORE UPDATE ON {name}
           BEGIN SELECT RAISE(ABORT, 'append-only rows are never changed'); END;
         CREATE TRIGGER {delete} BEFORE DELETE ON {name}
           BEGIN SELECT RAISE(ABORT, 'synced rows are never hard-deleted'); END;"
    ))
}

/// The same guard for a [`DynamicTable`].
pub fn install_dynamic_guard(tx: &Transaction, table: &DynamicTable) -> rusqlite::Result<()> {
    install_guard_on(tx, table.table)
}

fn install_guard_on(tx: &Transaction, name: &str) -> rusqlite::Result<()> {
    let [insert, update, delete] = trigger_names(name);
    tx.execute_batch(&format!(
        "CREATE TRIGGER {insert} BEFORE INSERT ON {name}
           WHEN NOT EXISTS (SELECT 1 FROM {GUARD_TABLE})
           BEGIN SELECT RAISE(ABORT, 'synced table written outside the write path'); END;
         CREATE TRIGGER {update} BEFORE UPDATE ON {name}
           WHEN NOT EXISTS (SELECT 1 FROM {GUARD_TABLE})
           BEGIN SELECT RAISE(ABORT, 'synced table written outside the write path'); END;
         CREATE TRIGGER {delete} BEFORE DELETE ON {name}
           BEGIN SELECT RAISE(ABORT, 'synced rows are never hard-deleted'); END;"
    ))
}

/// Checks that a migrated database matches `tables`: every table is local or synced, every column
/// of a synced table is `id` or a register, and every synced table has its guard triggers.
/// Returns what is wrong, for the test that runs it on every schema.
#[cfg(test)]
pub(crate) fn check_schema(
    conn: &rusqlite::Connection,
    tables: &[SyncedTable],
) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let names = |sql: &str| -> Vec<String> {
        let mut statement = conn.prepare(sql).unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let existing =
        names("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'");
    let triggers = names("SELECT name FROM sqlite_master WHERE type = 'trigger'");
    for name in &existing {
        let synced = tables.iter().any(|t| t.table == name)
            || DYNAMIC_TABLES.iter().any(|t| t.table == name)
            || APPEND_ONLY_TABLES.iter().any(|t| t.table == name);
        if !synced && !LOCAL_TABLES.contains(&name.as_str()) {
            problems.push(format!(
                "table `{name}` is neither in LOCAL_TABLES nor a SyncedTable"
            ));
        }
    }
    for table in tables {
        if !existing.iter().any(|name| name == table.table) {
            problems.push(format!("synced table `{}` does not exist", table.table));
            continue;
        }
        let mut columns = names(&format!(
            "SELECT name FROM pragma_table_info('{}')",
            table.table
        ));
        columns.sort();
        let mut expected: Vec<String> = ["id"]
            .into_iter()
            .chain(table.registers.iter().copied())
            .map(str::to_owned)
            .collect();
        expected.sort();
        if columns != expected {
            problems.push(format!(
                "columns of `{}` are {columns:?}, but its registers say {expected:?}",
                table.table
            ));
        }
        for trigger in trigger_names(table.table) {
            if !triggers.contains(&trigger) {
                problems.push(format!("`{}` has no trigger `{trigger}`", table.table));
            }
        }
    }
    for table in DYNAMIC_TABLES {
        if !existing.iter().any(|name| name == table.table) {
            continue;
        }
        let mut columns = names(&format!(
            "SELECT name FROM pragma_table_info('{}')",
            table.table
        ));
        columns.sort();
        let mut expected = vec![table.owner, table.key, table.value];
        expected.sort_unstable();
        if columns != expected {
            problems.push(format!(
                "columns of `{}` are {columns:?}, but its declaration says {expected:?}",
                table.table
            ));
        }
        for trigger in trigger_names(table.table) {
            if !triggers.contains(&trigger) {
                problems.push(format!("`{}` has no trigger `{trigger}`", table.table));
            }
        }
    }
    for table in APPEND_ONLY_TABLES {
        if !existing.iter().any(|name| name == table.table) {
            continue;
        }
        let mut columns = names(&format!(
            "SELECT name FROM pragma_table_info('{}')",
            table.table
        ));
        columns.sort();
        let mut expected: Vec<String> = ["id"]
            .into_iter()
            .chain(table.columns.iter().copied())
            .map(str::to_owned)
            .collect();
        expected.sort();
        if columns != expected {
            problems.push(format!(
                "columns of `{}` are {columns:?}, but its declaration says {expected:?}",
                table.table
            ));
        }
        for trigger in trigger_names(table.table) {
            if !triggers.contains(&trigger) {
                problems.push(format!("`{}` has no trigger `{trigger}`", table.table));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
