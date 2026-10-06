//! Collection-wide settings (ADR 0007, part 8). One synced row per setting, a missing row means the
//! default, and resetting means writing the default. The first setting is the hour the study day
//! starts.

use rusqlite::OptionalExtension;
use rusqlite::types::Value;

use super::{SETTING, SETTING_NAMESPACE, StudyError};
use crate::collection::Collection;
use crate::id::Id;

pub(super) const DAY_START_HOUR: &str = "day_start_hour";
pub const DEFAULT_DAY_START_HOUR: u8 = 0;

fn setting_id(key: &str) -> Id {
    Id::new_v5(SETTING_NAMESPACE, key.as_bytes())
}

impl Collection {
    /// The hour (0 to 23, local time) the study day starts. Midnight unless changed. A stored value
    /// that is not an hour (from a newer app) reads as midnight, so reading never fails.
    pub fn day_start_hour(&self) -> Result<u8, StudyError> {
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM collection_setting WHERE id = ?1",
                [setting_id(DAY_START_HOUR)],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored
            .and_then(|text| text.parse::<u8>().ok())
            .filter(|hour| *hour <= 23)
            .unwrap_or(DEFAULT_DAY_START_HOUR))
    }

    /// Sets the hour the study day starts, from then on. Cards keep the due days they have.
    pub fn set_day_start_hour(&self, hour: u8) -> Result<(), StudyError> {
        if hour > 23 {
            return Err(StudyError::StartHour);
        }
        if self.day_start_hour()? == hour {
            return Ok(());
        }
        let id = setting_id(DAY_START_HOUR);
        let value = Value::Text(hour.to_string());
        Ok(self.write(|w| {
            if w.get(SETTING.entity, id, "value")?.is_some() {
                w.set(SETTING.entity, id, "value", value)
            } else {
                w.insert(
                    SETTING.entity,
                    id,
                    vec![
                        ("key", Value::Text(DAY_START_HOUR.to_owned())),
                        ("value", value),
                    ],
                )
            }
        })?)
    }
}
