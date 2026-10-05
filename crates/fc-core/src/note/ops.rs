//! Changing notes. Every change goes through `Collection::write`, with the checks first (one
//! connection serves the collection, so nothing changes in between). A change that edits a note also
//! brings its cards in line, in the same transaction.

use rusqlite::types::Value;

use super::generate::{NoteState, reconcile};
use super::scan::comparison_key;
use super::{CARD, NOTE, NOTE_VALUE, NoteError};
use crate::collection::{Collection, CollectionError};
use crate::id::Id;
use crate::notetype::NoteType;
use crate::sync::WriteTx;

fn flag(on: bool) -> Value {
    Value::Integer(i64::from(on))
}

fn blob(id: Id) -> Value {
    Value::Blob(id.as_bytes().to_vec())
}

/// What `add_note` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedNote {
    pub id: Id,
    pub cards: Vec<Id>,
    /// Other live notes of the same note type with the same first field. A warning, never a block.
    pub duplicates: Vec<Id>,
}

/// What `set_note_fields` and `restore_note` did to a note's cards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteChange {
    /// Cards that were created, or came back with their history.
    pub added_cards: Vec<Id>,
    /// Cards that were deleted because the note no longer fills their template.
    pub removed_cards: Vec<Id>,
    /// Other live notes with the same first field, when this change set the first field.
    pub duplicates: Vec<Id>,
}

impl Collection {
    /// A live note type with its fields and templates.
    fn live_note_type(&self, id: Id) -> Result<NoteType, NoteError> {
        match self.note_type(id)? {
            Some(found) if !found.deleted => Ok(found),
            _ => Err(NoteError::NotFound),
        }
    }

    fn check_fields(note_type: &NoteType, values: &[(Id, &str)]) -> Result<(), NoteError> {
        if values
            .iter()
            .all(|(field, _)| note_type.fields.iter().any(|f| f.id == *field))
        {
            Ok(())
        } else {
            Err(NoteError::UnknownField)
        }
    }

    /// Adds a note, with values for fields by field ID (a field left out is empty), and makes its
    /// cards. Refused with `NoCards` if no card would be made, because the note would be invisible.
    pub fn add_note(&self, note_type: Id, values: &[(Id, &str)]) -> Result<AddedNote, NoteError> {
        let found = self.live_note_type(note_type)?;
        Self::check_fields(&found, values)?;
        let mut state = NoteState {
            id: Id::from_bytes([0; 16]),
            values: std::collections::HashMap::new(),
            cards: std::collections::HashMap::new(),
        };
        for (field, value) in values {
            state.values.insert(*field, (*value).to_owned());
        }
        if super::generate::wanted(&found, &state.values).is_empty() {
            return Err(NoteError::NoCards {
                cloze: found.kind == crate::notetype::Kind::Cloze,
            });
        }
        let first = found.fields.first().map(|f| f.id);
        let duplicates = match first.and_then(|f| state.values.get(&f)) {
            Some(value) => self.find_duplicates(note_type, value)?,
            None => Vec::new(),
        };
        let (id, done) = self.write(|w| {
            let id = w.new_id()?;
            w.insert(
                NOTE.entity,
                id,
                vec![("note_type", blob(note_type)), ("deleted", flag(false))],
            )?;
            for field in &found.fields {
                if let Some(value) = state.values.get(&field.id) {
                    w.set_value(NOTE_VALUE.entity, id, field.id, value)?;
                }
            }
            state.id = id;
            let done = reconcile(w, &found, &state)?;
            Ok((id, done))
        })?;
        Ok(AddedNote {
            id,
            cards: done.added,
            duplicates,
        })
    }

