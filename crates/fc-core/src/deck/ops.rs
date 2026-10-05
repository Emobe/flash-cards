//! Changing decks. Every change goes through `Collection::write`, so every register it writes gets
//! a clock. The checks run before the write: one connection serves the collection, so nothing
//! changes in between. Rules a merge could break (a deck's name being unique among its siblings, a
//! deck not being inside itself) are checked here, for the person using this device, and are not
//! constraints in the database (ADR 0006, section 6).

use std::collections::HashSet;

use rusqlite::types::Value;

use super::read::{Tree, card_deck};
use super::{DECK, DeckError, SEPARATOR, default_deck, default_preset};
use crate::collection::Collection;
use crate::id::Id;
use crate::note::{CARD, NOTE};

pub(super) fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

pub(super) fn blob(id: Id) -> Value {
    Value::Blob(id.as_bytes().to_vec())
}

fn parent_blob(parent: Option<Id>) -> Value {
    Value::Blob(parent.map_or_else(Vec::new, |id| id.as_bytes().to_vec()))
}

pub(super) fn flag(on: bool) -> Value {
    Value::Integer(i64::from(on))
}

pub(super) fn clean_name(name: &str) -> Result<String, DeckError> {
    let name = name.trim();
    if name.is_empty() {
        Err(DeckError::EmptyName)
    } else {
        Ok(name.to_owned())
    }
}

fn clean_deck_name(name: &str) -> Result<String, DeckError> {
    let name = clean_name(name)?;
    if name.contains(SEPARATOR) {
        Err(DeckError::NameHasSeparator)
    } else {
        Ok(name)
    }
}

/// Fails if a live deck other than `except` inside `parent` already has this name (ignoring case).
fn check_name_free(
    tree: &Tree,
    name: &str,
    parent: Option<Id>,
    except: Option<Id>,
) -> Result<(), DeckError> {
    let wanted = name.to_lowercase();
    if tree.rows.iter().any(|r| {
        !r.deleted && r.parent == parent && Some(r.id) != except && r.name.to_lowercase() == wanted
    }) {
        Err(DeckError::NameTaken(name.to_owned()))
    } else {
        Ok(())
    }
}

impl Collection {
    /// Makes a deck, at the top level or inside a live deck. It starts on the Default preset.
    pub fn create_deck(&self, name: &str, parent: Option<Id>) -> Result<Id, DeckError> {
        let name = clean_deck_name(name)?;
        let tree = Tree::load(&self.conn)?;
        if let Some(parent) = parent {
            tree.live(parent).ok_or(DeckError::NotFound)?;
        }
        check_name_free(&tree, &name, parent, None)?;
        Ok(self.write(|w| {
            let id = w.new_id()?;
            w.insert(
                DECK.entity,
                id,
                vec![
                    ("name", text(&name)),
                    ("parent", parent_blob(parent)),
                    ("options_preset", blob(default_preset())),
                    ("deleted", flag(false)),
                ],
            )?;
            Ok(id)
        })?)
    }

    pub fn rename_deck(&self, id: Id, name: &str) -> Result<(), DeckError> {
        let name = clean_deck_name(name)?;
        let tree = Tree::load(&self.conn)?;
        let row = tree.live(id).ok_or(DeckError::NotFound)?;
        check_name_free(&tree, &name, row.parent, Some(id))?;
        if row.name == name {
            return Ok(());
        }
        Ok(self.write(|w| w.set(DECK.entity, id, "name", text(&name)))?)
    }

    /// Moves a deck, with everything inside it, to the top level (`None`) or inside another live
    /// deck. Only the moved deck is written. A deck cannot go inside itself or its sub-decks.
    pub fn move_deck(&self, id: Id, parent: Option<Id>) -> Result<(), DeckError> {
        let tree = Tree::load(&self.conn)?;
        let row = tree.live(id).ok_or(DeckError::NotFound)?;
        if let Some(parent) = parent {
            tree.live(parent).ok_or(DeckError::NotFound)?;
            if tree.is_inside(parent, id) {
                return Err(DeckError::MoveIntoItself);
            }
        }
        if row.parent == parent {
            return Ok(());
        }
        check_name_free(&tree, &row.name, parent, Some(id))?;
        Ok(self.write(|w| w.set(DECK.entity, id, "parent", parent_blob(parent)))?)
    }

