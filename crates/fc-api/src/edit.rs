//! Editing a note and the bulk actions of Browse (step 3.2a, ADR 0014 decisions 3, 4 and 7): one
//! note's fields, tags and deck, every ID a search matches, moving, tagging, suspending and
//! deleting a selection, and undoing the last change.
//!
//! A selection is a `Target`: notes or cards. A card action on notes acts on every live card of
//! those notes, a note action on cards on their notes. Each change answers an undo token, and the
//! core keeps the latest one (`fc_core::undo`) until the next change, or until a collection opens or
//! closes. `undoChange` writes the old state back.

use std::collections::HashSet;

use fc_core::Core;
use fc_core::collection::{Collection, CollectionError};
use fc_core::deck::DeckError;
use fc_core::id::Id;
use fc_core::note::NoteError;
use fc_core::search::SearchError;
use fc_core::study::StudyError;
use fc_core::tag::TagError;
use fc_core::undo::{KeptChange, Reverse};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::collection::not_open;
use crate::notes::FieldValue;
use crate::search::{SearchMode, search_error};
use crate::study::{internal, parse_id};
use crate::{ApiError, ErrorKind, Method, OpContext};

/// The answer to an undo of a change that is no longer the latest, or was lost with a restart.
const GONE: &str = "That can no longer be undone.";

// ---- Errors ---------------------------------------------------------------------------------------

fn collection_error(what: &str, error: CollectionError) -> ApiError {
    match ApiError::from(error) {
        api if api == ApiError::internal() => internal(what, &api),
        api => api,
    }
}

fn note_error(what: &str, error: NoteError) -> ApiError {
    match error {
        NoteError::NotFound => ApiError::new(
            ErrorKind::NotFound,
            "That note or one of its cards was deleted. Search again and try once more.",
        ),
        NoteError::UnknownField | NoteError::NoCards { .. } | NoteError::Tag(_) => {
            ApiError::invalid_input(error.to_string())
        }
        NoteError::Collection(inner) => collection_error(what, inner),
    }
}

fn deck_error(what: &str, error: DeckError) -> ApiError {
    match error {
        DeckError::NotFound => ApiError::new(
            ErrorKind::NotFound,
            "That deck or one of the cards was deleted. Search again and try once more.",
        ),
        DeckError::Collection(inner) => collection_error(what, inner),
        other => internal(what, other),
    }
}

fn tag_error(what: &str, error: TagError) -> ApiError {
    match error {
        TagError::NotFound => note_error(what, NoteError::NotFound),
        TagError::Collection(inner) => collection_error(what, inner),
        other => ApiError::invalid_input(other.to_string()),
    }
}

fn study_error(what: &str, error: StudyError) -> ApiError {
    match error {
        StudyError::NotFound => note_error(what, NoteError::NotFound),
        StudyError::Collection(inner) => collection_error(what, inner),
        other => internal(what, other),
    }
}

/// Runs `f` on the open collection.
fn with<T>(core: &Core, f: impl FnOnce(&Collection) -> Result<T, ApiError>) -> Result<T, ApiError> {
    core.with_collection(f).ok_or_else(|| not_open(core))?
}

fn parse_ids(texts: &[String]) -> Result<Vec<Id>, ApiError> {
    texts.iter().map(|text| parse_id(text)).collect()
}

/// `1 card`, `18 cards`.
fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

// ---- Targets and undo tokens ----------------------------------------------------------------------

/// What a bulk action acts on: the rows of Browse, notes or cards.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum Target {
    /// Note IDs (UUID strings).
    Notes(Vec<String>),
    /// Card IDs (UUID strings).
    Cards(Vec<String>),
}

/// A target with its IDs read.
enum Ids {
    Notes(Vec<Id>),
    Cards(Vec<Id>),
}

impl Target {
    fn parse(&self) -> Result<Ids, ApiError> {
        Ok(match self {
            Self::Notes(ids) => Ids::Notes(parse_ids(ids)?),
            Self::Cards(ids) => Ids::Cards(parse_ids(ids)?),
        })
    }
}

impl Ids {
    /// The cards to act on: the live cards of the notes, or the cards.
    fn cards(&self, c: &Collection, what: &str) -> Result<Vec<Id>, ApiError> {
        match self {
            Self::Notes(notes) => c.cards_of_notes(notes).map_err(|e| note_error(what, e)),
            Self::Cards(cards) => Ok(cards.clone()),
        }
    }

