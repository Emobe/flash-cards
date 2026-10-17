//! Changing the note type of notes (ADR 0014, decision 6). The notes keep their IDs and change type
//! in place, with a field map and a card map. A card the new type makes from a card of the old one
//! starts with that card's schedule, through a `transfer` event, and in its deck and suspension.
//! The old values and cards stay stored, hidden, so changing back brings them back.

use std::collections::{HashMap, HashSet};

use rusqlite::types::Value;

use super::card_id;
use super::generate::{Plan, reconcile, wanted};
use super::{CARD, NOTE, NOTE_VALUE, NoteError};
use crate::collection::{Collection, CollectionError};
use crate::deck::dead_decks;
use crate::events::Event;
use crate::id::Id;
use crate::notetype::NoteType;
use crate::study::{Carry, Stamp, StudyError, write_carries};
use crate::sync::state;

/// What `change_note_type` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeChange {
    /// The note type the notes had, `None` if no note was given.
    pub from: Option<Id>,
    /// The notes changed, each once.
    pub notes: Vec<Id>,
    /// Cards the new type made, or brought back.
    pub cards_added: Vec<Id>,
    /// Cards of the old type that went to the trash.
    pub cards_removed: Vec<Id>,
    /// `(card, source)` for each schedule carried onto a card by a `transfer` event.
    pub carried: Vec<(Id, Id)>,
    /// Cards that came back and were reset to new, because the person chose "Start as new".
    pub reset: Vec<Id>,
}

fn blob(id: Id) -> Value {
    Value::Blob(id.as_bytes().to_vec())
}

impl Collection {
    /// Changes the note type of `notes` to `to`, in one write. The notes must be live and all of one
    /// note type, which must differ from `to`.
    ///
    /// - `fields`: for a field of the new type, the old field whose value fills it, or `None` to
    ///   empty it. A field of the new type not listed keeps the value it has stored, which is how an
    ///   undo puts everything back.
    /// - `templates`: for a template of the new type, the old template whose cards give it their
    ///   schedule, or `None` for "start as new" (a card that comes back from an earlier life is reset).
    ///   A template not listed is left as it is. Cloze to cloze carries by cloze number; between
    ///   kinds nothing is carried and `templates` is ignored. An old template given to two new ones
    ///   is refused.
    ///
    /// Refused, with nothing written, if a note would be left with no cards. Answers what changed.
    pub fn change_note_type(
        &self,
        notes: &[Id],
        to: Id,
        fields: &[(Id, Option<Id>)],
        templates: &[(Id, Option<Id>)],
    ) -> Result<TypeChange, NoteError> {
        let mut ids: Vec<Id> = Vec::with_capacity(notes.len());
        let mut seen = HashSet::new();
        let mut from: Option<Id> = None;
        for note in notes {
            let (note_type, deleted): (Id, i64) = match self.conn.query_row(
                "SELECT note_type, deleted FROM note WHERE id = ?1",
                [note],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ) {
                Ok(found) => found,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Err(NoteError::NotFound),
                Err(error) => return Err(error.into()),
            };
            if deleted != 0 {
                return Err(NoteError::NotFound);
            }
            match from {
                None => from = Some(note_type),
                Some(first) if first != note_type => return Err(NoteError::MixedTypes),
                Some(_) => {}
            }
            if seen.insert(*note) {
                ids.push(*note);
            }
        }
        let Some(from) = from else {
            return Ok(TypeChange::default());
        };
        if from == to {
            return Err(NoteError::SameType);
        }
        let old = self.note_type(from)?.ok_or(NoteError::NotFound)?;
        let new = self
            .note_type(to)?
            .filter(|t| !t.deleted)
            .ok_or(NoteError::NotFound)?;
        let field_map = check_fields(&old, &new, fields)?;
        let carry_cards = old.kind == new.kind;
        let template_map = if carry_cards {
            check_templates(&old, &new, templates)?
        } else {
            HashMap::new()
        };

        // Every note's new values and the cards they would make, before anything is written.
        let plan = Plan::new(&new);
        let dead = dead_decks(&self.conn)?;
        let mut states = Vec::with_capacity(ids.len());
        let mut empty = 0;
        for note in &ids {
            let mut state = self.note_state(*note)?;
            let mut changed = Vec::new();
            for (field, source) in &field_map {
                let value =
                    source.map_or("", |old| state.values.get(&old).map_or("", String::as_str));
                let value = value.to_owned();
                if state.values.get(field).map_or("", String::as_str) != value {
                    changed.push((*field, value.clone()));
                }
                if !value.is_empty() || state.values.contains_key(field) {
                    state.values.insert(*field, value);
                }
            }
            if wanted(&plan, &state.values).is_empty() {
                empty += 1;
            }
            states.push((state, changed));
        }
        if empty > 0 {
            return Err(NoteError::LeavesNoCards { notes: empty });
        }

        let now = self.host.clock.now();
        let stamp = Stamp {
            unix_ms: now.unix_ms,
            utc_offset: now.utc_offset_minutes,
            start_hour: self.day_start_hour().map_err(study_failed)?,
            device: state::device_id(&self.conn)?,
        };
        let done = self.write(|w| {
            let mut done = TypeChange {
                from: Some(from),
                notes: ids.clone(),
                ..TypeChange::default()
            };
            let mut carries: Vec<(Id, Carry)> = Vec::new();
            // The cards that take a source's deck and suspension, with that source.
            let mut follow: Vec<(Id, Id)> = Vec::new();
            for (state, changed) in &states {
                w.set(NOTE.entity, state.id, "note_type", blob(to))?;
                for (field, value) in changed {
                    w.set_value(NOTE_VALUE.entity, state.id, *field, value)?;
                }
                let reconciled = reconcile(w, &plan, state, &dead)?;
                let back: HashSet<Id> = reconciled.added.iter().copied().collect();
                for (template, ordinal) in wanted(&plan, &state.values) {
                    let card = card_id(state.id, template, ordinal);
                    if !back.contains(&card) {
                        continue;
                    }
                    match template_map.get(&template) {
                        Some(Some(old_template)) => {
                            let source = card_id(state.id, *old_template, ordinal);
                            if state.cards.get(&source) == Some(&false) {
                                carries.push((card, Carry::Schedule(source)));
                                follow.push((card, source));
                            }
                        }
                        Some(None) if state.cards.contains_key(&card) => {
                            carries.push((card, Carry::Reset));
                        }
                        _ => {}
                    }
                }
                w.emit(Event::NoteEdited {
                    note: state.id,
                    fields: changed.iter().map(|(field, _)| *field).collect(),
                    cards_added: reconciled.added.clone(),
                    cards_removed: reconciled.removed.clone(),
                });
                done.cards_added.extend(reconciled.added);
                done.cards_removed.extend(reconciled.removed);
            }
            for (card, source) in &follow {
                self.follow_source(w, *card, *source)?;
            }
            let carried = write_carries(w, &stamp, &carries)?;
            done.carried = carried.transferred;
            done.reset = carried.reset;
            Ok(done)
        })?;
        Ok(done)
    }

