//! Reading a backup: the manifest, restore and import.

use std::io::{BufReader, Read, Seek};

use rusqlite::params;
use sha2::{Digest, Sha256};
use zip::ZipArchive;
use zip::result::ZipError;

use super::format::{hex, manifest_from_json, read_changes};
use super::{BackupError, CHANGES_ENTRY, MANIFEST_ENTRY, MEDIA_PREFIX, Manifest, ManifestScope};
use crate::collection::{Collection, CollectionError};
use crate::sync::{FileMode, Rejected, WriteTx};

/// What a restore or an import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub manifest: Manifest,
    /// Values written again, as new changes.
    pub registers_written: usize,
    /// Values the collection already had.
    pub registers_unchanged: usize,
    /// Review events and parameter sets that were new here.
    pub rows_added: usize,
    /// Registers and rows this build does not know and kept for a newer one.
    pub unknown_kept: usize,
    pub notes_reconciled: usize,
    pub cards_rebuilt: usize,
    /// What a restore moved to the trash because the backup did not have it (and tags it took off
    /// notes). Always 0 for an import.
    pub removed: usize,
    /// Media files whose bytes were new here.
    pub media_added: usize,
    /// Changes that did not fit and were skipped.
    pub rejected: Vec<Rejected>,
}

fn open<R: Read + Seek>(input: R) -> Result<(ZipArchive<R>, Manifest), BackupError> {
    let mut archive = ZipArchive::new(input).map_err(|error| match error {
        ZipError::Io(error) => BackupError::Io(error.to_string()),
        _ => BackupError::NotABackup,
    })?;
    let manifest = {
        let mut entry = archive
            .by_name(MANIFEST_ENTRY)
            .map_err(|error| match error {
                ZipError::FileNotFound => BackupError::NotABackup,
                other => other.into(),
            })?;
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .map_err(|_| BackupError::NotABackup)?;
        let json = serde_json::from_str(&text).map_err(|_| BackupError::NotABackup)?;
        manifest_from_json(&json)?
    };
    Ok((archive, manifest))
}

/// Reads only what a file says it holds, so a host can show it before anything is restored.
pub fn read_manifest<R: Read + Seek>(input: R) -> Result<Manifest, BackupError> {
    open(input).map(|(_, manifest)| manifest)
}

/// Stores the media bytes of the file that are not here yet, checking each against its name. A
/// mismatch is an error, so the transaction it runs in is rolled back.
fn store_media<R: Read + Seek>(
    w: &mut WriteTx<'_>,
    archive: &mut ZipArchive<R>,
    failure: &mut Option<BackupError>,
) -> Result<usize, CollectionError> {
    let names: Vec<String> = archive
        .file_names()
        .filter(|name| name.starts_with(MEDIA_PREFIX))
        .map(str::to_owned)
        .collect();
    let mut added = 0;
    for name in names {
        let wanted = &name[MEDIA_PREFIX.len()..];
        let result = (|| -> Result<Vec<u8>, BackupError> {
            if wanted.len() != 64
                || !wanted
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err(BackupError::Damaged(format!(
                    "`{name}` is not a media entry"
                )));
            }
            let mut data = Vec::new();
            archive.by_name(&name)?.read_to_end(&mut data)?;
            if hex(&Sha256::digest(&data)) != wanted {
                return Err(BackupError::Damaged(format!(
                    "`{name}` does not match its hash"
                )));
            }
            Ok(data)
        })();
        match result {
            Ok(data) => {
                added += w.local().execute(
                    "INSERT OR IGNORE INTO media_blob (hash, data) VALUES (?1, ?2)",
                    params![wanted, data],
                )?;
            }
            Err(error) => {
                // The rollback carries the error out of the transaction.
                *failure = Some(error);
                return Err(CollectionError::Storage(
                    "a backup entry is damaged".to_owned(),
                ));
            }
        }
    }
    Ok(added)
}

impl Collection {
    /// Makes the collection match a whole-collection backup (ADR 0006, section 11):
    ///
    /// - every value that differs from the backup's is written again as a new change, with a new
    ///   device ID, so a sync sends it and it is not overwritten by what the server has;
    /// - notes, cards, decks and the rest that the backup did not have go to the trash, and tags
    ///   the backup's notes did not have are taken off. Nothing is deleted for good, and the trash
    ///   brings them back;
    /// - review events are added and never taken away, so reviews done since the backup are kept
    ///   and a card's schedule is folded again from all of them;
    /// - settings the backup does not have are left as they are.
    ///
    /// It is all one transaction: a damaged file leaves the collection as it was. A file that holds
    /// one deck is refused ([`BackupError::IsADeck`]), because restoring it would remove everything
    /// else. Use [`Collection::import_backup`] for that.
    pub fn restore_backup<R: Read + Seek>(&self, input: R) -> Result<RestoreReport, BackupError> {
        self.read_backup(input, FileMode::Restore)
    }

    /// Adds what a backup file has to the collection and keeps everything the collection already
    /// has. Where both have a value, the newer write wins, as in a merge, and what is taken is
    /// written as a new change so a sync sends it. It reads a deck file or a whole backup.
    ///
    /// A note in the file that has more cards in the collection's note type than the file has
    /// (a sibling card in another deck) gets them added, as for any note.
    pub fn import_backup<R: Read + Seek>(&self, input: R) -> Result<RestoreReport, BackupError> {
        self.read_backup(input, FileMode::Import)
    }

    fn read_backup<R: Read + Seek>(
        &self,
        input: R,
        mode: FileMode,
    ) -> Result<RestoreReport, BackupError> {
        let (mut archive, manifest) = open(input)?;
        if mode == FileMode::Restore && matches!(manifest.scope, ManifestScope::Deck { .. }) {
            return Err(BackupError::IsADeck);
        }
        let changes = {
            let entry = archive
                .by_name(CHANGES_ENTRY)
                .map_err(|error| match error {
                    ZipError::FileNotFound => {
                        BackupError::Damaged("changes.jsonl is missing".to_owned())
                    }
                    other => other.into(),
                })?;
            read_changes(BufReader::new(entry))?
        };
        if mode == FileMode::Restore {
            // The restore is written by a new actor (ADR 0006, section 1).
            self.regenerate_device_id()?;
        }
        let mut failure = None;
        let applied = self.apply_file(&changes, mode, |w| {
            store_media(w, &mut archive, &mut failure)
        });
        let applied = match applied {
            Ok(applied) => applied,
            Err(error) => return Err(failure.unwrap_or(error.into())),
        };
        let report = applied.report;
        Ok(RestoreReport {
            manifest,
            registers_written: report.registers_applied,
            registers_unchanged: report.registers_ignored,
            rows_added: report.rows_added,
            unknown_kept: report.unknown_registers + report.unknown_rows,
            notes_reconciled: report.notes_reconciled,
            cards_rebuilt: report.cards_rebuilt,
            removed: applied.trashed,
            media_added: applied.extra,
            rejected: report.rejected,
        })
    }
}
