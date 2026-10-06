//! Which cards a note has, and bringing the stored cards in line with that.

use std::collections::{HashMap, HashSet};

use rusqlite::types::Value;

use super::CARD;
use crate::collection::{Collection, CollectionError};
use crate::deck::{card_deck, dead_decks, default_deck};
use crate::id::Id;
use crate::notetype::{Kind, NoteType};
use crate::sync::WriteTx;
use crate::template::{Parsed, cloze_numbers, front_shows_content, parse_front};

/// The ID of the card a note makes from a template, for a cloze number (0 when the template is not
/// cloze). The same inputs always give the same ID, on every device.
pub fn card_id(note: Id, template: Id, ordinal: u32) -> Id {
    let mut name = [0u8; 20];
    name[..16].copy_from_slice(template.as_bytes());
    name[16..].copy_from_slice(&ordinal.to_be_bytes());
    Id::new_v5(note, &name)
}

/// What the cards of a note are now: its values by field ID, and the cards it has stored, with
/// whether each is deleted.
pub(super) struct NoteState {
    pub id: Id,
    pub values: HashMap<Id, String>,
    pub cards: HashMap<Id, bool>,
    /// The deck of each stored card.
    pub card_decks: HashMap<Id, Id>,
    /// The deck for a new card when the note has no card to follow (a note being added).
    pub deck: Option<Id>,
}

impl NoteState {
    /// Where a card made now goes: the deck the note's live cards are in (the lowest card ID if they
    /// differ), otherwise the deck of any card it has, otherwise the one it was added to, otherwise
    /// the Default deck.
    fn new_card_deck(&self) -> Id {
        let pick = |deleted: bool| {
            self.cards
                .iter()
                .filter(|(_, d)| **d == deleted)
                .min_by_key(|(id, _)| **id)
                .and_then(|(id, _)| self.card_decks.get(id).copied())
        };
        pick(false)
            .or_else(|| pick(true))
            .or(self.deck)
            .unwrap_or_else(default_deck)
    }
}

/// A note type with the fronts of its templates read once, so that reconciling many notes of one
/// type does not read each template again for every note.
pub(super) struct Plan<'a> {
    pub note_type: &'a NoteType,
    fronts: Vec<Parsed>,
}

impl<'a> Plan<'a> {
    pub(super) fn new(note_type: &'a NoteType) -> Self {
        Self {
            note_type,
            fronts: note_type
                .templates
                .iter()
                .map(|t| parse_front(&t.front))
                .collect(),
        }
    }
}

/// A note's values by field name, for the live fields of its note type only: a removed field reads
/// as empty. If a merge left two fields with one name, the first in order is the one a template
/// reads.
pub(super) fn values_by_name<'a>(
    note_type: &'a NoteType,
    values: &'a HashMap<Id, String>,
) -> HashMap<&'a str, &'a str> {
    let mut by_name: HashMap<&str, &str> = HashMap::new();
    for field in &note_type.fields {
        let value = values.get(&field.id).map_or("", String::as_str);
        by_name.entry(field.name.as_str()).or_insert(value);
    }
    by_name
}

/// Reads a field by name from `values_by_name`. A named function, so that the closure gets the
/// right lifetimes (the value outlives the name it is looked up by).
pub(super) fn reader<'v>(
    by_name: &'v HashMap<&'v str, &'v str>,
) -> impl Fn(&str) -> Option<&'v str> + 'v {
    move |name| by_name.get(name).copied()
}

/// The cards a note should have, as `(template, ordinal)`, in template order.
pub(super) fn wanted(plan: &Plan<'_>, values: &HashMap<Id, String>) -> Vec<(Id, u32)> {
    let by_name = values_by_name(plan.note_type, values);
    let value = reader(&by_name);
    let mut wanted = Vec::new();
    for (template, front) in plan.note_type.templates.iter().zip(&plan.fronts) {
        match plan.note_type.kind {
            Kind::Standard => {
                if front_shows_content(front, &value) {
                    wanted.push((template.id, 0));
                }
            }
            Kind::Cloze => {
                for number in cloze_numbers(front, &value) {
                    wanted.push((template.id, number));
                }
            }
        }
    }
    wanted
}

/// What reconciling did to a note's cards.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Reconciled {
    /// Cards that were created or came back.
    pub added: Vec<Id>,
    /// Cards that were deleted.
    pub removed: Vec<Id>,
}

fn flag(on: bool) -> Value {
    Value::Integer(i64::from(on))
}

fn blob(id: Id) -> Value {
    Value::Blob(id.as_bytes().to_vec())
}

/// Makes the cards of one live note match its note type, in the write `w`. A card that is deleted
/// and in a deck of `dead_decks` is not brought back: it went to the trash with its deck, and an
/// edit of the note must not undo that. (Restoring the note or the deck does.)
pub(super) fn reconcile(
    w: &mut WriteTx<'_>,
    plan: &Plan<'_>,
    note: &NoteState,
    dead_decks: &HashSet<Id>,
) -> Result<Reconciled, CollectionError> {
    let mut done = Reconciled::default();
    let new_deck = note.new_card_deck();
    let mut keep = Vec::new();
    for (template, ordinal) in wanted(plan, &note.values) {
        let id = card_id(note.id, template, ordinal);
        keep.push(id);
        match note.cards.get(&id) {
            Some(false) => {}
            Some(true) => {
                if note
                    .card_decks
                    .get(&id)
                    .is_some_and(|deck| dead_decks.contains(deck))
                {
                    continue;
                }
                w.set(CARD.entity, id, "deleted", flag(false))?;
                done.added.push(id);
            }
            None => {
                w.insert(
                    CARD.entity,
                    id,
                    vec![
                        ("note", blob(note.id)),
                        ("template", blob(template)),
                        ("ordinal", Value::Integer(i64::from(ordinal))),
                        ("deck", blob(new_deck)),
                        ("suspended", flag(false)),
                        ("buried_until", Value::Integer(0)),
                        ("deleted", flag(false)),
                    ],
                )?;
                done.added.push(id);
            }
        }
    }
    let mut stale: Vec<Id> = note
        .cards
        .iter()
        .filter(|(id, deleted)| !**deleted && !keep.contains(id))
        .map(|(id, _)| *id)
        .collect();
    stale.sort();
    for id in stale {
        w.set(CARD.entity, id, "deleted", flag(true))?;
        done.removed.push(id);
    }
    Ok(done)
}

