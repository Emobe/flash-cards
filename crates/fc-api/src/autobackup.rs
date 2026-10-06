//! Automatic backups and the copy made before a migration (step 1.13b). Native only: the core
//! never touches the file system (ADR 0003), and the web has no folder to write to.
//!
//! The host calls [`copy_before_migration`] before it opens the collection and [`run_if_due`] after.
//! Files go in one folder and are named so that this module touches only its own:
//!
//! - `backup-YYYY-MM-DD-HHMMSS.fcbackup`: a whole-collection backup with history, local time. The
//!   newest `keep` are kept (a setting).
//! - `before-update-v<from>-to-v<to>-YYYY-MM-DD-HHMMSS.db`: a raw copy of the collection file, made
//!   when opening it would migrate it. The newest [`KEEP_BEFORE_UPDATE`] are kept. It is not a
//!   `.fcbackup`: the export reads the newest schema, and it cannot run on a file that has not been
//!   migrated. The app opens the copy and migrates it again.

use std::fs::File;
use std::path::{Path, PathBuf};

use fc_core::Core;
use fc_core::backup::{ExportOptions, read_manifest};
use fc_core::clock::Reading;
use fc_core::collection::Collection;

use crate::ApiError;
use crate::backup::{files::write_new, with_open};

const BACKUP_PREFIX: &str = "backup-";
const BACKUP_EXT: &str = ".fcbackup";
const UPDATE_PREFIX: &str = "before-update-";
const UPDATE_EXT: &str = ".db";
/// How many copies made before a migration are kept.
pub const KEEP_BEFORE_UPDATE: usize = 2;

/// What [`run_if_due`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The interval is 0.
    Off,
    /// The newest backup is younger than the interval.
    NotDue,
    /// A backup was written. `removed` older ones were deleted to keep the number set.
    Written { path: PathBuf, removed: usize },
}

/// Writes a backup of the open collection into `dir` if the newest one is older than the interval
/// in the settings (or there is none), then deletes the oldest beyond the number to keep. A failure
/// is returned and also saved as the settings' `last_error`, which the next success clears. Older
/// backups are never removed after a failure.
pub fn run_if_due(core: &Core, dir: &Path, now: Reading) -> Result<Outcome, ApiError> {
    let settings = with_open(core, |c| c.backup_settings())?;
    if settings.interval_hours == 0 {
        return Ok(Outcome::Off);
    }
    let interval_ms = i64::from(settings.interval_hours) * 3_600_000;
    if let Some(newest) = list(dir, BACKUP_PREFIX, BACKUP_EXT).pop()
        && let Ok(manifest) = File::open(&newest)
            .map_err(|_| ())
            .and_then(|f| read_manifest(f).map_err(|_| ()))
    {
        // A backup dated in the future (the clock was set back) does not count.
        let age = now.unix_ms - manifest.created_ms;
        if (0..interval_ms).contains(&age) {
            return Ok(Outcome::NotDue);
        }
    }
    let path = dir.join(format!("{BACKUP_PREFIX}{}{BACKUP_EXT}", stamp(now)));
    let written = std::fs::create_dir_all(dir)
        .map_err(|error| fc_core::backup::BackupError::Io(error.to_string()))
        .and_then(|()| {
            with_open_raw(core, |c| {
                write_new(&path, |out| c.export_backup(out, &ExportOptions::default()))
            })
        });
    match written {
        Ok(_) => {
            let _ = with_open(core, |c| c.set_backup_error(None));
            let removed = prune(dir, BACKUP_PREFIX, BACKUP_EXT, settings.keep as usize);
            Ok(Outcome::Written { path, removed })
        }
        Err(error) => {
            let error = ApiError::from(error);
            let message = error.message.clone();
            let _ = with_open(core, |c| c.set_backup_error(Some(&message)));
            Err(error)
        }
    }
}

