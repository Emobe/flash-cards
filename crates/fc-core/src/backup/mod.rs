//! Backup, export and restore (step 1.13a). The file format is documented in
//! `docs/backup-format.md`: a zip holding `manifest.json`, `changes.jsonl` (the [`Changes`] of ADR
//! 0008, one per line) and the media bytes by hash.
//!
//! `fc-core` never touches the file system (ADR 0003), so every call takes a reader or a writer
//! that the host opened: a file on native, a byte buffer on the web.
//!
//! - [`Collection::export_backup`] writes the whole collection, or one deck with what it needs.
//! - [`Collection::restore_backup`] makes the collection match a whole-collection backup. It writes
//!   the difference as new changes (ADR 0006, section 11), so it syncs like any edit.
//! - [`Collection::import_backup`] adds what a file has to the collection and keeps everything the
//!   collection already has. It reads a deck file, and a whole backup too.

mod error;
mod export;
mod format;
mod read;
mod select;
#[cfg(test)]
mod tests;

use crate::id::Id;

pub use error::BackupError;
pub use read::{RestoreReport, read_manifest};

/// The newest format this build writes and reads. A file with a higher number is refused.
pub const FORMAT_VERSION: u32 = 1;

/// Names of the entries in the zip.
pub(crate) const MANIFEST_ENTRY: &str = "manifest.json";
pub(crate) const CHANGES_ENTRY: &str = "changes.jsonl";
pub(crate) const MEDIA_PREFIX: &str = "media/";

/// What an export holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Everything, including the trash, the settings and every media file.
    Collection,
    /// A deck and its sub-decks with their cards, notes, note types, presets and media. Deleted
    /// notes and cards are left out, and so are settings and saved searches.
    Deck(Id),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub scope: Scope,
    /// Include the review history (card events and the FSRS parameter sets they use). Without it
    /// every card reads as new when the file is read back.
    pub history: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            scope: Scope::Collection,
            history: true,
        }
    }
}

/// What a file holds, from its `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub format_version: u32,
    /// The version of `fc-core` that wrote the file.
    pub app_version: String,
    /// The storage version of the collection it came from.
    pub storage_version: u32,
    pub created_ms: i64,
    /// The device ID of the collection it came from.
    pub device: Id,
    pub scope: ManifestScope,
    pub history: bool,
    /// How many registers, append-only rows and media files `changes.jsonl` and `media/` hold.
    pub registers: u64,
    pub rows: u64,
    pub media_files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestScope {
    Collection,
    Deck { id: Id, name: String },
}

/// What [`Collection::export_backup`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    pub manifest: Manifest,
    /// The size of the media bytes written.
    pub media_bytes: u64,
    /// Media files the collection knows but has no bytes for on this device (not yet downloaded).
    /// The file lists them, and the bytes can still arrive later from sync.
    pub media_without_bytes: usize,
}
