//! Core events (ADR 0009): what the core tells the UI, and later add-ons, after a change is stored.
//!
//! This module is the public surface for events, documented in `docs/events.md`. Everything else in
//! `fc-core` is internal.
//!
//! An operation adds its events to its write transaction, and the collection hands them to every
//! listener in one batch after the commit. A write that fails or rolls back emits nothing, and so
//! does a no-op. Events carry IDs and small facts, never field values, tags or templates: card
//! content is untrusted.
//!
//! Listener rules (ADR 0009, part 3):
//!
//! - **Return quickly, and never call into the core.** The collection lock is held while listeners
//!   run, so a call back into the core deadlocks. Hand the events off instead: send them on a
//!   channel, post a message, set a flag.
//! - **Ignore event kinds you do not know.** New kinds are added without notice.
//! - **Do not panic.** On native, a panic is caught and ignored, the listener stays registered and
//!   the other listeners still get the batch. On the web (wasm), a panic traps the worker whatever
//!   the core does; the write has already committed.
//! - Delivery is after the commit, so an event always describes data that is stored.
//! - Listeners are added for the life of the `Core` or `Collection`. There is no removal.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use crate::id::Id;
use crate::scheduling::Rating;
use crate::study::{EndReason, SessionSummary};

/// Something that changed in the collection. Not `#[non_exhaustive]`: `fc-api` matches on every
/// variant, so a new one cannot be dropped silently (ADR 0009, part 2).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A note was added, with the cards it made.
    NoteAdded {
        note: Id,
        note_type: Id,
        deck: Id,
        cards: Vec<Id>,
    },
    /// A note's fields changed, or its cards did. `fields` are the fields whose value changed.
    NoteEdited {
        note: Id,
        fields: Vec<Id>,
        cards_added: Vec<Id>,
        cards_removed: Vec<Id>,
    },
    /// A note and its cards went to the trash.
    NoteDeleted { note: Id },
    /// A note came back from the trash, with the cards that came back.
    NoteRestored { note: Id, cards: Vec<Id> },
    /// A card was answered. `event` is the review in the card's history; `session` is the open study
    /// session, if any.
    CardAnswered {
        card: Id,
        deck: Id,
        event: Id,
        rating: Rating,
        session: Option<Id>,
    },
    /// An answer was taken back. `event` is the review that is now voided.
    AnswerUndone { card: Id, event: Id },
    /// A study session started, on a deck or (with none) the whole collection.
    StudySessionStarted { session: Id, deck: Option<Id> },
    /// A study session ended.
    StudySessionEnded {
        session: Id,
        deck: Option<Id>,
        reason: EndReason,
        summary: SessionSummary,
    },
    /// A batch from another collection was merged. One event for the whole batch: the UI refetches.
    MergeApplied {
        registers_applied: u32,
        rows_added: u32,
        /// Registers and rows this build does not know, newly kept.
        unknown_kept: u32,
        notes_reconciled: u32,
        cards_rebuilt: u32,
        rejected: u32,
    },
    /// A sync round finished. A stub: nothing emits it until Phase 4.
    SyncCompleted,
}

/// Receives events. See the module docs for the rules a listener must follow.
pub trait Listener: Send + Sync {
    /// One operation's events, in the order it emitted them. Never empty.
    fn on_events(&self, events: &[Event]);
}

/// The listeners of a `Core` or a `Collection`. Clones share one list.
#[derive(Clone, Default)]
pub struct Listeners(Arc<Mutex<Vec<Arc<dyn Listener>>>>);

impl Listeners {
    pub fn add(&self, listener: Arc<dyn Listener>) {
        self.list().push(listener);
    }

    fn list(&self) -> std::sync::MutexGuard<'_, Vec<Arc<dyn Listener>>> {
        // The list is only pushed to and copied, so a poisoned lock still holds a usable list.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Hands a batch to every listener. A listener is never called with the list's lock held, and a
    /// panic in one is ignored (native only: wasm cannot catch it).
    pub(crate) fn deliver(&self, events: &[Event]) {
        if events.is_empty() {
            return;
        }
        let listeners = self.list().clone();
        for listener in listeners {
            let _ = catch_unwind(AssertUnwindSafe(|| listener.on_events(events)));
        }
    }
}

impl fmt::Debug for Listeners {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Listeners({})", self.list().len())
    }
}

/// Saturates a count to `u32`, the widest integer events carry.
pub(crate) fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "events_tests.rs"]
pub(crate) mod tests;
