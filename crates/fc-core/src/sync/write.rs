//! The one write path for synced tables (ADR 0006, section 3). Every register written here gets
//! `(hlc, device, pushed = false)` in `register_clock`, in the same transaction as the value.

use rusqlite::types::Value;
use rusqlite::{
    Connection, OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter,
};

use super::registry::{APPEND_ONLY_TABLES, DYNAMIC_TABLES, GUARD_TABLE};
use super::{Hlc, SyncedTable, state};
use crate::clock::Host;
use crate::collection::{Collection, CollectionError};
use crate::id::Id;

/// The clock saved for one register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterClock {
    pub hlc: Hlc,
    pub device: Id,
    /// False until a sync has pushed this write.
    pub pushed: bool,
}

/// An open write transaction. It commits when the closure given to `Collection::write` returns
/// `Ok` and rolls back otherwise. There is no way to run raw SQL on it: synced tables change only
/// through `insert` and `set`.
pub struct WriteTx<'c> {
    tx: Transaction<'c>,
    host: &'c Host,
    tables: &'c [SyncedTable],
    device: Id,
    hlc: Hlc,
}

fn problem(message: String) -> CollectionError {
    CollectionError::Storage(message)
}

impl WriteTx<'_> {
    /// A new random ID, time-ordered (UUIDv7).
    pub fn new_id(&self) -> Result<Id, CollectionError> {
        state::new_id(self.host)
    }

    fn table(&self, entity: &str) -> Result<&SyncedTable, CollectionError> {
        self.tables
            .iter()
            .find(|t| t.entity == entity)
            .ok_or_else(|| problem(format!("`{entity}` is not a synced entity type")))
    }

    /// The stamp for the next register write.
    fn stamp(&mut self) -> Hlc {
        self.hlc = self.hlc.next(self.host.clock.now().unix_ms);
        self.hlc
    }

    fn record(&mut self, entity: &str, id: Id, field: &str) -> Result<(), CollectionError> {
        let hlc = self.stamp();
        self.tx.execute(
            "INSERT INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)
             ON CONFLICT (entity_type, entity_id, field)
             DO UPDATE SET hlc = excluded.hlc, device = excluded.device, pushed = 0",
            params![entity, id, field, hlc.to_stored(), self.device],
        )?;
        Ok(())
    }

    /// Adds a row. Every register of the entity must be given a value, so every register has a
    /// clock from the start.
    pub fn insert(
        &mut self,
        entity: &str,
        id: Id,
        values: Vec<(&str, Value)>,
    ) -> Result<(), CollectionError> {
        let table = self.table(entity)?;
        let (name, registers) = (table.table, table.registers);
        for register in registers {
            if values.iter().filter(|(field, _)| field == register).count() != 1 {
                return Err(problem(format!(
                    "`{entity}` needs exactly one value for register `{register}`"
                )));
            }
        }
        if let Some((field, _)) = values.iter().find(|(f, _)| !registers.contains(f)) {
            return Err(problem(format!("`{entity}` has no register `{field}`")));
        }
        let columns: String = values
            .iter()
            .map(|(field, _)| format!(", \"{field}\""))
            .collect();
        let marks = ", ?".repeat(values.len());
        let args = std::iter::once(Value::Blob(id.as_bytes().to_vec()))
            .chain(values.iter().map(|(_, value)| value.clone()));
        self.tx.execute(
            &format!("INSERT INTO \"{name}\" (id{columns}) VALUES (?{marks})"),
            params_from_iter(args),
        )?;
        for (field, _) in &values {
            self.record(entity, id, field)?;
        }
        Ok(())
    }

    /// Adds a row to an append-only table (a card event, a parameter set). Every column must be
    /// given a value (`Null` for one the row does not use). A row with this ID that exists already is
    /// the same row, so nothing happens. Returns whether the row was added. A new row is listed as
    /// not pushed yet.
    pub fn insert_row(
        &mut self,
        entity: &str,
        id: Id,
        values: Vec<(&str, Value)>,
    ) -> Result<bool, CollectionError> {
        let table = APPEND_ONLY_TABLES
            .iter()
            .find(|t| t.entity == entity)
            .ok_or_else(|| problem(format!("`{entity}` is not an append-only entity type")))?;
        for column in table.columns {
            if values.iter().filter(|(field, _)| field == column).count() != 1 {
                return Err(problem(format!(
                    "`{entity}` needs exactly one value for column `{column}`"
                )));
            }
        }
        if let Some((field, _)) = values.iter().find(|(f, _)| !table.columns.contains(f)) {
            return Err(problem(format!("`{entity}` has no column `{field}`")));
        }
        let name = table.table;
        let columns: String = values
            .iter()
            .map(|(field, _)| format!(", \"{field}\""))
            .collect();
        let marks = ", ?".repeat(values.len());
        let args = std::iter::once(Value::Blob(id.as_bytes().to_vec()))
            .chain(values.iter().map(|(_, value)| value.clone()));
        let added = self.tx.execute(
            &format!("INSERT OR IGNORE INTO \"{name}\" (id{columns}) VALUES (?{marks})"),
            params_from_iter(args),
        )?;
        if added == 1 {
            self.tx.execute(
                "INSERT OR IGNORE INTO unpushed_row (entity_type, row_id) VALUES (?1, ?2)",
                params![entity, id],
            )?;
        }
        Ok(added == 1)
    }

    /// The connection of this transaction, for reading and for the local tables (`card_schedule`).
    /// Synced tables must be written through the methods above, which keep their clocks: the guard
    /// is open here and will not catch a raw write.
    pub(crate) fn local(&self) -> &Connection {
        &self.tx
    }

    /// Changes one register of an existing row.
    pub fn set(
        &mut self,
        entity: &str,
        id: Id,
        field: &str,
        value: Value,
    ) -> Result<(), CollectionError> {
        let table = self.table(entity)?;
        if !table.registers.contains(&field) {
            return Err(problem(format!("`{entity}` has no register `{field}`")));
        }
        let changed = self.tx.execute(
            &format!(
                "UPDATE \"{}\" SET \"{field}\" = ?1 WHERE id = ?2",
                table.table
            ),
            params![value, id],
        )?;
        if changed != 1 {
            return Err(problem(format!("no `{entity}` row {id} to change")));
        }
        self.record(entity, id, field)
    }

    /// Sets one value of a dynamic table (a note's value for one field), and writes its clock under
    /// the register name `key`. The value is created if there is none yet. Writing the value a
    /// register already has changes nothing and writes no clock.
    pub fn set_value(
        &mut self,
        entity: &str,
        owner: Id,
        key: Id,
        value: &str,
    ) -> Result<(), CollectionError> {
        self.set_dynamic(entity, owner, &key, &key.to_string(), value)
    }

    /// The same for a dynamic table whose key is text (a note's tag), written under the register
    /// name `key` as it is.
    pub fn set_text_value(
        &mut self,
        entity: &str,
        owner: Id,
        key: &str,
        value: &str,
    ) -> Result<(), CollectionError> {
        self.set_dynamic(entity, owner, &key, key, value)
    }

    fn set_dynamic(
        &mut self,
        entity: &str,
        owner: Id,
        key: &dyn rusqlite::ToSql,
        register: &str,
        value: &str,
    ) -> Result<(), CollectionError> {
        let table = DYNAMIC_TABLES
            .iter()
            .find(|t| t.entity == entity)
            .ok_or_else(|| problem(format!("`{entity}` has no dynamic registers")))?;
        let (name, o, k, v) = (table.table, table.owner, table.key, table.value);
        let current: Option<String> = self
            .tx
            .query_row(
                &format!("SELECT \"{v}\" FROM \"{name}\" WHERE \"{o}\" = ?1 AND \"{k}\" = ?2"),
                params![owner, key],
                |row| row.get(0),
            )
            .optional()?;
        if current.as_deref() == Some(value) || (current.is_none() && value.is_empty()) {
            return Ok(());
        }
        self.tx.execute(
            &format!(
                "INSERT INTO \"{name}\" (\"{o}\", \"{k}\", \"{v}\") VALUES (?1, ?2, ?3)
                 ON CONFLICT (\"{o}\", \"{k}\") DO UPDATE SET \"{v}\" = excluded.\"{v}\""
            ),
            params![owner, key, value],
        )?;
        self.record(entity, owner, register)
    }

    /// Reads one value, inside this transaction.
    pub fn get(&self, entity: &str, id: Id, field: &str) -> Result<Option<Value>, CollectionError> {
        let table = self.table(entity)?;
        if !table.registers.contains(&field) {
            return Err(problem(format!("`{entity}` has no register `{field}`")));
        }
        let value = self.tx.query_row(
            &format!("SELECT \"{field}\" FROM \"{}\" WHERE id = ?1", table.table),
            [id],
            |row| row.get(0),
        );
        match value {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

/// Writes a built-in row from inside a migration, with the lowest clock there is: `(0, nil device)`,
/// already marked pushed. Every device seeds the same rows, so there is nothing to push, and any real
/// edit on any device beats the seed. A seed written with the current time would instead beat an
/// older edit made elsewhere, after the first sync of a fresh collection. The row's content must
/// never change once a version has shipped it, because devices would then differ silently.
pub(crate) fn seed_row(
    tx: &Transaction,
    table: &SyncedTable,
    id: Id,
    values: &[(&str, Value)],
) -> rusqlite::Result<()> {
    let columns: String = values
        .iter()
        .map(|(field, _)| format!(", \"{field}\""))
        .collect();
    let marks = ", ?".repeat(values.len());
    let args = std::iter::once(Value::Blob(id.as_bytes().to_vec()))
        .chain(values.iter().map(|(_, value)| value.clone()));
    tx.execute(&format!("INSERT INTO {GUARD_TABLE} (id) VALUES (1)"), [])?;
    tx.execute(
        &format!(
            "INSERT INTO \"{}\" (id{columns}) VALUES (?{marks})",
            table.table
        ),
        params_from_iter(args),
    )?;
    tx.execute(&format!("DELETE FROM {GUARD_TABLE}"), [])?;
    for (field, _) in values {
        tx.execute(
            "INSERT INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES (?1, ?2, ?3, 0, ?4, 1)",
            params![table.entity, id, field, Id::from_bytes([0; 16])],
        )?;
    }
    Ok(())
}

impl Collection {
    /// Runs `f` in one write transaction. This is the only way to change a synced table.
    pub fn write<T>(
        &self,
        f: impl FnOnce(&mut WriteTx<'_>) -> Result<T, CollectionError>,
    ) -> Result<T, CollectionError> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        tx.execute(&format!("INSERT INTO {GUARD_TABLE} (id) VALUES (1)"), [])?;
        let mut write = WriteTx {
            device: state::device_id(&tx)?,
            hlc: state::hlc_last(&tx)?,
            host: &self.host,
            tables: self.schema.tables,
            tx,
        };
        let value = f(&mut write)?;
        state::set_hlc_last(&write.tx, write.hlc)?;
        write
            .tx
            .execute(&format!("DELETE FROM {GUARD_TABLE}"), [])?;
        write.tx.commit()?;
        Ok(value)
    }

    /// The clock saved for a register, or `None` if it was never written here.
    pub fn register_clock(
        &self,
        entity: &str,
        id: Id,
        field: &str,
    ) -> Result<Option<RegisterClock>, CollectionError> {
        read_clock(&self.conn, entity, id, field)
    }
}

pub(super) fn read_clock(
    conn: &Connection,
    entity: &str,
    id: Id,
    field: &str,
) -> Result<Option<RegisterClock>, CollectionError> {
    let found = conn.query_row(
        "SELECT hlc, device, pushed FROM register_clock
         WHERE entity_type = ?1 AND entity_id = ?2 AND field = ?3",
        params![entity, id, field],
        |row| {
            Ok(RegisterClock {
                hlc: Hlc::from_stored(row.get(0)?),
                device: row.get(1)?,
                pushed: row.get::<_, i64>(2)? != 0,
            })
        },
    );
    match found {
        Ok(clock) => Ok(Some(clock)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
