//! Options presets: reading them, the rules for their values, and changing them. Every change goes
//! through `Collection::write`, with the checks first (one connection serves the collection, so
//! nothing changes in between).

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use rusqlite::types::Value;

use super::ops::{blob, clean_name, flag, text};
use super::read::Tree;
use super::{
    DECK, DEFAULT_DESIRED_RETENTION, DEFAULT_LEARNING_STEPS, DEFAULT_NEW_PER_DAY,
    DEFAULT_RELEARNING_STEPS, DEFAULT_REVIEWS_PER_DAY, DeckError, PRESET, Preset, default_preset,
};
use crate::collection::Collection;
use crate::id::Id;

const MAX_LIMIT: u32 = 9999;
const MAX_STEPS: usize = 8;
const MAX_STEP_MINUTES: u32 = 1440;
const MIN_RETENTION: f64 = 0.70;
const MAX_RETENTION: f64 = 0.99;

/// A change to a preset's options. A field left as `None` stays as it is, and only a value that
/// differs is written, so two devices that change different options both keep their change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetChange {
    pub new_per_day: Option<u32>,
    pub reviews_per_day: Option<u32>,
    pub learning_steps: Option<Vec<u32>>,
    pub relearning_steps: Option<Vec<u32>>,
    pub desired_retention: Option<f64>,
}

/// A preset is deleted if its register says so and no live deck still uses it. A deck given the
/// preset on another device while this one deleted it keeps it alive (ADR 0006, section 5). The
/// Default preset is never deleted. Decided here, when reading, so nothing is written.
fn effectively_deleted() -> String {
    let default: String = default_preset()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "(p.deleted <> 0 AND p.id <> x'{default}' AND NOT EXISTS
            (SELECT 1 FROM deck d WHERE d.options_preset = p.id AND d.deleted = 0))"
    )
}

/// The presets that exist and are not deleted.
pub(super) fn usable_presets(conn: &Connection) -> rusqlite::Result<HashSet<Id>> {
    let mut statement = conn.prepare(&format!(
        "SELECT p.id FROM options_preset p WHERE NOT {}",
        effectively_deleted()
    ))?;
    statement.query_map([], |row| row.get(0))?.collect()
}

/// Steps as stored: whole minutes separated by spaces. A value that is not a valid step (from a
/// newer app) is left out, so reading never fails.
pub(super) fn parse_steps(stored: &str) -> Vec<u32> {
    stored
        .split_whitespace()
        .filter_map(|word| word.parse::<u32>().ok())
        .filter(|minutes| (1..=MAX_STEP_MINUTES).contains(minutes))
        .take(MAX_STEPS)
        .collect()
}

/// The FSRS parameters as stored: numbers separated by spaces, or empty for the defaults. A value
/// that is not a valid set (a wrong count, a number that is not finite) reads as the defaults. A
/// set from an older FSRS version is filled to 21 numbers.
pub(super) fn parse_parameters(stored: &str) -> Vec<f32> {
    let numbers: Option<Vec<f32>> = stored
        .split_whitespace()
        .map(|word| word.parse::<f32>().ok())
        .collect();
    numbers
        .filter(|numbers| !numbers.is_empty())
        .and_then(|numbers| crate::scheduling::fill_parameters(&numbers))
        .unwrap_or_default()
}

fn check_steps(steps: &[u32]) -> Result<(), DeckError> {
    if steps.len() <= MAX_STEPS && steps.iter().all(|s| (1..=MAX_STEP_MINUTES).contains(s)) {
        Ok(())
    } else {
        Err(DeckError::Steps)
    }
}

fn check_limit(limit: u32) -> Result<(), DeckError> {
    if limit <= MAX_LIMIT {
        Ok(())
    } else {
        Err(DeckError::Limit)
    }
}

fn check_retention(retention: f64) -> Result<(), DeckError> {
    if (MIN_RETENTION..=MAX_RETENTION).contains(&retention) {
        Ok(())
    } else {
        Err(DeckError::Retention)
    }
}

