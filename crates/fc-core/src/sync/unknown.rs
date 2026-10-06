//! The generic store for registers this build does not know (ADR 0006, section 10): a new entity
//! type, or a new field of a known one. They are kept unchanged and never written locally, so an
//! older app cannot overwrite them. After an upgrade, a migration applies what was stored.

use rusqlite::types::Value;
use rusqlite::{Connection, Transaction, TransactionBehavior, params};

use super::{Hlc, state};
use crate::collection::{Collection, CollectionError};
use crate::id::Id;

/// One register as it arrived from another device.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownRegister {
    pub entity_type: String,
    pub entity_id: Id,
    pub field: String,
    /// Kept exactly as received: any SQLite value.
    pub value: Value,
    pub hlc: Hlc,
    pub device: Id,
}

/// The one rule for keeping an unknown register: the higher `(hlc, device)` wins, so applying the
/// same changes in any order, or twice, gives the same result. Returns whether the stored value
/// changed. The merge and `store_unknown_register` both use it.
pub(super) fn store_unknown(
    conn: &Connection,
    register: &UnknownRegister,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "INSERT INTO unknown_register (entity_type, entity_id, field, value, hlc, device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (entity_type, entity_id, field) DO UPDATE
         SET value = excluded.value, hlc = excluded.hlc, device = excluded.device
         WHERE (excluded.hlc, excluded.device) > (hlc, device)",
        params![
            register.entity_type,
            register.entity_id,
            register.field,
            register.value,
            register.hlc.to_stored(),
            register.device
        ],
    )?;
    Ok(changed == 1)
}

impl Collection {
    /// Whether this build has a table with this register. Anything else goes to the unknown store.
    pub fn knows_register(&self, entity_type: &str, field: &str) -> bool {
        self.schema
            .tables
            .iter()
            .any(|t| t.entity == entity_type && t.registers.contains(&field))
            || (super::DYNAMIC_TABLES.iter().any(|t| {
                t.entity == entity_type
                    && if t.text_key {
                        // Known when this database has the table (a smaller test schema may not).
                        self.conn
                            .query_row(
                                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                                [t.table],
                                |_| Ok(()),
                            )
                            .is_ok()
                    } else {
                        field.parse::<Id>().is_ok()
                            && self.schema.tables.iter().any(|s| s.entity == entity_type)
                    }
            }))
    }

