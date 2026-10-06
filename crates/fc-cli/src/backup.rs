//! `fc export`, `fc restore`, `fc import` and `fc backup-info`: the backup file of step 1.13a
//! (`docs/backup-format.md`).

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};

use fc_core::backup::{
    ExportOptions, Manifest, ManifestScope, RestoreReport, Scope, read_manifest,
};

use super::{Failure, find_deck, open, plural};

impl From<fc_core::backup::BackupError> for Failure {
    fn from(error: fc_core::backup::BackupError) -> Self {
        Self::Core(error.to_string())
    }
}

fn read_file(path: &str) -> Result<BufReader<File>, Failure> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|e| Failure::Core(format!("Could not open {path}: {e}")))
}

fn describe(manifest: &Manifest) -> String {
    let scope = match &manifest.scope {
        ManifestScope::Collection => "the whole collection".to_owned(),
        ManifestScope::Deck { name, .. } => format!("the deck \"{name}\""),
    };
    format!(
        "Holds: {scope}\n\
         Review history: {}\n\
         Format version: {}\n\
         Made by: fc-core {} (storage version {}) on device {}\n\
         Made at (Unix ms): {}\n\
         Registers: {}\n\
         Rows (reviews and parameter sets): {}\n\
         Media files: {}",
        if manifest.history {
            "included"
        } else {
            "not included"
        },
        manifest.format_version,
        manifest.app_version,
        manifest.storage_version,
        manifest.device,
        manifest.created_ms,
        manifest.registers,
        manifest.rows,
        manifest.media_files
    )
}

/// Writes a backup of the collection, or of one deck. Never overwrites a file.
pub(super) fn export(
    file: &str,
    backup: &str,
    deck: Option<&str>,
    history: bool,
) -> Result<String, Failure> {
    let collection = open(file)?;
    let scope = match deck {
        Some(path) => Scope::Deck(find_deck(&collection, path)?.id),
        None => Scope::Collection,
    };
    let out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup)
        .map_err(|e| {
            Failure::Core(if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!("{backup} exists already. Choose a file name that is not used.")
            } else {
                format!("Could not create {backup}: {e}")
            })
        })?;
    let mut out = BufWriter::new(out);
    let result = collection.export_backup(&mut out, &ExportOptions { scope, history });
    let result = result.and_then(|summary| {
        out.flush()?;
        Ok(summary)
    });
    let summary = match result {
        Ok(summary) => summary,
        Err(error) => {
            drop(out);
            let _ = std::fs::remove_file(backup);
            return Err(error.into());
        }
    };
    collection.close()?;
    let mut text = format!("Wrote {backup}\n{}", describe(&summary.manifest));
    text.push_str(&format!("\nMedia bytes: {}", summary.media_bytes));
    if summary.media_without_bytes > 0 {
        text.push_str(&format!(
            "\n{} named in the collection has no bytes on this device, so the file does not hold it.",
            plural(summary.media_without_bytes, "media file")
        ));
    }
    Ok(text)
}

pub(super) fn info(backup: &str) -> Result<String, Failure> {
    let manifest = read_manifest(read_file(backup)?)?;
    Ok(format!("{backup}\n{}", describe(&manifest)))
}

fn report(what: &str, file: &str, backup: &str, report: &RestoreReport) -> String {
    let mut text = format!(
        "{what} {backup} into {file}\n\
         Registers written: {}\n\
         Registers already the same: {}\n\
         Reviews and parameter sets added: {}\n\
         Unknown registers and rows kept: {}\n\
         Notes reconciled: {}\n\
         Cards rebuilt: {}\n\
         Removed (to the trash, or taken off a note): {}\n\
         Media files added: {}",
        report.registers_written,
        report.registers_unchanged,
        report.rows_added,
        report.unknown_kept,
        report.notes_reconciled,
        report.cards_rebuilt,
        report.removed,
        report.media_added
    );
    if !report.rejected.is_empty() {
        text.push_str(&format!(
            "\n{} could not be applied:",
            plural(report.rejected.len(), "change")
        ));
        for rejected in &report.rejected {
            text.push_str(&format!(
                "\n  {} {}{}: {}",
                rejected.entity,
                rejected.id,
                rejected
                    .field
                    .as_ref()
                    .map_or(String::new(), |f| format!(" {f}")),
                rejected.reason
            ));
        }
    }
    text
}

/// Makes the collection match a whole backup.
pub(super) fn restore(file: &str, backup: &str) -> Result<String, Failure> {
    let input = read_file(backup)?;
    let collection = open(file)?;
    let result = collection.restore_backup(input)?;
    collection.close()?;
    Ok(report("Restored", file, backup, &result))
}

/// Adds what a backup file holds, and keeps what the collection has.
pub(super) fn import(file: &str, backup: &str) -> Result<String, Failure> {
    let input = read_file(backup)?;
    let collection = open(file)?;
    let result = collection.import_backup(input)?;
    collection.close()?;
    Ok(report("Imported", file, backup, &result))
}
