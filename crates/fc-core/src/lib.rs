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
pub mod undo;

use std::sync::{Arc, Mutex};

use clock::Host;
use collection::{Collection, CollectionError};
use events::{Listener, Listeners};
use id::Id;

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
    /// Why the last `open_collection` failed, until a later one works or the collection is closed.
    open_error: Mutex<Option<CollectionError>>,
    /// Shared with every collection the core opens.
    listeners: Listeners,
    /// Where a native host keeps backups, for the methods that list and write them.
    backup_dir: Mutex<Option<std::path::PathBuf>>,
    /// The last change that can be undone (`undo`). Dropped when a collection opens or closes.
    last_change: Mutex<Option<undo::KeptChange>>,
}

impl Core {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tells the core the folder for automatic backups. Only native hosts set it.
    pub fn set_backup_dir(&self, dir: std::path::PathBuf) {
        *self.backup_dir.lock().expect("backup dir lock") = Some(dir);
    }

    /// The folder set by [`Core::set_backup_dir`], if any.
    pub fn backup_dir(&self) -> Option<std::path::PathBuf> {
        self.backup_dir.lock().expect("backup dir lock").clone()
    }

    /// Adds a listener for the events of every collection this core opens, before or after this
    /// call (see `events`).
    pub fn listen(&self, listener: Arc<dyn Listener>) {
        self.listeners.add(listener);
    }

    /// Opens the collection at a path or `file:` URI, creating it if nothing is there yet. Replaces
    /// (and closes) any collection that was open.
    pub fn open_collection(&self, location: &str, host: Host) -> Result<(), CollectionError> {
        let mut collection = match Collection::open_or_create(location, host) {
            Ok(collection) => collection,
            Err(error) => {
                *self.open_error.lock().expect("open error lock") = Some(error.clone());
                return Err(error);
            }
        };
        collection.set_listeners(self.listeners.clone());
        let mut open = self.collection.lock().expect("collection lock");
        if let Some(old) = open.as_ref() {
            old.end_open_session(study::EndReason::Closed);
        }
        *open = Some(collection);
        *self.open_error.lock().expect("open error lock") = None;
        *self.last_change.lock().expect("last change lock") = None;
        Ok(())
    }

    /// Why the last attempt to open a collection failed, so a host can tell the user instead of
    /// answering "no collection is open". `None` once a collection opens or is closed on purpose.
    pub fn open_error(&self) -> Option<CollectionError> {
        self.open_error.lock().expect("open error lock").clone()
    }

    /// Closes the open collection, if any.
    pub fn close_collection(&self) -> Result<(), CollectionError> {
        // Closed under the lock, so the session's end event arrives in order with the others.
        let mut open = self.collection.lock().expect("collection lock");
        *self.open_error.lock().expect("open error lock") = None;
        *self.last_change.lock().expect("last change lock") = None;
        open.take().map_or(Ok(()), Collection::close)
    }

    /// Keeps a change so it can be undone, in place of the one kept before.
    pub fn keep_change(&self, change: undo::KeptChange) {
        *self.last_change.lock().expect("last change lock") = Some(change);
    }

    /// Takes the kept change if it is the one with this ID, so it can be undone once. `None` if
    /// another change came after it, or a collection was opened or closed since.
    pub fn take_change(&self, id: Id) -> Option<undo::KeptChange> {
        let mut kept = self.last_change.lock().expect("last change lock");
        if kept.as_ref().is_some_and(|change| change.id == id) {
            kept.take()
        } else {
            None
        }
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

/// Card HTML as plain text for a list or a summary (tags dropped, entities decoded, sounds and
/// pictures named). Never put the result in a page as HTML.
pub fn readable_text(html: &str) -> String {
    html::readable(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_change_is_taken_once_by_its_id_and_dropped_by_a_new_collection() {
        let core = Core::new();
        let change = |n: u8| undo::KeptChange {
            id: Id::from_bytes([n; 16]),
            reverse: undo::Reverse::RestoreNotes(vec![]),
        };
        core.keep_change(change(1));
        core.keep_change(change(2));
        assert_eq!(core.take_change(Id::from_bytes([1; 16])), None);
        assert_eq!(core.take_change(Id::from_bytes([2; 16])), Some(change(2)));
        assert_eq!(core.take_change(Id::from_bytes([2; 16])), None);
        core.keep_change(change(3));
        core.close_collection().unwrap();
        assert_eq!(core.take_change(Id::from_bytes([3; 16])), None);
    }

    #[test]
    fn version_matches_manifest() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
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