    /// Deletes a deck, the decks inside it and their cards, which move to the trash with their
    /// history (ADR 0006, section 5). A note that is left with no live card is deleted too. A card
    /// in another deck stays where it is, and so does its note.
    ///
    /// The Default deck cannot be deleted. If it was moved inside the deck being deleted, it goes
    /// to the top level and keeps its cards. Deleting a deleted deck does nothing.
    pub fn delete_deck(&self, id: Id) -> Result<(), DeckError> {
        if id == default_deck() {
            return Err(DeckError::Default);
        }
        let tree = Tree::load(&self.conn)?;
        let row = tree.get(id).ok_or(DeckError::NotFound)?;
        if row.deleted {
            return Ok(());
        }
        let kept: HashSet<Id> = tree.subtree(default_deck()).iter().map(|r| r.id).collect();
        let doomed: Vec<_> = tree
            .subtree(id)
            .iter()
            .filter(|r| !kept.contains(&r.id))
            .collect();
        let default_moves = tree.is_inside(default_deck(), id);

        let mut cards: Vec<(Id, Id)> = Vec::new();
        let mut statement = self.conn.prepare(
            "SELECT c.id, c.note FROM card c JOIN note n ON n.id = c.note
             WHERE c.deck = ?1 AND c.deleted = 0 AND n.deleted = 0 ORDER BY c.id",
        )?;
        for deck in &doomed {
            for found in statement.query_map([deck.id], |r| Ok((r.get(0)?, r.get(1)?)))? {
                cards.push(found?);
            }
        }
        let gone: HashSet<Id> = cards.iter().map(|(card, _)| *card).collect();
        let mut notes: Vec<Id> = cards.iter().map(|(_, note)| *note).collect();
        notes.sort();
        notes.dedup();
        let mut left_empty = Vec::new();
        let mut statement = self
            .conn
            .prepare("SELECT id FROM card WHERE note = ?1 AND deleted = 0")?;
        for note in notes {
            let mut remaining = statement.query_map([note], |r| r.get::<_, Id>(0))?;
            if remaining.all(|card| card.map_or(true, |card| gone.contains(&card))) {
                left_empty.push(note);
            }
        }

        Ok(self.write(|w| {
            if default_moves {
                w.set(DECK.entity, default_deck(), "parent", parent_blob(None))?;
            }
            for deck in &doomed {
                if !deck.register_deleted {
                    w.set(DECK.entity, deck.id, "deleted", flag(true))?;
                }
            }
            for (card, _) in &cards {
                w.set(CARD.entity, *card, "deleted", flag(true))?;
            }
            for note in &left_empty {
                w.set(NOTE.entity, *note, "deleted", flag(true))?;
            }
            Ok(())
        })?)
    }

    /// Brings a deleted deck back, with the decks, cards and notes that were deleted along with it.
    /// A card or note deleted on its own before stays in the trash. Fails if a live deck next to
    /// it has its name now. Restoring a deck that is not deleted does nothing.
    pub fn restore_deck(&self, id: Id) -> Result<(), DeckError> {
        let tree = Tree::load(&self.conn)?;
        let row = tree.get(id).ok_or(DeckError::NotFound)?;
        if !row.register_deleted {
            return Ok(());
        }
        check_name_free(&tree, &row.name, row.parent, Some(id))?;
        let since = self.deleted_since(DECK.entity, id)?;
        let restored: Vec<Id> = tree
            .subtree(id)
            .iter()
            .filter(|r| {
                r.register_deleted
                    && self
                        .deleted_since(DECK.entity, r.id)
                        .is_ok_and(|clock| clock >= since)
            })
            .map(|r| r.id)
            .collect();

        let mut cards: Vec<(Id, Id)> = Vec::new();
        let mut statement = self.conn.prepare(
            "SELECT c.id, c.note FROM card c
             JOIN register_clock rc ON rc.entity_type = 'card' AND rc.entity_id = c.id
                                   AND rc.field = 'deleted'
             WHERE c.deck = ?1 AND c.deleted = 1 AND rc.hlc >= ?2 ORDER BY c.id",
        )?;
        for deck in &restored {
            for found in statement.query_map(rusqlite::params![deck, since], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })? {
                cards.push(found?);
            }
        }
        let mut notes: Vec<Id> = cards.iter().map(|(_, note)| *note).collect();
        notes.sort();
        notes.dedup();
        let mut statement = self.conn.prepare(
            "SELECT n.deleted FROM note n
             JOIN register_clock rc ON rc.entity_type = 'note' AND rc.entity_id = n.id
                                   AND rc.field = 'deleted'
             WHERE n.id = ?1 AND rc.hlc >= ?2",
        )?;
        let mut revived = Vec::new();
        for note in notes {
            let deleted: Option<i64> = statement
                .query_map(rusqlite::params![note, since], |r| r.get(0))?
                .next()
                .transpose()?;
            if deleted == Some(1) {
                revived.push(note);
            }
        }

        Ok(self.write(|w| {
            for deck in &restored {
                w.set(DECK.entity, *deck, "deleted", flag(false))?;
            }
            for note in &revived {
                w.set(NOTE.entity, *note, "deleted", flag(false))?;
            }
            for (card, _) in &cards {
                w.set(CARD.entity, *card, "deleted", flag(false))?;
            }
            Ok(())
        })?)
    }

    /// The clock of a `deleted` register, or 0 if it has none.
    fn deleted_since(&self, entity: &str, id: Id) -> Result<i64, DeckError> {
        Ok(self
            .register_clock(entity, id, "deleted")?
            .map_or(0, |clock| clock.hlc.to_stored()))
    }

    /// Moves live cards into a live deck. Nothing moves if any card is not a live card of a live
    /// note. Only a card whose deck changes is written.
    pub fn move_cards(&self, cards: &[Id], deck: Id) -> Result<(), DeckError> {
        Tree::load(&self.conn)?
            .live(deck)
            .ok_or(DeckError::NotFound)?;
        let mut statement = self.conn.prepare(
            "SELECT c.deck FROM card c JOIN note n ON n.id = c.note
             WHERE c.id = ?1 AND c.deleted = 0 AND n.deleted = 0",
        )?;
        let mut moving = Vec::new();
        for card in cards {
            let stored: Vec<u8> =
                statement
                    .query_row([card], |r| r.get(0))
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => DeckError::NotFound,
                        e => e.into(),
                    })?;
            if card_deck(&stored) != deck {
                moving.push(*card);
            }
        }
        Ok(self.write(|w| {
            for card in &moving {
                w.set(CARD.entity, *card, "deck", blob(deck))?;
            }
            Ok(())
        })?)
    }
}
