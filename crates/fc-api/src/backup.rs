//! Backup, export and restore (step 1.13b), over the core of step 1.13a (`docs/backup-format.md`).
//!
//! Two families of methods do the same jobs:
//!
//! - **Bytes** (`exportBackup`, `restoreBackup`, `importBackup`, `readBackupInfo`): the backup is the
//!   request or reply attachment. Every host has them, and the web has only these.
//! - **Files** (`exportBackupToFile` and the others, native only): the host reads or writes a path,
//!   so a large collection never passes through the UI (ADR 0002, "What does not go through
//!   `call`"). Not compiled for wasm, which has no file system.
//!
//! The settings for the automatic backup are kept by the core and used by `autobackup`.

use std::io::Cursor;

use fc_core::Core;
use fc_core::backup::{
    BackupError, BackupSettings, ExportOptions, ExportSummary, Manifest, ManifestScope,
    RestoreReport, Scope,
};
use fc_core::collection::Collection;
use fc_core::id::Id;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ApiError, ErrorKind, Method, OpContext};

impl From<BackupError> for ApiError {
    fn from(error: BackupError) -> Self {
        let kind = match &error {
            BackupError::NotABackup
            | BackupError::Damaged(_)
            | BackupError::IsADeck
            | BackupError::BadSettings
            | BackupError::Exists(_) => ErrorKind::InvalidInput,
            BackupError::TooNew { .. } => ErrorKind::UpdateRequired,
            BackupError::NoSuchDeck | BackupError::NoFile(_) => ErrorKind::NotFound,
            BackupError::Io(_) => ErrorKind::Unavailable,
            BackupError::Collection(_) => ErrorKind::Internal,
        };
        if let BackupError::Collection(inner) = error {
            return Self::from(inner);
        }
        if let BackupError::Damaged(detail) | BackupError::Io(detail) = &error {
            eprintln!("Backup problem: {detail}");
        }
        Self::new(kind, error.to_string())
    }
}

/// What a backup file holds, from its manifest.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackupInfo {
    pub format_version: u32,
    /// The version of the core that wrote the file.
    pub app_version: String,
    pub storage_version: u32,
    /// Unix milliseconds.
    pub created_ms: f64,
    /// The device ID of the collection it came from.
    pub device_id: String,
    pub scope: BackupScope,
    /// Whether the review history is in the file.
    pub history: bool,
    pub registers: u32,
    pub rows: u32,
    pub media_files: u32,
}

#[derive(Debug, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BackupScope {
    /// The whole collection. Only this can be restored.
    Collection,
    /// One deck with its sub-decks. Can only be imported.
    Deck { id: String, name: String },
}

