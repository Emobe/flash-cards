//! Managing note types (step 3.4a, ADR 0015 decision 6): reading one for editing, the draft that a
//! screen edits, the checks and the live preview, and saving, creating and deleting. A draft is
//! saved once, as one change (`fc_core::notetype::Draft`), and the change can be undone.
//!
//! Card content is untrusted. The rendered HTML goes to the card frame only; names in the answers
//! are plain text.

use fc_core::Core;
use fc_core::id::Id;
use fc_core::notetype::{Draft, DraftField, DraftTemplate, Kind, NoteType, NoteTypeError, Preview};
use fc_core::template::{self, Side, TemplateProblem};
use fc_core::undo::Reverse;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::edit::{UndoToken, collection_error, keep, note_error, with};
use crate::notes::NoteTypeKind;
use crate::study::{internal, parse_id};
use crate::{ApiError, ErrorKind, Method, OpContext};

pub(crate) fn notetype_error(what: &str, error: NoteTypeError) -> ApiError {
    match error {
        NoteTypeError::NotFound => ApiError::new(ErrorKind::NotFound, error.to_string()),
        NoteTypeError::Collection(inner) => collection_error(what, inner),
        other => ApiError::invalid_input(other.to_string()),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

// ---- Draft types ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FieldDraft {
    /// A UUID string, or null for a new field. A removed field's ID means "restore it".
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TemplateDraft {
    /// A UUID string, or null for a new template.
    pub id: Option<String>,
    pub name: String,
    pub front: String,
    pub back: String,
}

/// A note type as a screen edits it. Order is the order wanted.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteTypeDraft {
    pub name: String,
    pub css: String,
    /// The field that sorts and names notes, as an index into `fields`. Null leaves it alone.
    pub sort_field: Option<u32>,
    pub fields: Vec<FieldDraft>,
    pub templates: Vec<TemplateDraft>,
}

fn optional_id(id: &Option<String>) -> Result<Option<Id>, ApiError> {
    id.as_deref().map(parse_id).transpose()
}

impl NoteTypeDraft {
    fn to_core(&self) -> Result<Draft, ApiError> {
        Ok(Draft {
            name: self.name.clone(),
            css: self.css.clone(),
            sort_field: self.sort_field.map(|i| i as usize),
            fields: self
                .fields
                .iter()
                .map(|f| {
                    Ok(DraftField {
                        id: optional_id(&f.id)?,
                        name: f.name.clone(),
                    })
                })
                .collect::<Result<_, ApiError>>()?,
            templates: self
                .templates
                .iter()
                .map(|t| {
                    Ok(DraftTemplate {
                        id: optional_id(&t.id)?,
                        name: t.name.clone(),
                        front: t.front.clone(),
                        back: t.back.clone(),
                    })
                })
                .collect::<Result<_, ApiError>>()?,
        })
    }
}

fn field_draft(id: Id, name: &str) -> FieldDraft {
    FieldDraft {
        id: Some(id.to_string()),
        name: name.to_owned(),
    }
}

fn template_draft(t: &fc_core::notetype::Template) -> TemplateDraft {
    TemplateDraft {
        id: Some(t.id.to_string()),
        name: t.name.clone(),
        front: t.front.clone(),
        back: t.back.clone(),
    }
}

// ---- getNoteTypeDetails ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteTypeInput {
    /// A UUID string.
    pub note_type_id: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteTypeDetails {
    pub id: String,
    pub kind: NoteTypeKind,
    /// How many notes have this type (deleting it deletes them).
    pub note_count: u32,
    /// The draft to start editing from: the live fields and templates.
    pub draft: NoteTypeDraft,
    /// Fields that were removed and can be restored, in their old order.
    pub removed_fields: Vec<FieldDraft>,
    pub removed_templates: Vec<TemplateDraft>,
}

fn kind_of(kind: Kind) -> NoteTypeKind {
    match kind {
        Kind::Standard => NoteTypeKind::Standard,
        Kind::Cloze => NoteTypeKind::Cloze,
    }
}

fn draft_of(found: &NoteType) -> NoteTypeDraft {
    let draft = Draft::of(found);
    NoteTypeDraft {
        name: draft.name,
        css: draft.css,
        sort_field: draft.sort_field.map(count),
        fields: found
            .fields
            .iter()
            .map(|f| field_draft(f.id, &f.name))
            .collect(),
        templates: found.templates.iter().map(template_draft).collect(),
    }
}

/// One note type with everything the editing screens need.
pub struct GetNoteTypeDetails;

impl Method for GetNoteTypeDetails {
    const NAME: &'static str = "getNoteTypeDetails";
    type Input = NoteTypeInput;
    type Output = NoteTypeDetails;

    fn call(core: &Core, input: NoteTypeInput, _: &OpContext) -> Result<NoteTypeDetails, ApiError> {
        let what = "read a note type";
        let id = parse_id(&input.note_type_id)?;
        with(core, |c| {
            let found = c
                .note_type(id)
                .map_err(|e| notetype_error(what, e))?
                .filter(|t| !t.deleted)
                .ok_or_else(|| notetype_error(what, NoteTypeError::NotFound))?;
            Ok(NoteTypeDetails {
                id: id.to_string(),
                kind: kind_of(found.kind),
                note_count: count(c.note_count(id).map_err(|e| notetype_error(what, e))?),
                draft: draft_of(&found),
                removed_fields: c
                    .removed_fields(id)
                    .map_err(|e| notetype_error(what, e))?
                    .iter()
                    .map(|f| field_draft(f.id, &f.name))
                    .collect(),
                removed_templates: c
                    .removed_templates(id)
                    .map_err(|e| notetype_error(what, e))?
                    .iter()
                    .map(template_draft)
                    .collect(),
            })
        })
    }
}

// ---- renameFieldInTemplates -----------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RenameFieldInTemplatesInput {
    pub templates: Vec<TemplateDraft>,
    /// The field's name now.
    pub from: String,
    /// The name it is being given.
    pub to: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RenameFieldInTemplatesOutput {
    pub templates: Vec<TemplateDraft>,
}

/// The templates of a draft with every tag that names `from` changed to name `to`, so a rename in
/// the draft does not make a template stop showing the field. Reads and writes nothing stored.
pub struct RenameFieldInTemplates;

impl Method for RenameFieldInTemplates {
    const NAME: &'static str = "renameFieldInTemplates";
    type Input = RenameFieldInTemplatesInput;
    type Output = RenameFieldInTemplatesOutput;

    fn call(
        _: &Core,
        input: RenameFieldInTemplatesInput,
        _: &OpContext,
    ) -> Result<RenameFieldInTemplatesOutput, ApiError> {
        let (from, to) = (input.from.trim(), input.to.trim());
        Ok(RenameFieldInTemplatesOutput {
            templates: input
                .templates
                .into_iter()
                .map(|t| TemplateDraft {
                    front: template::rename_field(&t.front, from, to),
                    back: template::rename_field(&t.back, from, to),
                    ..t
                })
                .collect(),
        })
    }
}

// ---- previewNotes and previewCard -----------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PreviewNotesInput {
    pub note_type_id: String,
    /// At most this many (default 20).
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PreviewNote {
    pub note_id: String,
    /// The sort field as plain text, shortened, to tell notes apart.
    pub title: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PreviewNotesOutput {
    /// The newest notes of the type first.
    pub notes: Vec<PreviewNote>,
    /// How many notes the type has in all.
    pub total: u32,
}

/// Real notes of a note type to preview a template with.
pub struct PreviewNotes;

impl Method for PreviewNotes {
    const NAME: &'static str = "previewNotes";
    type Input = PreviewNotesInput;
    type Output = PreviewNotesOutput;

    fn call(
        core: &Core,
        input: PreviewNotesInput,
        _: &OpContext,
    ) -> Result<PreviewNotesOutput, ApiError> {
        let what = "list notes to preview";
        let id = parse_id(&input.note_type_id)?;
        let limit = input.limit.unwrap_or(20).min(200) as usize;
        with(core, |c| {
            let found = c
                .note_type(id)
                .map_err(|e| notetype_error(what, e))?
                .filter(|t| !t.deleted)
                .ok_or_else(|| notetype_error(what, NoteTypeError::NotFound))?;
            let sort = found.sort_field;
            let notes = c.notes(id).map_err(|e| note_error(what, e))?;
            let total = notes.len();
            let notes = notes
                .iter()
                .rev()
                .take(limit)
                .map(|note| {
                    let first = note
                        .fields
                        .iter()
                        .find(|f| Some(f.field) == sort)
                        .or_else(|| note.fields.first());
                    let title: String = first
                        .map(|f| fc_core::readable_text(&f.value))
                        .unwrap_or_default()
                        .chars()
                        .take(80)
                        .collect();
                    PreviewNote {
                        note_id: note.id.to_string(),
                        title,
                    }
                })
                .collect();
            Ok(PreviewNotesOutput {
                notes,
                total: count(total),
            })
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum TemplateSide {
    Front,
    Back,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TemplateMistake {
    /// Counted from 1.
    pub line: u32,
    /// Counted from 1.
    pub column: u32,
    /// What is wrong and what to do, without the place.
    pub message: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TemplateMistakes {
    pub side: TemplateSide,
    pub mistakes: Vec<TemplateMistake>,
}

fn mistakes(problem: &TemplateProblem) -> TemplateMistakes {
    TemplateMistakes {
        side: match problem.side {
            Side::Front => TemplateSide::Front,
            Side::Back => TemplateSide::Back,
        },
        mistakes: problem
            .errors
            .iter()
            .map(|error| {
                let full = error.to_string();
                let message = full
                    .split_once(": ")
                    .map_or(full.clone(), |(_, rest)| rest.to_owned());
                TemplateMistake {
                    line: count(error.line),
                    column: count(error.column),
                    message,
                }
            })
            .collect(),
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PreviewCardInput {
    pub note_type_id: String,
    pub draft: NoteTypeDraft,
    /// Which template of the draft, counted from 0.
    pub template_index: u32,
    /// A UUID string. Null shows each field's name in brackets.
    pub note_id: Option<String>,
    /// The cloze number, for a cloze note type. Ignored otherwise.
    pub ordinal: u32,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PreviewCardOutput {
    /// A complete HTML document for the card frame. Null when a side has a mistake.
    pub front: Option<String>,
    pub back: Option<String>,
    /// Media files the card names, to supply to the frame.
    pub media: Vec<String>,
    /// Mistakes per side. Empty when the template renders.
    pub problems: Vec<TemplateMistakes>,
    /// Field names the template reads that the draft does not have (a warning, not a mistake).
    pub unknown_fields: Vec<String>,
    /// Whether the note would have this card with this draft.
    pub makes_card: bool,
}

/// Renders one card of an unsaved draft. Writes nothing.
pub struct PreviewCard;

impl Method for PreviewCard {
    const NAME: &'static str = "previewCard";
    type Input = PreviewCardInput;
    type Output = PreviewCardOutput;

    fn call(
        core: &Core,
        input: PreviewCardInput,
        _: &OpContext,
    ) -> Result<PreviewCardOutput, ApiError> {
        let what = "preview a card";
        let id = parse_id(&input.note_type_id)?;
        let draft = input.draft.to_core()?;
        let note = input.note_id.as_deref().map(parse_id).transpose()?;
        with(core, |c| {
            let Preview {
                rendered,
                problems,
                unknown_fields,
                makes_card,
            } = c
                .preview_card(
                    id,
                    &draft,
                    input.template_index as usize,
                    note,
                    input.ordinal,
                )
                .map_err(|e| notetype_error(what, e))?;
            let (front, back, media) = match rendered {
                Some(card) => (Some(card.front), Some(card.back), card.media),
                None => (None, None, Vec::new()),
            };
            Ok(PreviewCardOutput {
                front,
                back,
                media,
                problems: problems.iter().map(mistakes).collect(),
                unknown_fields,
                makes_card,
            })
        })
    }
}

// ---- checkNoteType and saveNoteType ---------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DraftInput {
    pub note_type_id: String,
    pub draft: NoteTypeDraft,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CheckNoteTypeOutput {
    /// Cards a save would create or bring back from the trash.
    pub cards_added: u32,
    /// Cards a save would send to the trash.
    pub cards_removed: u32,
    /// Notes that would gain or lose a card.
    pub notes_affected: u32,
}

/// Checks a draft and says what saving it would do to the cards, without saving. A draft with a
/// mistake answers `invalidInput` with the reason.
pub struct CheckNoteType;

impl Method for CheckNoteType {
    const NAME: &'static str = "checkNoteType";
    type Input = DraftInput;
    type Output = CheckNoteTypeOutput;

    fn call(
        core: &Core,
        input: DraftInput,
        _: &OpContext,
    ) -> Result<CheckNoteTypeOutput, ApiError> {
        let what = "check a note type";
        let id = parse_id(&input.note_type_id)?;
        let draft = input.draft.to_core()?;
        with(core, |c| {
            c.check_note_type(id, &draft)
                .map_err(|e| notetype_error(what, e))?;
            let plan = c
                .note_type_consequences(id, &draft)
                .map_err(|e| notetype_error(what, e))?;
            Ok(CheckNoteTypeOutput {
                cards_added: count(plan.cards_added),
                cards_removed: count(plan.cards_removed),
                notes_affected: count(plan.notes_affected),
            })
        })
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteTypeSaved {
    /// "Saved Polish words. 120 new cards."
    pub message: String,
    pub cards_added: u32,
    pub cards_removed: u32,
    /// Null when nothing differed, so nothing was written and there is nothing to undo.
    pub undo: Option<UndoToken>,
}

/// Saves a draft as one change and keeps it for `undoChange`.
pub struct SaveNoteType;

impl Method for SaveNoteType {
    const NAME: &'static str = "saveNoteType";
    type Input = DraftInput;
    type Output = NoteTypeSaved;

    fn call(core: &Core, input: DraftInput, _: &OpContext) -> Result<NoteTypeSaved, ApiError> {
        let what = "save a note type";
        let id = parse_id(&input.note_type_id)?;
        let draft = input.draft.to_core()?;
        with(core, |c| {
            let done = c
                .save_note_type(id, &draft)
                .map_err(|e| notetype_error(what, e))?;
            if done.undo.is_empty() {
                return Ok(NoteTypeSaved {
                    message: format!("No changes to save in {}.", draft.name.trim()),
                    cards_added: 0,
                    cards_removed: 0,
                    undo: None,
                });
            }
            let mut message = format!("Saved {}.", draft.name.trim());
            if done.cards_added > 0 {
                message += &format!(" {} new.", plural(done.cards_added, "card", "cards"));
            }
            if done.cards_removed > 0 {
                message += &format!(
                    " {} moved to the trash.",
                    plural(done.cards_removed, "card", "cards")
                );
            }
            let undo = keep(
                core,
                c,
                message.clone(),
                Reverse::NoteTypeSave {
                    note_type: id,
                    registers: done.undo,
                },
            )?;
            Ok(NoteTypeSaved {
                message,
                cards_added: count(done.cards_added),
                cards_removed: count(done.cards_removed),
                undo: Some(undo),
            })
        })
    }
}

// ---- createNoteType and deleteNoteType ------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NewNoteTypeInput {
    pub name: String,
    /// A UUID string: copy this note type (fields, templates, styling). Null starts from a shape.
    pub start_from: Option<String>,
    /// The shape to start from when `startFrom` is null. Null means standard.
    pub kind: Option<NoteTypeKindInput>,
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum NoteTypeKindInput {
    Standard,
    Cloze,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NewNoteTypeOutput {
    /// A UUID string.
    pub note_type_id: String,
    pub undo: UndoToken,
}

/// Adds a note type, as a copy or from a Basic or Cloze shape.
pub struct CreateNoteType;

impl Method for CreateNoteType {
    const NAME: &'static str = "createNoteType";
    type Input = NewNoteTypeInput;
    type Output = NewNoteTypeOutput;

    fn call(
        core: &Core,
        input: NewNoteTypeInput,
        _: &OpContext,
    ) -> Result<NewNoteTypeOutput, ApiError> {
        let what = "add a note type";
        let source = input.start_from.as_deref().map(parse_id).transpose()?;
        with(core, |c| {
            let id = match source {
                Some(source) => c.copy_note_type(source, &input.name),
                None => c.create_note_type(
                    &input.name,
                    match input.kind {
                        Some(NoteTypeKindInput::Cloze) => Kind::Cloze,
                        _ => Kind::Standard,
                    },
                ),
            }
            .map_err(|e| notetype_error(what, e))?;
            let undo = keep(
                core,
                c,
                format!("Added {}.", input.name.trim()),
                Reverse::DeleteNoteType(id),
            )?;
            Ok(NewNoteTypeOutput {
                note_type_id: id.to_string(),
                undo,
            })
        })
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteTypeDeleted {
    pub undo: UndoToken,
}

/// Deletes a note type with its notes and cards (they go to the trash with it), and keeps the
/// change for `undoChange`.
pub struct DeleteNoteType;

impl Method for DeleteNoteType {
    const NAME: &'static str = "deleteNoteType";
    type Input = NoteTypeInput;
    type Output = NoteTypeDeleted;

    fn call(core: &Core, input: NoteTypeInput, _: &OpContext) -> Result<NoteTypeDeleted, ApiError> {
        let what = "delete a note type";
        let id = parse_id(&input.note_type_id)?;
        with(core, |c| {
            let found = c
                .note_type(id)
                .map_err(|e| notetype_error(what, e))?
                .filter(|t| !t.deleted)
                .ok_or_else(|| notetype_error(what, NoteTypeError::NotFound))?;
            let notes = c.note_count(id).map_err(|e| notetype_error(what, e))?;
            c.delete_note_type(id)
                .map_err(|e| notetype_error(what, e))?;
            let message = if notes == 0 {
                format!("Deleted {}.", found.name)
            } else {
                format!(
                    "Deleted {} and {}.",
                    found.name,
                    plural(notes, "note", "notes")
                )
            };
            let undo = keep(core, c, message, Reverse::RestoreNoteType(id))?;
            Ok(NoteTypeDeleted { undo })
        })
    }
}

// ---- undo -----------------------------------------------------------------------------------------

/// Writes a note type change back, for `undoChange`. `None` when the note type is gone.
pub(crate) fn undo(
    c: &fc_core::collection::Collection,
    reverse: Reverse,
) -> Result<Option<String>, ApiError> {
    let what = "undo a note type change";
    let name_of = |id: Id| -> Result<String, ApiError> {
        Ok(c.note_type(id)
            .map_err(|e| notetype_error(what, e))?
            .map_or_else(String::new, |t| t.name))
    };
    let gone = |error: NoteTypeError| match error {
        NoteTypeError::NotFound => Ok(None),
        other => Err(notetype_error(what, other)),
    };
    match reverse {
        Reverse::NoteTypeSave {
            note_type,
            registers,
        } => match c.restore_note_type_registers(note_type, &registers) {
            Ok(()) => Ok(Some(format!(
                "Put back the earlier version of {}.",
                name_of(note_type)?
            ))),
            Err(error) => gone(error),
        },
        Reverse::DeleteNoteType(id) => {
            let name = name_of(id)?;
            match c.delete_note_type(id) {
                Ok(()) => Ok(Some(format!("Removed {name}."))),
                Err(error) => gone(error),
            }
        }
        Reverse::RestoreNoteType(id) => match c.restore_note_type(id) {
            Ok(()) => Ok(Some(format!("Restored {}.", name_of(id)?))),
            Err(error) => gone(error),
        },
        _ => Err(internal(what, "not a note type change")),
    }
}
