//! Changing note types. Every change goes through `Collection::write`, so every register it writes
//! gets a clock. The checks run before the write: one connection serves the collection, so nothing
//! changes in between. Rules that a merge could break (a note type with no field left, two fields
//! with one name) are checked here, for the person using this device, and are not constraints in
//! the database (ADR 0006, section 6).

use rusqlite::types::Value;

use super::builtin::{self, CLOZE, Def};
use super::position::{between, evenly, strictly_between};
use super::read::{Item, Part, item_info, live_items, note_type_info};
use super::{FIELD, Kind, NOTE_TYPE, NoteTypeError, TEMPLATE};
use crate::collection::Collection;
use crate::id::Id;
use crate::sync::WriteTx;

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn blob(id: Id) -> Value {
    Value::Blob(id.as_bytes().to_vec())
}

fn flag(on: bool) -> Value {
    Value::Integer(i64::from(on))
}

fn clean_name(name: &str) -> Result<String, NoteTypeError> {
    let name = name.trim();
    if name.is_empty() {
        Err(NoteTypeError::EmptyName)
    } else {
        Ok(name.to_owned())
    }
}

/// Fails if a live item other than `except` already has this name (ignoring case).
fn check_name_free(items: &[Item], name: &str, except: Option<Id>) -> Result<(), NoteTypeError> {
    let wanted = name.to_lowercase();
    if items
        .iter()
        .any(|item| Some(item.id) != except && item.name.to_lowercase() == wanted)
    {
        Err(NoteTypeError::NameTaken(name.to_owned()))
    } else {
        Ok(())
    }
}

/// Where an item goes among the others: a position that fits between its neighbours, or, when the
/// neighbours leave no room (equal or malformed positions after a merge), new positions for the
/// whole list.
enum Slot {
    Between(String),
    Renumber(Vec<String>),
}

fn slot(others: &[Item], index: usize) -> Slot {
    let before = index.checked_sub(1).map(|i| others[i].position.as_str());
    let after = others.get(index).map(|other| other.position.as_str());
    if strictly_between(before, after) {
        Slot::Between(between(before, after))
    } else {
        Slot::Renumber(evenly(others.len() + 1))
    }
}

/// Applies `slot` for an item that goes at `index` among `others`, and returns the position of the
/// item itself.
fn place(
    w: &mut WriteTx<'_>,
    part: Part,
    others: &[Item],
    index: usize,
    slot: Slot,
) -> Result<String, crate::collection::CollectionError> {
    match slot {
        Slot::Between(position) => Ok(position),
        Slot::Renumber(mut positions) => {
            let own = positions.remove(index);
            for (other, position) in others.iter().zip(positions) {
                if other.position != position {
                    w.set(part.entity(), other.id, "position", Value::Text(position))?;
                }
            }
            Ok(own)
        }
    }
}

impl Collection {
    fn live_type(&self, id: Id) -> Result<Kind, NoteTypeError> {
        match note_type_info(&self.conn, id)? {
            Some(info) if !info.deleted => Ok(info.kind),
            _ => Err(NoteTypeError::NotFound),
        }
    }

    /// A note type of this kind with the standard fields and one template: Front and Back with one
    /// card for Basic, Text and Extra with a cloze template for Cloze. Add or change fields and
    /// templates afterwards.
    pub fn create_note_type(&self, name: &str, kind: Kind) -> Result<Id, NoteTypeError> {
        let name = clean_name(name)?;
        let shape: &Def = match kind {
            Kind::Standard => &builtin::BASIC,
            Kind::Cloze => &CLOZE,
        };
        let positions = evenly(shape.fields.len().max(shape.templates.len()));
        Ok(self.write(|w| {
            let id = w.new_id()?;
            let field_ids = shape
                .fields
                .iter()
                .map(|_| w.new_id())
                .collect::<Result<Vec<_>, _>>()?;
            w.insert(
                NOTE_TYPE.entity,
                id,
                vec![
                    ("name", text(&name)),
                    ("kind", text(kind.as_text())),
                    ("css", text(builtin::CSS)),
                    ("sort_field", blob(field_ids[0])),
                    ("deleted", flag(false)),
                ],
            )?;
            for ((field_id, field), position) in field_ids.iter().zip(shape.fields).zip(&positions)
            {
                w.insert(
                    FIELD.entity,
                    *field_id,
                    vec![
                        ("note_type", blob(id)),
                        ("name", text(field)),
                        ("position", text(position)),
                        ("deleted", flag(false)),
                    ],
                )?;
            }
            for (template, position) in shape.templates.iter().zip(&positions) {
                let template_id = w.new_id()?;
                w.insert(
                    TEMPLATE.entity,
                    template_id,
                    vec![
                        ("note_type", blob(id)),
                        ("name", text(template.name)),
                        ("position", text(position)),
                        ("front", text(template.front)),
                        ("back", text(template.back)),
                        ("deleted", flag(false)),
                    ],
                )?;
            }
            Ok(id)
        })?)
    }

