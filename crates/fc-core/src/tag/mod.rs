//! Tags on notes (ADR 0006, sections 3 and 6). Step 1.6.
//!
//! One table, `note_tag`: a row per `(note, tag)` whose `present` value is `1` while the note has the
//! tag and empty after it was removed. It is a [`DynamicTable`] with a text key: in sync data the
//! entity is `note_tag`, the entity ID is the note and the register name is the tag as written. So
//! two devices that add different tags to one note both keep theirs, and the later add or remove of
//! the same tag wins. A tag has no row of its own: it exists while a live note has it.
//!
//! A tag's identity is its name, matched ignoring case. A name with `::` in it is inside the part
//! before it: `lang::polish` is a child of `lang`, and a parent exists as soon as a child does.
//! Renaming a tag (or a parent) rewrites the notes that carry it, in one transaction. The price of
//! having no tag IDs is that a note tagged with the old name on another device after a rename keeps
//! the old name. Nothing is lost.
//!
//! Merges can leave a note with the same tag in two spellings (`Polish` and `polish`). The readers
//! settle it the same way on every device, when reading and without writing: every spelling of a
//! name part is shown as the smallest one in the collection, and a tag is on a note once.

mod error;
mod ops;
mod read;
#[cfg(test)]
mod tests;

use crate::sync::DynamicTable;

pub use error::TagError;
pub use read::clean as check;

/// The sync entity type and register names are part of the sync format and never change (ADR 0006,
/// section 10). The register name is the tag as written, and the value is `1` or empty.
pub const NOTE_TAG: DynamicTable = DynamicTable {
    entity: "note_tag",
    table: "note_tag",
    owner: "note",
    key: "tag",
    value: "present",
    text_key: true,
};

/// The separator between a tag and its parent in a name. Deck paths use the same one.
pub const SEPARATOR: &str = "::";

/// What `present` holds while a note has the tag.
const PRESENT: &str = "1";

/// A tag as people see it, after the read-time rules above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The full name, with `::` between the parts.
    pub name: String,
    /// 0 for a tag with no parent.
    pub depth: usize,
    /// Live notes that have exactly this tag. A parent that only exists because of its children
    /// has none.
    pub notes: usize,
    /// Live notes that have this tag or one inside it.
    pub total: usize,
}
