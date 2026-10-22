//! Saving a note type as one change (ADR 0015, decision 2). A screen edits a `Draft` (the whole
//! note type as the person wants it) and saves it once: one write, only the registers that differ,
//! and one reconcile of the cards. `note_type_consequences` runs the same write and rolls it back,
//! so what it answers is exactly what a save then does.

use std::collections::{HashMap, HashSet};

use rusqlite::params;
use rusqlite::types::Value;

use super::ops::{check_template_text, clean_name};
use super::position::{between, evenly, strictly_between};
use super::read::{Part, note_type_info};
use super::{FIELD, Field, Kind, NOTE_TYPE, NoteType, NoteTypeError, TEMPLATE, Template};
use crate::collection::{Collection, CollectionError};
use crate::deck::dead_decks;
use crate::id::Id;
use crate::note::wanted_cards;
use crate::template::{self, RenderedCard, Side, TemplateProblem};

/// A field in a draft. `id` is `None` for a new one. An ID that is removed in storage means
/// "restore it".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftField {
    pub id: Option<Id>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftTemplate {
    pub id: Option<Id>,
    pub name: String,
    pub front: String,
    pub back: String,
}

/// A note type as a screen is editing it, not yet saved. Order is the order wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub name: String,
    pub css: String,
    /// The field that sorts and names notes, as an index into `fields`. `None` leaves the stored
    /// choice alone.
    pub sort_field: Option<usize>,
    pub fields: Vec<DraftField>,
    pub templates: Vec<DraftTemplate>,
}

impl Draft {
    /// The draft of a stored note type, as the screen starts from it.
    pub fn of(note_type: &NoteType) -> Self {
        Self {
            name: note_type.name.clone(),
            css: note_type.css.clone(),
            sort_field: note_type
                .sort_field
                .and_then(|id| note_type.fields.iter().position(|f| f.id == id)),
            fields: note_type
                .fields
                .iter()
                .map(|f| DraftField {
                    id: Some(f.id),
                    name: f.name.clone(),
                })
                .collect(),
            templates: note_type
                .templates
                .iter()
                .map(|t| DraftTemplate {
                    id: Some(t.id),
                    name: t.name.clone(),
                    front: t.front.clone(),
                    back: t.back.clone(),
                })
                .collect(),
        }
    }
}

/// A value a register held, as undo writes it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterValue {
    Null,
    Text(String),
    Id(Id),
    Flag(bool),
}

impl RegisterValue {
    fn to_sql(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Text(text) => Value::Text(text.clone()),
            Self::Id(id) => Value::Blob(id.as_bytes().to_vec()),
            Self::Flag(on) => Value::Integer(i64::from(*on)),
        }
    }
}

/// One register a save wrote, with what it held before (undo writes it back). A row the save
/// created is undone by setting its `deleted` register, so it is listed as `deleted` held `true`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterChange {
    pub entity: &'static str,
    pub id: Id,
    pub register: &'static str,
    pub old: RegisterValue,
}

/// What a save did, or a dry run says it would do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SaveOutcome {
    /// Cards created or brought back from the trash.
    pub cards_added: usize,
    /// Cards sent to the trash.
    pub cards_removed: usize,
    /// Notes that gained or lost a card.
    pub notes_affected: usize,
    /// What to write back to undo the save. Empty when nothing differed.
    pub undo: Vec<RegisterChange>,
}

/// The result of rendering one card of a draft (`Collection::preview_card`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The card, when neither side has a mistake.
    pub rendered: Option<RenderedCard>,
    /// The mistakes, one per side that has any.
    pub problems: Vec<TemplateProblem>,
    /// Field names either side reads that the draft does not have.
    pub unknown_fields: Vec<String>,
    /// Whether the note would have this card with the draft (always true with no note).
    pub makes_card: bool,
}

struct StoredItem {
    id: Id,
    name: String,
    position: String,
    deleted: bool,
    front: String,
    back: String,
}