    /// Puts a card in the deck of the card it replaces, suspended if that one was.
    fn follow_source(
        &self,
        w: &mut crate::sync::WriteTx<'_>,
        card: Id,
        source: Id,
    ) -> Result<(), CollectionError> {
        let read = |id: Id| -> Result<(Vec<u8>, i64), CollectionError> {
            Ok(w.local().query_row(
                "SELECT deck, suspended FROM card WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?)
        };
        let (deck, suspended) = read(source)?;
        let (own_deck, own_suspended) = read(card)?;
        if deck != own_deck {
            w.set(CARD.entity, card, "deck", Value::Blob(deck))?;
        }
        if suspended != own_suspended {
            w.set(CARD.entity, card, "suspended", Value::Integer(suspended))?;
        }
        Ok(())
    }
}

fn study_failed(error: StudyError) -> NoteError {
    match error {
        StudyError::Collection(inner) => NoteError::Collection(inner),
        _ => NoteError::NotFound,
    }
}

/// The field map as `(new field, old field or none)`, each checked against its note type.
fn check_fields(
    old: &NoteType,
    new: &NoteType,
    fields: &[(Id, Option<Id>)],
) -> Result<Vec<(Id, Option<Id>)>, NoteError> {
    let mut out: Vec<(Id, Option<Id>)> = Vec::new();
    for (to, from) in fields {
        let known = new.fields.iter().any(|f| f.id == *to)
            && from.is_none_or(|from| old.fields.iter().any(|f| f.id == from));
        if !known {
            return Err(NoteError::UnknownField);
        }
        out.retain(|(field, _)| field != to);
        out.push((*to, *from));
    }
    Ok(out)
}

/// The card map, each template checked against its note type, with no old template twice.
fn check_templates(
    old: &NoteType,
    new: &NoteType,
    templates: &[(Id, Option<Id>)],
) -> Result<HashMap<Id, Option<Id>>, NoteError> {
    let mut out: HashMap<Id, Option<Id>> = HashMap::new();
    for (to, from) in templates {
        let known = new.templates.iter().any(|t| t.id == *to)
            && from.is_none_or(|from| old.templates.iter().any(|t| t.id == from));
        if !known {
            return Err(NoteError::UnknownField);
        }
        out.insert(*to, *from);
    }
    let mut used = HashSet::new();
    if out.values().flatten().any(|from| !used.insert(*from)) {
        return Err(NoteError::DuplicateSource);
    }
    Ok(out)
}