    /// The notes to act on: the notes, or the notes of the cards.
    fn notes(&self, c: &Collection, what: &str) -> Result<Vec<Id>, ApiError> {
        match self {
            Self::Notes(notes) => Ok(notes.clone()),
            Self::Cards(cards) => c.notes_of_cards(cards).map_err(|e| note_error(what, e)),
        }
    }
}

/// What the UI shows after a change, and the ID that undoes it.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UndoToken {
    /// Pass to `undoChange`.
    pub id: String,
    /// What happened, as a sentence: "Moved 18 cards to Polish::Verbs."
    pub message: String,
}

/// Keeps a change for undo, in place of the one before, and answers its token.
fn keep(
    core: &Core,
    c: &Collection,
    message: String,
    reverse: Reverse,
) -> Result<UndoToken, ApiError> {
    let id = Id::generate(c.now_ms()).map_err(|e| internal("make an undo ID", e))?;
    core.keep_change(KeptChange { id, reverse });
    Ok(UndoToken {
        id: id.to_string(),
        message,
    })
}

// ---- getNote --------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteInput {
    /// A UUID string.
    pub note_id: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NoteDetails {
    /// A UUID string.
    pub note_type_id: String,
    /// Every field of the note type, in order. A field with no value is empty.
    pub fields: Vec<FieldValue>,
    pub tags: Vec<String>,
    /// The decks of its live cards, each once, in card order. More than one means "Several decks".
    pub deck_ids: Vec<String>,
    pub card_count: u32,
}

/// One live note, to edit.
pub struct GetNote;

impl Method for GetNote {
    const NAME: &'static str = "getNote";
    type Input = NoteInput;
    type Output = NoteDetails;

    fn call(core: &Core, input: NoteInput, _: &OpContext) -> Result<NoteDetails, ApiError> {
        let note = parse_id(&input.note_id)?;
        let what = "read a note";
        with(core, |c| {
            let found = c
                .note(note)
                .map_err(|e| note_error(what, e))?
                .filter(|n| !n.deleted)
                .ok_or_else(|| note_error(what, NoteError::NotFound))?;
            let tags = c.note_tags(note).map_err(|e| tag_error(what, e))?;
            let cards = c.cards_of_note(note).map_err(|e| note_error(what, e))?;
            let mut seen = HashSet::new();
            let deck_ids = cards
                .iter()
                .filter(|card| seen.insert(card.deck))
                .map(|card| card.deck.to_string())
                .collect();
            Ok(NoteDetails {
                note_type_id: found.note_type.to_string(),
                fields: found
                    .fields
                    .into_iter()
                    .map(|f| FieldValue {
                        field_id: f.field.to_string(),
                        value: f.value,
                    })
                    .collect(),
                tags,
                deck_ids,
                card_count: count(cards.len()),
            })
        })
    }
}

// ---- updateNote -----------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UpdateNoteInput {
    /// A UUID string.
    pub note_id: String,
    /// The fields to set. Only values that differ are written; a field left out stays.
    pub fields: Vec<FieldValue>,
    /// The note's tags, all of them.
    pub tags: Vec<String>,
    /// Moves every card of the note to this deck after the edit. Null leaves the decks alone.
    #[ts(optional)]
    #[serde(default)]
    pub deck_id: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UpdateNoteOutput {
    /// Cards made or brought back by the edit (UUID strings).
    pub cards_added: Vec<String>,
    /// Cards the edit removed, because the note no longer fills their template.
    pub cards_removed: Vec<String>,
    /// Other live notes with the same first field, when the first field changed.
    pub duplicates: Vec<String>,
    /// Null when nothing changed.
    pub undo: Option<UndoToken>,
}

/// Saves the note editor: fields and tags in one write, then the deck. One undo covers both.
pub struct UpdateNote;

impl Method for UpdateNote {
    const NAME: &'static str = "updateNote";
    type Input = UpdateNoteInput;
    type Output = UpdateNoteOutput;

