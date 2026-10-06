//! Change records: what one collection gives another (ADR 0008, parts 1 and 2).
//!
//! A [`Changes`] is plain Rust data with no encoding. It is self-describing (entity types and field
//! names are strings), so a build that does not know an entity or a field can still keep it and
//! pass it on. The wire encoding, paging and cursors belong to Phase 4.

use std::collections::BTreeMap;

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

use super::{APPEND_ONLY_TABLES, DYNAMIC_TABLES, Hlc, SyncedTable};
use crate::collection::{Collection, CollectionError};
use crate::id::Id;

/// The stamp of a register: the higher `(hlc, device)` wins (ADR 0006, section 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Clock {
    pub hlc: Hlc,
    pub device: Id,
}

/// The value of one register, with the clock it was written under.
#[derive(Debug, Clone, PartialEq)]
pub struct RegisterChange {
    /// `"note"`, `"card"`, `"note_tag"`, or a type this build does not know.
    pub entity: String,
    pub entity_id: Id,
    /// A column name, a field ID as text (a note's value), or a tag as written.
    pub field: String,
    pub value: Value,
    pub clock: Clock,
}

/// One immutable row of an append-only table (a card event, a parameter set), with every column
/// as `(name, value)`. Columns this build does not know are included.
#[derive(Debug, Clone, PartialEq)]
pub struct RowChange {
    pub entity: String,
    pub id: Id,
    pub columns: Vec<(String, Value)>,
}

/// A batch of changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub registers: Vec<RegisterChange>,
    pub rows: Vec<RowChange>,
}

/// Which changes [`Collection::changes`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Everything this collection has, which is what a new device needs.
    All,
    /// Only what no sync has pushed yet.
    Unpushed,
}

impl Collection {
    /// The changes held by this collection (ADR 0008, part 2).
    ///
    /// - A register with no clock holds the column default on every device, so it is not sent.
    /// - Seeded registers (clock `0`) are sent like any other: they are equal everywhere, so
    ///   applying them does nothing.
    /// - Registers and rows this build keeps as unknown data are in `All` and never in `Unpushed`.
    pub fn changes(&self, selection: Selection) -> Result<Changes, CollectionError> {
        let mut changes = Changes::default();
        self.read_registers(selection, &mut changes)?;
        if selection == Selection::All {
            self.read_unknown_registers(&mut changes)?;
        }
        self.read_rows(selection, &mut changes)?;
        Ok(changes)
    }

