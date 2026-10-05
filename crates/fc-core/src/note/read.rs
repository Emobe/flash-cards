//! Reading notes and cards. Nothing here writes.

use std::collections::HashMap;

use super::{Card, Note, NoteError, NoteField};
use crate::collection::Collection;
use crate::id::Id;

impl Collection {
    fn build_notes(&self, rows: Vec<(Id, Id, bool)>) -> Result<Vec<Note>, NoteError> {
        let mut types = HashMap::new();
        let mut notes = Vec::with_capacity(rows.len());
        let mut statement = self
            .conn
            .prepare("SELECT field, value FROM note_field_value WHERE note = ?1")?;
        for (id, note_type, deleted) in rows {
            if let std::collections::hash_map::Entry::Vacant(entry) = types.entry(note_type) {
                entry.insert(self.note_type(note_type)?);
            }
            let values: HashMap<Id, String> = statement
                .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<_, _>>()?;
            let fields = types[&note_type]
                .as_ref()
                .map(|nt| {
                    nt.fields
                        .iter()
                        .map(|field| NoteField {
                            field: field.id,
                            name: field.name.clone(),
                            value: values.get(&field.id).cloned().unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            notes.push(Note {
                id,
                note_type,
                fields,
                deleted,
            });
        }
        Ok(notes)
    }

    fn note_rows(&self, filter: &str, arg: Option<Id>) -> Result<Vec<Note>, NoteError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT id, note_type, deleted FROM note WHERE {filter} ORDER BY id"
        ))?;
        let map = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Id>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        };
        let rows = match arg {
            Some(arg) => statement
                .query_map([arg], map)?
                .collect::<Result<Vec<_>, _>>()?,
            None => statement
                .query_map([], map)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        self.build_notes(rows)
    }

    /// One note, deleted or not.
    pub fn note(&self, id: Id) -> Result<Option<Note>, NoteError> {
        Ok(self.note_rows("id = ?1", Some(id))?.pop())
    }

    /// The live notes of a note type, oldest first.
    pub fn notes(&self, note_type: Id) -> Result<Vec<Note>, NoteError> {
        self.note_rows("note_type = ?1 AND deleted = 0", Some(note_type))
    }

    /// The deleted notes (the trash), oldest first.
    pub fn deleted_notes(&self) -> Result<Vec<Note>, NoteError> {
        self.note_rows("deleted = 1", None)
    }

    fn cards_with(&self, note: Id, deleted: bool) -> Result<Vec<Card>, NoteError> {
        let mut statement = self.conn.prepare(
            "SELECT c.id, c.template, c.ordinal, n.note_type FROM card c
             JOIN note n ON n.id = c.note
             WHERE c.note = ?1 AND c.deleted = ?2 AND (n.deleted = 0 OR ?2 = 1)",
        )?;
        let mut found: Vec<(Card, Id)> = statement
            .query_map(rusqlite::params![note, i64::from(deleted)], |row| {
                Ok((
                    Card {
                        id: row.get(0)?,
                        note,
                        template: row.get(1)?,
                        ordinal: row.get(2)?,
                        deleted,
                    },
                    row.get(3)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        let order: Vec<Id> = match found.first() {
            Some((_, note_type)) => self
                .note_type(*note_type)?
                .map(|nt| nt.templates.iter().map(|t| t.id).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let place = |card: &Card| {
            order
                .iter()
                .position(|t| *t == card.template)
                .unwrap_or(usize::MAX)
        };
        found.sort_by_key(|(card, _)| (place(card), card.ordinal, card.id));
        Ok(found.into_iter().map(|(card, _)| card).collect())
    }

    /// The live cards of a live note, in template order and then cloze number.
    pub fn cards_of_note(&self, note: Id) -> Result<Vec<Card>, NoteError> {
        self.cards_with(note, false)
    }

    /// The deleted cards of a note, which come back if the note makes them again.
    pub fn deleted_cards_of_note(&self, note: Id) -> Result<Vec<Card>, NoteError> {
        self.cards_with(note, true)
    }
}
