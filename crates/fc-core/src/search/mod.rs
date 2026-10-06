//! Search and filtering (PRODUCT.md, "Search and browse"). Step 1.9.
//!
//! A query is text a person can type, and the filters a screen offers as buttons build the same
//! text (see `parse` for the grammar). It is read into a tree, its names are looked up, and the
//! whole thing runs as one SQL statement over the cards, so the database does the filtering and
//! sorting. Nothing here writes, except saved searches.
//!
//! The filters, with `a..b` meaning from a to b and `..b` or `a..` leaving a side open:
//!
//! | Query | Cards that |
//! | --- | --- |
//! | `dog`, `"two words"`, `k*t` | have the text in any field (case ignored in every alphabet, `*` matches anything) |
//! | `front:dog`, `front:` | have the text in the field called Front, or nothing in it |
//! | `deck:Polish`, `deckonly:Polish` | are in the deck and the decks inside it, or only in it |
//! | `tag:verbs` | are on a note with the tag or a tag inside it (`-tag:*` is untagged) |
//! | `note:Basic`, `card:2`, `card:Reverse` | are of a note type, the second template, or a template by name |
//! | `is:new`, `is:learning`, `is:review`, `is:due`, `is:suspended`, `is:buried` | are in that state |
//! | `due:0`, `due:-3..0`, `due:..0` | are due on those days, counted from today (0) |
//! | `added:7`, `added:2026-09-01..2026-09-30` | were added in the last 7 days, or on those dates |
//! | `rated:7`, `rated:7:1`, `introduced:30` | were answered in the last 7 days (with Again), or first answered in the last 30 |
//! | `difficulty:>7`, `stability:<10`, `lapses:>=3`, `reviews:2..6` | have a number in that range |
//!
//! Terms are joined by a space (and), `or`, `not` or a leading `-`, and grouped with brackets.
//!
//! Sorting is an option of the search, not part of the query. Results are the live cards of live
//! notes, or one row per note.
//!
//! Decisions that are not in the grammar:
//!
//! - **Text** is matched on what a person reads: tags and entities in the field's HTML are left
//!   out. Values left behind by a deleted field do not match.
//! - **Days** are study days (the day-start hour applies) in the device's time zone now. A learning
//!   card is "due" on the day its due time falls in.
//! - **Added** is the time in the note's ID (UUIDv7), so a card made later by a new template has
//!   its note's date.
//! - **Answers that were undone** do not count for `rated`, `introduced` or `reviews`.
//! - A filter that finds nothing (a deck or tag that does not exist) matches nothing, but an
//!   unknown field name is an error: it is almost always a typing mistake.

mod compile;
mod error;
mod functions;
mod parse;
mod pattern;
mod run;
mod saved;
#[cfg(test)]
mod tests;

use crate::id::Id;
use crate::scheduling::CardState;
use crate::sync::SyncedTable;

pub use error::SearchError;
pub use saved::{SavedSearch, SavedSearchChange};

pub(crate) use functions::register as register_functions;

/// The sync entity type and register names are part of the sync format and never change (ADR 0006,
/// section 10). `sort` and `mode` are the text `Sort` and `Mode` show.
pub const SAVED_SEARCH: SyncedTable = SyncedTable {
    entity: "saved_search",
    table: "saved_search",
    registers: &["name", "query", "sort", "mode", "deleted"],
};

/// Whether results are cards or notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Cards,
    /// One row per note that has a matching card. Its card is the first of them.
    Notes,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Cards => "cards",
            Self::Notes => "notes",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.to_lowercase().as_str() {
            "cards" => Some(Self::Cards),
            "notes" => Some(Self::Notes),
            _ => None,
        }
    }
}

/// What to sort by. A note's row in notes mode takes the earliest due, the deck that comes first,
/// the highest difficulty and lapses, the lowest stability and the total reviews of its cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// When the note was added.
    Created,
    Due,
    /// The note's sort field, ignoring case.
    SortField,
    /// The deck's place in the deck list.
    Deck,
    NoteType,
    Difficulty,
    Stability,
    Lapses,
    Reviews,
    /// A shuffle that stays the same for the same `SearchOptions::seed`.
    Random,
}

impl SortKey {
    pub const ALL: [Self; 10] = [
        Self::Created,
        Self::Due,
        Self::SortField,
        Self::Deck,
        Self::NoteType,
        Self::Difficulty,
        Self::Stability,
        Self::Lapses,
        Self::Reviews,
        Self::Random,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Due => "due",
            Self::SortField => "field",
            Self::Deck => "deck",
            Self::NoteType => "note",
            Self::Difficulty => "difficulty",
            Self::Stability => "stability",
            Self::Lapses => "lapses",
            Self::Reviews => "reviews",
            Self::Random => "random",
        }
    }
}

/// A sort: a key and a direction. Cards with nothing to sort by (a new card has no due date) come
/// last in either direction. Ties are in ID order, so the order is always the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for Sort {
    /// Newest first.
    fn default() -> Self {
        Self {
            key: SortKey::Created,
            descending: true,
        }
    }
}

impl Sort {
    pub fn ascending(key: SortKey) -> Self {
        Self {
            key,
            descending: false,
        }
    }

    pub fn descending(key: SortKey) -> Self {
        Self {
            key,
            descending: true,
        }
    }

    /// Reads `due`, `due:asc` or `due:desc`.
    pub fn parse(text: &str) -> Result<Self, SearchError> {
        let lower = text.trim().to_lowercase();
        let (name, direction) = match lower.split_once(':') {
            Some((name, direction)) => (name, Some(direction)),
            None => (lower.as_str(), None),
        };
        let key = SortKey::ALL
            .into_iter()
            .find(|k| k.name() == name)
            .ok_or_else(|| SearchError::UnknownSort(text.to_owned()))?;
        let descending = match direction {
            None | Some("asc") => false,
            Some("desc") => true,
            Some(_) => return Err(SearchError::UnknownSort(text.to_owned())),
        };
        Ok(Self { key, descending })
    }
}

impl std::fmt::Display for Sort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.key.name(),
            if self.descending { "desc" } else { "asc" }
        )
    }
}

/// How to run a search. `limit` 0 only counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub sort: Sort,
    pub mode: Mode,
    pub offset: u32,
    pub limit: u32,
    /// For `SortKey::Random`.
    pub seed: u64,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            sort: Sort::default(),
            mode: Mode::Cards,
            offset: 0,
            limit: 50,
            seed: 0,
        }
    }
}

/// The most rows one search returns, so one call cannot be asked for the whole collection at once.
pub const MAX_LIMIT: u32 = 1000;

/// One result: enough for a row in a browser list.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRow {
    pub card: Id,
    pub note: Id,
    pub note_type: Id,
    pub deck: Id,
    /// The note's sort field as plain text.
    pub title: String,
    pub state: CardState,
    /// Review cards: the study day it is due. Learning cards: the day is in `due_ms`.
    pub due_day: Option<i64>,
    pub due_ms: Option<i64>,
    pub suspended: bool,
    /// Buried through today (it is back tomorrow).
    pub buried: bool,
}

/// One page of results, and how many cards or notes the whole search found.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchPage {
    pub total: u64,
    pub rows: Vec<SearchRow>,
}

/// Checks that a query can be read, without running it (for a screen that checks as people type).
pub fn check_query(query: &str) -> Result<(), SearchError> {
    parse::parse(query).map(|_| ())
}
