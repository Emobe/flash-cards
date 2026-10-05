//! Core library for the app. Holds domain logic shared by the desktop and
//! mobile apps, the web client, the dev CLI and the sync server.
//!
//! Placeholder until Phase 1. It must stay free of Tauri, UI and
//! platform-specific dependencies (see `docs/adr/0001-workspace-layout.md`).

pub mod scheduling;
pub mod spike;

use std::sync::Mutex;

use spike::{SpikeError, SpikeStore};

/// Version of the core library, taken from its Cargo manifest.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Handle to the core. Holds only the temporary spike store until Phase 1 adds a collection.
#[derive(Debug, Default)]
pub struct Core {
    spike: Mutex<Option<SpikeStore>>,
}

impl Core {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the spike store at a path or `file:` URI (step 0.4, deleted in 1.1).
    pub fn open_spike(&self, uri: &str) -> Result<(), SpikeError> {
        let store = SpikeStore::open(uri)?;
        *self.spike.lock().expect("spike lock") = Some(store);
        Ok(())
    }

    /// Runs `f` on the spike store, or returns `None` when none is open.
    pub fn with_spike<T>(
        &self,
        f: impl FnOnce(&SpikeStore) -> Result<T, SpikeError>,
    ) -> Option<Result<T, SpikeError>> {
        self.spike.lock().expect("spike lock").as_ref().map(f)
    }
}

/// Dividing by zero was requested.
#[derive(Debug, PartialEq, Eq)]
pub struct DivideByZero;

/// Example calculation for the bridge (step 0.3). Deleted when real functions arrive.
pub fn example_divide(dividend: f64, divisor: f64) -> Result<f64, DivideByZero> {
    if divisor == 0.0 {
        Err(DivideByZero)
    } else {
        Ok(dividend / divisor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_manifest() {
        assert_eq!(version(), "0.0.0");
    }

    #[test]
    fn divides() {
        assert_eq!(example_divide(7.0, 2.0), Ok(3.5));
    }

    #[test]
    fn division_by_zero_is_an_error() {
        assert_eq!(example_divide(1.0, 0.0), Err(DivideByZero));
    }
}