impl Collection {
    /// The stored state of one note.
    pub(super) fn note_state(&self, note: Id) -> Result<NoteState, CollectionError> {
        let mut values = HashMap::new();
        let mut statement = self
            .conn
            .prepare("SELECT field, value FROM note_field_value WHERE note = ?1")?;
        for row in statement.query_map([note], |row| Ok((row.get(0)?, row.get(1)?)))? {
            let (field, value): (Id, String) = row?;
            values.insert(field, value);
        }
        let mut cards = HashMap::new();
        let mut card_decks = HashMap::new();
        let mut statement = self
            .conn
            .prepare("SELECT id, deleted, deck FROM card WHERE note = ?1")?;
        for row in statement.query_map([note], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, i64>(1)? != 0,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })? {
            let (id, deleted, deck) = row?;
            cards.insert(id, deleted);
            card_decks.insert(id, card_deck(&deck));
        }
        Ok(NoteState {
            id: note,
            values,
            cards,
            card_decks,
            deck: None,
        })
    }

    /// The stored state of every live note of a note type, by note ID.
    fn note_states(&self, note_type: Id) -> Result<Vec<NoteState>, CollectionError> {
        let mut by_note: HashMap<Id, NoteState> = HashMap::new();
        let mut statement = self
            .conn
            .prepare("SELECT id FROM note WHERE note_type = ?1 AND deleted = 0 ORDER BY id")?;
        let ids = statement
            .query_map([note_type], |row| row.get::<_, Id>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for id in &ids {
            by_note.insert(
                *id,
                NoteState {
                    id: *id,
                    values: HashMap::new(),
                    cards: HashMap::new(),
                    card_decks: HashMap::new(),
                    deck: None,
                },
            );
        }
        let mut statement = self.conn.prepare(
            "SELECT v.note, v.field, v.value FROM note_field_value v
             JOIN note n ON n.id = v.note WHERE n.note_type = ?1 AND n.deleted = 0",
        )?;
        for row in statement.query_map([note_type], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Id>(1)?,
                row.get::<_, String>(2)?,
            ))
        })? {
            let (note, field, value) = row?;
            if let Some(state) = by_note.get_mut(&note) {
                state.values.insert(field, value);
            }
        }
        let mut statement = self.conn.prepare(
            "SELECT c.note, c.id, c.deleted, c.deck FROM card c
             JOIN note n ON n.id = c.note WHERE n.note_type = ?1 AND n.deleted = 0",
        )?;
        for row in statement.query_map([note_type], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Id>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })? {
            let (note, card, deleted, deck) = row?;
            if let Some(state) = by_note.get_mut(&note) {
                state.cards.insert(card, deleted);
                state.card_decks.insert(card, card_deck(&deck));
            }
        }
        Ok(ids.iter().filter_map(|id| by_note.remove(id)).collect())
    }

    /// Makes the cards of every live note of a note type match its fields and templates. Called
    /// inside the write that changed the note type, so the change and its cards commit together.
    pub(crate) fn reconcile_note_type(
        &self,
        w: &mut WriteTx<'_>,
        note_type: Id,
    ) -> Result<(), CollectionError> {
        self.reconcile_note_type_counting(w, note_type).map(drop)
    }

    /// `reconcile_note_type`, returning how many notes had their cards changed.
    pub(crate) fn reconcile_note_type_counting(
        &self,
        w: &mut WriteTx<'_>,
        note_type: Id,
    ) -> Result<usize, CollectionError> {
        let Some(found) = self.note_type(note_type).map_err(CollectionError::from)? else {
            return Ok(0);
        };
        let plan = Plan::new(&found);
        let dead = dead_decks(&self.conn)?;
        let mut changed = 0;
        for note in self.note_states(note_type)? {
            changed += usize::from(reconciled(&reconcile(w, &plan, &note, &dead)?));
        }
        Ok(changed)
    }

    /// The same for some notes of one note type, as the merge needs it (ADR 0008, part 5). A note
    /// that is deleted, or of another note type, is left alone. Returns how many notes had their
    /// cards changed.
    pub(crate) fn reconcile_notes(
        &self,
        w: &mut WriteTx<'_>,
        note_type: Id,
        notes: &[Id],
        dead: &HashSet<Id>,
    ) -> Result<usize, CollectionError> {
        let Some(found) = self.note_type(note_type).map_err(CollectionError::from)? else {
            return Ok(0);
        };
        let plan = Plan::new(&found);
        let mut changed = 0;
        for id in notes {
            let live: Option<i64> = self
                .conn
                .query_row(
                    "SELECT 1 FROM note WHERE id = ?1 AND note_type = ?2 AND deleted = 0",
                    rusqlite::params![id, note_type],
                    |row| row.get(0),
                )
                .ok();
            if live.is_some() {
                let state = self.note_state(*id)?;
                changed += usize::from(reconciled(&reconcile(w, &plan, &state, dead)?));
            }
        }
        Ok(changed)
    }
}

fn reconciled(done: &Reconciled) -> bool {
    !done.added.is_empty() || !done.removed.is_empty()
}
