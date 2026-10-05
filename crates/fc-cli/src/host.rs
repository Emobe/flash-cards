//! What the core needs from its host: the system clock, and an installation ID kept outside the
//! collection (ADR 0006, sections 1 and 2).

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Local, Offset};
use fc_core::clock::{Clock, Host, Reading};
use fc_core::id::Id;

#[derive(Debug)]
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Reading {
        let now = Local::now();
        Reading {
            unix_ms: now.timestamp_millis(),
            utc_offset_minutes: now.offset().fix().local_minus_utc() / 60,
        }
    }
}

/// The host for a collection file. The installation ID lives in `<file>.installation`, next to the
/// collection but not in it, so copying the `.db` alone makes a copy that gets its own device ID.
/// With `persist` false a new ID is not saved (for opening a file that may not exist).
pub fn host_for(file: &str, persist: bool) -> Result<Host, String> {
    let path = PathBuf::from(format!("{file}.installation"));
    let installation_id = match std::fs::read_to_string(&path) {
        Ok(text) => text.trim().parse().ok(),
        Err(_) => None,
    };
    let installation_id = match installation_id {
        Some(id) => id,
        None => {
            let id = Id::generate(SystemClock.now().unix_ms).map_err(|e| e.to_string())?;
            if persist {
                std::fs::write(&path, id.to_string())
                    .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
            }
            id
        }
    };
    Ok(Host {
        clock: Arc::new(SystemClock),
        installation_id,
    })
}
