//! Applying a batch from another collection (ADR 0008, parts 3 to 6).
//!
//! `Collection::merge` is the only place a remote change is applied. It writes through
//! `WriteTx::apply_register` and `WriteTx::apply_row` (crate-only), and then brings the derived
//! state in line: the cards of notes, and the schedule cache.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::types::Value;
use rusqlite::{Connection, params, params_from_iter};

use super::changes::{Changes, Clock, RegisterChange, RowChange, table_exists};
use super::registry::{AppendOnlyTable, DynamicTable};
use super::unknown::{UnknownRegister, store_unknown};
use super::write::read_clock;
use super::{APPEND_ONLY_TABLES, DYNAMIC_TABLES, SyncedTable, WriteTx};
use crate::collection::{Collection, CollectionError, Schema};
use crate::deck::dead_decks;
use crate::id::Id;
use crate::study::{CARD_EVENT, PARAMETER_SET, ParameterSets, rebuild_card};

/// One change the merge did not apply, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub entity: String,
    pub id: Id,
    /// The register or column, or `None` when a whole row was rejected.
    pub field: Option<String>,
    pub reason: String,
}

/// What a merge did (ADR 0008, part 3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// Registers whose value and clock were taken from the batch.
    pub registers_applied: usize,
    /// Registers older than or equal to what this collection has.
    pub registers_ignored: usize,
    /// Registers of types or fields this build does not know, newly kept.
    pub unknown_registers: usize,
    /// Rows that came with columns, or entity types, this build does not know, newly kept.
    pub unknown_rows: usize,
    /// Append-only rows that were new here.
    pub rows_added: usize,
    /// Notes whose cards the merge changed to match their fields and note type.
    pub notes_reconciled: usize,
    /// Cards whose schedule was folded again from their events.
    pub cards_rebuilt: usize,
    pub rejected: Vec<Rejected>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Integer,
    Real,
    Text,
    Blob,
    Any,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Column {
    pub(super) kind: Kind,
    pub(super) not_null: bool,
}

impl Column {
    /// A SQLite value fits a column when its type is the declared one. An integer also fits a
    /// REAL column. NULL fits only a nullable column (ADR 0008, part 4).
    pub(super) fn check(self, value: &Value) -> Result<(), String> {
        let fits = match (value, self.kind) {
            (Value::Null, _) => {
                return if self.not_null {
                    Err("NULL for a column that cannot be empty".to_owned())
                } else {
                    Ok(())
                };
            }
            (_, Kind::Any)
            | (Value::Integer(_), Kind::Integer | Kind::Real)
            | (Value::Real(_), Kind::Real)
            | (Value::Text(_), Kind::Text)
            | (Value::Blob(_), Kind::Blob) => true,
            _ => false,
        };
        if fits {
            Ok(())
        } else {
            Err(format!("{} for a {:?} column", type_name(value), self.kind))
        }
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NULL",
        Value::Integer(_) => "an integer",
        Value::Real(_) => "a real number",
        Value::Text(_) => "text",
        Value::Blob(_) => "a blob",
    }
}

fn kind_of(declared: &str) -> Kind {
    let declared = declared.to_ascii_uppercase();
    if declared.contains("INT") {
        Kind::Integer
    } else if declared.contains("CHAR") || declared.contains("TEXT") || declared.contains("CLOB") {
        Kind::Text
    } else if declared.contains("BLOB") {
        Kind::Blob
    } else if declared.contains("REAL") || declared.contains("FLOA") || declared.contains("DOUB") {
        Kind::Real
    } else {
        Kind::Any
    }
}