    pub fn rename_note_type(&self, id: Id, name: &str) -> Result<(), NoteTypeError> {
        let name = clean_name(name)?;
        self.live_type(id)?;
        Ok(self.write(|w| w.set(NOTE_TYPE.entity, id, "name", Value::Text(name)))?)
    }

    pub fn set_note_type_css(&self, id: Id, css: &str) -> Result<(), NoteTypeError> {
        self.live_type(id)?;
        Ok(self.write(|w| w.set(NOTE_TYPE.entity, id, "css", text(css)))?)
    }

    /// Chooses the field used to sort and name notes. It must be a live field of this note type.
    pub fn set_sort_field(&self, note_type: Id, field: Id) -> Result<(), NoteTypeError> {
        self.live_type(note_type)?;
        if !live_items(&self.conn, Part::Field, note_type)?
            .iter()
            .any(|item| item.id == field)
        {
            return Err(NoteTypeError::NotFound);
        }
        Ok(self.write(|w| w.set(NOTE_TYPE.entity, note_type, "sort_field", blob(field)))?)
    }

    /// Deletes a note type: it moves to the trash and can be restored with everything in it. Its
    /// fields and templates are not touched, they are hidden with it. Its notes and their cards are
    /// deleted with it, one tombstone each (ADR 0006, section 5). Deleting a deleted note type does
    /// nothing.
    pub fn delete_note_type(&self, id: Id) -> Result<(), NoteTypeError> {
        match note_type_info(&self.conn, id)? {
            None => Err(NoteTypeError::NotFound),
            Some(info) if info.deleted => Ok(()),
            Some(info) => Ok(self.write(|w| {
                if !info.register_deleted {
                    w.set(NOTE_TYPE.entity, id, "deleted", flag(true))?;
                }
                self.tombstone_notes_of(w, id)
            })?),
        }
    }

    /// Brings a deleted note type back, with the notes that were deleted along with it and their
    /// cards. A note deleted on its own before stays in the trash. Restoring one that is not
    /// deleted does nothing.
    pub fn restore_note_type(&self, id: Id) -> Result<(), NoteTypeError> {
        match note_type_info(&self.conn, id)? {
            None => Err(NoteTypeError::NotFound),
            Some(info) if !info.register_deleted => Ok(()),
            Some(_) => {
                let since = self
                    .register_clock(NOTE_TYPE.entity, id, "deleted")?
                    .map_or(0, |clock| clock.hlc.to_stored());
                Ok(self.write(|w| {
                    w.set(NOTE_TYPE.entity, id, "deleted", flag(false))?;
                    self.restore_notes_deleted_since(w, id, since)?;
                    self.reconcile_note_type(w, id)
                })?)
            }
        }
    }

    // Fields.

    /// Adds a field at the end.
    pub fn add_field(&self, note_type: Id, name: &str) -> Result<Id, NoteTypeError> {
        self.live_type(note_type)?;
        self.add_part(Part::Field, note_type, name, Vec::new())
    }

    pub fn rename_field(&self, note_type: Id, field: Id, name: &str) -> Result<(), NoteTypeError> {
        self.rename_part(Part::Field, note_type, field, name)
    }

    /// Moves a field to `index` among the live fields (counting from 0, past the end means last).
    /// Only the moved field is written.
    pub fn move_field(&self, note_type: Id, field: Id, index: usize) -> Result<(), NoteTypeError> {
        self.move_part(Part::Field, note_type, field, index)
    }

    /// Removes a field. Its values stay in the notes, hidden, and come back if the field is
    /// restored. The last field cannot be removed.
    pub fn remove_field(&self, note_type: Id, field: Id) -> Result<(), NoteTypeError> {
        self.remove_part(Part::Field, note_type, field)
    }

    /// Brings a removed field back, at its old place. Fails if a live field has its name now.
    pub fn restore_field(&self, note_type: Id, field: Id) -> Result<(), NoteTypeError> {
        self.restore_part(Part::Field, note_type, field)
    }