fn stored_items(
    collection: &Collection,
    part: Part,
    note_type: Id,
) -> Result<Vec<StoredItem>, NoteTypeError> {
    let columns = match part {
        Part::Field => "id, name, position, deleted, '', ''",
        Part::Template => "id, name, position, deleted, front, back",
    };
    let mut statement = collection.conn.prepare(&format!(
        "SELECT {columns} FROM {} WHERE note_type = ?1 ORDER BY position, id",
        part.table()
    ))?;
    Ok(statement
        .query_map(params![note_type], |row| {
            Ok(StoredItem {
                id: row.get(0)?,
                name: row.get(1)?,
                position: row.get(2)?,
                deleted: row.get::<_, i64>(3)? != 0,
                front: row.get(4)?,
                back: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn unique_names<'a>(names: impl Iterator<Item = &'a str>) -> Result<(), NoteTypeError> {
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(name.to_lowercase()) {
            return Err(NoteTypeError::NameTaken(name.to_owned()));
        }
    }
    Ok(())
}

/// The longest run of items whose old positions increase strictly, as indexes into `old`. Items
/// without an old position (`None`) are never in it.
fn longest_increasing(old: &[Option<&str>]) -> Vec<usize> {
    // best[i] = (length, previous) of the longest run ending at i.
    let mut best: Vec<Option<(usize, Option<usize>)>> = vec![None; old.len()];
    let mut end: Option<usize> = None;
    for i in 0..old.len() {
        let Some(here) = old[i] else { continue };
        let mut run = (1, None);
        for j in 0..i {
            if let (Some(there), Some((length, _))) = (old[j], best[j])
                && there < here
                && length + 1 > run.0
            {
                run = (length + 1, Some(j));
            }
        }
        best[i] = Some(run);
        if end.is_none_or(|e| best[e].is_some_and(|(length, _)| run.0 > length)) {
            end = Some(i);
        }
    }
    let mut keep = Vec::new();
    let mut at = end;
    while let Some(i) = at {
        keep.push(i);
        at = best[i].and_then(|(_, previous)| previous);
    }
    keep.reverse();
    keep
}

/// The positions the draft's items need: `Some` for an item that must be written, `None` for one
/// that keeps its stored position. `old` is each item's stored position, if it has one. Only the
/// items outside the longest run that already is in order are given new positions, each between
/// its neighbours; when a gap has no room the whole list is numbered again.
fn new_positions(old: &[Option<&str>]) -> Vec<Option<String>> {
    let keep: HashSet<usize> = longest_increasing(old).into_iter().collect();
    let mut result: Vec<Option<String>> = vec![None; old.len()];
    let mut before: Option<String> = None;
    for i in 0..old.len() {
        if keep.contains(&i) {
            before = old[i].map(str::to_owned);
            continue;
        }
        let after = (i + 1..old.len())
            .find(|j| keep.contains(j))
            .and_then(|j| old[j]);
        if strictly_between(before.as_deref(), after) {
            let position = between(before.as_deref(), after);
            before = Some(position.clone());
            result[i] = Some(position);
        } else {
            return evenly(old.len())
                .into_iter()
                .zip(old)
                .map(|(position, old)| (old != &Some(position.as_str())).then_some(position))
                .collect();
        }
    }
    result
}

/// Where an item a save touches is: stored already, or the nth to be created.
#[derive(Clone, Copy)]
enum Ref {
    Old(Id),
    New(usize),
}

struct SetStep {
    entity: &'static str,
    id: Id,
    register: &'static str,
    new: RegisterValue,
    old: RegisterValue,
}

struct InsertStep {
    entity: &'static str,
    /// Which of the new IDs this row gets.
    new: usize,
    values: Vec<(&'static str, Value)>,
}

/// Everything a save writes, worked out before the write.
#[derive(Default)]
struct Prepared {
    sets: Vec<SetStep>,
    inserts: Vec<InsertStep>,
    sort: Option<(Ref, RegisterValue)>,
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn blob(id: Id) -> Value {
    Value::Blob(id.as_bytes().to_vec())
}

/// One drafted field or template, as the diff reads it.
struct Wanted<'a> {
    id: Option<Id>,
    name: &'a str,
    /// Front and back, for a template.
    sides: Option<(&'a str, &'a str)>,
}

impl Prepared {
    /// Works out the steps that bring one list (fields or templates) to the draft, and answers
    /// each drafted item's `Ref`. `renames` rewrite the text of templates the draft leaves out.
    fn diff_part(
        &mut self,
        part: Part,
        note_type: Id,
        wanted: &[Wanted<'_>],
        stored: &[StoredItem],
        renames: &[(String, String)],
    ) -> Vec<Ref> {
        let entity = part.entity();
        let by_id: HashMap<Id, &StoredItem> = stored.iter().map(|s| (s.id, s)).collect();
        let olds: Vec<Option<&str>> = wanted
            .iter()
            .map(|w| {
                w.id.and_then(|id| by_id.get(&id))
                    .map(|s| s.position.as_str())
            })
            .collect();
        let positions = new_positions(&olds);
        let mut refs = Vec::with_capacity(wanted.len());
        let mut news = 0;
        for (item, position) in wanted.iter().zip(positions) {
            match item.id.and_then(|id| by_id.get(&id)) {
                Some(old) => {
                    refs.push(Ref::Old(old.id));
                    let mut set =
                        |register: &'static str, new: RegisterValue, was: RegisterValue| {
                            if new != was {
                                self.sets.push(SetStep {
                                    entity,
                                    id: old.id,
                                    register,
                                    new,
                                    old: was,
                                });
                            }
                        };
                    set(
                        "deleted",
                        RegisterValue::Flag(false),
                        RegisterValue::Flag(old.deleted),
                    );
                    set(
                        "name",
                        RegisterValue::Text(item.name.to_owned()),
                        RegisterValue::Text(old.name.clone()),
                    );
                    if let Some((front, back)) = item.sides {
                        set(
                            "front",
                            RegisterValue::Text(front.to_owned()),
                            RegisterValue::Text(old.front.clone()),
                        );
                        set(
                            "back",
                            RegisterValue::Text(back.to_owned()),
                            RegisterValue::Text(old.back.clone()),
                        );
                    }
                    if let Some(position) = position {
                        set(
                            "position",
                            RegisterValue::Text(position),
                            RegisterValue::Text(old.position.clone()),
                        );
                    }
                }
                None => {
                    refs.push(Ref::New(news));
                    let mut values = vec![
                        ("note_type", blob(note_type)),
                        ("name", text(item.name)),
                        ("position", Value::Text(position.unwrap_or_default())),
                        ("deleted", Value::Integer(0)),
                    ];
                    if let Some((front, back)) = item.sides {
                        values.push(("front", text(front)));
                        values.push(("back", text(back)));
                    }
                    self.inserts.push(InsertStep {
                        entity,
                        new: news,
                        values,
                    });
                    news += 1;
                }
            }
        }
        let drafted: HashSet<Id> = wanted.iter().filter_map(|w| w.id).collect();
        for old in stored.iter().filter(|s| !drafted.contains(&s.id)) {
            if !old.deleted {
                self.sets.push(SetStep {
                    entity,
                    id: old.id,
                    register: "deleted",
                    new: RegisterValue::Flag(true),
                    old: RegisterValue::Flag(false),
                });
            }
            if part == Part::Template {
                for (register, was) in [("front", &old.front), ("back", &old.back)] {
                    let mut now = was.clone();
                    for (from, to) in renames {
                        now = template::rename_field(&now, from, to);
                    }
                    if now != *was {
                        self.sets.push(SetStep {
                            entity,
                            id: old.id,
                            register,
                            new: RegisterValue::Text(now),
                            old: RegisterValue::Text(was.clone()),
                        });
                    }
                }
            }
        }
        refs
    }
}

impl Collection {
    /// Checks a draft of a note type against the rules of ADR 0006 (names, at least one field and
    /// template, one template for a cloze type, template syntax). Writes nothing.
    pub fn check_note_type(&self, id: Id, draft: &Draft) -> Result<(), NoteTypeError> {
        self.prepare_save(id, draft).map(drop)
    }

    /// Saves a draft as the new state of a note type, in one write. Registers that did not change
    /// are not written, so a merge with another device sees only real edits. Items missing from the
    /// draft are removed, items with a removed ID are restored, new items get new IDs, and the cards
    /// of every note are brought in line once. Answers what happened to the cards and what undo
    /// needs. A draft with a mistake writes nothing.
    pub fn save_note_type(&self, id: Id, draft: &Draft) -> Result<SaveOutcome, NoteTypeError> {
        self.apply_draft(id, draft, false)
    }

    /// What `save_note_type` would do to the cards, without writing anything: the same write, rolled
    /// back, so the counts are what a save then does.
    pub fn note_type_consequences(
        &self,
        id: Id,
        draft: &Draft,
    ) -> Result<SaveOutcome, NoteTypeError> {
        self.apply_draft(id, draft, true)
    }

    fn apply_draft(&self, id: Id, draft: &Draft, dry: bool) -> Result<SaveOutcome, NoteTypeError> {
        let prepared = self.prepare_save(id, draft)?;
        let dead = dead_decks(&self.conn)?;
        let mut outcome = SaveOutcome::default();
        let mut rolled_back = false;
        let result = self.write(|w| {
            let mut fresh = Vec::new();
            for _ in 0..prepared.inserts.len() {
                fresh.push(w.new_id()?);
            }
            let mut undo = Vec::new();
            for step in &prepared.inserts {
                let new_id = fresh[step.new];
                // New items of the two lists are numbered separately, so `new` is an index into
                // this part's IDs: fields first, then templates (see `prepare_save`).
                w.insert(step.entity, new_id, step.values.clone())?;
                undo.push(RegisterChange {
                    entity: step.entity,
                    id: new_id,
                    register: "deleted",
                    old: RegisterValue::Flag(true),
                });
            }
            for step in &prepared.sets {
                w.set(step.entity, step.id, step.register, step.new.to_sql())?;
                undo.push(RegisterChange {
                    entity: step.entity,
                    id: step.id,
                    register: step.register,
                    old: step.old.clone(),
                });
            }
            if let Some((target, old)) = &prepared.sort {
                let field = match target {
                    Ref::Old(id) => *id,
                    Ref::New(n) => fresh[*n],
                };
                w.set(NOTE_TYPE.entity, id, "sort_field", blob(field))?;
                undo.push(RegisterChange {
                    entity: NOTE_TYPE.entity,
                    id,
                    register: "sort_field",
                    old: old.clone(),
                });
            }
            let totals = self.reconcile_note_type_totals(w, id, &dead)?;
            outcome = SaveOutcome {
                cards_added: totals.cards_added,
                cards_removed: totals.cards_removed,
                notes_affected: totals.notes,
                undo,
            };
            if dry {
                // Leaving with an error rolls the whole write back.
                rolled_back = true;
                return Err(CollectionError::Storage("dry run".to_owned()));
            }
            Ok(())
        });
        match result {
            Ok(()) => Ok(outcome),
            Err(_) if rolled_back => Ok(SaveOutcome {
                undo: Vec::new(),
                ..outcome
            }),
            Err(error) => Err(error.into()),
        }
    }

    /// The checks of a save, and the steps it needs.
    fn prepare_save(&self, id: Id, draft: &Draft) -> Result<Prepared, NoteTypeError> {
        let kind = match note_type_info(&self.conn, id)? {
            Some(info) if !info.deleted => info.kind,
            _ => return Err(NoteTypeError::NotFound),
        };
        let name = clean_name(&draft.name)?;
        if draft.fields.is_empty() {
            return Err(NoteTypeError::LastField);
        }
        if draft.templates.is_empty() {
            return Err(NoteTypeError::LastTemplate);
        }
        if kind == Kind::Cloze && draft.templates.len() != 1 {
            return Err(NoteTypeError::ClozeTemplate);
        }
        let field_names = draft
            .fields
            .iter()
            .map(|f| clean_name(&f.name))
            .collect::<Result<Vec<_>, _>>()?;
        unique_names(field_names.iter().map(String::as_str))?;
        let template_names = draft
            .templates
            .iter()
            .map(|t| clean_name(&t.name))
            .collect::<Result<Vec<_>, _>>()?;
        unique_names(template_names.iter().map(String::as_str))?;
        for (template, name) in draft.templates.iter().zip(&template_names) {
            check_template_text(name, &template.front, &template.back)?;
        }
        if draft.sort_field.is_some_and(|i| i >= draft.fields.len()) {
            return Err(NoteTypeError::NotFound);
        }
        let stored_fields = stored_items(self, Part::Field, id)?;
        let stored_templates = stored_items(self, Part::Template, id)?;
        let known = |ids: Vec<Id>, stored: &[StoredItem]| {
            let mut seen = HashSet::new();
            ids.into_iter()
                .all(|i| stored.iter().any(|s| s.id == i) && seen.insert(i))
        };
        if !known(
            draft.fields.iter().filter_map(|f| f.id).collect(),
            &stored_fields,
        ) || !known(
            draft.templates.iter().filter_map(|t| t.id).collect(),
            &stored_templates,
        ) {
            return Err(NoteTypeError::NotFound);
        }

        let mut prepared = Prepared::default();
        // Renames rewrite the templates the draft leaves out (the draft's own were rewritten by
        // the screen), as renaming a field does everywhere else.
        let renames: Vec<(String, String)> = draft
            .fields
            .iter()
            .zip(&field_names)
            .filter_map(|(f, new)| {
                let old = stored_fields.iter().find(|s| Some(s.id) == f.id)?;
                (old.name != *new && !old.deleted).then(|| (old.name.clone(), new.clone()))
            })
            .collect();
        let wanted_fields: Vec<Wanted<'_>> = draft
            .fields
            .iter()
            .zip(&field_names)
            .map(|(f, name)| Wanted {
                id: f.id,
                name,
                sides: None,
            })
            .collect();
        let field_refs = prepared.diff_part(Part::Field, id, &wanted_fields, &stored_fields, &[]);
        let wanted_templates: Vec<Wanted<'_>> = draft
            .templates
            .iter()
            .zip(&template_names)
            .map(|(t, name)| Wanted {
                id: t.id,
                name,
                sides: Some((&t.front, &t.back)),
            })
            .collect();
        // Fields and templates number their new rows from 0 each, so give the templates' inserts
        // numbers after the fields'.
        let fields_new = prepared.inserts.len();
        prepared.diff_part(
            Part::Template,
            id,
            &wanted_templates,
            &stored_templates,
            &renames,
        );
        for step in &mut prepared.inserts[fields_new..] {
            step.new += fields_new;
        }

        let (stored_name, stored_css, stored_sort): (String, String, Option<Id>) =
            self.conn.query_row(
                "SELECT name, css, sort_field FROM note_type WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        for (register, new, was) in [
            ("name", name.as_str(), stored_name.as_str()),
            ("css", draft.css.as_str(), stored_css.as_str()),
        ] {
            if new != was {
                prepared.sets.push(SetStep {
                    entity: NOTE_TYPE.entity,
                    id,
                    register,
                    new: RegisterValue::Text(new.to_owned()),
                    old: RegisterValue::Text(was.to_owned()),
                });
            }
        }
        if let Some(index) = draft.sort_field {
            let target = field_refs[index];
            let unchanged = matches!(target, Ref::Old(field) if Some(field) == stored_sort);
            if !unchanged {
                prepared.sort = Some((
                    target,
                    stored_sort.map_or(RegisterValue::Null, RegisterValue::Id),
                ));
            }
        }
        Ok(prepared)
    }

    /// Writes registers back to the values a save recorded (undo), then brings the cards in line.
    pub fn restore_note_type_registers(
        &self,
        note_type: Id,
        changes: &[RegisterChange],
    ) -> Result<(), NoteTypeError> {
        let dead = dead_decks(&self.conn)?;
        Ok(self.write(|w| {
            for change in changes.iter().rev() {
                w.set(
                    change.entity,
                    change.id,
                    change.register,
                    change.old.to_sql(),
                )?;
            }
            self.reconcile_note_type_totals(w, note_type, &dead)
                .map(drop)
        })?)
    }

    /// A copy of a note type under a new name: new IDs, the same fields, templates, styling and
    /// sort field, in one write. Its notes are not copied.
    pub fn copy_note_type(&self, source: Id, name: &str) -> Result<Id, NoteTypeError> {
        let name = clean_name(name)?;
        let found = match self.note_type(source)? {
            Some(found) if !found.deleted => found,
            _ => return Err(NoteTypeError::NotFound),
        };
        let positions = evenly(found.fields.len().max(found.templates.len()));
        Ok(self.write(|w| {
            let id = w.new_id()?;
            let mut field_ids = Vec::new();
            for _ in &found.fields {
                field_ids.push(w.new_id()?);
            }
            let sort = found
                .sort_field
                .and_then(|s| found.fields.iter().position(|f| f.id == s))
                .map(|i| field_ids[i]);
            w.insert(
                NOTE_TYPE.entity,
                id,
                vec![
                    ("name", text(&name)),
                    ("kind", text(found.kind.as_text())),
                    ("css", text(&found.css)),
                    ("sort_field", sort.map_or(Value::Null, blob)),
                    ("deleted", Value::Integer(0)),
                ],
            )?;
            for ((field, field_id), position) in found.fields.iter().zip(&field_ids).zip(&positions)
            {
                w.insert(
                    FIELD.entity,
                    *field_id,
                    vec![
                        ("note_type", blob(id)),
                        ("name", text(&field.name)),
                        ("position", text(position)),
                        ("deleted", Value::Integer(0)),
                    ],
                )?;
            }
            for (template, position) in found.templates.iter().zip(&positions) {
                let template_id = w.new_id()?;
                w.insert(
                    TEMPLATE.entity,
                    template_id,
                    vec![
                        ("note_type", blob(id)),
                        ("name", text(&template.name)),
                        ("position", text(position)),
                        ("front", text(&template.front)),
                        ("back", text(&template.back)),
                        ("deleted", Value::Integer(0)),
                    ],
                )?;
            }
            Ok(id)
        })?)
    }

    /// Renders one card of a draft of `note_type`, for the live preview, without saving. With no
    /// note, every field reads `(Name)`. For a cloze type `ordinal` is the cloze number. A template
    /// with a mistake answers the mistakes and no card.
    pub fn preview_card(
        &self,
        note_type: Id,
        draft: &Draft,
        template: usize,
        note: Option<Id>,
        ordinal: u32,
    ) -> Result<Preview, NoteTypeError> {
        let kind = match note_type_info(&self.conn, note_type)? {
            Some(info) if !info.deleted => info.kind,
            _ => return Err(NoteTypeError::NotFound),
        };
        let drafted = draft
            .templates
            .get(template)
            .ok_or(NoteTypeError::NotFound)?;
        let placeholder = |n: usize| Id::new_v7(0, &[n as u8 + 1; 10]);
        let shape = NoteType {
            id: note_type,
            name: draft.name.clone(),
            kind,
            css: draft.css.clone(),
            sort_field: None,
            fields: draft
                .fields
                .iter()
                .enumerate()
                .map(|(n, f)| Field {
                    id: f.id.unwrap_or_else(|| placeholder(n)),
                    name: f.name.clone(),
                })
                .collect(),
            templates: draft
                .templates
                .iter()
                .enumerate()
                .map(|(n, t)| Template {
                    id: t.id.unwrap_or_else(|| placeholder(n + 128)),
                    name: t.name.clone(),
                    front: t.front.clone(),
                    back: t.back.clone(),
                })
                .collect(),
            deleted: false,
        };
        let ordinal = if kind == Kind::Cloze { ordinal } else { 0 };
        let values: HashMap<Id, String> = match note {
            Some(note) => self.note_field_values(note)?,
            None => shape
                .fields
                .iter()
                .map(|f| (f.id, format!("({})", f.name)))
                .collect(),
        };
        let makes_card = note.is_none()
            || wanted_cards(&shape, &values).contains(&(shape.templates[template].id, ordinal));

        let mut problems = Vec::new();
        for (side, side_text) in [(Side::Front, &drafted.front), (Side::Back, &drafted.back)] {
            let errors = template::check(side_text, side);
            if !errors.is_empty() {
                problems.push(TemplateProblem {
                    template: drafted.name.clone(),
                    side,
                    errors,
                });
            }
        }
        let known: Vec<&str> = draft.fields.iter().map(|f| f.name.as_str()).collect();
        let mut unknown_fields = template::unknown_fields(&drafted.front, &known);
        for name in template::unknown_fields(&drafted.back, &known) {
            if !unknown_fields.contains(&name) {
                unknown_fields.push(name);
            }
        }
        let rendered = if problems.is_empty() {
            let mut by_name: HashMap<&str, &str> = HashMap::new();
            for field in &shape.fields {
                let value = values.get(&field.id).map_or("", String::as_str);
                by_name.entry(field.name.as_str()).or_insert(value);
            }
            Some(
                template::render(
                    &shape,
                    &shape.templates[template],
                    &|name| by_name.get(name).copied(),
                    ordinal,
                )
                .map_err(|_| NoteTypeError::NotFound)?,
            )
        } else {
            None
        };
        Ok(Preview {
            rendered,
            problems,
            unknown_fields,
            makes_card,
        })
    }
}
