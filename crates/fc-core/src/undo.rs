//! The last change made from Browse or the note editor, kept in memory so it can be undone (ADR
//! 0014, decision 4). One level: a new change replaces the kept one, and opening or closing a
//! collection drops it. `fc-api` builds the change and does the undoing; the core only keeps it.
//!
//! Undo is a new write, not a rollback: it writes the old values back even if something changed
//! them in between.

use crate::id::Id;

/// A change that can be undone, by the ID its method answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptChange {
    pub id: Id,
    pub reverse: Reverse,
}

/// What writes the old state back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reverse {
    /// Move each card back to the deck it was in: `(card, deck)`.
    Decks(Vec<(Id, Id)>),
    /// Take these `(note, tag)` pairs off again (they were added).
    RemoveTags(Vec<(Id, String)>),
    /// Put these `(note, tag)` pairs back (they were removed).
    AddTags(Vec<(Id, String)>),
    /// Set these cards' suspension back to `suspended`.
    Suspension { cards: Vec<Id>, suspended: bool },
    /// Restore these notes from the trash.
    RestoreNotes(Vec<Id>),
    /// Void these card events (a set due date or a reset), which puts each card's schedule back.
    Events(Vec<Id>),
    /// Change these notes back to the note type they had, with the maps reversed: `fields` is
    /// `(field of `from`, field of `to` that filled it)` and `templates` likewise. Notes that are
    /// not of type `to` any more are left alone.
    NoteType {
        notes: Vec<Id>,
        from: Id,
        to: Id,
        fields: Vec<(Id, Option<Id>)>,
        templates: Vec<(Id, Option<Id>)>,
    },
    /// Write a note's old values back, its old tags if they changed, and move its cards back.
    Edit {
        note: Id,
        values: Vec<(Id, String)>,
        tags: Option<Vec<String>>,
        decks: Vec<(Id, Id)>,
    },
}
