//! What names media: references, and the lists of unused and missing files.

use std::collections::{BTreeMap, BTreeSet};

use super::name::digits_in;
use super::{HASH_DIGITS, MediaError, MediaFile};
use crate::collection::Collection;
use crate::html;
use crate::id::Id;

/// A name that notes, templates or styles use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub name: String,
    /// The notes with the name in a field (including notes in the trash).
    pub notes: Vec<Id>,
    /// The note types with the name in a template or in their CSS.
    pub note_types: Vec<Id>,
}

/// The result of [`Collection::check_media`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaReport {
    /// Files nothing names. [`Collection::delete_unused_media`] deletes these.
    pub unused: Vec<MediaFile>,
    /// Names that belong to no file: the file was never added, or the name is not in the format.
    pub missing: Vec<Reference>,
    /// Files in use whose bytes are not on this device.
    pub without_bytes: Vec<MediaFile>,
}

/// A name in a template that is filled in later (`<img src="{{Image}}">`) is not a file.
fn is_template_text(name: &str) -> bool {
    name.contains("{{") || name.contains("}}")
}

impl Collection {
    /// Every name that a note field, a template or a note type's CSS uses, with where. Notes in the
    /// trash and removed templates count, because restoring them brings the name back. Ordered by
    /// name.
    pub fn media_references(&self) -> Result<Vec<Reference>, MediaError> {
        let mut found: BTreeMap<String, (BTreeSet<Id>, BTreeSet<Id>)> = BTreeMap::new();

        let mut values = self
            .conn
            .prepare("SELECT note, value FROM note_field_value WHERE value LIKE '%<%' OR value LIKE '%[sound:%'")?;
        let mut rows = values.query([])?;
        while let Some(row) = rows.next()? {
            let (note, value): (Id, String) = (row.get(0)?, row.get(1)?);
            for name in html::media_names(&html::expand_sound(&value)) {
                found.entry(name).or_default().0.insert(note);
            }
        }

        let mut templates = self
            .conn
            .prepare("SELECT note_type, front, back FROM template")?;
        let mut rows = templates.query([])?;
        while let Some(row) = rows.next()? {
            let note_type: Id = row.get(0)?;
            for side in [row.get::<_, String>(1)?, row.get::<_, String>(2)?] {
                for name in html::media_names(&html::expand_sound(&side)) {
                    if !is_template_text(&name) {
                        found.entry(name).or_default().1.insert(note_type);
                    }
                }
            }
        }

        let mut styles = self.conn.prepare("SELECT id, css FROM note_type")?;
        let mut rows = styles.query([])?;
        while let Some(row) = rows.next()? {
            let (note_type, css): (Id, String) = (row.get(0)?, row.get(1)?);
            for name in html::css_media_names(&css) {
                if !is_template_text(&name) {
                    found.entry(name).or_default().1.insert(note_type);
                }
            }
        }

        Ok(found
            .into_iter()
            .map(|(name, (notes, note_types))| Reference {
                name,
                notes: notes.into_iter().collect(),
                note_types: note_types.into_iter().collect(),
            })
            .collect())
    }

    /// Finds the unused files, the missing files and the used files without bytes.
    pub fn check_media(&self) -> Result<MediaReport, MediaError> {
        let references = self.media_references()?;
        let rows = self.media_rows()?;
        let mut by_digits: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (at, row) in rows.iter().enumerate() {
            by_digits
                .entry(&row.hash[..HASH_DIGITS.min(row.hash.len())])
                .or_default()
                .push(at);
        }
        let mut used = BTreeSet::new();
        let mut missing = Vec::new();
        for reference in references {
            let found = digits_in(&reference.name)
                .and_then(|digits| by_digits.get(digits.as_str()))
                .filter(|rows| rows.len() == 1);
            match found {
                Some(rows) => {
                    used.insert(rows[0]);
                }
                None => missing.push(reference),
            }
        }
        let mut report = MediaReport {
            missing,
            ..Default::default()
        };
        for (at, row) in rows.iter().enumerate() {
            let is_used = used.contains(&at);
            if is_used && !row.has_bytes {
                report.without_bytes.push(row.file(true));
            }
            if !is_used && !row.deleted {
                report.unused.push(row.file(false));
            }
        }
        Ok(report)
    }
}

/// The first `HASH_DIGITS` digits of every file that some name belongs to.
pub(super) fn named_digits(collection: &Collection) -> Result<BTreeSet<String>, MediaError> {
    Ok(collection
        .media_references()?
        .iter()
        .filter_map(|r| digits_in(&r.name))
        .collect())
}