fn count(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<&Manifest> for BackupInfo {
    fn from(manifest: &Manifest) -> Self {
        Self {
            format_version: manifest.format_version,
            app_version: manifest.app_version.clone(),
            storage_version: manifest.storage_version,
            // Whole milliseconds, far below 2^53.
            created_ms: manifest.created_ms as f64,
            device_id: manifest.device.to_string(),
            scope: match &manifest.scope {
                ManifestScope::Collection => BackupScope::Collection,
                ManifestScope::Deck { id, name } => BackupScope::Deck {
                    id: id.to_string(),
                    name: name.clone(),
                },
            },
            history: manifest.history,
            registers: count(manifest.registers),
            rows: count(manifest.rows),
            media_files: count(manifest.media_files),
        }
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ExportOutput {
    pub info: BackupInfo,
    /// The size of the media bytes in the file.
    pub media_bytes: f64,
    /// Media files the collection names but has no bytes for on this device, so the file lacks them.
    pub media_without_bytes: u32,
}

impl From<ExportSummary> for ExportOutput {
    fn from(summary: ExportSummary) -> Self {
        Self {
            info: BackupInfo::from(&summary.manifest),
            media_bytes: summary.media_bytes as f64,
            media_without_bytes: count(summary.media_without_bytes as u64),
        }
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RejectedChange {
    pub entity: String,
    pub id: String,
    pub field: Option<String>,
    pub reason: String,
}

/// What a restore or an import did.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RestoreOutput {
    pub info: BackupInfo,
    pub registers_written: u32,
    pub registers_unchanged: u32,
    pub rows_added: u32,
    pub unknown_kept: u32,
    pub notes_reconciled: u32,
    pub cards_rebuilt: u32,
    /// What a restore moved to the trash because the backup did not have it. 0 for an import.
    pub removed: u32,
    pub media_added: u32,
    /// Changes that did not fit and were skipped.
    pub rejected: Vec<RejectedChange>,
}

impl From<RestoreReport> for RestoreOutput {
    fn from(report: RestoreReport) -> Self {
        let n = |n: usize| count(n as u64);
        Self {
            info: BackupInfo::from(&report.manifest),
            registers_written: n(report.registers_written),
            registers_unchanged: n(report.registers_unchanged),
            rows_added: n(report.rows_added),
            unknown_kept: n(report.unknown_kept),
            notes_reconciled: n(report.notes_reconciled),
            cards_rebuilt: n(report.cards_rebuilt),
            removed: n(report.removed),
            media_added: n(report.media_added),
            rejected: report
                .rejected
                .into_iter()
                .map(|r| RejectedChange {
                    entity: r.entity,
                    id: r.id.to_string(),
                    field: r.field,
                    reason: r.reason,
                })
                .collect(),
        }
    }
}

/// Runs `f` on the open collection.
pub(crate) fn with_open<T>(
    core: &Core,
    f: impl FnOnce(&Collection) -> Result<T, BackupError>,
) -> Result<T, ApiError> {
    core.with_collection(f)
        .ok_or_else(|| crate::collection::not_open(core))?
        .map_err(ApiError::from)
}

fn options(deck: Option<&str>, history: bool) -> Result<ExportOptions, ApiError> {
    let scope = match deck {
        None => Scope::Collection,
        Some(text) => Scope::Deck(text.parse::<Id>().map_err(|_| {
            ApiError::invalid_input(
                "The app named a deck that is not a deck ID. Choose a deck again.",
            )
        })?),
    };
    Ok(ExportOptions { scope, history })
}

fn missing_attachment(name: &str) -> ApiError {
    ApiError::invalid_input(format!(
        "\"{name}\" needs the backup file sent with it. Update the app and try again."
    ))
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ExportInput {
    /// A deck ID to export that deck and its sub-decks, or `null` for the whole collection.
    pub deck: Option<String>,
    /// Whether to include the review history.
    pub history: bool,
}

/// Writes a backup and returns it as the reply attachment.
pub struct ExportBackup;

impl Method for ExportBackup {
    const NAME: &'static str = "exportBackup";
    const ATTACHMENT_OUT: bool = true;
    type Input = ExportInput;
    type Output = ExportOutput;

    fn call(core: &Core, input: ExportInput, ctx: &OpContext) -> Result<ExportOutput, ApiError> {
        ctx.checkpoint()?;
        let options = options(input.deck.as_deref(), input.history)?;
        let mut out = Cursor::new(Vec::new());
        let summary = with_open(core, |c| c.export_backup(&mut out, &options))?;
        ctx.set_reply_attachment(out.into_inner());
        Ok(summary.into())
    }
}

/// Reads what a backup holds without changing anything. The backup is the request attachment.
pub struct ReadBackupInfo;

impl Method for ReadBackupInfo {
    const NAME: &'static str = "readBackupInfo";
    const ATTACHMENT_IN: bool = true;
    type Input = ();
    type Output = BackupInfo;

    fn call(_: &Core, (): (), ctx: &OpContext) -> Result<BackupInfo, ApiError> {
        let bytes = ctx
            .attachment()
            .ok_or_else(|| missing_attachment(Self::NAME))?;
        let manifest = fc_core::backup::read_manifest(Cursor::new(bytes))?;
        Ok(BackupInfo::from(&manifest))
    }
}

/// Makes the collection match a whole backup (the request attachment). What the backup lacks goes
/// to the trash. A file that holds one deck is refused: import it.
pub struct RestoreBackup;

impl Method for RestoreBackup {
    const NAME: &'static str = "restoreBackup";
    const ATTACHMENT_IN: bool = true;
    type Input = ();
    type Output = RestoreOutput;

    fn call(core: &Core, (): (), ctx: &OpContext) -> Result<RestoreOutput, ApiError> {
        ctx.checkpoint()?;
        let bytes = ctx
            .attachment()
            .ok_or_else(|| missing_attachment(Self::NAME))?;
        with_open(core, |c| c.restore_backup(Cursor::new(bytes))).map(Into::into)
    }
}

/// Adds what a backup or a deck file (the request attachment) has and deletes nothing.
pub struct ImportBackup;

impl Method for ImportBackup {
    const NAME: &'static str = "importBackup";
    const ATTACHMENT_IN: bool = true;
    type Input = ();
    type Output = RestoreOutput;

    fn call(core: &Core, (): (), ctx: &OpContext) -> Result<RestoreOutput, ApiError> {
        ctx.checkpoint()?;
        let bytes = ctx
            .attachment()
            .ok_or_else(|| missing_attachment(Self::NAME))?;
        with_open(core, |c| c.import_backup(Cursor::new(bytes))).map(Into::into)
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BackupSettingsOutput {
    /// A backup is made on start when the last is older than this many hours. 0 means never.
    pub interval_hours: u32,
    /// How many automatic backups are kept.
    pub keep: u32,
    /// Why the last automatic backup failed, or `null` if it worked.
    pub last_error: Option<String>,
}

impl From<BackupSettings> for BackupSettingsOutput {
    fn from(settings: BackupSettings) -> Self {
        Self {
            interval_hours: settings.interval_hours,
            keep: settings.keep,
            last_error: settings.last_error,
        }
    }
}

/// The automatic backup settings. They belong to this device and are not synced.
pub struct GetBackupSettings;

impl Method for GetBackupSettings {
    const NAME: &'static str = "getBackupSettings";
    type Input = ();
    type Output = BackupSettingsOutput;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<BackupSettingsOutput, ApiError> {
        with_open(core, |c| c.backup_settings()).map(Into::into)
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SetBackupSettingsInput {
    pub interval_hours: u32,
    pub keep: u32,
}

/// Sets how often to back up on start (0 is never) and how many backups to keep.
pub struct SetBackupSettings;

impl Method for SetBackupSettings {
    const NAME: &'static str = "setBackupSettings";
    type Input = SetBackupSettingsInput;
    type Output = BackupSettingsOutput;

    fn call(
        core: &Core,
        input: SetBackupSettingsInput,
        _: &OpContext,
    ) -> Result<BackupSettingsOutput, ApiError> {
        with_open(core, |c| {
            c.set_backup_settings(input.interval_hours, input.keep)?;
            c.backup_settings()
        })
        .map(Into::into)
    }
}

/// The methods that read and write files, for hosts that have a file system.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod files {
    use std::fs::File;
    use std::io::{BufReader, BufWriter, Write};
    use std::path::Path;

    use super::*;

    #[derive(Debug, Deserialize, TS)]
    #[serde(rename_all = "camelCase")]
    #[ts(rename_all = "camelCase")]
    pub struct ExportToFileInput {
        /// Where to write. A file that exists is never replaced.
        pub path: String,
        pub deck: Option<String>,
        pub history: bool,
    }

    #[derive(Debug, Deserialize, TS)]
    #[serde(rename_all = "camelCase")]
    #[ts(rename_all = "camelCase")]
    pub struct PathInput {
        pub path: String,
    }

    /// Writes `path` through `<path>.partial`, so a crash or a failure never leaves a half-written
    /// file with the right name. Refuses a file that exists.
    pub(crate) fn write_new<T>(
        path: &Path,
        write: impl FnOnce(&mut BufWriter<File>) -> Result<T, BackupError>,
    ) -> Result<T, BackupError> {
        let exists = || BackupError::Exists(path.display().to_string());
        if path.symlink_metadata().is_ok() {
            return Err(exists());
        }
        let mut partial = path.as_os_str().to_owned();
        partial.push(".partial");
        let partial = std::path::PathBuf::from(partial);
        let result = File::create(&partial)
            .map(BufWriter::new)
            .map_err(BackupError::from)
            .and_then(|mut out| {
                let value = write(&mut out)?;
                out.flush()?;
                out.get_ref().sync_all()?;
                Ok(value)
            })
            .and_then(|value| {
                // Another writer may have made the file meanwhile. Rename would replace it.
                if path.symlink_metadata().is_ok() {
                    return Err(exists());
                }
                std::fs::rename(&partial, path)?;
                Ok(value)
            });
        if result.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        result
    }

    pub(crate) fn open_file(path: &str) -> Result<BufReader<File>, BackupError> {
        File::open(path).map(BufReader::new).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BackupError::NoFile(path.to_owned())
            } else {
                BackupError::from(error)
            }
        })
    }

    /// Writes a backup to a new file.
    pub struct ExportBackupToFile;

    impl Method for ExportBackupToFile {
        const NAME: &'static str = "exportBackupToFile";
        type Input = ExportToFileInput;
        type Output = ExportOutput;

        fn call(
            core: &Core,
            input: ExportToFileInput,
            ctx: &OpContext,
        ) -> Result<ExportOutput, ApiError> {
            ctx.checkpoint()?;
            let options = options(input.deck.as_deref(), input.history)?;
            with_open(core, |c| {
                write_new(Path::new(&input.path), |out| c.export_backup(out, &options))
            })
            .map(Into::into)
        }
    }

    /// Reads what a backup file holds without changing anything.
    pub struct ReadBackupFileInfo;

    impl Method for ReadBackupFileInfo {
        const NAME: &'static str = "readBackupFileInfo";
        type Input = PathInput;
        type Output = BackupInfo;

        fn call(_: &Core, input: PathInput, _: &OpContext) -> Result<BackupInfo, ApiError> {
            let manifest = fc_core::backup::read_manifest(open_file(&input.path)?)?;
            Ok(BackupInfo::from(&manifest))
        }
    }

    /// Makes the collection match a whole backup file.
    pub struct RestoreBackupFromFile;

    impl Method for RestoreBackupFromFile {
        const NAME: &'static str = "restoreBackupFromFile";
        type Input = PathInput;
        type Output = RestoreOutput;

        fn call(core: &Core, input: PathInput, ctx: &OpContext) -> Result<RestoreOutput, ApiError> {
            ctx.checkpoint()?;
            let file = open_file(&input.path)?;
            with_open(core, |c| c.restore_backup(file)).map(Into::into)
        }
    }

    /// Adds what a backup file or a deck file has and deletes nothing.
    pub struct ImportBackupFromFile;

    impl Method for ImportBackupFromFile {
        const NAME: &'static str = "importBackupFromFile";
        type Input = PathInput;
        type Output = RestoreOutput;

        fn call(core: &Core, input: PathInput, ctx: &OpContext) -> Result<RestoreOutput, ApiError> {
            ctx.checkpoint()?;
            let file = open_file(&input.path)?;
            with_open(core, |c| c.import_backup(file)).map(Into::into)
        }
    }

    #[derive(Debug, Serialize, TS)]
    #[serde(rename_all = "camelCase")]
    #[ts(rename_all = "camelCase")]
    pub struct BackupEntry {
        /// The file name, which `restoreListedBackup` takes.
        pub name: String,
        /// Unix milliseconds, from the backup's manifest. `null` when the file cannot be read.
        pub created_ms: Option<f64>,
        pub size_bytes: f64,
        /// Whether the file reads as a whole-collection backup. A damaged one is listed but cannot
        /// be restored.
        pub restorable: bool,
    }

    #[derive(Debug, Serialize, TS)]
    #[serde(rename_all = "camelCase")]
    #[ts(rename_all = "camelCase")]
    pub struct ListBackupsOutput {
        pub backups: Vec<BackupEntry>,
    }

    fn folder(core: &Core) -> Result<std::path::PathBuf, ApiError> {
        core.backup_dir().ok_or_else(|| {
            ApiError::new(
                ErrorKind::Unavailable,
                "This version of the app has no backups folder.",
            )
        })
    }

    fn entry(path: &Path) -> BackupEntry {
        let size = std::fs::metadata(path).map_or(0.0, |m| m.len() as f64);
        let manifest = File::open(path)
            .ok()
            .and_then(|f| fc_core::backup::read_manifest(f).ok());
        BackupEntry {
            name: path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            created_ms: manifest.as_ref().map(|m| m.created_ms as f64),
            size_bytes: size,
            restorable: manifest.is_some_and(|m| matches!(m.scope, ManifestScope::Collection)),
        }
    }

    /// The automatic and manual backups in the app's backups folder, newest first.
    pub struct ListBackups;

    impl Method for ListBackups {
        const NAME: &'static str = "listBackups";
        type Input = ();
        type Output = ListBackupsOutput;

        fn call(core: &Core, (): (), _: &OpContext) -> Result<ListBackupsOutput, ApiError> {
            let dir = folder(core)?;
            Ok(ListBackupsOutput {
                backups: crate::autobackup::list_backups(&dir)
                    .iter()
                    .map(|p| entry(p))
                    .collect(),
            })
        }
    }

    /// Makes a backup now, whatever the interval says, and deletes the oldest beyond `keep`. A
    /// failure is saved as the settings' `last_error`.
    pub struct BackupNow;

    impl Method for BackupNow {
        const NAME: &'static str = "backupNow";
        type Input = ();
        type Output = BackupEntry;

        fn call(core: &Core, (): (), ctx: &OpContext) -> Result<BackupEntry, ApiError> {
            ctx.checkpoint()?;
            let dir = folder(core)?;
            let (now, keep) =
                with_open(core, |c| Ok((c.clock_reading(), c.backup_settings()?.keep)))?;
            match crate::autobackup::write_backup(core, &dir, now, keep as usize) {
                Ok((path, _)) => Ok(entry(&path)),
                Err(error) => {
                    let error = ApiError::from(error);
                    let message = error.message.clone();
                    let _ = with_open(core, |c| c.set_backup_error(Some(&message)));
                    Err(error)
                }
            }
        }
    }

    #[derive(Debug, Deserialize, TS)]
    #[serde(rename_all = "camelCase")]
    #[ts(rename_all = "camelCase")]
    pub struct NameInput {
        pub name: String,
    }

    /// Restores a backup from the backups folder, named as `listBackups` gives it. A backup of the
    /// collection as it is now is made first, so a restore can be undone with another restore.
    pub struct RestoreListedBackup;

    impl Method for RestoreListedBackup {
        const NAME: &'static str = "restoreListedBackup";
        type Input = NameInput;
        type Output = RestoreOutput;

        fn call(core: &Core, input: NameInput, ctx: &OpContext) -> Result<RestoreOutput, ApiError> {
            ctx.checkpoint()?;
            let dir = folder(core)?;
            // Only a name that the listing gives, never a path.
            let path = crate::autobackup::list_backups(&dir)
                .into_iter()
                .find(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy() == input.name)
                })
                .ok_or_else(|| BackupError::NoFile(input.name.clone()))?;
            let file = open_file(&path.to_string_lossy())?;
            // Check the file before touching anything.
            fc_core::backup::read_manifest(open_file(&path.to_string_lossy())?)?;
            let (now, keep) =
                with_open(core, |c| Ok((c.clock_reading(), c.backup_settings()?.keep)))?;
            // A backup made within the same second as the last one already holds this state.
            match crate::autobackup::write_backup(core, &dir, now, keep.max(2) as usize) {
                Ok(_) | Err(BackupError::Exists(_)) => {}
                Err(error) => return Err(error.into()),
            }
            with_open(core, |c| c.restore_backup(file)).map(Into::into)
        }
    }
}