    /// The live note with this ID, with its note type.
    fn live_note(&self, note: Id) -> Result<NoteType, NoteError> {
        let stored: Option<(Id, i64)> = self
            .conn
            .query_row(
                "SELECT note_type, deleted FROM note WHERE id = ?1",
                [note],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        match stored {
            Some((note_type, 0)) => Ok(self.note_type(note_type)?.ok_or(NoteError::NotFound)?),
            _ => Err(NoteError::NotFound),
        }
    }

    /// Sets values of a note's fields, by field ID. Only a value that changes is written, so edits
    /// of different fields on two devices both survive a merge. Cards are added, restored or
    /// deleted to match: filling a field a template needs makes its card, emptying it removes it.
    pub fn set_note_fields(
        &self,
        note: Id,
        values: &[(Id, &str)],
    ) -> Result<NoteChange, NoteError> {
        let found = self.live_note(note)?;
        Self::check_fields(&found, values)?;
        let mut state = self.note_state(note)?;
        let first = found.fields.first().map(|f| f.id);
        let mut changed = Vec::new();
        for (field, value) in values {
            let current = state.values.get(field).map_or("", String::as_str);
            if current != *value {
                changed.push((*field, (*value).to_owned()));
                state.values.insert(*field, (*value).to_owned());
            }
        }
        let duplicates = match first {
            Some(first) if changed.iter().any(|(f, _)| *f == first) => {
                let value = state.values.get(&first).map_or("", String::as_str);
                self.find_duplicates(found.id, value)?
                    .into_iter()
                    .filter(|id| *id != note)
                    .collect()
            }
            _ => Vec::new(),
        };
        let done = self.write(|w| {
            for (field, value) in &changed {
                w.set_value(NOTE_VALUE.entity, note, *field, value)?;
            }
            reconcile(w, &found, &state)
        })?;
        Ok(NoteChange {
            added_cards: done.added,
            removed_cards: done.removed,
            duplicates,
        })
    }

    /// Deletes a note: it and its cards move to the trash, and the cards' history stays. Deleting
    /// a deleted note does nothing.
    pub fn delete_note(&self, note: Id) -> Result<(), NoteError> {
        match self.note_deleted(note)? {
            None => Err(NoteError::NotFound),
            Some(true) => Ok(()),
            Some(false) => Ok(self.write(|w| {
                w.set(NOTE.entity, note, "deleted", flag(true))?;
                self.tombstone_cards(w, "WHERE note = ?1", note)
            })?),
        }
    }

    /// Brings a deleted note back, with the cards it should have: the same cards, with their
    /// history, for what is still filled in. Restoring a live note does nothing.
    pub fn restore_note(&self, note: Id) -> Result<NoteChange, NoteError> {
        match self.note_deleted(note)? {
            None => Err(NoteError::NotFound),
            Some(false) => Ok(NoteChange::default()),
            Some(true) => {
                let note_type: Id = self.conn.query_row(
                    "SELECT note_type FROM note WHERE id = ?1",
                    [note],
                    |row| row.get(0),
                )?;
                let found = self.note_type(note_type)?.ok_or(NoteError::NotFound)?;
                let done = self.write(|w| {
                    w.set(NOTE.entity, note, "deleted", flag(false))?;
                    reconcile(w, &found, &self.note_state(note)?)
                })?;
                Ok(NoteChange {
                    added_cards: done.added,
                    removed_cards: done.removed,
                    duplicates: Vec::new(),
                })
            }
        }
    }

    fn note_deleted(&self, note: Id) -> Result<Option<bool>, NoteError> {
        match self
            .conn
            .query_row("SELECT deleted FROM note WHERE id = ?1", [note], |row| {
                row.get::<_, i64>(0)
            }) {
            Ok(deleted) => Ok(Some(deleted != 0)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Deletes every live card matched by `filter` (a WHERE clause on `card`, with `?1` bound).
    fn tombstone_cards(
        &self,
        w: &mut WriteTx<'_>,
        filter: &str,
        arg: Id,
    ) -> Result<(), CollectionError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT id FROM card {filter} AND deleted = 0 ORDER BY id"
        ))?;
        let ids = statement
            .query_map([arg], |row| row.get::<_, Id>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for id in ids {
            w.set(CARD.entity, id, "deleted", flag(true))?;
        }
        Ok(())
    }

    /// Deletes every live note of a note type, and their cards: the tombstones that deleting a
    /// note type writes (ADR 0006, section 5).
    pub(crate) fn tombstone_notes_of(
        &self,
        w: &mut WriteTx<'_>,
        note_type: Id,
    ) -> Result<(), CollectionError> {
        let mut statement = self
            .conn
            .prepare("SELECT id FROM note WHERE note_type = ?1 AND deleted = 0 ORDER BY id")?;
        let notes = statement
            .query_map([note_type], |row| row.get::<_, Id>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for note in notes {
            w.set(NOTE.entity, note, "deleted", flag(true))?;
        }
        let mut statement = self.conn.prepare(
            "SELECT c.id FROM card c JOIN note n ON n.id = c.note
             WHERE n.note_type = ?1 AND c.deleted = 0 ORDER BY c.id",
        )?;
        let cards = statement
            .query_map([note_type], |row| row.get::<_, Id>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for card in cards {
            w.set(CARD.entity, card, "deleted", flag(true))?;
        }
        Ok(())
    }

    /// Restores the notes of a note type that were deleted with it: those whose `deleted` register
    /// was written at or after `since` (the clock of the note type's own delete). A note deleted
    /// on its own, before, stays in the trash. Their cards come back when the note type's cards are
    /// reconciled.
    pub(crate) fn restore_notes_deleted_since(
        &self,
        w: &mut WriteTx<'_>,
        note_type: Id,
        since: i64,
    ) -> Result<(), CollectionError> {
        let mut statement = self.conn.prepare(
            "SELECT n.id FROM note n
             JOIN register_clock rc ON rc.entity_type = 'note' AND rc.entity_id = n.id
                                   AND rc.field = 'deleted'
             WHERE n.note_type = ?1 AND n.deleted = 1 AND rc.hlc >= ?2 ORDER BY n.id",
        )?;
        let notes = statement
            .query_map(rusqlite::params![note_type, since], |row| {
                row.get::<_, Id>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for note in notes {
            w.set(NOTE.entity, note, "deleted", flag(false))?;
        }
        Ok(())
    }

    /// The live notes of a note type whose first field equals `value`, ignoring tags, spacing and
    /// case. The first field is the first live field of the note type. An empty value has no
    /// duplicates.
    pub fn find_duplicates(&self, note_type: Id, value: &str) -> Result<Vec<Id>, NoteError> {
        let key = comparison_key(value);
        if key.is_empty() {
            return Ok(Vec::new());
        }
        let Some(first) = self
            .note_type(note_type)?
            .and_then(|nt| nt.fields.first().map(|f| f.id))
        else {
            return Ok(Vec::new());
        };
        let mut statement = self.conn.prepare(
            "SELECT n.id, v.value FROM note n
             JOIN note_field_value v ON v.note = n.id AND v.field = ?2
             WHERE n.note_type = ?1 AND n.deleted = 0 ORDER BY n.id",
        )?;
        let mut found = Vec::new();
        for row in statement.query_map(rusqlite::params![note_type, first], |row| {
            Ok((row.get::<_, Id>(0)?, row.get::<_, String>(1)?))
        })? {
            let (id, stored) = row?;
            if comparison_key(&stored) == key {
                found.push(id);
            }
        }
        Ok(found)
    }
}