/// What this build can apply, read once per merge: the tables of its schema that this database has,
/// and the declared type of every column.
pub(crate) struct Targets {
    synced: Vec<SyncedTable>,
    dynamic: Vec<DynamicTable>,
    append_only: Vec<AppendOnlyTable>,
    columns: HashMap<(&'static str, String), Column>,
}

enum Target<'a> {
    Synced(&'a SyncedTable),
    /// A value of a dynamic table, with the key as the table stores it.
    Dynamic(&'a DynamicTable, Value),
    Unknown,
}

impl Targets {
    fn load(conn: &Connection, schema: Schema) -> Result<Self, CollectionError> {
        let mut targets = Self {
            synced: schema.tables.to_vec(),
            dynamic: Vec::new(),
            append_only: Vec::new(),
            columns: HashMap::new(),
        };
        for table in DYNAMIC_TABLES {
            if table_exists(conn, table.table)? {
                targets.dynamic.push(*table);
            }
        }
        for table in APPEND_ONLY_TABLES {
            if table_exists(conn, table.table)? {
                targets.append_only.push(*table);
            }
        }
        let names: Vec<&'static str> = targets
            .synced
            .iter()
            .map(|t| t.table)
            .chain(targets.dynamic.iter().map(|t| t.table))
            .chain(targets.append_only.iter().map(|t| t.table))
            .collect();
        let mut statement =
            conn.prepare("SELECT name, type, \"notnull\" FROM pragma_table_info(?1)")?;
        for name in names {
            let rows = statement.query_map([name], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    Column {
                        kind: kind_of(&row.get::<_, String>(1)?),
                        not_null: row.get::<_, i64>(2)? != 0,
                    },
                ))
            })?;
            for row in rows {
                let (column, rule) = row?;
                targets.columns.insert((name, column), rule);
            }
        }
        Ok(targets)
    }

    fn column(&self, table: &'static str, column: &str) -> Option<Column> {
        self.columns.get(&(table, column.to_owned())).copied()
    }

    fn classify(&self, entity: &str, field: &str) -> Target<'_> {
        let synced = self.synced.iter().find(|t| t.entity == entity);
        if let Some(table) = synced.filter(|t| t.registers.contains(&field)) {
            return Target::Synced(table);
        }
        for table in self.dynamic.iter().filter(|t| t.entity == entity) {
            if table.text_key {
                return Target::Dynamic(table, Value::Text(field.to_owned()));
            }
            if let (Ok(key), Some(_)) = (field.parse::<Id>(), synced) {
                return Target::Dynamic(table, Value::Blob(key.as_bytes().to_vec()));
            }
        }
        Target::Unknown
    }
}

/// How one register of a batch was handled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The remote value and clock were taken.
    Applied,
    /// The local clock is higher or equal.
    Ignored,
    /// Kept as unknown data.
    Unknown,
    Rejected(String),
}

/// How one row of a batch was handled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RowOutcome {
    Added,
    /// Added, and some of its columns were kept as unknown data.
    AddedWithUnknown,
    /// The row was here already.
    Present,
    /// Only unknown columns were kept (the whole row, for an unknown entity type).
    Unknown,
    Rejected(String),
}

