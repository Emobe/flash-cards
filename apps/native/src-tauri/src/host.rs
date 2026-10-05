//! What the core needs from this host: the system clock, and an installation ID kept outside the
//! collection (ADR 0006, sections 1 and 2).

use std::path::Path;
use std::sync::Arc;

use chrono::{Local, Offset};
use fc_api::{Clock, Host, Id, Reading};

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

/// The host for the app. The installation ID is a file in `config_dir`, away from the collection in
/// the data directory, so a copied collection is recognised as a copy and gets its own device ID.
pub fn host(config_dir: &Path) -> Result<Host, String> {
    let path = config_dir.join("installation-id");
    let saved = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<Id>().ok());
    let installation_id = match saved {
        Some(id) => id,
        None => {
            let id = Id::generate(SystemClock.now().unix_ms).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, id.to_string()).map_err(|e| e.to_string())?;
            id
        }
    };
    Ok(Host {
        clock: Arc::new(SystemClock),
        installation_id,
    })
}