    fn call(
        core: &Core,
        input: UpdateNoteInput,
        _: &OpContext,
    ) -> Result<UpdateNoteOutput, ApiError> {
        let note = parse_id(&input.note_id)?;
        let deck = input.deck_id.as_deref().map(parse_id).transpose()?;
        let values = input
            .fields
            .iter()
            .map(|f| Ok((parse_id(&f.field_id)?, f.value.as_str())))
            .collect::<Result<Vec<_>, ApiError>>()?;
        let tags: Vec<&str> = input.tags.iter().map(String::as_str).collect();
        let what = "save a note";
        with(core, |c| {
            if let Some(deck) = deck {
                // Checked first, so the move cannot fail after the edit is written.
                c.deck(deck)
                    .map_err(|e| deck_error(what, e))?
                    .filter(|d| !d.deleted)
                    .ok_or_else(|| deck_error(what, DeckError::NotFound))?;
            }
            let edit = c
                .edit_note_with(note, &values, Some(&tags))
                .map_err(|e| note_error(what, e))?;
            let moved = match deck {
                Some(deck) => {
                    let cards = c.cards_of_notes(&[note]).map_err(|e| note_error(what, e))?;
                    c.move_cards(&cards, deck)
                        .map_err(|e| deck_error(what, e))?
                }
                None => Vec::new(),
            };
            let changed =
                !edit.old_values.is_empty() || edit.old_tags.is_some() || !moved.is_empty();
            let undo = if changed {
                Some(keep(
                    core,
                    c,
                    "Saved.".to_owned(),
                    Reverse::Edit {
                        note,
                        values: edit.old_values,
                        tags: edit.old_tags,
                        decks: moved,
                    },
                )?)
            } else {
                None
            };
            let strings = |ids: Vec<Id>| ids.iter().map(ToString::to_string).collect();
            Ok(UpdateNoteOutput {
                cards_added: strings(edit.change.added_cards),
                cards_removed: strings(edit.change.removed_cards),
                duplicates: strings(edit.change.duplicates),
                undo,
            })
        })
    }
}

// ---- searchIds ------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SearchIdsInput {
    pub query: String,
    pub mode: SearchMode,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SearchIdsOutput {
    /// Note IDs in notes mode, card IDs in cards mode: every match, with no limit.
    pub ids: Vec<String>,
}

/// Every ID a search matches, for "Select all".
pub struct SearchIds;

impl Method for SearchIds {
    const NAME: &'static str = "searchIds";
    type Input = SearchIdsInput;
    type Output = SearchIdsOutput;

    fn call(
        core: &Core,
        input: SearchIdsInput,
        _: &OpContext,
    ) -> Result<SearchIdsOutput, ApiError> {
        with(core, |c| {
            let ids = c
                .search_ids(&input.query, input.mode.core())
                .map_err(|e: SearchError| search_error("list the matches of a search", e))?;
            Ok(SearchIdsOutput {
                ids: ids.iter().map(ToString::to_string).collect(),
            })
        })
    }
}

