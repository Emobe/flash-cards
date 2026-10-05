//! Decks and option presets (ADR 0006, sections 3, 5 and 6). Step 1.5.
//!
//! Two synced tables: `deck` (name, parent, options preset, deleted) and `options_preset` (name,
//! daily limits, learning steps, desired retention, deleted). A card's deck is a register of the
//! card (`note::CARD`). Decks nest through their `parent` register, so a deck's `name` is only its
//! own part, and the path people see is the names joined by `::`.
//!
//! Merges can leave a collection in states the database cannot rule out (section 6), so the readers
//! settle them the same way on every device, when reading and without writing anything:
//!
//! - **Cycles.** Two devices that move decks into each other make a loop. In each loop the deck
//!   whose `parent` was written last shows at the top level.
//! - **Missing parents** show the deck at the top level.
//! - **Same names** next to each other show as "Polish" and "Polish (2)", in ID order.
//! - **Still referenced, so still alive** (section 5): a deleted deck that holds a live card or a
//!   live sub-deck reads as live, and so does a deleted preset that a live deck uses.
//!
//! The Default deck and the Default preset have fixed IDs, like the built-in note types, and are
//! written by migration 5 with the lowest clock. They cannot be deleted: a card with no deck is in
//! the Default deck, and a deck with no usable preset uses the Default preset.

mod error;
mod ops;
mod preset;
mod read;
#[cfg(test)]
mod tests;

use rusqlite::Transaction;
use rusqlite::types::Value;

use crate::id::Id;
use crate::notetype::builtin::NAMESPACE;
use crate::sync::{SyncedTable, seed_row};

pub use error::DeckError;
pub use preset::PresetChange;
pub(crate) use read::{card_deck, dead_decks};

/// The sync entity types and register names are part of the sync format and never change (ADR 0006,
/// section 10). `parent` and `options_preset` are an ID, or empty for none.
pub const DECK: SyncedTable = SyncedTable {
    entity: "deck",
    table: "deck",
    registers: &["name", "parent", "options_preset", "deleted"],
};

pub const PRESET: SyncedTable = SyncedTable {
    entity: "options_preset",
    table: "options_preset",
    registers: &[
        "name",
        "new_per_day",
        "reviews_per_day",
        "learning_steps",
        "desired_retention",
        "relearning_steps",
        "fsrs_parameters",
        "deleted",
    ],
};

/// The separator between a deck's name and its parent's in a path. A deck name cannot contain it.
pub const SEPARATOR: &str = "::";

/// The values of a new preset, and of the Default preset. Frozen once shipped (see `seed`).
pub const DEFAULT_NEW_PER_DAY: u32 = 20;
pub const DEFAULT_REVIEWS_PER_DAY: u32 = 200;
pub const DEFAULT_LEARNING_STEPS: &str = "1 10";
pub const DEFAULT_RELEARNING_STEPS: &str = "10";
pub const DEFAULT_DESIRED_RETENTION: f64 = 0.9;

/// The ID of the Default deck, the same in every collection.
pub fn default_deck() -> Id {
    Id::new_v5(NAMESPACE, b"default/deck")
}

/// The ID of the Default options preset, the same in every collection.
pub fn default_preset() -> Id {
    Id::new_v5(NAMESPACE, b"default/preset")
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

/// Writes the Default deck and preset with the lowest clock. Called by migration 5 only. Like the
/// built-in note types, the content is frozen once a version has shipped it.
pub(crate) fn seed(tx: &Transaction) -> rusqlite::Result<()> {
    seed_row(
        tx,
        &PRESET,
        default_preset(),
        &[
            ("name", text("Default")),
            ("new_per_day", Value::Integer(DEFAULT_NEW_PER_DAY.into())),
            (
                "reviews_per_day",
                Value::Integer(DEFAULT_REVIEWS_PER_DAY.into()),
            ),
            ("learning_steps", text(DEFAULT_LEARNING_STEPS)),
            ("desired_retention", Value::Real(DEFAULT_DESIRED_RETENTION)),
            ("deleted", Value::Integer(0)),
        ],
    )?;
    seed_row(
        tx,
        &DECK,
        default_deck(),
        &[
            ("name", text("Default")),
            ("parent", Value::Blob(Vec::new())),
            (
                "options_preset",
                Value::Blob(default_preset().as_bytes().to_vec()),
            ),
            ("deleted", Value::Integer(0)),
        ],
    )
}

/// A deck as people see it, after the read-time rules above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deck {
    pub id: Id,
    /// What is stored: the deck's own name.
    pub name: String,
    /// The name to show: `name`, with " (2)", " (3)" for decks with the same name and parent.
    pub display_name: String,
    /// The display names from the top level down, joined by `::`.
    pub path: String,
    /// The deck it is inside, or `None` at the top level.
    pub parent: Option<Id>,
    /// 0 at the top level.
    pub depth: usize,
    /// The options preset it uses: its own, or the Default preset if that is gone.
    pub preset: Id,
    /// Live cards directly in this deck (not in its sub-decks).
    pub cards: usize,
    pub deleted: bool,
}

/// An options preset. Several decks can share one.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub id: Id,
    pub name: String,
    pub new_per_day: u32,
    pub reviews_per_day: u32,
    /// Minutes between the steps a new or lapsed card goes through before FSRS sets its interval.
    pub learning_steps: Vec<u32>,
    /// Minutes between the steps a lapsed card goes through again. Empty means a lapse is a review
    /// card again with a shorter interval.
    pub relearning_steps: Vec<u32>,
    pub desired_retention: f64,
    /// The FSRS parameters, 21 numbers, or empty for the defaults. Written by the optimiser (1.8).
    /// A stored value that is not a valid set reads as empty, so reading never fails.
    pub fsrs_parameters: Vec<f32>,
    /// Live decks that use it.
    pub decks: usize,
    pub deleted: bool,
}