fn with_open_raw<T>(
    core: &Core,
    f: impl FnOnce(&Collection) -> Result<T, fc_core::backup::BackupError>,
) -> Result<T, fc_core::backup::BackupError> {
    core.with_collection(f)
        .unwrap_or(Err(fc_core::backup::BackupError::Collection(
            fc_core::collection::CollectionError::NotFound,
        )))
}

/// If opening the collection file at `location` would migrate it, copies the file into `dir` first
/// and returns the copy. `None` when there is nothing to migrate (no file, up to date, not ours or
/// too new). Call it before the collection is opened. The error is a sentence for the user.
pub fn copy_before_migration(
    location: &Path,
    dir: &Path,
    now: Reading,
) -> Result<Option<PathBuf>, ApiError> {
    let text = location.to_str().ok_or_else(ApiError::internal)?;
    let Some(pending) = Collection::pending_migration(text)? else {
        return Ok(None);
    };
    let name = format!(
        "{UPDATE_PREFIX}v{}-to-v{}-{}{UPDATE_EXT}",
        pending.from,
        pending.to,
        stamp(now)
    );
    let copy = dir.join(name);
    std::fs::create_dir_all(dir)
        .map_err(|error| ApiError::from(fc_core::backup::BackupError::from(error)))?;
    write_new(&copy, |out| {
        std::io::copy(&mut File::open(location)?, out)?;
        Ok(())
    })?;
    prune(dir, UPDATE_PREFIX, UPDATE_EXT, KEEP_BEFORE_UPDATE);
    Ok(Some(copy))
}

