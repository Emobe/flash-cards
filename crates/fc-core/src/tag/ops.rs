//! Changing tags. Every change goes through `Collection::write`, so every register it writes gets a
//! clock, and only the `(note, tag)` pairs that change are written, so two devices that change
//! different tags of one note both keep theirs. The checks run before the write: one connection
//! serves the collection, so nothing changes in between.

use std::collections::{HashMap, HashSet};

use super::read::{Snapshot, clean, is_within, key};
use super::{NOTE_TAG, PRESENT, SEPARATOR, TagError};
use crate::collection::Collection;
use crate::id::Id;

/// One change to one `(note, tag)` register.
struct Change {
    note: Id,
    tag: String,
    present: bool,
}

impl Collection {
    /// Fails with `NotFound` unless every note is live.
    fn check_live_notes(&self, notes: &[Id]) -> Result<(), TagError> {
        for note in notes {
            let deleted: Option<i64> = rusqlite::OptionalExtension::optional(self.conn.query_row(
                "SELECT deleted FROM note WHERE id = ?1",
                [note],
                |row| row.get(0),
            ))?;
            if deleted != Some(0) {
                return Err(TagError::NotFound);
            }
        }
        Ok(())
    }

    fn apply(&self, changes: &[Change]) -> Result<(), TagError> {
        if changes.is_empty() {
            return Ok(());
        }
        Ok(self.write(|w| {
            for change in changes {
                w.set_text_value(
                    NOTE_TAG.entity,
                    change.note,
                    &change.tag,
                    if change.present { PRESENT } else { "" },
                )?;
            }
            Ok(())
        })?)
    }

    /// Gives every note the tags it does not have yet. A tag is spelled the way the collection
    /// already spells it (`Polish` becomes `polish` if `polish` is in use). All the notes must be
    /// live, or nothing changes. Returns how many `(note, tag)` pairs were added.
    pub fn add_tags(&self, notes: &[Id], tags: &[&str]) -> Result<usize, TagError> {
        let tags = tags
            .iter()
            .map(|tag| clean(tag))
            .collect::<Result<Vec<_>, _>>()?;
        self.check_live_notes(notes)?;
        let snapshot = Snapshot::load(&self.conn)?;
        let have = snapshot.by_note();
        let mut wanted = Vec::new();
        let mut seen = HashSet::new();
        for tag in &tags {
            let shown = snapshot.display(tag);
            if seen.insert(key(&shown)) {
                wanted.push(shown);
            }
        }
        let mut changes = Vec::new();
        let mut done = HashSet::new();
        for note in notes.iter().filter(|note| done.insert(**note)) {
            let present: HashSet<String> = have
                .get(note)
                .map(|tags| tags.iter().map(|tag| key(tag)).collect())
                .unwrap_or_default();
            for tag in &wanted {
                if !present.contains(&key(tag)) {
                    changes.push(Change {
                        note: *note,
                        tag: tag.clone(),
                        present: true,
                    });
                }
            }
        }
        self.apply(&changes)?;
        Ok(changes.len())
    }

    /// Takes tags off notes. Only the tag itself goes, not tags inside it, and a note that does not
    /// have it is left alone. All the notes must be live, or nothing changes. Returns how many
    /// `(note, tag)` pairs were removed.
    pub fn remove_tags(&self, notes: &[Id], tags: &[&str]) -> Result<usize, TagError> {
        let keys: HashSet<String> = tags
            .iter()
            .map(|tag| clean(tag).map(|tag| key(&tag)))
            .collect::<Result<_, _>>()?;
        self.check_live_notes(notes)?;
        let snapshot = Snapshot::load(&self.conn)?;
        let notes: HashSet<Id> = notes.iter().copied().collect();
        let changes: Vec<Change> = snapshot
            .rows
            .iter()
            .filter(|row| notes.contains(&row.note) && keys.contains(&key(&row.tag)))
            .map(|row| Change {
                note: row.note,
                tag: row.tag.clone(),
                present: false,
            })
            .collect();
        self.apply(&changes)?;
        Ok(changes.len())
    }

    /// Makes the tags of a live note exactly `tags`: it gains the ones it lacks and loses the others.
    pub fn set_note_tags(&self, note: Id, tags: &[&str]) -> Result<(), TagError> {
        let tags = tags
            .iter()
            .map(|tag| clean(tag))
            .collect::<Result<Vec<_>, _>>()?;
        self.check_live_notes(&[note])?;
        let snapshot = Snapshot::load(&self.conn)?;
        let mut wanted: HashMap<String, String> = HashMap::new();
        for tag in &tags {
            let shown = snapshot.display(tag);
            wanted.entry(key(&shown)).or_insert(shown);
        }
        let mut present = HashSet::new();
        let mut changes = Vec::new();
        for row in snapshot.rows.iter().filter(|row| row.note == note) {
            present.insert(key(&row.tag));
            if !wanted.contains_key(&key(&row.tag)) {
                changes.push(Change {
                    note,
                    tag: row.tag.clone(),
                    present: false,
                });
            }
        }
        let mut added: Vec<_> = wanted
            .into_iter()
            .filter(|(k, _)| !present.contains(k))
            .map(|(_, tag)| Change {
                note,
                tag,
                present: true,
            })
            .collect();
        added.sort_by(|a, b| a.tag.cmp(&b.tag));
        changes.extend(added);
        self.apply(&changes)
    }

