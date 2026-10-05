//! Note types: the fields a note has and the card templates that turn it into cards (ADR 0006,
//! section 3). Step 1.2.
//!
//! Three synced tables, one row each for a note type, one of its fields and one of its templates.
//! Everything is keyed by ID: a note will store its field values by field ID (step 1.3), so
//! renaming, reordering or removing a field never touches note data, and a removed field's values
//! come back with it. Nothing here is ever hard-deleted. Removing sets the `deleted` register, and
//! restoring clears it (ADR 0006, section 5).
//!
//! Two things are decided by later steps and only stored here: the text of templates (opaque to this
//! module until step 1.4 defines the grammar, which also owns rewriting `{{Field}}` references when
//! a field is renamed), and the notes and cards of a deleted note type (step 1.3 adds their
//! tombstones and the "still referenced, so still alive" rule).

pub mod builtin;
mod error;
mod ops;
mod position;
mod read;
#[cfg(test)]
mod tests;

use crate::id::Id;
use crate::sync::SyncedTable;

pub use error::NoteTypeError;

/// The sync entity types and register names are part of the sync format and never change (ADR 0006,
/// section 10).
pub const NOTE_TYPE: SyncedTable = SyncedTable {
    entity: "note_type",
    table: "note_type",
    registers: &["name", "kind", "css", "sort_field", "deleted"],
};

pub const FIELD: SyncedTable = SyncedTable {
    entity: "note_type_field",
    table: "note_type_field",
    registers: &["note_type", "name", "position", "deleted"],
};

pub const TEMPLATE: SyncedTable = SyncedTable {
    entity: "template",
    table: "template",
    registers: &["note_type", "name", "position", "front", "back", "deleted"],
};

/// How a note type makes cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One card per template.
    Standard,
    /// One template, and one card per cloze number in the note.
    Cloze,
}

impl Kind {
    pub(crate) fn as_text(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Cloze => "cloze",
        }
    }

    /// An unknown name (from a newer app) reads as a standard type, which is the safe reading: it
    /// makes no card that a template does not ask for.
    pub(crate) fn from_text(text: &str) -> Self {
        if text == "cloze" {
            Self::Cloze
        } else {
            Self::Standard
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub id: Id,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: Id,
    pub name: String,
    pub front: String,
    pub back: String,
}

/// A note type with its live fields and templates, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteType {
    pub id: Id,
    pub name: String,
    pub kind: Kind,
    pub css: String,
    /// The field used to sort and name notes: the chosen one if it is still live, otherwise the
    /// first live field (a read-time rule, so a merge cannot leave it dangling).
    pub sort_field: Option<Id>,
    pub fields: Vec<Field>,
    pub templates: Vec<Template>,
    pub deleted: bool,
}
