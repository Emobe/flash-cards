//! Notes and the cards made from them (ADR 0006, sections 1, 3 and 5). Step 1.3.
//!
//! A note holds one value per field of its note type, keyed by field ID (so renaming, reordering or
//! removing a field never touches note data). A note makes cards: one per template whose front
//! would show something, or, for a cloze type, one per cloze number. Card IDs are
//! `UUIDv5(note ID, template ID ‖ ordinal)`, so two devices that make the cards of one note make the
//! same cards, and a card that comes back after being removed is the same card, with its history.
//!
//! Which cards a note has is always decided by one rule, applied by `generate::reconcile` whenever a
//! note or its note type changes: cards that are wanted and missing are created, wanted and deleted
//! are restored, and live but no longer wanted are deleted. Nothing is ever hard-deleted.
//!
//! Three tables: `note` (note type, deleted), `note_field_value` (the values, a [`DynamicTable`]
//! because the register names are field IDs) and `card` (note, template, ordinal, deleted). A card's
//! deck, suspension, flag and scheduling arrive with steps 1.5 and 1.7, as extra registers.

mod error;
mod generate;
mod ops;
mod read;
pub(crate) mod scan;
#[cfg(test)]
mod tests;

use crate::id::Id;
use crate::sync::{DynamicTable, SyncedTable};

pub use error::NoteError;
pub use generate::card_id;
pub use ops::{AddedNote, NoteChange};

/// The sync entity types and register names are part of the sync format and never change (ADR 0006,
/// section 10). A note's field values are registers of the entity `note` too, named by field ID.
pub const NOTE: SyncedTable = SyncedTable {
    entity: "note",
    table: "note",
    registers: &["note_type", "deleted"],
};

pub const NOTE_VALUE: DynamicTable = DynamicTable {
    entity: "note",
    table: "note_field_value",
    owner: "note",
    key: "field",
    value: "value",
};

pub const CARD: SyncedTable = SyncedTable {
    entity: "card",
    table: "card",
    registers: &["note", "template", "ordinal", "deleted"],
};

/// A note with its values for the live fields of its note type, in field order. A field with no
/// value reads as empty. The values of removed fields are kept but not shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: Id,
    pub note_type: Id,
    pub fields: Vec<NoteField>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteField {
    pub field: Id,
    pub name: String,
    pub value: String,
}

/// One card of a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub id: Id,
    pub note: Id,
    pub template: Id,
    /// The cloze number, or 0 for a template that is not cloze.
    pub ordinal: u32,
    pub deleted: bool,
}
