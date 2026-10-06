//! What a one-deck export holds: the deck and its sub-decks, their live cards, the notes of those
//! cards, the notes' types (with fields and templates), the decks' presets, the media those notes
//! and types name, and (with history) the events of those cards.

use std::collections::{BTreeSet, HashSet};

use rusqlite::types::Value;

use super::BackupError;
use crate::collection::Collection;
use crate::deck::{Tree, card_deck};
use crate::id::Id;
use crate::sync::Changes;

/// A deck's slice of the collection.
pub(super) struct DeckSlice {
    pub changes: Changes,
    /// SHA-256 (hex) of the media files to include.
    pub media: BTreeSet<String>,
    pub deck_name: String,
}

fn blob_id(value: &Value) -> Option<Id> {
    match value {
        Value::Blob(bytes) => Id::from_slice(bytes),
        _ => None,
    }
}

impl Collection {
    pub(super) fn select_deck(
        &self,
        deck: Id,
        all: Changes,
        history: bool,
    ) -> Result<DeckSlice, BackupError> {
        let tree = Tree::load(&self.conn)?;
        let top = tree.live(deck).ok_or(BackupError::NoSuchDeck)?;
        let deck_name = top.path.clone();
        let live_decks: Vec<_> = tree
            .subtree(deck)
            .iter()
            .filter(|row| !row.deleted)
            .collect();
        let decks: HashSet<Id> = live_decks.iter().map(|row| row.id).collect();
        let presets: HashSet<Id> = live_decks.iter().map(|row| row.preset).collect();

        let mut cards: HashSet<Id> = HashSet::new();
        let mut notes: HashSet<Id> = HashSet::new();
        let mut statement = self
            .conn
            .prepare("SELECT id, note, deck FROM card WHERE deleted = 0")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        for row in rows {
            let (card, note, in_deck) = row?;
            if decks.contains(&card_deck(&in_deck))
                && let Some(note) = Id::from_slice(&note)
            {
                cards.insert(card);
                notes.insert(note);
            }
        }
        drop(statement);

        let mut note_types: HashSet<Id> = HashSet::new();
        let mut statement = self.conn.prepare("SELECT id, note_type FROM note")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, Id>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        for row in rows {
            let (note, note_type) = row?;
            if notes.contains(&note)
                && let Some(note_type) = Id::from_slice(&note_type)
            {
                note_types.insert(note_type);
            }
        }
        drop(statement);

        // Fields and templates belong to a note type through their `note_type` register.
        let mut children: HashSet<Id> = HashSet::new();
        for change in &all.registers {
            if matches!(change.entity.as_str(), "note_type_field" | "template")
                && change.field == "note_type"
                && blob_id(&change.value).is_some_and(|owner| note_types.contains(&owner))
            {
                children.insert(change.entity_id);
            }
        }

        let mut media: BTreeSet<String> = BTreeSet::new();
        let mut media_ids: HashSet<Id> = HashSet::new();
        for reference in self.media_references()? {
            let used = reference.notes.iter().any(|n| notes.contains(n))
                || reference.note_types.iter().any(|t| note_types.contains(t));
            if used && let Some(file) = self.media_file(&reference.name)? {
                media_ids.insert(file.id);
                media.insert(file.hash);
            }
        }

        let keep = |entity: &str, id: Id| match entity {
            "deck" => decks.contains(&id),
            "options_preset" => presets.contains(&id),
            "note_type" => note_types.contains(&id),
            "note_type_field" | "template" => children.contains(&id),
            "note" | "note_tag" => notes.contains(&id),
            "card" => cards.contains(&id),
            "media_file" => media_ids.contains(&id),
            "requirement" => true,
            _ => false,
        };
        let mut changes = Changes {
            registers: all
                .registers
                .into_iter()
                .filter(|c| keep(&c.entity, c.entity_id))
                .collect(),
            rows: Vec::new(),
        };

        if history {
            let (events, others): (Vec<_>, Vec<_>) = all
                .rows
                .into_iter()
                .filter(|row| matches!(row.entity.as_str(), "card_event" | "fsrs_parameter_set"))
                .partition(|row| row.entity == "card_event");
            let column = |row: &crate::sync::RowChange, name: &str| {
                row.columns
                    .iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, value)| blob_id(value))
            };
            let events: Vec<_> = events
                .into_iter()
                .filter(|row| column(row, "card").is_some_and(|card| cards.contains(&card)))
                .collect();
            let sets: HashSet<Id> = events
                .iter()
                .filter_map(|row| column(row, "parameters"))
                .collect();
            changes.rows = events;
            changes
                .rows
                .extend(others.into_iter().filter(|row| sets.contains(&row.id)));
        }
        Ok(DeckSlice {
            changes,
            media,
            deck_name,
        })
    }
}