/// The files in `dir` named `<prefix>...<stamp><ext>`, oldest first by stamp. Anything else in the
/// folder is not ours and is left out.
fn list(dir: &Path, prefix: &str, ext: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let middle = name.strip_prefix(prefix)?.strip_suffix(ext)?;
            let at = middle.len().checked_sub(STAMP_LEN)?;
            let stamp = middle.get(at..)?;
            // What comes before the stamp must be empty or end in a dash, like `v3-to-v4-`.
            let before = &middle[..at];
            (is_stamp(stamp) && (before.is_empty() || before.ends_with('-')))
                .then(|| (stamp.to_owned(), entry.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

/// Deletes all but the newest `keep` files of the family. Returns how many it deleted. A file that
/// cannot be deleted is skipped: it only means one backup too many.
fn prune(dir: &Path, prefix: &str, ext: &str, keep: usize) -> usize {
    let files = list(dir, prefix, ext);
    let surplus = files.len().saturating_sub(keep);
    files
        .into_iter()
        .take(surplus)
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count()
}

const STAMP_LEN: usize = 17;

fn is_stamp(text: &str) -> bool {
    text.len() == STAMP_LEN
        && text.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 | 10 => b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// `YYYY-MM-DD-HHMMSS` in local time, so the names sort by time.
fn stamp(now: Reading) -> String {
    let local_secs = (now.unix_ms + i64::from(now.utc_offset_minutes) * 60_000).div_euclid(1000);
    let (days, secs) = (local_secs.div_euclid(86_400), local_secs.rem_euclid(86_400));
    // Days to a calendar date (proleptic Gregorian, the well-known civil-from-days algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}-{:02}{:02}{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use fc_core::clock::{Host, ManualClock};
    use fc_core::id::Id;

    use super::*;
    use crate::ErrorKind;

    const HOUR: i64 = 3_600_000;
    /// 2026-10-07 09:00:00 UTC.
    const T0: i64 = 1_791_363_600_000;

    fn at(unix_ms: i64) -> Reading {
        Reading {
            unix_ms,
            utc_offset_minutes: 0,
        }
    }

    /// A folder in the temp directory, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("fc-autobackup-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn core_at(location: &str) -> Core {
        let core = Core::new();
        let host = Host {
            clock: Arc::new(ManualClock::new(T0)),
            installation_id: Id::from_bytes([7; 16]),
        };
        core.open_collection(location, host).unwrap();
        core
    }

    #[test]
    fn stamps_are_local_time_and_sort_by_time() {
        assert_eq!(stamp(at(0)), "1970-01-01-000000");
        assert_eq!(stamp(at(T0)), "2026-10-07-090000");
        // Leap day, and the last second before a year.
        assert_eq!(stamp(at(1_709_208_000_000 + 59_000)), "2024-02-29-120059");
        assert_eq!(stamp(at(1_798_761_599_000)), "2026-12-31-235959");
        // Local time: an hour ahead of UTC, and before 1970.
        let ahead = Reading {
            unix_ms: T0,
            utc_offset_minutes: 60,
        };
        assert_eq!(stamp(ahead), "2026-10-07-100000");
        assert_eq!(stamp(at(-1000)), "1969-12-31-235959");
        assert!(is_stamp(&stamp(at(T0))));
        assert!(!is_stamp("2026-10-07-09000"));
        assert!(!is_stamp("2026x10-07-090000"));
    }

    #[test]
    fn the_first_start_writes_a_backup_and_a_second_start_soon_after_does_not() {
        let dir = TempDir::new();
        let core = core_at(":memory:");
        let Outcome::Written { path, removed } = run_if_due(&core, &dir.0, at(T0)).unwrap() else {
            panic!("expected a backup");
        };
        assert_eq!(removed, 0);
        assert_eq!(dir.names(), ["backup-2026-10-07-090000.fcbackup"]);
        // It is a real backup of this collection.
        let manifest = read_manifest(File::open(path).unwrap()).unwrap();
        assert_eq!(manifest.created_ms, T0);
        assert!(manifest.history);

        assert_eq!(
            run_if_due(&core, &dir.0, at(T0 + 23 * HOUR)).unwrap(),
            Outcome::NotDue
        );
        assert!(matches!(
            run_if_due(&core, &dir.0, at(T0 + 24 * HOUR)).unwrap(),
            Outcome::Written { .. }
        ));
        assert_eq!(dir.names().len(), 2);
    }

    #[test]
    fn the_interval_is_a_setting_and_zero_turns_backups_off() {
        let dir = TempDir::new();
        let core = core_at(":memory:");
        with_open(&core, |c| c.set_backup_settings(0, 5)).unwrap();
        assert_eq!(run_if_due(&core, &dir.0, at(T0)).unwrap(), Outcome::Off);
        assert!(dir.names().is_empty());

        with_open(&core, |c| c.set_backup_settings(1, 5)).unwrap();
        run_if_due(&core, &dir.0, at(T0)).unwrap();
        assert_eq!(
            run_if_due(&core, &dir.0, at(T0 + HOUR - 1)).unwrap(),
            Outcome::NotDue
        );
        assert!(matches!(
            run_if_due(&core, &dir.0, at(T0 + HOUR)).unwrap(),
            Outcome::Written { .. }
        ));
    }

    #[test]
    fn only_the_newest_few_are_kept_and_foreign_files_are_left_alone() {
        let dir = TempDir::new();
        let core = core_at(":memory:");
        with_open(&core, |c| c.set_backup_settings(24, 2)).unwrap();
        for name in [
            "notes.txt",
            "backup-latest.fcbackup",
            "backup-2026-10-07-080000.fcbackup.partial",
            "before-update-v3-to-v4-2020-01-01-000000.db",
        ] {
            std::fs::write(dir.0.join(name), b"mine").unwrap();
        }
        let mut removed = 0;
        for day in 0..4 {
            if let Outcome::Written { removed: n, .. } =
                run_if_due(&core, &dir.0, at(T0 + day * 24 * HOUR)).unwrap()
            {
                removed += n;
            }
        }
        assert_eq!(removed, 2);
        assert_eq!(
            dir.names(),
            [
                "backup-2026-10-07-080000.fcbackup.partial",
                "backup-2026-10-09-090000.fcbackup",
                "backup-2026-10-10-090000.fcbackup",
                "backup-latest.fcbackup",
                "before-update-v3-to-v4-2020-01-01-000000.db",
                "notes.txt",
            ]
        );
    }

    #[test]
    fn a_damaged_or_future_newest_backup_does_not_stop_the_next_one() {
        let dir = TempDir::new();
        let core = core_at(":memory:");
        std::fs::write(dir.0.join("backup-2026-10-07-080000.fcbackup"), b"junk").unwrap();
        assert!(matches!(
            run_if_due(&core, &dir.0, at(T0)).unwrap(),
            Outcome::Written { .. }
        ));
        // The clock was set back: the newest backup is "from the future".
        assert!(matches!(
            run_if_due(&core, &dir.0, at(T0 - 5 * HOUR)).unwrap(),
            Outcome::Written { .. }
        ));
    }

    #[test]
    fn a_failure_is_recorded_keeps_older_backups_and_the_next_success_clears_it() {
        let dir = TempDir::new();
        let core = core_at(":memory:");
        with_open(&core, |c| c.set_backup_settings(24, 1)).unwrap();
        run_if_due(&core, &dir.0, at(T0)).unwrap();

        // The folder is now a file, so nothing can be written in it.
        let blocked = dir.0.join("blocked");
        std::fs::write(&blocked, b"x").unwrap();
        let error = run_if_due(&core, &blocked, at(T0 + 48 * HOUR)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unavailable);
        let settings = with_open(&core, |c| c.backup_settings()).unwrap();
        assert_eq!(settings.last_error, Some(error.message));
        assert!(
            dir.names()
                .contains(&"backup-2026-10-07-090000.fcbackup".to_owned())
        );

        run_if_due(&core, &dir.0, at(T0 + 48 * HOUR)).unwrap();
        assert_eq!(
            with_open(&core, |c| c.backup_settings())
                .unwrap()
                .last_error,
            None
        );
    }

    #[test]
    fn without_a_collection_nothing_is_written() {
        let dir = TempDir::new();
        let error = run_if_due(&Core::new(), &dir.0, at(T0)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert!(dir.names().is_empty());
    }

    /// A collection file at version 1, which the newest build would migrate.
    fn old_collection(dir: &TempDir) -> PathBuf {
        let path = dir.0.join("collection.db");
        let core = core_at(path.to_str().unwrap());
        core.close_collection().unwrap();
        // The schema version is the 4-byte number at offset 60 of the SQLite header.
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[60..64].copy_from_slice(&1u32.to_be_bytes());
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_collection_that_would_be_migrated_is_copied_first_and_a_current_one_is_not() {
        let data = TempDir::new();
        let backups = TempDir::new();
        let path = old_collection(&data);
        let newest = Collection::pending_migration(path.to_str().unwrap())
            .unwrap()
            .unwrap()
            .to;

        let copy = copy_before_migration(&path, &backups.0, at(T0))
            .unwrap()
            .unwrap();
        assert_eq!(
            copy.file_name().unwrap().to_str().unwrap(),
            format!("before-update-v1-to-v{newest}-2026-10-07-090000.db")
        );
        assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&path).unwrap());

        // After the migration there is nothing more to copy, and a missing file is not an error.
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[60..64].copy_from_slice(&newest.to_be_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(
            copy_before_migration(&path, &backups.0, at(T0 + HOUR)).unwrap(),
            None
        );
        let missing = data.0.join("none.db");
        assert_eq!(
            copy_before_migration(&missing, &backups.0, at(T0)).unwrap(),
            None
        );
        assert_eq!(backups.names().len(), 1);
    }

    #[test]
    fn the_copy_leaves_only_the_newest_two_and_automatic_backups_are_not_counted() {
        let data = TempDir::new();
        let backups = TempDir::new();
        let path = old_collection(&data);
        std::fs::write(backups.0.join("backup-2020-01-01-000000.fcbackup"), b"b").unwrap();
        for day in 0..3 {
            copy_before_migration(&path, &backups.0, at(T0 + day * 24 * HOUR)).unwrap();
        }
        let names = backups.names();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names[0].starts_with("backup-2020"));
        assert!(names[1].ends_with("2026-10-08-090000.db"));
        assert!(names[2].ends_with("2026-10-09-090000.db"));
    }
}
