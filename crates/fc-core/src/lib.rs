//! Core library for the app. Holds domain logic shared by the desktop and
//! mobile apps, the web client, the dev CLI and the sync server.
//!
//! It must stay free of Tauri, UI and
//! platform-specific dependencies (see `docs/adr/0001-workspace-layout.md`).

pub mod clock;
pub mod collection;
pub mod id;
pub mod scheduling;
pub mod sync;

use std::sync::Mutex;

use clock::Host;
use collection::{Collection, CollectionError};

/// Version of the core library, taken from its Cargo manifest.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Handle to the core. Holds the open collection, if any.
///
/// One connection behind a mutex: a call that holds the collection blocks the others. Reads during
/// a long write need a second connection, which a later step adds if it needs it (ADR 0003).
#[derive(Debug, Default)]
pub struct Core {
    collection: Mutex<Option<Collection>>,
}

impl Core {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the collection at a path or `file:` URI, creating it if nothing is there yet. Replaces
    /// (and closes) any collection that was open.
    pub fn open_collection(&self, location: &str, host: Host) -> Result<(), CollectionError> {
        let collection = Collection::open_or_create(location, host)?;
        *self.collection.lock().expect("collection lock") = Some(collection);
        Ok(())
    }

    /// Closes the open collection, if any.
    pub fn close_collection(&self) -> Result<(), CollectionError> {
        let taken = self.collection.lock().expect("collection lock").take();
        taken.map_or(Ok(()), Collection::close)
    }

    /// Runs `f` on the open collection, or returns `None` when none is open.
    pub fn with_collection<T>(&self, f: impl FnOnce(&Collection) -> T) -> Option<T> {
        self.collection
            .lock()
            .expect("collection lock")
            .as_ref()
            .map(f)
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