    fn read_registers(
        &self,
        selection: Selection,
        changes: &mut Changes,
    ) -> Result<(), CollectionError> {
        let filter = match selection {
            Selection::All => "",
            Selection::Unpushed => "WHERE pushed = 0",
        };
        let mut statement = self.conn.prepare(&format!(
            "SELECT entity_type, entity_id, field, hlc, device FROM register_clock {filter}
             ORDER BY entity_type, entity_id, field"
        ))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let entity: String = row.get(0)?;
            let entity_id: Id = row.get(1)?;
            let field: String = row.get(2)?;
            let clock = Clock {
                hlc: Hlc::from_stored(row.get(3)?),
                device: row.get(4)?,
            };
            if let Some(value) = self.register_value(&entity, entity_id, &field)? {
                changes.registers.push(RegisterChange {
                    entity,
                    entity_id,
                    field,
                    value,
                    clock,
                });
            }
        }
        Ok(())
    }

    /// The stored value of a register, or `None` if there is no such row (a clock without a value
    /// is not a change).
    fn register_value(
        &self,
        entity: &str,
        id: Id,
        field: &str,
    ) -> Result<Option<Value>, CollectionError> {
        read_register(&self.conn, self.schema.tables, entity, id, field)
    }

    fn read_unknown_registers(&self, changes: &mut Changes) -> Result<(), CollectionError> {
        let mut statement = self.conn.prepare(
            "SELECT entity_type, entity_id, field, value, hlc, device FROM unknown_register
             ORDER BY entity_type, entity_id, field",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RegisterChange {
                entity: row.get(0)?,
                entity_id: row.get(1)?,
                field: row.get(2)?,
                value: row.get(3)?,
                clock: Clock {
                    hlc: Hlc::from_stored(row.get(4)?),
                    device: row.get(5)?,
                },
            })
        })?;
        for row in rows {
            changes.registers.push(row?);
        }
        Ok(())
    }

    fn read_rows(
        &self,
        selection: Selection,
        changes: &mut Changes,
    ) -> Result<(), CollectionError> {
        for table in APPEND_ONLY_TABLES {
            if !table_exists(&self.conn, table.table)? {
                continue;
            }
            let columns: String = table
                .columns
                .iter()
                .map(|c| format!(", t.\"{c}\""))
                .collect();
            let source = match selection {
                Selection::All => format!("\"{}\" t", table.table),
                Selection::Unpushed => format!(
                    "unpushed_row u JOIN \"{}\" t ON t.id = u.row_id AND u.entity_type = '{}'",
                    table.table, table.entity
                ),
            };
            let mut statement = self
                .conn
                .prepare(&format!("SELECT t.id{columns} FROM {source} ORDER BY t.id"))?;
            let mut rows = statement.query([])?;
            let mut found: Vec<RowChange> = Vec::new();
            while let Some(row) = rows.next()? {
                let mut values = Vec::with_capacity(table.columns.len());
                for (i, name) in table.columns.iter().enumerate() {
                    values.push(((*name).to_owned(), row.get::<_, Value>(i + 1)?));
                }
                found.push(RowChange {
                    entity: table.entity.to_owned(),
                    id: row.get(0)?,
                    columns: values,
                });
            }
            if selection == Selection::All {
                let mut extras = self.unknown_columns(table.entity)?;
                for row in &mut found {
                    if let Some(more) = extras.remove(&row.id) {
                        row.columns.extend(more);
                    }
                }
            }
            changes.rows.extend(found);
        }
        if selection == Selection::All {
            let mut statement = self.conn.prepare(
                "SELECT DISTINCT entity_type FROM unknown_row_value ORDER BY entity_type",
            )?;
            let types = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for entity in types {
                if APPEND_ONLY_TABLES.iter().any(|t| t.entity == entity) {
                    continue;
                }
                for (id, columns) in self.unknown_columns(&entity)? {
                    changes.rows.push(RowChange {
                        entity: entity.clone(),
                        id,
                        columns,
                    });
                }
            }
        }
        Ok(())
    }

    /// The stored columns of one entity type, by row.
    fn unknown_columns(
        &self,
        entity: &str,
    ) -> Result<BTreeMap<Id, Vec<(String, Value)>>, CollectionError> {
        let mut statement = self.conn.prepare(
            "SELECT row_id, column, value FROM unknown_row_value WHERE entity_type = ?1
             ORDER BY row_id, column",
        )?;
        let rows = statement.query_map([entity], |row| {
            Ok((row.get::<_, Id>(0)?, row.get::<_, String>(1)?, row.get(2)?))
        })?;
        let mut by_row: BTreeMap<Id, Vec<(String, Value)>> = BTreeMap::new();
        for row in rows {
            let (id, column, value) = row?;
            by_row.entry(id).or_default().push((column, value));
        }
        Ok(by_row)
    }
}

/// The stored value of a register, or `None` if there is no such row.
pub(super) fn read_register(
    conn: &Connection,
    tables: &[SyncedTable],
    entity: &str,
    id: Id,
    field: &str,
) -> Result<Option<Value>, CollectionError> {
    let table = tables.iter().find(|t| t.entity == entity);
    if let Some(table) = table.filter(|t| t.registers.contains(&field)) {
        let sql = format!("SELECT \"{field}\" FROM \"{}\" WHERE id = ?1", table.table);
        return Ok(conn
            .prepare_cached(&sql)?
            .query_row([id], |row| row.get(0))
            .optional()?);
    }
    for dynamic in DYNAMIC_TABLES.iter().filter(|d| d.entity == entity) {
        let (name, o, k, v) = (dynamic.table, dynamic.owner, dynamic.key, dynamic.value);
        let sql = format!("SELECT \"{v}\" FROM \"{name}\" WHERE \"{o}\" = ?1 AND \"{k}\" = ?2");
        let found = if dynamic.text_key {
            conn.prepare_cached(&sql)
                .and_then(|mut s| s.query_row(params![id, field], |row| row.get(0)).optional())
        } else if let (Ok(key), Some(_)) = (field.parse::<Id>(), table) {
            conn.prepare_cached(&sql)
                .and_then(|mut s| s.query_row(params![id, key], |row| row.get(0)).optional())
        } else {
            continue;
        };
        return match found {
            Ok(value) => Ok(value),
            // A smaller schema without the table: nothing to send.
            Err(rusqlite::Error::SqliteFailure(_, Some(message)))
                if message.starts_with("no such table") =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        };
    }
    Ok(None)
}

pub(super) fn table_exists(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
