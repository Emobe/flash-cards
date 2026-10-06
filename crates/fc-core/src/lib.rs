//! Core library for the app. Holds domain logic shared by the desktop and
//! mobile apps, the web client, the dev CLI and the sync server.
//!
//! It must stay free of Tauri, UI and
//! platform-specific dependencies (see `docs/adr/0001-workspace-layout.md`).

pub mod backup;
pub mod clock;
pub mod collection;
pub mod deck;
pub mod events;
mod html;
pub mod id;
pub mod media;
pub mod note;
pub mod notetype;
pub mod scheduling;
pub mod search;
pub mod stats;
pub mod study;
pub mod sync;
pub mod tag;
pub mod template;

use std::sync::{Arc, Mutex};

use clock::Host;
use collection::{Collection, CollectionError};
use events::{Listener, Listeners};

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
    /// Shared with every collection the core opens.
    listeners: Listeners,
}

impl Core {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a listener for the events of every collection this core opens, before or after this
    /// call (see `events`).
    pub fn listen(&self, listener: Arc<dyn Listener>) {
        self.listeners.add(listener);
    }

    /// Opens the collection at a path or `file:` URI, creating it if nothing is there yet. Replaces
    /// (and closes) any collection that was open.
    pub fn open_collection(&self, location: &str, host: Host) -> Result<(), CollectionError> {
        let mut collection = Collection::open_or_create(location, host)?;
        collection.set_listeners(self.listeners.clone());
        let mut open = self.collection.lock().expect("collection lock");
        if let Some(old) = open.as_ref() {
            old.end_open_session(study::EndReason::Closed);
        }
        *open = Some(collection);
        Ok(())
    }

    /// Closes the open collection, if any.
    pub fn close_collection(&self) -> Result<(), CollectionError> {
        // Closed under the lock, so the session's end event arrives in order with the others.
        let mut open = self.collection.lock().expect("collection lock");
        open.take().map_or(Ok(()), Collection::close)
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
