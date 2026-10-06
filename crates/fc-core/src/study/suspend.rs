//! Suspending and burying cards (ADR 0007, part 11). Both are registers of the card, so they merge
//! like any other register: suspending on one device and reviewing on another keeps both.
//!
//! A suspended card stays out of the queue until it is unsuspended. A buried card stays out until
//! the study day after the one it was buried on: `buried_until` holds that day, the card is hidden
//! while today is at or before it, and a new day lifts it with no write. Only cards that change are
//! written.

use rusqlite::types::Value;

use super::StudyError;
use crate::collection::Collection;
use crate::deck::{Tree, card_deck};
use crate::id::Id;
use crate::note::CARD;
use crate::scheduling::study_day;

/// A live card's suspension and burial, as stored.
struct Hidden {
    suspended: bool,
    buried_until: i64,
}

impl Collection {
    /// The current study day.
    pub(super) fn today(&self) -> Result<i64, StudyError> {
        let now = self.host.clock.now();
        Ok(study_day(
            now.unix_ms,
            now.utc_offset_minutes,
            self.day_start_hour()?,
        ))
    }

    /// Reads the cards, or fails with `NotFound` if any is deleted or missing, before anything is
    /// written.
    fn hidden(&self, cards: &[Id]) -> Result<Vec<(Id, Hidden)>, StudyError> {
        let mut statement = self.conn.prepare(
            "SELECT c.suspended, c.buried_until FROM card c JOIN note n ON n.id = c.note
             WHERE c.id = ?1 AND c.deleted = 0 AND n.deleted = 0",
        )?;
        let mut found: Vec<(Id, Hidden)> = Vec::with_capacity(cards.len());
        for card in cards {
            if found.iter().any(|(id, _)| id == card) {
                continue;
            }
            let hidden = statement
                .query_row([card], |row| {
                    Ok(Hidden {
                        suspended: row.get::<_, i64>(0)? != 0,
                        buried_until: row.get(1)?,
                    })
                })
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => StudyError::NotFound,
                    error => error.into(),
                })?;
            found.push((*card, hidden));
        }
        Ok(found)
    }

    fn set_suspended(&self, cards: &[Id], on: bool) -> Result<(), StudyError> {
        let changing: Vec<Id> = self
            .hidden(cards)?
            .into_iter()
            .filter(|(_, hidden)| hidden.suspended != on)
            .map(|(id, _)| id)
            .collect();
        Ok(self.write(|w| {
            for card in &changing {
                w.set(
                    CARD.entity,
                    *card,
                    "suspended",
                    Value::Integer(i64::from(on)),
                )?;
            }
            Ok(())
        })?)
    }

    /// Takes the cards out of the queue until they are unsuspended. A card that is suspended
    /// already is left alone.
    pub fn suspend_cards(&self, cards: &[Id]) -> Result<(), StudyError> {
        self.set_suspended(cards, true)
    }

    pub fn unsuspend_cards(&self, cards: &[Id]) -> Result<(), StudyError> {
        self.set_suspended(cards, false)
    }

    /// Hides the cards for the rest of today. The next study day they are back.
    pub fn bury_cards(&self, cards: &[Id]) -> Result<(), StudyError> {
        let today = self.today()?;
        let changing: Vec<Id> = self
            .hidden(cards)?
            .into_iter()
            .filter(|(_, hidden)| hidden.buried_until != today)
            .map(|(id, _)| id)
            .collect();
        Ok(self.write(|w| {
            for card in &changing {
                w.set(CARD.entity, *card, "buried_until", Value::Integer(today))?;
            }
            Ok(())
        })?)
    }

    /// Brings buried cards back today. A card that is not buried is left alone.
    pub fn unbury_cards(&self, cards: &[Id]) -> Result<(), StudyError> {
        let today = self.today()?;
        let changing: Vec<Id> = self
            .hidden(cards)?
            .into_iter()
            .filter(|(_, hidden)| hidden.buried_until >= today)
            .map(|(id, _)| id)
            .collect();
        self.clear_burial(&changing)
    }

    /// Brings back every card buried in a deck and the decks inside it.
    pub fn unbury_deck(&self, deck: Id) -> Result<(), StudyError> {
        let today = self.today()?;
        let tree = Tree::load(&self.conn)?;
        tree.live(deck).ok_or(StudyError::NotFound)?;
        let mut statement = self.conn.prepare(
            "SELECT c.id, c.deck FROM card c JOIN note n ON n.id = c.note
             WHERE c.deleted = 0 AND n.deleted = 0 AND c.buried_until >= ?1",
        )?;
        let buried: Vec<(Id, Vec<u8>)> = statement
            .query_map([today], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let changing: Vec<Id> = buried
            .into_iter()
            .filter(|(_, stored)| tree.is_inside(card_deck(stored), deck))
            .map(|(id, _)| id)
            .collect();
        self.clear_burial(&changing)
    }

    fn clear_burial(&self, cards: &[Id]) -> Result<(), StudyError> {
        Ok(self.write(|w| {
            for card in cards {
                w.set(CARD.entity, *card, "buried_until", Value::Integer(0))?;
            }
            Ok(())
        })?)
    }
}