    /// Keeps a register this build does not know. The higher `(hlc, device)` wins, so applying
    /// the same changes in any order, or twice, gives the same result. Returns whether the stored
    /// value changed. It also moves the saved HLC up to what was received.
    pub fn store_unknown_register(
        &self,
        register: &UnknownRegister,
    ) -> Result<bool, CollectionError> {
        if self.knows_register(&register.entity_type, &register.field) {
            return Err(CollectionError::Storage(format!(
                "`{}.{}` is a known register, not unknown data",
                register.entity_type, register.field
            )));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let changed = store_unknown(&tx, register)?;
        let last = state::hlc_last(&tx)?.observe(register.hlc);
        state::set_hlc_last(&tx, last)?;
        tx.commit()?;
        Ok(changed)
    }

    /// The unknown registers stored for one entity.
    pub fn unknown_registers(
        &self,
        entity_type: &str,
        entity_id: Id,
    ) -> Result<Vec<UnknownRegister>, CollectionError> {
        let mut statement = self.conn.prepare(
            "SELECT field, value, hlc, device FROM unknown_register
             WHERE entity_type = ?1 AND entity_id = ?2 ORDER BY field",
        )?;
        let rows = statement.query_map(params![entity_type, entity_id], |row| {
            Ok(UnknownRegister {
                entity_type: entity_type.to_owned(),
                entity_id,
                field: row.get(0)?,
                value: row.get(1)?,
                hlc: Hlc::from_stored(row.get(2)?),
                device: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The saved HLC: the newest stamp this collection has written or seen.
    pub fn hlc(&self) -> Result<Hlc, CollectionError> {
        state::hlc_last(&self.conn)
    }

    /// Moves the saved HLC up to a stamp received from another device.
    pub fn observe_hlc(&self, received: Hlc) -> Result<(), CollectionError> {
        let last = state::hlc_last(&self.conn)?.observe(received);
        state::set_hlc_last(&self.conn, last)
    }

    /// This copy's device ID. Never shared by two copies of a collection.
    pub fn device_id(&self) -> Result<Id, CollectionError> {
        state::device_id(&self.conn)
    }

    /// Gives this copy a new device ID. A restore or an import calls this (ADR 0006, section 1).
    pub fn regenerate_device_id(&self) -> Result<Id, CollectionError> {
        state::regenerate_device_id(&self.conn, &self.host)
    }
}

/// Called by a migration that adds registers or columns this build used to keep as unknown data
/// (ADR 0008, part 6). It moves what was stored into `table`, and removes it from the store:
///
/// - **Registers** named in `names` (for an entity whose rows have registers): each is written into
///   its row with its stored clock, marked pushed, unless the row already has a register clock that
///   is equal or higher. A row that does not exist is created with only that value.
/// - **Columns** named in `names` (for an append-only entity): each is written into its row, or the
///   whole row is created if the table has none yet.
///
/// `entity` is the entity type in sync data and `table` the SQLite table, which the migration has
/// just created or altered. Returns how many values were moved.
///
/// A later migration that adds a register or column calls this once for it, with the names of
/// the new registers or columns, after its own schema changes.
#[cfg_attr(not(test), allow(dead_code))] // No migration needs it yet; the tests do.
pub(crate) fn adopt_unknown(
    tx: &Transaction,
    entity: &str,
    table: &str,
    names: &[&str],
) -> rusqlite::Result<usize> {
    let mut moved = adopt_registers(tx, entity, table, names)?;
    moved += adopt_columns(tx, entity, table, names)?;
    Ok(moved)
}

/// Opens the write guard for the rest of the closure, if it is not open already.
#[cfg_attr(not(test), allow(dead_code))] // No migration needs it yet; the tests do.
fn with_guard<T>(tx: &Transaction, f: impl FnOnce() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    let opened = tx.execute("INSERT OR IGNORE INTO write_guard (id) VALUES (1)", [])? == 1;
    let result = f();
    if opened {
        tx.execute("DELETE FROM write_guard", [])?;
    }
    result
}

#[cfg_attr(not(test), allow(dead_code))] // No migration needs it yet; the tests do.
fn adopt_registers(
    tx: &Transaction,
    entity: &str,
    table: &str,
    names: &[&str],
) -> rusqlite::Result<usize> {
    let mut statement = tx.prepare(
        "SELECT entity_id, field, value, hlc, device FROM unknown_register
         WHERE entity_type = ?1 ORDER BY entity_id, field",
    )?;
    let stored = statement
        .query_map([entity], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Value>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Id>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut moved = 0;
    for (id, field, value, hlc, device) in stored {
        if !names.contains(&field.as_str()) {
            continue;
        }
        let local: Option<(i64, Id)> = tx
            .query_row(
                "SELECT hlc, device FROM register_clock
                 WHERE entity_type = ?1 AND entity_id = ?2 AND field = ?3",
                params![entity, id, field],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        if local.is_none_or(|local| (hlc, device) > local) {
            with_guard(tx, || {
                tx.execute(
                    &format!(
                        "INSERT INTO \"{table}\" (id, \"{field}\") VALUES (?1, ?2)
                         ON CONFLICT (id) DO UPDATE SET \"{field}\" = excluded.\"{field}\""
                    ),
                    params![id, value],
                )
            })?;
            tx.execute(
                "INSERT INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)
                 ON CONFLICT (entity_type, entity_id, field)
                 DO UPDATE SET hlc = excluded.hlc, device = excluded.device, pushed = 1",
                params![entity, id, field, hlc, device],
            )?;
        }
        tx.execute(
            "DELETE FROM unknown_register WHERE entity_type = ?1 AND entity_id = ?2 AND field = ?3",
            params![entity, id, field],
        )?;
        moved += 1;
    }
    Ok(moved)
}

#[cfg_attr(not(test), allow(dead_code))] // No migration needs it yet; the tests do.
fn adopt_columns(
    tx: &Transaction,
    entity: &str,
    table: &str,
    names: &[&str],
) -> rusqlite::Result<usize> {
    let mut statement = tx.prepare(
        "SELECT row_id, column, value FROM unknown_row_value
         WHERE entity_type = ?1 ORDER BY row_id, column",
    )?;
    let stored = statement
        .query_map([entity], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Value>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut by_row: std::collections::BTreeMap<Id, Vec<(String, Value)>> = Default::default();
    for (id, column, value) in stored {
        if names.contains(&column.as_str()) {
            by_row.entry(id).or_default().push((column, value));
        }
    }
    if by_row.is_empty() {
        return Ok(0);
    }
    // An append-only table refuses every UPDATE. The migration lifts that for the statements below
    // and puts the trigger back as it was.
    let update_trigger = format!("{table}_guard_update");
    let saved: Option<String> = tx
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
            [&update_trigger],
            |row| row.get(0),
        )
        .ok();
    if saved.is_some() {
        tx.execute_batch(&format!("DROP TRIGGER \"{update_trigger}\""))?;
    }
    let mut moved = 0;
    for (id, columns) in &by_row {
        let exists = tx
            .query_row(
                &format!("SELECT 1 FROM \"{table}\" WHERE id = ?1"),
                [id],
                |_| Ok(()),
            )
            .is_ok();
        if exists {
            for (column, value) in columns {
                tx.execute(
                    &format!("UPDATE \"{table}\" SET \"{column}\" = ?1 WHERE id = ?2"),
                    params![value, id],
                )?;
            }
        } else {
            let names: String = columns.iter().map(|(c, _)| format!(", \"{c}\"")).collect();
            let marks = ", ?".repeat(columns.len());
            let args = std::iter::once(Value::Blob(id.as_bytes().to_vec()))
                .chain(columns.iter().map(|(_, v)| v.clone()));
            with_guard(tx, || {
                tx.execute(
                    &format!("INSERT INTO \"{table}\" (id{names}) VALUES (?{marks})"),
                    rusqlite::params_from_iter(args),
                )
            })?;
        }
        for (column, _) in columns {
            tx.execute(
                "DELETE FROM unknown_row_value WHERE entity_type = ?1 AND row_id = ?2 AND column = ?3",
                params![entity, id, column],
            )?;
            moved += 1;
        }
    }
    if let Some(sql) = saved {
        tx.execute_batch(&sql)?;
    }
    Ok(moved)
}
