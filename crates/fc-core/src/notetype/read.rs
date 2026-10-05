//! Reading note types. Nothing here writes, and deleted rows are left out unless asked for.

use rusqlite::{Connection, params};

use super::{Field, Kind, NoteType, NoteTypeError, Template};
use crate::collection::Collection;
use crate::id::Id;

/// A field or template as the ordering code needs it.
#[derive(Debug, Clone)]
pub(super) struct Item {
    pub id: Id,
    pub name: String,
    pub position: String,
}

/// Which of the two ordered lists of a note type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Field,
    Template,
}

impl Part {
    pub fn entity(self) -> &'static str {
        match self {
            Self::Field => super::FIELD.entity,
            Self::Template => super::TEMPLATE.entity,
        }
    }

    pub fn table(self) -> &'static str {
        match self {
            Self::Field => super::FIELD.table,
            Self::Template => super::TEMPLATE.table,
        }
    }
}

/// The live items of a note type, in order: by position, and by ID where positions are equal, so
/// every device shows the same order.
pub(super) fn live_items(
    conn: &Connection,
    part: Part,
    note_type: Id,
) -> rusqlite::Result<Vec<Item>> {
    let mut statement = conn.prepare(&format!(
        "SELECT id, name, position FROM {} WHERE note_type = ?1 AND deleted = 0
         ORDER BY position, id",
        part.table()
    ))?;
    statement
        .query_map(params![note_type], |row| {
            Ok(Item {
                id: row.get(0)?,
                name: row.get(1)?,
                position: row.get(2)?,
            })
        })?
        .collect()
}

/// What is stored about one field or template.
pub(super) struct ItemInfo {
    pub note_type: Id,
    pub name: String,
    pub deleted: bool,
}

pub(super) fn item_info(
    conn: &Connection,
    part: Part,
    id: Id,
) -> rusqlite::Result<Option<ItemInfo>> {
    let mut statement = conn.prepare(&format!(
        "SELECT note_type, name, deleted FROM {} WHERE id = ?1",
        part.table()
    ))?;
    let mut rows = statement.query_map(params![id], |row| {
        Ok(ItemInfo {
            note_type: row.get(0)?,
            name: row.get(1)?,
            deleted: row.get::<_, i64>(2)? != 0,
        })
    })?;
    rows.next().transpose()
}

/// A note type is deleted if its `deleted` register says so and no live note still uses it. A note
/// added on another device while this one deleted the type keeps it alive (ADR 0006, section 5).
/// Decided here, when reading, so nothing is written and nothing can loop.
const EFFECTIVELY_DELETED: &str = "(nt.deleted <> 0 AND NOT EXISTS
    (SELECT 1 FROM note n WHERE n.note_type = nt.id AND n.deleted = 0))";

/// What is stored about one note type.
pub(super) struct TypeInfo {
    pub kind: Kind,
    /// What its `deleted` register says.
    pub register_deleted: bool,
    /// Whether it is deleted as far as anyone can tell: the register, unless a live note keeps it.
    pub deleted: bool,
}

pub(super) fn note_type_info(conn: &Connection, id: Id) -> rusqlite::Result<Option<TypeInfo>> {
    let mut statement = conn.prepare(&format!(
        "SELECT kind, deleted, {EFFECTIVELY_DELETED} FROM note_type nt WHERE id = ?1"
    ))?;
    let mut rows = statement.query_map(params![id], |row| {
        Ok(TypeInfo {
            kind: Kind::from_text(&row.get::<_, String>(0)?),
            register_deleted: row.get::<_, i64>(1)? != 0,
            deleted: row.get::<_, i64>(2)? != 0,
        })
    })?;
    rows.next().transpose()
}

fn load(
    conn: &Connection,
    only: Option<Id>,
    deleted: Option<bool>,
) -> Result<Vec<NoteType>, NoteTypeError> {
    let mut statement = conn.prepare(&format!(
        "SELECT id, name, kind, css, sort_field, {EFFECTIVELY_DELETED} FROM note_type nt
         WHERE (?1 IS NULL OR id = ?1) AND (?2 IS NULL OR {EFFECTIVELY_DELETED} = ?2)"
    ))?;
    let rows = statement
        .query_map(params![only, deleted], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<Id>>(4)?,
                row.get::<_, i64>(5)? != 0,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut note_types = Vec::with_capacity(rows.len());
    for (id, name, kind, css, chosen, deleted) in rows {
        let fields: Vec<Field> = live_items(conn, Part::Field, id)?
            .into_iter()
            .map(|item| Field {
                id: item.id,
                name: item.name,
            })
            .collect();
        let mut statement = conn.prepare(
            "SELECT id, name, front, back FROM template WHERE note_type = ?1 AND deleted = 0
             ORDER BY position, id",
        )?;
        let templates = statement
            .query_map(params![id], |row| {
                Ok(Template {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    front: row.get(2)?,
                    back: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let sort_field = chosen
            .filter(|chosen| fields.iter().any(|field| field.id == *chosen))
            .or_else(|| fields.first().map(|field| field.id));
        note_types.push(NoteType {
            id,
            name,
            kind: Kind::from_text(&kind),
            css,
            sort_field,
            fields,
            templates,
            deleted,
        });
    }
    note_types.sort_by_cached_key(|nt| (nt.name.to_lowercase(), nt.id));
    Ok(note_types)
}

impl Collection {
    /// Every note type that is not deleted, by name.
    pub fn note_types(&self) -> Result<Vec<NoteType>, NoteTypeError> {
        load(&self.conn, None, Some(false))
    }

    /// The deleted note types (the trash), by name.
    pub fn deleted_note_types(&self) -> Result<Vec<NoteType>, NoteTypeError> {
        load(&self.conn, None, Some(true))
    }

    /// One note type, deleted or not.
    pub fn note_type(&self, id: Id) -> Result<Option<NoteType>, NoteTypeError> {
        Ok(load(&self.conn, Some(id), None)?.pop())
    }
}