impl WriteTx<'_> {
    /// Applies one remote register (ADR 0008, part 3): the higher `(hlc, device)` wins, a row that
    /// does not exist yet is created with this one value and the defaults, and the clock is saved as
    /// already pushed.
    pub(crate) fn apply_register(
        &mut self,
        targets: &Targets,
        change: &RegisterChange,
    ) -> Result<Outcome, CollectionError> {
        let (sql, column_table, column, key) = match targets.classify(&change.entity, &change.field)
        {
            Target::Unknown => {
                let stored = store_unknown(
                    self.local(),
                    &UnknownRegister {
                        entity_type: change.entity.clone(),
                        entity_id: change.entity_id,
                        field: change.field.clone(),
                        value: change.value.clone(),
                        hlc: change.clock.hlc,
                        device: change.clock.device,
                    },
                )?;
                return Ok(if stored {
                    Outcome::Unknown
                } else {
                    Outcome::Ignored
                });
            }
            Target::Synced(table) => (
                format!(
                    "INSERT INTO \"{t}\" (id, \"{f}\") VALUES (?1, ?2)
                     ON CONFLICT (id) DO UPDATE SET \"{f}\" = excluded.\"{f}\"",
                    t = table.table,
                    f = change.field
                ),
                table.table,
                change.field.clone(),
                None,
            ),
            Target::Dynamic(table, key) => (
                format!(
                    "INSERT INTO \"{t}\" (\"{o}\", \"{k}\", \"{v}\") VALUES (?1, ?3, ?2)
                     ON CONFLICT (\"{o}\", \"{k}\") DO UPDATE SET \"{v}\" = excluded.\"{v}\"",
                    t = table.table,
                    o = table.owner,
                    k = table.key,
                    v = table.value
                ),
                table.table,
                table.value.to_owned(),
                Some(key),
            ),
        };
        if let Some(rule) = targets.column(column_table, &column)
            && let Err(reason) = rule.check(&change.value)
        {
            return Ok(Outcome::Rejected(reason));
        }
        let local = read_clock(
            self.local(),
            &change.entity,
            change.entity_id,
            &change.field,
        )?;
        if local.is_some_and(|local| {
            Clock {
                hlc: local.hlc,
                device: local.device,
            } >= change.clock
        }) {
            return Ok(Outcome::Ignored);
        }
        match key {
            None => self
                .local()
                .execute(&sql, params![change.entity_id, change.value])?,
            Some(key) => self
                .local()
                .execute(&sql, params![change.entity_id, change.value, key])?,
        };
        self.local().execute(
            "INSERT INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)
             ON CONFLICT (entity_type, entity_id, field)
             DO UPDATE SET hlc = excluded.hlc, device = excluded.device, pushed = 1",
            params![
                change.entity,
                change.entity_id,
                change.field,
                change.clock.hlc.to_stored(),
                change.clock.device
            ],
        )?;
        Ok(Outcome::Applied)
    }

    /// Applies one remote append-only row: it is inserted if its ID is new, and never listed as
    /// unpushed. Columns this build does not know are kept in `unknown_row_value`, and so is every
    /// column of an entity type it does not know.
    pub(crate) fn apply_row(
        &mut self,
        targets: &Targets,
        change: &RowChange,
    ) -> Result<RowOutcome, CollectionError> {
        let Some(table) = targets
            .append_only
            .iter()
            .find(|t| t.entity == change.entity)
        else {
            return Ok(self.keep_columns(&change.entity, change.id, &change.columns)?);
        };
        let (known, extra): (Vec<_>, Vec<_>) = change
            .columns
            .iter()
            .cloned()
            .partition(|(name, _)| table.columns.contains(&name.as_str()));
        for column in table.columns {
            let given = known.iter().find(|(name, _)| name == column);
            let Some(rule) = targets.column(table.table, column) else {
                continue;
            };
            match given {
                Some((_, value)) => {
                    if let Err(reason) = rule.check(value) {
                        return Ok(RowOutcome::Rejected(format!("column `{column}`: {reason}")));
                    }
                }
                None if rule.not_null => {
                    return Ok(RowOutcome::Rejected(format!(
                        "column `{column}` is missing"
                    )));
                }
                None => {}
            }
        }
        let names: String = known.iter().map(|(n, _)| format!(", \"{n}\"")).collect();
        let marks = ", ?".repeat(known.len());
        let args = std::iter::once(Value::Blob(change.id.as_bytes().to_vec()))
            .chain(known.iter().map(|(_, v)| v.clone()));
        let added = self.local().execute(
            &format!(
                "INSERT OR IGNORE INTO \"{}\" (id{names}) VALUES (?{marks})",
                table.table
            ),
            params_from_iter(args),
        )?;
        let kept = self.keep_columns(&change.entity, change.id, &extra)?;
        Ok(if added == 1 && kept == RowOutcome::Unknown {
            RowOutcome::AddedWithUnknown
        } else if added == 1 {
            RowOutcome::Added
        } else if kept == RowOutcome::Unknown {
            RowOutcome::Unknown
        } else {
            RowOutcome::Present
        })
    }

    /// Keeps columns unchanged in the unknown-row store. `Unknown` if any was new.
    fn keep_columns(
        &mut self,
        entity: &str,
        id: Id,
        columns: &[(String, Value)],
    ) -> rusqlite::Result<RowOutcome> {
        let mut new = false;
        for (column, value) in columns {
            new |= self.local().execute(
                "INSERT OR IGNORE INTO unknown_row_value (entity_type, row_id, column, value)
                 VALUES (?1, ?2, ?3, ?4)",
                params![entity, id, column, value],
            )? == 1;
        }
        Ok(if new {
            RowOutcome::Unknown
        } else {
            RowOutcome::Present
        })
    }
}

/// What the registers and rows of a batch changed, for the derived state.
#[derive(Default)]
struct Touched {
    notes: BTreeSet<Id>,
    note_types: BTreeSet<Id>,
    /// `(table, id)` of changed note type fields and templates, whose note type is read after the
    /// batch is applied.
    note_type_children: Vec<(&'static str, Id)>,
    event_cards: BTreeSet<Id>,
    new_sets: BTreeSet<Id>,
}

impl Collection {
    /// Applies a batch from another collection in one write transaction (ADR 0008, part 3). It
    /// can be applied again, in another order or in halves, with the same result. A change that does
    /// not fit its column is rejected and reported; the rest of the batch still applies. A failure
    /// of the database rolls everything back.
    pub fn merge(&self, changes: &Changes) -> Result<MergeReport, CollectionError> {
        let targets = Targets::load(&self.conn, self.schema)?;
        self.write(|w| {
            let mut report = MergeReport::default();
            let mut touched = Touched::default();
            // The clock first: whatever the reconcile writes below is stamped after everything
            // received, including registers that lose.
            if let Some(highest) = changes.registers.iter().map(|r| r.clock.hlc).max() {
                w.hlc = w.hlc.observe(highest);
            }
            for change in &changes.registers {
                match w.apply_register(&targets, change)? {
                    Outcome::Applied => {
                        report.registers_applied += 1;
                        touch_register(&mut touched, change);
                    }
                    Outcome::Ignored => report.registers_ignored += 1,
                    Outcome::Unknown => report.unknown_registers += 1,
                    Outcome::Rejected(reason) => report.rejected.push(Rejected {
                        entity: change.entity.clone(),
                        id: change.entity_id,
                        field: Some(change.field.clone()),
                        reason,
                    }),
                }
            }
            for change in &changes.rows {
                match w.apply_row(&targets, change)? {
                    RowOutcome::Added => {
                        report.rows_added += 1;
                        touch_row(&mut touched, change);
                    }
                    RowOutcome::AddedWithUnknown => {
                        report.rows_added += 1;
                        report.unknown_rows += 1;
                        touch_row(&mut touched, change);
                    }
                    RowOutcome::Present => {}
                    RowOutcome::Unknown => report.unknown_rows += 1,
                    RowOutcome::Rejected(reason) => report.rejected.push(Rejected {
                        entity: change.entity.clone(),
                        id: change.id,
                        field: None,
                        reason,
                    }),
                }
            }
            self.reconcile_after_merge(w, &touched, &mut report)?;
            self.rebuild_after_merge(w, &touched, &mut report)?;
            Ok(report)
        })
    }

