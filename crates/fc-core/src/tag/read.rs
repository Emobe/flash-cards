//! Reading tags, and the read-time rules that make every device show the same thing.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::Connection;

use super::{PRESENT, SEPARATOR, Tag, TagError};
use crate::collection::Collection;
use crate::id::Id;

/// Trims a tag and checks its shape. A tag is not empty, has no whitespace and has no empty part.
pub(super) fn clean(name: &str) -> Result<String, TagError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TagError::Empty);
    }
    if name.chars().any(char::is_whitespace) {
        return Err(TagError::Whitespace(name.to_owned()));
    }
    if name.split(SEPARATOR).any(str::is_empty) {
        return Err(TagError::EmptyPart(name.to_owned()));
    }
    Ok(name.to_owned())
}

/// What tags are compared by: the name in lower case.
pub(super) fn key(name: &str) -> String {
    name.to_lowercase()
}

/// Whether `stored` is the tag `wanted` or inside it, both as keys.
pub(super) fn is_within(stored: &str, wanted: &str) -> bool {
    stored == wanted
        || stored
            .strip_prefix(wanted)
            .is_some_and(|rest| rest.starts_with(SEPARATOR))
}

/// One `(note, tag)` with the tag present, as stored.
pub(super) struct Row {
    pub note: Id,
    pub tag: String,
    /// The note is not deleted.
    pub live: bool,
}

/// Every present tag of every note, and how each part of a name is spelled.
pub(super) struct Snapshot {
    pub rows: Vec<Row>,
    /// For each prefix of a name (as a key), the spelling of its last part: the smallest spelling
    /// of that part under that parent in any note, deleted ones too, so a restore changes nothing.
    spelling: HashMap<String, String>,
}

impl Snapshot {
    pub fn load(conn: &Connection) -> Result<Self, TagError> {
        let mut statement = conn.prepare(
            "SELECT t.note, t.tag, n.deleted FROM note_tag t JOIN note n ON n.id = t.note
             WHERE t.present = ?1 ORDER BY t.note, t.tag",
        )?;
        let rows = statement
            .query_map([PRESENT], |row| {
                Ok(Row {
                    note: row.get(0)?,
                    tag: row.get(1)?,
                    live: row.get::<_, i64>(2)? == 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(rows))
    }

    pub fn new(rows: Vec<Row>) -> Self {
        let mut spelling: HashMap<String, String> = HashMap::new();
        for row in &rows {
            let mut prefix = String::new();
            for part in row.tag.split(SEPARATOR) {
                if !prefix.is_empty() {
                    prefix.push_str(SEPARATOR);
                }
                prefix.push_str(&key(part));
                let entry = spelling
                    .entry(prefix.clone())
                    .or_insert_with(|| part.to_owned());
                if part < entry.as_str() {
                    *entry = part.to_owned();
                }
            }
        }
        Self { rows, spelling }
    }

    /// The name with every part spelled the way the collection already spells it. A part the
    /// collection does not have yet stays as typed.
    pub fn display(&self, name: &str) -> String {
        let mut prefix = String::new();
        let mut out = Vec::new();
        for part in name.split(SEPARATOR) {
            if !prefix.is_empty() {
                prefix.push_str(SEPARATOR);
            }
            prefix.push_str(&key(part));
            out.push(self.spelling.get(&prefix).map_or(part, String::as_str));
        }
        out.join(SEPARATOR)
    }

    /// The tags each note has, as stored.
    pub fn by_note(&self) -> HashMap<Id, Vec<&str>> {
        let mut found: HashMap<Id, Vec<&str>> = HashMap::new();
        for row in &self.rows {
            found.entry(row.note).or_default().push(&row.tag);
        }
        found
    }
}

impl Collection {
    /// The tags of a note as shown, in alphabetical order. Works for deleted notes too.
    pub fn note_tags(&self, note: Id) -> Result<Vec<String>, TagError> {
        let snapshot = Snapshot::load(&self.conn)?;
        let mut shown: Vec<String> = snapshot
            .rows
            .iter()
            .filter(|row| row.note == note)
            .map(|row| snapshot.display(&row.tag))
            .collect();
        shown.sort_by_key(|name| key(name));
        shown.dedup_by_key(|name| key(name));
        Ok(shown)
    }

    /// Every tag that a live note has, and every parent of one, as a tree in alphabetical order
    /// (a tag is followed by what is inside it), with counts of live notes.
    pub fn tags(&self) -> Result<Vec<Tag>, TagError> {
        let snapshot = Snapshot::load(&self.conn)?;
        // key -> (exact, total) as sets of notes, so a note counts once however it is tagged.
        let mut exact: HashMap<String, HashSet<Id>> = HashMap::new();
        let mut within: HashMap<String, HashSet<Id>> = HashMap::new();
        for row in snapshot.rows.iter().filter(|row| row.live) {
            let name = key(&row.tag);
            exact.entry(name.clone()).or_default().insert(row.note);
            let mut prefix = String::new();
            for part in name.split(SEPARATOR) {
                if !prefix.is_empty() {
                    prefix.push_str(SEPARATOR);
                }
                prefix.push_str(part);
                within.entry(prefix.clone()).or_default().insert(row.note);
            }
        }
        let ordered: BTreeMap<Vec<&str>, &String> = within
            .keys()
            .map(|name| (name.split(SEPARATOR).collect(), name))
            .collect();
        Ok(ordered
            .into_values()
            .map(|name| Tag {
                name: snapshot.display(name),
                depth: name.matches(SEPARATOR).count(),
                notes: exact.get(name).map_or(0, HashSet::len),
                total: within[name].len(),
            })
            .collect())
    }

    /// The live notes that have a tag, or, with `include_children`, a tag inside it too, in
    /// ID order. This is what a search for the tag matches (step 1.9).
    pub fn notes_with_tag(&self, tag: &str, include_children: bool) -> Result<Vec<Id>, TagError> {
        let wanted = key(&clean(tag)?);
        let snapshot = Snapshot::load(&self.conn)?;
        let mut found: Vec<Id> = snapshot
            .rows
            .iter()
            .filter(|row| row.live)
            .filter(|row| {
                let stored = key(&row.tag);
                if include_children {
                    is_within(&stored, &wanted)
                } else {
                    stored == wanted
                }
            })
            .map(|row| row.note)
            .collect();
        found.sort();
        found.dedup();
        Ok(found)
    }
}