    /// The removed fields of a note type, which can be restored.
    pub fn removed_fields(&self, note_type: Id) -> Result<Vec<super::Field>, NoteTypeError> {
        self.removed_parts(Part::Field, note_type).map(|items| {
            items
                .into_iter()
                .map(|item| super::Field {
                    id: item.id,
                    name: item.name,
                })
                .collect()
        })
    }

    // Templates.

    /// Adds a card template at the end. A cloze note type has exactly one, so this fails for it.
    pub fn add_template(
        &self,
        note_type: Id,
        name: &str,
        front: &str,
        back: &str,
    ) -> Result<Id, NoteTypeError> {
        if self.live_type(note_type)? == Kind::Cloze {
            return Err(NoteTypeError::ClozeTemplate);
        }
        self.add_part(
            Part::Template,
            note_type,
            name,
            vec![("front", text(front)), ("back", text(back))],
        )
    }

    pub fn rename_template(
        &self,
        note_type: Id,
        template: Id,
        name: &str,
    ) -> Result<(), NoteTypeError> {
        self.rename_part(Part::Template, note_type, template, name)
    }

    /// Changes the text of a template. The text is stored as it is: step 1.4 checks it.
    pub fn set_template_text(
        &self,
        note_type: Id,
        template: Id,
        front: &str,
        back: &str,
    ) -> Result<(), NoteTypeError> {
        self.live_type(note_type)?;
        match item_info(&self.conn, Part::Template, template)? {
            Some(info) if info.note_type == note_type && !info.deleted => {}
            _ => return Err(NoteTypeError::NotFound),
        }
        Ok(self.write(|w| {
            w.set(TEMPLATE.entity, template, "front", text(front))?;
            w.set(TEMPLATE.entity, template, "back", text(back))?;
            self.reconcile_note_type(w, note_type)
        })?)
    }

    pub fn move_template(
        &self,
        note_type: Id,
        template: Id,
        index: usize,
    ) -> Result<(), NoteTypeError> {
        self.move_part(Part::Template, note_type, template, index)
    }

    /// Removes a template. Its cards are removed with it by step 1.3, and come back, with their
    /// history, if the template is restored. The last template cannot be removed.
    pub fn remove_template(&self, note_type: Id, template: Id) -> Result<(), NoteTypeError> {
        self.remove_part(Part::Template, note_type, template)
    }

    /// Brings a removed template back, at its old place.
    pub fn restore_template(&self, note_type: Id, template: Id) -> Result<(), NoteTypeError> {
        if self.live_type(note_type)? == Kind::Cloze
            && !live_items(&self.conn, Part::Template, note_type)?.is_empty()
        {
            return Err(NoteTypeError::ClozeTemplate);
        }
        self.restore_part(Part::Template, note_type, template)
    }

    /// The removed templates of a note type, which can be restored.
    pub fn removed_templates(&self, note_type: Id) -> Result<Vec<super::Template>, NoteTypeError> {
        let removed = self.removed_parts(Part::Template, note_type)?;
        let mut statement = self
            .conn
            .prepare("SELECT front, back FROM template WHERE id = ?1")?;
        removed
            .into_iter()
            .map(|item| {
                let (front, back) =
                    statement.query_row([item.id], |row| Ok((row.get(0)?, row.get(1)?)))?;
                Ok(super::Template {
                    id: item.id,
                    name: item.name,
                    front,
                    back,
                })
            })
            .collect()
    }

    // The same rules for both lists.

    fn add_part(
        &self,
        part: Part,
        note_type: Id,
        name: &str,
        extra: Vec<(&str, Value)>,
    ) -> Result<Id, NoteTypeError> {
        let name = clean_name(name)?;
        let items = live_items(&self.conn, part, note_type)?;
        check_name_free(&items, &name, None)?;
        let index = items.len();
        let slot = slot(&items, index);
        Ok(self.write(|w| {
            let id = w.new_id()?;
            let position = place(w, part, &items, index, slot)?;
            let mut values = vec![
                ("note_type", blob(note_type)),
                ("name", Value::Text(name)),
                ("position", Value::Text(position)),
                ("deleted", flag(false)),
            ];
            values.extend(extra);
            w.insert(part.entity(), id, values)?;
            if part == Part::Template {
                self.reconcile_note_type(w, note_type)?;
            }
            Ok(id)
        })?)
    }

