//! Reading a backup file back in (ADR 0006, section 11, "A backup restored on a synced device").
//!
//! A restore is not a merge. A backup's registers are older than the ones in the collection, so a
//! merge would ignore all of them and roll nothing back. A restore writes the *difference*: every
//! value that differs from the backup's is written again as a new local write (new clock, not
//! pushed), so a sync carries it to the other devices like any edit, and it does not lose to a
//! change made elsewhere in the meantime. What exists now and did not exist in the backup is moved to
//! the trash, never hard-deleted. Card events are only added, never taken away.

use std::collections::{HashMap, HashSet};

use rusqlite::types::Value;

use super::changes::Changes;
use super::merge::{MergeReport, Origin, Targets, Touched, touch_entity};
use super::{DYNAMIC_TABLES, WriteTx};
use crate::collection::{Collection, CollectionError};
use crate::id::Id;

/// How a file is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileMode {
    /// A whole-collection backup: the collection ends up with the backup's content.
    Restore,
    /// A deck from a file: what the file has is added, and what the collection has stays.
    Import,
}

/// What applying a file did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Applied {
    pub report: MergeReport,
    /// Things that were not in the backup and went to the trash (a restore only).
    pub trashed: usize,
    /// What the `extra` step of the call returned (the media files it added).
    pub extra: usize,
}

impl Collection {
    /// Applies a file's changes in one write transaction, then runs `extra` inside the same
    /// transaction (the media bytes), so that a failure of either leaves the collection as it was.
    pub(crate) fn apply_file(
        &self,
        changes: &Changes,
        mode: FileMode,
        extra: impl FnOnce(&mut WriteTx<'_>) -> Result<usize, CollectionError>,
    ) -> Result<Applied, CollectionError> {
        let targets = Targets::load(&self.conn, self.schema)?;
        let origin = match mode {
            FileMode::Restore => Origin::Restore,
            FileMode::Import => Origin::Import,
        };
        self.write(|w| {
            // What this writes is stamped after everything in the file, so reading the same file
            // again finds nothing newer in it, and a device whose clock is behind the file's still
            // beats it.
            if let Some(highest) = changes.registers.iter().map(|r| r.clock.hlc).max() {
                w.hlc = w.hlc.observe(highest);
            }
            let (mut report, mut touched) = self.apply_batch(w, &targets, changes, origin)?;
            let trashed = match mode {
                FileMode::Restore => trash_what_is_not_in(w, &targets, changes, &mut touched)?,
                FileMode::Import => 0,
            };
            let extra = extra(w)?;
            self.finish_batch(w, &touched, &mut report)?;
            Ok(Applied {
                report,
                trashed,
                extra,
            })
        })
    }
}

/// Everything live that the backup does not know goes to the trash, and so does a tag a note in
/// the backup did not have. Two kinds of thing stay as they are:
///
/// - built-in rows that were never edited (their `deleted` clock is the seed's), because a backup
///   from an older version does not have the built-ins a newer one added;
/// - entities with no `deleted` register (settings), which a restore leaves alone.
fn trash_what_is_not_in(
    w: &mut WriteTx<'_>,
    targets: &Targets,
    changes: &Changes,
    touched: &mut Touched,
) -> Result<usize, CollectionError> {
    let mut known: HashMap<&str, HashSet<Id>> = HashMap::new();
    let mut known_tags: HashSet<(Id, &str)> = HashSet::new();
    for change in &changes.registers {
        known
            .entry(change.entity.as_str())
            .or_default()
            .insert(change.entity_id);
        if change.entity == "note_tag" {
            known_tags.insert((change.entity_id, change.field.as_str()));
        }
    }
    let mut trashed = 0;
    for table in targets
        .synced
        .iter()
        .filter(|t| t.registers.contains(&"deleted"))
    {
        let mut statement = w.local().prepare(&format!(
            "SELECT t.id FROM \"{}\" t
             JOIN register_clock c
               ON c.entity_type = ?1 AND c.entity_id = t.id AND c.field = 'deleted'
             WHERE t.deleted = 0 AND c.hlc <> 0",
            table.table
        ))?;
        let live: Vec<Id> = statement
            .query_map([table.entity], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        drop(statement);
        let in_backup = known.get(table.entity);
        for id in live {
            if in_backup.is_some_and(|ids| ids.contains(&id)) {
                continue;
            }
            w.set(table.entity, id, "deleted", Value::Integer(1))?;
            touch_entity(touched, table.entity, id);
            trashed += 1;
        }
    }
    // Tags: only on notes the backup has, since a note that is going to the trash keeps its tags.
    if let Some(tag_table) = DYNAMIC_TABLES.iter().find(|t| t.entity == "note_tag")
        && let Some(notes) = known.get("note")
    {
        let mut statement = w.local().prepare(&format!(
            "SELECT \"{o}\", \"{k}\" FROM \"{t}\" WHERE \"{v}\" <> ''",
            t = tag_table.table,
            o = tag_table.owner,
            k = tag_table.key,
            v = tag_table.value
        ))?;
        let tagged: Vec<(Id, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        drop(statement);
        for (note, tag) in tagged {
            if notes.contains(&note) && !known_tags.contains(&(note, tag.as_str())) {
                w.set_text_value("note_tag", note, &tag, "")?;
                trashed += 1;
            }
        }
    }
    Ok(trashed)
}
