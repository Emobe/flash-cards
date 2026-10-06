//! Writing a backup.

use std::collections::BTreeSet;
use std::io::{BufWriter, Seek, Write};

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::format::{manifest_to_json, write_changes};
use super::{
    BackupError, CHANGES_ENTRY, ExportOptions, ExportSummary, FORMAT_VERSION, MANIFEST_ENTRY,
    MEDIA_PREFIX, Manifest, ManifestScope, Scope,
};
use crate::collection::Collection;
use crate::sync::Selection;

impl Collection {
    /// Writes a backup to `out`: the whole collection, or one deck (see [`Scope`]). The collection
    /// is read in one snapshot, so the file is consistent. It holds the collection's changes in
    /// memory while it writes, and media one file at a time.
    pub fn export_backup<W: Write + Seek>(
        &self,
        out: W,
        options: &ExportOptions,
    ) -> Result<ExportSummary, BackupError> {
        let _snapshot = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let mut changes = self.changes(Selection::All)?;

        let (scope, hashes): (ManifestScope, Vec<String>) = match options.scope {
            Scope::Collection => {
                if !options.history {
                    changes.rows.retain(|r| {
                        !matches!(r.entity.as_str(), "card_event" | "fsrs_parameter_set")
                    });
                }
                let mut statement = self
                    .conn
                    .prepare("SELECT hash FROM media_blob ORDER BY hash")?;
                let hashes = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                (ManifestScope::Collection, hashes)
            }
            Scope::Deck(deck) => {
                let slice = self.select_deck(deck, changes, options.history)?;
                changes = slice.changes;
                let scope = ManifestScope::Deck {
                    id: deck,
                    name: slice.deck_name,
                };
                (scope, slice.media.into_iter().collect())
            }
        };

        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let mut zip = ZipWriter::new(out);

        zip.start_file(CHANGES_ENTRY, deflated.large_file(true))?;
        {
            let mut lines = BufWriter::new(&mut zip);
            write_changes(&mut lines, &changes)?;
            lines.flush()?;
        }

        let (mut written, mut media_bytes) = (0u64, 0u64);
        let mut without_bytes: BTreeSet<&str> = BTreeSet::new();
        let mut blob = self
            .conn
            .prepare("SELECT data FROM media_blob WHERE hash = ?1")?;
        for hash in &hashes {
            let Some(data) = blob
                .query_row([hash], |row| row.get::<_, Vec<u8>>(0))
                .optional()?
            else {
                without_bytes.insert(hash);
                continue;
            };
            zip.start_file(format!("{MEDIA_PREFIX}{hash}"), stored.large_file(true))?;
            zip.write_all(&data)?;
            written += 1;
            media_bytes += data.len() as u64;
        }
        // Files the collection names but holds no bytes for (a collection scope lists them by
        // `media_file`, since `media_blob` has no row to list).
        let mut missing = without_bytes.len();
        if options.scope == Scope::Collection {
            missing = self.media_files()?.iter().filter(|f| !f.has_bytes).count();
        }

        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            app_version: crate::version().to_owned(),
            storage_version: self.info()?.schema_version,
            created_ms: self.host.clock.now().unix_ms,
            device: self.device_id()?,
            scope,
            history: options.history,
            registers: changes.registers.len() as u64,
            rows: changes.rows.len() as u64,
            media_files: written,
        };
        zip.start_file(MANIFEST_ENTRY, deflated)?;
        serde_json::to_writer_pretty(&mut zip, &manifest_to_json(&manifest))
            .map_err(|e| BackupError::Io(e.to_string()))?;
        zip.write_all(b"\n")?;
        zip.finish()?;
        Ok(ExportSummary {
            manifest,
            media_bytes,
            media_without_bytes: missing,
        })
    }
}