// ---- The bulk actions -----------------------------------------------------------------------------

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ChangeOutput {
    /// How many cards (move, suspend) or notes (tags) changed.
    pub count: u32,
    /// Null when nothing changed.
    pub undo: Option<UndoToken>,
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MoveCardsInput {
    pub target: Target,
    /// A UUID string.
    pub deck_id: String,
}

/// Moves cards to a deck.
pub struct MoveCards;

impl Method for MoveCards {
    const NAME: &'static str = "moveCards";
    type Input = MoveCardsInput;
    type Output = ChangeOutput;

    fn call(core: &Core, input: MoveCardsInput, _: &OpContext) -> Result<ChangeOutput, ApiError> {
        let target = input.target.parse()?;
        let deck = parse_id(&input.deck_id)?;
        let what = "move cards";
        with(core, |c| {
            let cards = target.cards(c, what)?;
            let moved = c
                .move_cards(&cards, deck)
                .map_err(|e| deck_error(what, e))?;
            if moved.is_empty() {
                return Ok(ChangeOutput {
                    count: 0,
                    undo: None,
                });
            }
            let path = c
                .deck(deck)
                .map_err(|e| deck_error(what, e))?
                .map(|d| d.path)
                .unwrap_or_default();
            let message = format!("Moved {} to {path}.", plural(moved.len(), "card", "cards"));
            let n = count(moved.len());
            let undo = keep(core, c, message, Reverse::Decks(moved))?;
            Ok(ChangeOutput {
                count: n,
                undo: Some(undo),
            })
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TagsInput {
    pub target: Target,
    pub tags: Vec<String>,
}

/// The notes in some `(note, tag)` pairs, each once.
fn notes_in(pairs: &[(Id, String)]) -> usize {
    pairs
        .iter()
        .map(|(note, _)| note)
        .collect::<HashSet<_>>()
        .len()
}

/// "polish" for one tag, "2 tags" for more.
fn tags_text(pairs: &[(Id, String)]) -> String {
    let mut tags: Vec<String> = pairs.iter().map(|(_, tag)| tag.to_lowercase()).collect();
    tags.sort();
    tags.dedup();
    match tags.as_slice() {
        [_] => pairs[0].1.clone(),
        _ => plural(tags.len(), "tag", "tags"),
    }
}

fn change_tags(core: &Core, input: TagsInput, add: bool) -> Result<ChangeOutput, ApiError> {
    let target = input.target.parse()?;
    let tags: Vec<&str> = input.tags.iter().map(String::as_str).collect();
    let what = if add { "add tags" } else { "remove tags" };
    with(core, |c| {
        let notes = target.notes(c, what)?;
        let pairs = if add {
            c.add_tags(&notes, &tags)
        } else {
            c.remove_tags(&notes, &tags)
        }
        .map_err(|e| tag_error(what, e))?;
        if pairs.is_empty() {
            return Ok(ChangeOutput {
                count: 0,
                undo: None,
            });
        }
        let changed = notes_in(&pairs);
        let notes_text = plural(changed, "note", "notes");
        let tag = tags_text(&pairs);
        let (message, reverse) = if add {
            (
                format!("Added {tag} to {notes_text}."),
                Reverse::RemoveTags(pairs),
            )
        } else {
            (
                format!("Removed {tag} from {notes_text}."),
                Reverse::AddTags(pairs),
            )
        };
        let undo = keep(core, c, message, reverse)?;
        Ok(ChangeOutput {
            count: count(changed),
            undo: Some(undo),
        })
    })
}

/// Adds tags to notes.
pub struct AddTags;

impl Method for AddTags {
    const NAME: &'static str = "addTags";
    type Input = TagsInput;
    type Output = ChangeOutput;

    fn call(core: &Core, input: TagsInput, _: &OpContext) -> Result<ChangeOutput, ApiError> {
        change_tags(core, input, true)
    }
}

/// Takes tags off notes.
pub struct RemoveTags;

impl Method for RemoveTags {
    const NAME: &'static str = "removeTags";
    type Input = TagsInput;
    type Output = ChangeOutput;

    fn call(core: &Core, input: TagsInput, _: &OpContext) -> Result<ChangeOutput, ApiError> {
        change_tags(core, input, false)
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TargetInput {
    pub target: Target,
}

fn set_suspended(core: &Core, input: TargetInput, on: bool) -> Result<ChangeOutput, ApiError> {
    let target = input.target.parse()?;
    let what = if on {
        "suspend cards"
    } else {
        "unsuspend cards"
    };
    with(core, |c| {
        let cards = target.cards(c, what)?;
        let changed = if on {
            c.suspend_cards(&cards)
        } else {
            c.unsuspend_cards(&cards)
        }
        .map_err(|e| study_error(what, e))?;
        if changed.is_empty() {
            return Ok(ChangeOutput {
                count: 0,
                undo: None,
            });
        }
        let verb = if on { "Suspended" } else { "Unsuspended" };
        let message = format!("{verb} {}.", plural(changed.len(), "card", "cards"));
        let n = count(changed.len());
        let reverse = Reverse::Suspension {
            cards: changed,
            suspended: !on,
        };
        let undo = keep(core, c, message, reverse)?;
        Ok(ChangeOutput {
            count: n,
            undo: Some(undo),
        })
    })
}

/// Takes cards out of study until they are unsuspended.
pub struct SuspendCards;

impl Method for SuspendCards {
    const NAME: &'static str = "suspendCards";
    type Input = TargetInput;
    type Output = ChangeOutput;

    fn call(core: &Core, input: TargetInput, _: &OpContext) -> Result<ChangeOutput, ApiError> {
        set_suspended(core, input, true)
    }
}

/// Puts suspended cards back into study.
pub struct UnsuspendCards;

impl Method for UnsuspendCards {
    const NAME: &'static str = "unsuspendCards";
    type Input = TargetInput;
    type Output = ChangeOutput;

    fn call(core: &Core, input: TargetInput, _: &OpContext) -> Result<ChangeOutput, ApiError> {
        set_suspended(core, input, false)
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeletedNotes {
    /// How many notes went to the trash.
    pub notes: u32,
    /// How many of their cards went with them.
    pub cards: u32,
    /// Null when nothing changed.
    pub undo: Option<UndoToken>,
}

/// Moves notes and their cards to the trash, in one write.
pub struct DeleteNotes;

impl Method for DeleteNotes {
    const NAME: &'static str = "deleteNotes";
    type Input = TargetInput;
    type Output = DeletedNotes;

    fn call(core: &Core, input: TargetInput, _: &OpContext) -> Result<DeletedNotes, ApiError> {
        let target = input.target.parse()?;
        let what = "delete notes";
        with(core, |c| {
            let notes = target.notes(c, what)?;
            let done = c.delete_notes(&notes).map_err(|e| note_error(what, e))?;
            let (n, k) = (count(done.notes.len()), count(done.cards.len()));
            if done.notes.is_empty() {
                return Ok(DeletedNotes {
                    notes: 0,
                    cards: 0,
                    undo: None,
                });
            }
            let message = format!(
                "Deleted {} and {} {}.",
                plural(done.notes.len(), "note", "notes"),
                if done.notes.len() == 1 {
                    "its"
                } else {
                    "their"
                },
                plural(done.cards.len(), "card", "cards")
            );
            let undo = keep(core, c, message, Reverse::RestoreNotes(done.notes))?;
            Ok(DeletedNotes {
                notes: n,
                cards: k,
                undo: Some(undo),
            })
        })
    }
}

// ---- undoChange -----------------------------------------------------------------------------------

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UndoChangeInput {
    /// The `id` of an undo token.
    pub undo_id: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UndoChangeOutput {
    /// What the undo did, as a sentence, including anything it could not put back.
    pub message: String,
}

/// Undoes the last change, if `undoId` is still the latest. Otherwise answers `notFound` with
/// "That can no longer be undone."
pub struct UndoChange;

impl Method for UndoChange {
    const NAME: &'static str = "undoChange";
    type Input = UndoChangeInput;
    type Output = UndoChangeOutput;

    fn call(
        core: &Core,
        input: UndoChangeInput,
        _: &OpContext,
    ) -> Result<UndoChangeOutput, ApiError> {
        let gone = || ApiError::new(ErrorKind::NotFound, GONE);
        let id = parse_id(&input.undo_id).map_err(|_| gone())?;
        with(core, |c| {
            let kept = core.take_change(id).ok_or_else(gone)?;
            let message = reverse(c, kept.reverse)?.ok_or_else(gone)?;
            Ok(UndoChangeOutput { message })
        })
    }
}

/// How many of a reverse's groups worked, and how many items were in the ones that did not.
#[derive(Default)]
struct Tally {
    done: usize,
    failed: usize,
}

/// Runs `write` on each group of items, in order of first appearance. A group the core refuses
/// because something in it is gone is counted as failed; any other failure stops.
fn grouped<K: PartialEq + Clone, V: Clone, E>(
    items: &[(V, K)],
    mut write: impl FnMut(&K, &[V]) -> Result<bool, E>,
    tally: &mut Tally,
) -> Result<(), E> {
    let mut keys: Vec<K> = Vec::new();
    for (_, key) in items {
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    for key in keys {
        let group: Vec<V> = items
            .iter()
            .filter(|(_, k)| *k == key)
            .map(|(v, _)| v.clone())
            .collect();
        if write(&key, &group)? {
            tally.done += group.len();
        } else {
            tally.failed += group.len();
        }
    }
    Ok(())
}

/// `Ok(true)` when the write worked, `Ok(false)` when the core said something in it is gone.
fn refused_if_gone<T, E>(result: Result<T, E>, gone: impl Fn(&E) -> bool) -> Result<bool, E> {
    match result {
        Ok(_) => Ok(true),
        Err(error) if gone(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

/// " 3 could not be put back: they or their deck were deleted." when some failed.
fn failed_text(failed: usize, one: &str, many: &str) -> String {
    if failed == 0 {
        String::new()
    } else {
        format!(
            " {} could not be put back, because {} deleted.",
            plural(failed, one, many),
            if failed == 1 {
                "it or its deck was"
            } else {
                "they or their decks were"
            }
        )
    }
}

/// Writes the old state back. `None` when nothing could be undone because what it changed is
/// gone.
fn reverse(c: &Collection, reverse: Reverse) -> Result<Option<String>, ApiError> {
    let what = "undo a change";
    match reverse {
        Reverse::Decks(pairs) => {
            let mut tally = Tally::default();
            move_back(c, &pairs, &mut tally)?;
            Ok((tally.done > 0).then(|| {
                format!(
                    "Moved {} back.{}",
                    plural(tally.done, "card", "cards"),
                    failed_text(tally.failed, "card", "cards")
                )
            }))
        }
        Reverse::RemoveTags(pairs) | Reverse::AddTags(pairs) if pairs.is_empty() => Ok(None),
        Reverse::RemoveTags(pairs) => tags_back(c, &pairs, false),
        Reverse::AddTags(pairs) => tags_back(c, &pairs, true),
        Reverse::Suspension { cards, suspended } => {
            let result = if suspended {
                c.suspend_cards(&cards)
            } else {
                c.unsuspend_cards(&cards)
            };
            if !refused_if_gone(result, |e| *e == StudyError::NotFound)
                .map_err(|e| study_error(what, e))?
            {
                return Ok(None);
            }
            let verb = if suspended {
                "Suspended"
            } else {
                "Unsuspended"
            };
            Ok(Some(format!(
                "{verb} {} again.",
                plural(cards.len(), "card", "cards")
            )))
        }
        Reverse::RestoreNotes(notes) => {
            let done = match c.restore_notes(&notes) {
                Ok(done) => done,
                Err(NoteError::NotFound) => return Ok(None),
                Err(error) => return Err(note_error(what, error)),
            };
            Ok(Some(format!(
                "Restored {} and {}.",
                plural(done.notes.len(), "note", "notes"),
                plural(done.cards.len(), "card", "cards")
            )))
        }
        Reverse::Edit {
            note,
            values,
            tags,
            decks,
        } => {
            // The cards go back first: undoing the edit can remove a card it made.
            let mut tally = Tally::default();
            move_back(c, &decks, &mut tally)?;
            let values: Vec<(Id, &str)> = values.iter().map(|(f, v)| (*f, v.as_str())).collect();
            let tags: Option<Vec<&str>> = tags
                .as_ref()
                .map(|tags| tags.iter().map(String::as_str).collect());
            match c.edit_note_with(note, &values, tags.as_deref()) {
                Ok(_) => {}
                Err(NoteError::NotFound) | Err(NoteError::UnknownField) if tally.done == 0 => {
                    return Ok(None);
                }
                Err(NoteError::NotFound) | Err(NoteError::UnknownField) => {
                    return Ok(Some("Moved the note's cards back. Its fields could not be put back, because the note or its note type changed.".to_owned()));
                }
                Err(error) => return Err(note_error(what, error)),
            }
            Ok(Some(format!(
                "The note is back as it was.{}",
                failed_text(tally.failed, "card", "cards")
            )))
        }
    }
}

/// Moves cards back to their old decks, one write per deck.
fn move_back(c: &Collection, pairs: &[(Id, Id)], tally: &mut Tally) -> Result<(), ApiError> {
    grouped(
        pairs,
        |deck, cards| refused_if_gone(c.move_cards(cards, *deck), |e| *e == DeckError::NotFound),
        tally,
    )
    .map_err(|e| deck_error("undo a move", e))
}

/// Takes added tags off again (`add` false) or puts removed ones back (`add` true), one write per
/// tag.
fn tags_back(
    c: &Collection,
    pairs: &[(Id, String)],
    add: bool,
) -> Result<Option<String>, ApiError> {
    let mut tally = Tally::default();
    let mut notes = HashSet::new();
    grouped(
        pairs,
        |tag, group| {
            let result = if add {
                c.add_tags(group, &[tag.as_str()])
            } else {
                c.remove_tags(group, &[tag.as_str()])
            };
            let worked = refused_if_gone(result, |e| *e == TagError::NotFound)?;
            if worked {
                notes.extend(group.iter().copied());
            }
            Ok(worked)
        },
        &mut tally,
    )
    .map_err(|e| tag_error("undo a tag change", e))?;
    if tally.done == 0 {
        return Ok(None);
    }
    let what = if add {
        format!(
            "Put the tags back on {}.",
            plural(notes.len(), "note", "notes")
        )
    } else {
        format!(
            "Took the tags back off {}.",
            plural(notes.len(), "note", "notes")
        )
    };
    let failed = if tally.failed == 0 {
        String::new()
    } else {
        " Some notes were deleted since, so their tags were not changed.".to_owned()
    };
    Ok(Some(format!("{what}{failed}")))
}
