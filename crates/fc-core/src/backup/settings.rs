//! How often the host backs up on its own, and how many backups it keeps (step 1.13b). Kept in
//! `meta`, which is local: it is not synced and not in a backup, because each device decides for
//! itself. The core only stores them. The host with a folder to write to does the backing up.

use super::BackupError;
use crate::collection::Collection;
use crate::sync::state;

const INTERVAL_HOURS: &str = "backup_interval_hours";
const KEEP: &str = "backup_keep";
const LAST_ERROR: &str = "backup_last_error";

/// A backup every day.
pub const DEFAULT_INTERVAL_HOURS: u32 = 24;
/// The last five.
pub const DEFAULT_KEEP: u32 = 5;
/// A year. More is a typing mistake.
pub const MAX_INTERVAL_HOURS: u32 = 24 * 365;
pub const MAX_KEEP: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSettings {
    /// A new backup is made on start when the last is older than this. 0 turns them off.
    pub interval_hours: u32,
    /// How many automatic backups to keep (1 to [`MAX_KEEP`]).
    pub keep: u32,
    /// Why the last automatic backup failed. Cleared by the next one that works.
    pub last_error: Option<String>,
}

impl Collection {
    /// The settings, with the defaults for what was never set. A stored value that does not make
    /// sense (from a newer app) reads as the default, so reading never fails.
    pub fn backup_settings(&self) -> Result<BackupSettings, BackupError> {
        let number = |key: &str, default: u32, valid: std::ops::RangeInclusive<u32>| {
            Ok::<_, BackupError>(
                state::get(&self.conn, key)?
                    .and_then(|text| text.parse::<u32>().ok())
                    .filter(|n| valid.contains(n))
                    .unwrap_or(default),
            )
        };
        Ok(BackupSettings {
            interval_hours: number(
                INTERVAL_HOURS,
                DEFAULT_INTERVAL_HOURS,
                0..=MAX_INTERVAL_HOURS,
            )?,
            keep: number(KEEP, DEFAULT_KEEP, 1..=MAX_KEEP)?,
            last_error: state::get(&self.conn, LAST_ERROR)?.filter(|text| !text.is_empty()),
        })
    }

    /// Sets how often to back up (0 is never) and how many to keep.
    pub fn set_backup_settings(&self, interval_hours: u32, keep: u32) -> Result<(), BackupError> {
        if interval_hours > MAX_INTERVAL_HOURS || !(1..=MAX_KEEP).contains(&keep) {
            return Err(BackupError::BadSettings);
        }
        state::set(&self.conn, INTERVAL_HOURS, &interval_hours.to_string())?;
        state::set(&self.conn, KEEP, &keep.to_string())?;
        Ok(())
    }

    /// Records why the last automatic backup failed, or clears it with `None`.
    pub fn set_backup_error(&self, error: Option<&str>) -> Result<(), BackupError> {
        state::set(&self.conn, LAST_ERROR, error.unwrap_or(""))?;
        Ok(())
    }
}
