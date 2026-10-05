//! The list of synced tables, and the database-level guard that keeps writes on the write path.

use rusqlite::Transaction;

use super::requires;

/// A table whose fields merge between devices, one register per field.
#[derive(Debug)]
pub struct SyncedTable {
    /// The entity type in sync data. Part of the sync format: never renamed (ADR 0006, section 10).
    pub entity: &'static str,
    /// The SQLite table. Its columns are `id` plus one per register.
    pub table: &'static str,
    /// Register names, which are also the column names. Part of the sync format.
    pub registers: &'static [&'static str],
}

/// Every synced table, in the newest schema.
pub const SYNCED_TABLES: &[SyncedTable] = &[requires::TABLE];

/// Tables that exist only on this device and never sync.
pub const LOCAL_TABLES: &[&str] = &["meta", "register_clock", "unknown_register", "write_guard"];

/// One row in `write_guard` exists only while a `WriteTx` is open.
pub(super) const GUARD_TABLE: &str = "write_guard";

fn trigger_names(table: &SyncedTable) -> [String; 3] {
    ["insert", "update", "delete"].map(|op| format!("{}_guard_{op}", table.table))
}

/// Makes the database refuse writes to `table` outside a `WriteTx`: an INSERT or UPDATE needs the
/// guard row, and a DELETE is never allowed (ADR 0006, section 5: only a purge may delete, and it
/// is designed in step 1.11). Call it in the migration that creates the table.
pub fn install_guard(tx: &Transaction, table: &SyncedTable) -> rusqlite::Result<()> {
    let [insert, update, delete] = trigger_names(table);
    let name = table.table;
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
        let synced = tables.iter().any(|t| t.table == name);
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
        for trigger in trigger_names(table) {
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