fn join_steps(steps: &[u32]) -> String {
    steps
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

struct Stored {
    preset: Preset,
    register_deleted: bool,
}

/// Every preset, with how many live decks use it. Values are brought into range, so a value from a
/// newer app never breaks reading.
fn load(conn: &Connection, tree: &Tree) -> Result<Vec<Stored>, DeckError> {
    let mut users: HashMap<Id, usize> = HashMap::new();
    for row in tree.rows.iter().filter(|r| !r.deleted) {
        *users.entry(row.preset).or_insert(0) += 1;
    }
    let mut statement = conn.prepare(&format!(
        "SELECT p.id, p.name, p.new_per_day, p.reviews_per_day, p.learning_steps,
                p.desired_retention, p.deleted, {}, p.relearning_steps, p.fsrs_parameters
         FROM options_preset p",
        effectively_deleted()
    ))?;
    let mut found = statement
        .query_map([], |row| {
            let id: Id = row.get(0)?;
            let retention: f64 = row.get(5)?;
            Ok(Stored {
                preset: Preset {
                    id,
                    name: row.get(1)?,
                    new_per_day: row.get::<_, i64>(2)?.clamp(0, MAX_LIMIT.into()) as u32,
                    reviews_per_day: row.get::<_, i64>(3)?.clamp(0, MAX_LIMIT.into()) as u32,
                    learning_steps: parse_steps(&row.get::<_, String>(4)?),
                    relearning_steps: parse_steps(&row.get::<_, String>(8)?),
                    desired_retention: if retention.is_nan() {
                        DEFAULT_DESIRED_RETENTION
                    } else {
                        retention.clamp(MIN_RETENTION, MAX_RETENTION)
                    },
                    fsrs_parameters: parse_parameters(&row.get::<_, String>(9)?),
                    decks: users.get(&id).copied().unwrap_or(0),
                    deleted: row.get::<_, i64>(7)? != 0,
                },
                register_deleted: row.get::<_, i64>(6)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    found.sort_by_cached_key(|s| (s.preset.name.to_lowercase(), s.preset.id));
    Ok(found)
}

impl Collection {
    fn stored_presets(&self) -> Result<Vec<Stored>, DeckError> {
        load(&self.conn, &Tree::load(&self.conn)?)
    }

    fn live_preset(&self, id: Id) -> Result<Stored, DeckError> {
        self.stored_presets()?
            .into_iter()
            .find(|s| s.preset.id == id && !s.preset.deleted)
            .ok_or(DeckError::NotFound)
    }

    fn check_preset_name_free(
        &self,
        name: &str,
        except: Option<Id>,
        presets: &[Stored],
    ) -> Result<(), DeckError> {
        let wanted = name.to_lowercase();
        if presets.iter().any(|s| {
            !s.preset.deleted
                && Some(s.preset.id) != except
                && s.preset.name.to_lowercase() == wanted
        }) {
            Err(DeckError::NameTaken(name.to_owned()))
        } else {
            Ok(())
        }
    }

    /// Every preset that is not deleted, by name.
    pub fn presets(&self) -> Result<Vec<Preset>, DeckError> {
        Ok(self
            .stored_presets()?
            .into_iter()
            .map(|s| s.preset)
            .filter(|p| !p.deleted)
            .collect())
    }

    /// The deleted presets (the trash), by name.
    pub fn deleted_presets(&self) -> Result<Vec<Preset>, DeckError> {
        Ok(self
            .stored_presets()?
            .into_iter()
            .map(|s| s.preset)
            .filter(|p| p.deleted)
            .collect())
    }

    /// One preset, deleted or not.
    pub fn preset(&self, id: Id) -> Result<Option<Preset>, DeckError> {
        Ok(self
            .stored_presets()?
            .into_iter()
            .map(|s| s.preset)
            .find(|p| p.id == id))
    }

    /// The preset a live deck uses.
    pub fn deck_preset(&self, deck: Id) -> Result<Preset, DeckError> {
        let tree = Tree::load(&self.conn)?;
        let preset = tree.live(deck).ok_or(DeckError::NotFound)?.preset;
        load(&self.conn, &tree)?
            .into_iter()
            .map(|s| s.preset)
            .find(|p| p.id == preset)
            .ok_or(DeckError::NotFound)
    }

    /// A new preset with the starting options: 20 new cards and 200 reviews a day, learning steps
    /// of 1 and 10 minutes, desired retention 0.90.
    pub fn create_preset(&self, name: &str) -> Result<Id, DeckError> {
        let name = clean_name(name)?;
        self.check_preset_name_free(&name, None, &self.stored_presets()?)?;
        Ok(self.write(|w| {
            let id = w.new_id()?;
            w.insert(
                PRESET.entity,
                id,
                vec![
                    ("name", text(&name)),
                    ("new_per_day", Value::Integer(DEFAULT_NEW_PER_DAY.into())),
                    (
                        "reviews_per_day",
                        Value::Integer(DEFAULT_REVIEWS_PER_DAY.into()),
                    ),
                    ("learning_steps", text(DEFAULT_LEARNING_STEPS)),
                    ("desired_retention", Value::Real(DEFAULT_DESIRED_RETENTION)),
                    ("relearning_steps", text(DEFAULT_RELEARNING_STEPS)),
                    ("fsrs_parameters", text("")),
                    ("deleted", flag(false)),
                ],
            )?;
            Ok(id)
        })?)
    }

    pub fn rename_preset(&self, id: Id, name: &str) -> Result<(), DeckError> {
        let name = clean_name(name)?;
        let presets = self.stored_presets()?;
        let current = self.live_preset(id)?;
        self.check_preset_name_free(&name, Some(id), &presets)?;
        if current.preset.name == name {
            return Ok(());
        }
        Ok(self.write(|w| w.set(PRESET.entity, id, "name", text(&name)))?)
    }

    /// Changes options of a preset. Everything is checked first, so a bad value changes nothing.
    pub fn set_preset_options(&self, id: Id, change: &PresetChange) -> Result<(), DeckError> {
        if let Some(limit) = change.new_per_day {
            check_limit(limit)?;
        }
        if let Some(limit) = change.reviews_per_day {
            check_limit(limit)?;
        }
        if let Some(steps) = &change.learning_steps {
            check_steps(steps)?;
        }
        if let Some(steps) = &change.relearning_steps {
            check_steps(steps)?;
        }
        if let Some(retention) = change.desired_retention {
            check_retention(retention)?;
        }
        let current = self.live_preset(id)?.preset;
        Ok(self.write(|w| {
            if let Some(limit) = change.new_per_day.filter(|l| *l != current.new_per_day) {
                w.set(
                    PRESET.entity,
                    id,
                    "new_per_day",
                    Value::Integer(limit.into()),
                )?;
            }
            if let Some(limit) = change
                .reviews_per_day
                .filter(|l| *l != current.reviews_per_day)
            {
                w.set(
                    PRESET.entity,
                    id,
                    "reviews_per_day",
                    Value::Integer(limit.into()),
                )?;
            }
            if let Some(steps) = change
                .learning_steps
                .as_ref()
                .filter(|s| **s != current.learning_steps)
            {
                w.set(
                    PRESET.entity,
                    id,
                    "learning_steps",
                    text(&join_steps(steps)),
                )?;
            }
            if let Some(steps) = change
                .relearning_steps
                .as_ref()
                .filter(|s| **s != current.relearning_steps)
            {
                w.set(
                    PRESET.entity,
                    id,
                    "relearning_steps",
                    text(&join_steps(steps)),
                )?;
            }
            if let Some(retention) = change
                .desired_retention
                .filter(|r| *r != current.desired_retention)
            {
                w.set(
                    PRESET.entity,
                    id,
                    "desired_retention",
                    Value::Real(retention),
                )?;
            }
            Ok(())
        })?)
    }

    /// Deletes a preset. The decks that use it go back to the Default preset, so no deck is left
    /// without options. The Default preset cannot be deleted. Deleting a deleted preset does
    /// nothing.
    pub fn delete_preset(&self, id: Id) -> Result<(), DeckError> {
        if id == default_preset() {
            return Err(DeckError::Default);
        }
        let stored = self
            .stored_presets()?
            .into_iter()
            .find(|s| s.preset.id == id)
            .ok_or(DeckError::NotFound)?;
        if stored.preset.deleted {
            return Ok(());
        }
        let mut statement = self
            .conn
            .prepare("SELECT id FROM deck WHERE options_preset = ?1 ORDER BY id")?;
        let decks = statement
            .query_map([id], |row| row.get::<_, Id>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.write(|w| {
            for deck in decks {
                w.set(DECK.entity, deck, "options_preset", blob(default_preset()))?;
            }
            if !stored.register_deleted {
                w.set(PRESET.entity, id, "deleted", flag(true))?;
            }
            Ok(())
        })?)
    }

    /// Brings a deleted preset back. The decks that used it stay on the Default preset. Fails if a
    /// live preset has its name now. Restoring a preset that is not deleted does nothing.
    pub fn restore_preset(&self, id: Id) -> Result<(), DeckError> {
        let presets = self.stored_presets()?;
        let stored = presets
            .iter()
            .find(|s| s.preset.id == id)
            .ok_or(DeckError::NotFound)?;
        if !stored.register_deleted {
            return Ok(());
        }
        self.check_preset_name_free(&stored.preset.name, Some(id), &presets)?;
        Ok(self.write(|w| w.set(PRESET.entity, id, "deleted", flag(false)))?)
    }

    /// Makes a live deck use a live preset. Several decks can use one preset.
    pub fn set_deck_preset(&self, deck: Id, preset: Id) -> Result<(), DeckError> {
        let tree = Tree::load(&self.conn)?;
        let row = tree.live(deck).ok_or(DeckError::NotFound)?;
        self.live_preset(preset)?;
        if row.preset == preset {
            return Ok(());
        }
        Ok(self.write(|w| w.set(DECK.entity, deck, "options_preset", blob(preset)))?)
    }
}