    /// Renames a tag and everything inside it: renaming `lang` to `language` also renames
    /// `lang::polish` to `language::polish`. A tag renamed onto one that exists merges with it. The
    /// rewrite includes notes in the trash, so restoring one does not bring the old name back. Only
    /// the pairs that change are written. Returns how many live notes changed.
    pub fn rename_tag(&self, from: &str, to: &str) -> Result<usize, TagError> {
        let from = clean(from)?;
        let to = clean(to)?;
        let from_key = key(&from);
        let snapshot = Snapshot::load(&self.conn)?;
        let affected = |tag: &str| is_within(&key(tag), &from_key);
        if !snapshot
            .rows
            .iter()
            .any(|row| row.live && affected(&row.tag))
        {
            return Err(TagError::NoSuchTag(from));
        }
        // Spell the new names after the tags that stay as they are.
        let rest = Snapshot::new(
            snapshot
                .rows
                .iter()
                .filter(|row| !affected(&row.tag))
                .map(|row| super::read::Row {
                    note: row.note,
                    tag: row.tag.clone(),
                    live: row.live,
                })
                .collect(),
        );
        let moved = from.split(SEPARATOR).count();
        let target = |stored: &str| {
            let inside: Vec<&str> = stored.split(SEPARATOR).skip(moved).collect();
            let mut name = to.clone();
            for part in inside {
                name.push_str(SEPARATOR);
                name.push_str(part);
            }
            rest.display(&name)
        };
        let mut old: HashMap<Id, HashSet<&str>> = HashMap::new();
        let mut new: HashMap<Id, HashSet<String>> = HashMap::new();
        let mut present: HashMap<Id, HashSet<&str>> = HashMap::new();
        for row in &snapshot.rows {
            present.entry(row.note).or_default().insert(&row.tag);
            if affected(&row.tag) {
                old.entry(row.note).or_default().insert(&row.tag);
                new.entry(row.note).or_default().insert(target(&row.tag));
            }
        }
        let live: HashSet<Id> = snapshot
            .rows
            .iter()
            .filter(|row| row.live)
            .map(|row| row.note)
            .collect();
        let mut changes = Vec::new();
        let mut notes: Vec<&Id> = old.keys().collect();
        notes.sort();
        for note in notes {
            let wanted = &new[note];
            let mut gone: Vec<&str> = old[note]
                .iter()
                .copied()
                .filter(|tag| !wanted.contains(*tag))
                .collect();
            gone.sort_unstable();
            changes.extend(gone.into_iter().map(|tag| Change {
                note: *note,
                tag: tag.to_owned(),
                present: false,
            }));
            let mut came: Vec<&String> = wanted
                .iter()
                .filter(|tag| !present[note].contains(tag.as_str()))
                .collect();
            came.sort();
            changes.extend(came.into_iter().map(|tag| Change {
                note: *note,
                tag: tag.clone(),
                present: true,
            }));
        }
        self.apply(&changes)?;
        let changed: HashSet<Id> = changes
            .iter()
            .map(|change| change.note)
            .filter(|note| live.contains(note))
            .collect();
        Ok(changed.len())
    }

    /// Takes a tag, and every tag inside it, off all notes (the trash too). Returns how many live
    /// notes had one.
    pub fn delete_tag(&self, tag: &str) -> Result<usize, TagError> {
        let tag = clean(tag)?;
        let wanted = key(&tag);
        let snapshot = Snapshot::load(&self.conn)?;
        let affected: Vec<_> = snapshot
            .rows
            .iter()
            .filter(|row| is_within(&key(&row.tag), &wanted))
            .collect();
        if !affected.iter().any(|row| row.live) {
            return Err(TagError::NoSuchTag(tag));
        }
        let changes: Vec<Change> = affected
            .iter()
            .map(|row| Change {
                note: row.note,
                tag: row.tag.clone(),
                present: false,
            })
            .collect();
        self.apply(&changes)?;
        let live: HashSet<Id> = affected
            .iter()
            .filter(|row| row.live)
            .map(|row| row.note)
            .collect();
        Ok(live.len())
    }
}