    /// The live item `id` of `note_type`, or `NotFound`.
    fn live_item(&self, part: Part, note_type: Id, id: Id) -> Result<String, NoteTypeError> {
        self.live_type(note_type)?;
        match item_info(&self.conn, part, id)? {
            Some(info) if info.note_type == note_type && !info.deleted => Ok(info.name),
            _ => Err(NoteTypeError::NotFound),
        }
    }

    fn rename_part(
        &self,
        part: Part,
        note_type: Id,
        id: Id,
        name: &str,
    ) -> Result<(), NoteTypeError> {
        let name = clean_name(name)?;
        let current = self.live_item(part, note_type, id)?;
        if current == name {
            return Ok(());
        }
        check_name_free(&live_items(&self.conn, part, note_type)?, &name, Some(id))?;
        Ok(self.write(|w| {
            w.set(part.entity(), id, "name", Value::Text(name.clone()))?;
            if part == Part::Field {
                self.rewrite_field_references(w, note_type, &current, &name)?;
                self.reconcile_note_type(w, note_type)?;
            }
            Ok(())
        })?)
    }

    fn move_part(
        &self,
        part: Part,
        note_type: Id,
        id: Id,
        index: usize,
    ) -> Result<(), NoteTypeError> {
        self.live_item(part, note_type, id)?;
        let items = live_items(&self.conn, part, note_type)?;
        let from = items
            .iter()
            .position(|item| item.id == id)
            .ok_or(NoteTypeError::NotFound)?;
        let index = index.min(items.len() - 1);
        if from == index {
            return Ok(());
        }
        let others: Vec<Item> = items.into_iter().filter(|item| item.id != id).collect();
        let slot = slot(&others, index);
        Ok(self.write(|w| {
            let position = place(w, part, &others, index, slot)?;
            w.set(part.entity(), id, "position", Value::Text(position))
        })?)
    }

    fn remove_part(&self, part: Part, note_type: Id, id: Id) -> Result<(), NoteTypeError> {
        self.live_type(note_type)?;
        match item_info(&self.conn, part, id)? {
            Some(info) if info.note_type == note_type => {
                if info.deleted {
                    return Ok(());
                }
            }
            _ => return Err(NoteTypeError::NotFound),
        }
        if live_items(&self.conn, part, note_type)?.len() <= 1 {
            return Err(match part {
                Part::Field => NoteTypeError::LastField,
                Part::Template => NoteTypeError::LastTemplate,
            });
        }
        Ok(self.write(|w| {
            w.set(part.entity(), id, "deleted", flag(true))?;
            self.reconcile_note_type(w, note_type)
        })?)
    }

    fn restore_part(&self, part: Part, note_type: Id, id: Id) -> Result<(), NoteTypeError> {
        self.live_type(note_type)?;
        let Some(info) = item_info(&self.conn, part, id)?.filter(|i| i.note_type == note_type)
        else {
            return Err(NoteTypeError::NotFound);
        };
        if !info.deleted {
            return Ok(());
        }
        check_name_free(
            &live_items(&self.conn, part, note_type)?,
            &info.name,
            Some(id),
        )?;
        Ok(self.write(|w| {
            w.set(part.entity(), id, "deleted", flag(false))?;
            self.reconcile_note_type(w, note_type)
        })?)
    }

    /// Changes `{{OldName}}` to `{{NewName}}` in every template of a note type, removed ones too, so
    /// that renaming a field does not make a template stop showing it (and cards disappear).
    fn rewrite_field_references(
        &self,
        w: &mut WriteTx<'_>,
        note_type: Id,
        old: &str,
        new: &str,
    ) -> Result<(), crate::collection::CollectionError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, front, back FROM template WHERE note_type = ?1 ORDER BY id")?;
        let rows = statement
            .query_map([note_type], |row| {
                Ok((
                    row.get::<_, Id>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, front, back) in rows {
            for (register, old_text) in [("front", front), ("back", back)] {
                let new_text = crate::note::scan::rename_field(&old_text, old, new);
                if new_text != old_text {
                    w.set(TEMPLATE.entity, id, register, Value::Text(new_text))?;
                }
            }
        }
        Ok(())
    }

    fn removed_parts(&self, part: Part, note_type: Id) -> Result<Vec<Item>, NoteTypeError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT id, name, position FROM {} WHERE note_type = ?1 AND deleted = 1
             ORDER BY position, id",
            part.table()
        ))?;
        Ok(statement
            .query_map([note_type], |row| {
                Ok(Item {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    position: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
}