    /// Cards of notes (ADR 0008, part 5): every live note whose fields the batch changed, and every
    /// live note of a note type it changed, is brought in line with its note type. A change to a card
    /// register triggers nothing, so a card sent to the trash with its deck stays there.
    fn reconcile_after_merge(
        &self,
        w: &mut WriteTx<'_>,
        touched: &Touched,
        report: &mut MergeReport,
    ) -> Result<(), CollectionError> {
        let mut types = touched.note_types.clone();
        for (table, id) in &touched.note_type_children {
            let owner: Option<Vec<u8>> = self
                .conn
                .query_row(
                    &format!("SELECT note_type FROM \"{table}\" WHERE id = ?1"),
                    [id],
                    |row| row.get(0),
                )
                .ok();
            if let Some(owner) = owner.and_then(|bytes| Id::from_slice(&bytes)) {
                types.insert(owner);
            }
        }
        report.notes_reconciled += types
            .iter()
            .map(|t| self.reconcile_note_type_counting(w, *t))
            .sum::<Result<usize, _>>()?;
        // Notes whose own registers changed, and that were not covered by their note type.
        let mut by_type: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
        for note in &touched.notes {
            let found: Option<(Vec<u8>, i64)> = self
                .conn
                .query_row(
                    "SELECT note_type, deleted FROM note WHERE id = ?1",
                    [note],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();
            if let Some((note_type, 0)) = found
                && let Some(note_type) = Id::from_slice(&note_type)
                && !types.contains(&note_type)
            {
                by_type.entry(note_type).or_default().push(*note);
            }
        }
        if !by_type.is_empty() {
            let dead = dead_decks(&self.conn)?;
            for (note_type, notes) in by_type {
                report.notes_reconciled += self.reconcile_notes(w, note_type, &notes, &dead)?;
            }
        }
        Ok(())
    }

    /// The schedule cache (ADR 0008, part 5): a card with a new event, or with an event that names
    /// a parameter set that just arrived, is folded again.
    fn rebuild_after_merge(
        &self,
        w: &mut WriteTx<'_>,
        touched: &Touched,
        report: &mut MergeReport,
    ) -> Result<(), CollectionError> {
        let mut cards = touched.event_cards.clone();
        let sets: Vec<Id> = touched.new_sets.iter().copied().collect();
        for chunk in sets.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(", ");
            let mut statement = self.conn.prepare(&format!(
                "SELECT DISTINCT card FROM card_event WHERE parameters IN ({marks})"
            ))?;
            let found = statement
                .query_map(params_from_iter(chunk), |row| row.get::<_, Id>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            cards.extend(found);
        }
        if cards.is_empty() {
            return Ok(());
        }
        let sets = ParameterSets::load(w.local())?;
        for card in &cards {
            rebuild_card(w.local(), *card, &sets)?;
        }
        report.cards_rebuilt += cards.len();
        Ok(())
    }
}

fn touch_register(touched: &mut Touched, change: &RegisterChange) {
    match change.entity.as_str() {
        "note" => {
            touched.notes.insert(change.entity_id);
        }
        "note_type" => {
            touched.note_types.insert(change.entity_id);
        }
        "note_type_field" => touched
            .note_type_children
            .push(("note_type_field", change.entity_id)),
        "template" => touched
            .note_type_children
            .push(("template", change.entity_id)),
        _ => {}
    }
}

fn touch_row(touched: &mut Touched, change: &RowChange) {
    if change.entity == CARD_EVENT.entity {
        let card = change
            .columns
            .iter()
            .find(|(name, _)| name == "card")
            .and_then(|(_, value)| match value {
                Value::Blob(bytes) => Id::from_slice(bytes),
                _ => None,
            });
        touched.event_cards.extend(card);
    } else if change.entity == PARAMETER_SET.entity {
        touched.new_sets.insert(change.id);
    }
}
