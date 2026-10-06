use std::sync::Arc;

use fc_core::Core;
use fc_core::events::{Event, Listener};
use fc_core::id::Id;
use fc_core::scheduling::Rating;
use fc_core::study::EndReason;
use serde::Serialize;
use ts_rs::TS;

/// Progress of a long operation. No 64-bit integers (see ADR 0002).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct Progress {
    pub done: u32,
    /// `None` when the total is not known.
    pub total: Option<u32>,
    /// Optional short description of the current stage, for the UI.
    pub message: Option<String>,
}

/// How well a card was remembered, as an event reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum EventRating {
    Again,
    Hard,
    Good,
    Easy,
}

/// Why a study session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SessionEndReason {
    /// The session was ended on request.
    Ended,
    /// Another session started.
    Replaced,
    /// The collection was closed.
    Closed,
}

/// What a study session did. Durations are milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SessionSummary {
    pub answered: u32,
    pub again: u32,
    pub studied_ms: u32,
    pub elapsed_ms: u32,
}

/// Something that happened in the core, the public surface of `docs/events.md`. The UI treats it as
/// "something changed, refetch", never as the only source of truth. IDs are UUID strings. Events
/// carry IDs and small facts, never field text. Adding a kind or a field is not breaking; renaming
/// or removing either, or changing what a field means, is (ADR 0009, part 4).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CoreEvent {
    /// A note was added, with the cards it made.
    NoteAdded {
        note: String,
        note_type: String,
        deck: String,
        cards: Vec<String>,
    },
    /// A note's fields or cards changed. `fields` are the field IDs whose value changed.
    NoteEdited {
        note: String,
        fields: Vec<String>,
        cards_added: Vec<String>,
        cards_removed: Vec<String>,
    },
    /// A note and its cards went to the trash.
    NoteDeleted { note: String },
    /// A note came back from the trash, with the cards that came back.
    NoteRestored { note: String, cards: Vec<String> },
    /// A card was answered. `event` is the review in its history; `session` is the open study
    /// session, if any.
    CardAnswered {
        card: String,
        deck: String,
        event: String,
        rating: EventRating,
        session: Option<String>,
    },
    /// An answer was taken back. `event` is the review that is now voided.
    AnswerUndone { card: String, event: String },
    /// A study session started, on a deck or (`null`) the whole collection.
    StudySessionStarted {
        session: String,
        deck: Option<String>,
    },
    /// A study session ended.
    StudySessionEnded {
        session: String,
        deck: Option<String>,
        reason: SessionEndReason,
        summary: SessionSummary,
    },
    /// A batch from another collection was merged. One event for the whole batch: refetch.
    MergeApplied {
        registers_applied: u32,
        rows_added: u32,
        unknown_kept: u32,
        notes_reconciled: u32,
        cards_rebuilt: u32,
        rejected: u32,
    },
    /// A sync round finished. A stub: nothing emits it until Phase 4.
    SyncCompleted,
    /// Used by the `debugEmitEvent` method, which only exists in debug builds.
    Debug { message: String },
}

fn ids(ids: &[Id]) -> Vec<String> {
    ids.iter().map(Id::to_string).collect()
}

impl From<&Event> for CoreEvent {
    /// Exhaustive on purpose, with no wildcard arm: a new core event must be given an API form
    /// here before the crate compiles.
    fn from(event: &Event) -> Self {
        match event {
            Event::NoteAdded {
                note,
                note_type,
                deck,
                cards,
            } => Self::NoteAdded {
                note: note.to_string(),
                note_type: note_type.to_string(),
                deck: deck.to_string(),
                cards: ids(cards),
            },
            Event::NoteEdited {
                note,
                fields,
                cards_added,
                cards_removed,
            } => Self::NoteEdited {
                note: note.to_string(),
                fields: ids(fields),
                cards_added: ids(cards_added),
                cards_removed: ids(cards_removed),
            },
            Event::NoteDeleted { note } => Self::NoteDeleted {
                note: note.to_string(),
            },
            Event::NoteRestored { note, cards } => Self::NoteRestored {
                note: note.to_string(),
                cards: ids(cards),
            },
            Event::CardAnswered {
                card,
                deck,
                event,
                rating,
                session,
            } => Self::CardAnswered {
                card: card.to_string(),
                deck: deck.to_string(),
                event: event.to_string(),
                rating: match rating {
                    Rating::Again => EventRating::Again,
                    Rating::Hard => EventRating::Hard,
                    Rating::Good => EventRating::Good,
                    Rating::Easy => EventRating::Easy,
                },
                session: session.map(|s| s.to_string()),
            },
            Event::AnswerUndone { card, event } => Self::AnswerUndone {
                card: card.to_string(),
                event: event.to_string(),
            },
            Event::StudySessionStarted { session, deck } => Self::StudySessionStarted {
                session: session.to_string(),
                deck: deck.map(|d| d.to_string()),
            },
            Event::StudySessionEnded {
                session,
                deck,
                reason,
                summary,
            } => Self::StudySessionEnded {
                session: session.to_string(),
                deck: deck.map(|d| d.to_string()),
                reason: match reason {
                    EndReason::Ended => SessionEndReason::Ended,
                    EndReason::Replaced => SessionEndReason::Replaced,
                    EndReason::Closed => SessionEndReason::Closed,
                },
                summary: SessionSummary {
                    answered: summary.answered,
                    again: summary.again,
                    studied_ms: summary.studied_ms,
                    elapsed_ms: summary.elapsed_ms,
                },
            },
            Event::MergeApplied {
                registers_applied,
                rows_added,
                unknown_kept,
                notes_reconciled,
                cards_rebuilt,
                rejected,
            } => Self::MergeApplied {
                registers_applied: *registers_applied,
                rows_added: *rows_added,
                unknown_kept: *unknown_kept,
                notes_reconciled: *notes_reconciled,
                cards_rebuilt: *cards_rebuilt,
                rejected: *rejected,
            },
            Event::SyncCompleted => Self::SyncCompleted,
        }
    }
}

/// Sends every core event to `sink` as a `Notice::Event`. Hosts call this once at startup, so that
/// events with no API call in progress (a merge, a background sync) reach the UI too. The sink runs
/// while the collection lock is held, so it must follow the listener rules in `docs/events.md`.
pub fn forward_events(core: &Core, sink: Arc<dyn EventSink>) {
    core.listen(Arc::new(Forwarder(sink)));
}

struct Forwarder(Arc<dyn EventSink>);

impl Listener for Forwarder {
    fn on_events(&self, events: &[Event]) {
        for event in events {
            self.0.send(Notice::Event {
                event: event.into(),
            });
        }
    }
}

/// The single stream from core to UI.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Notice {
    Event { event: CoreEvent },
    Progress { op: u32, progress: Progress },
}

/// Implemented by the host. Hosts throttle progress to a few updates per second.
pub trait EventSink: Send + Sync {
    fn send(&self, notice: Notice);
}

/// Drops every notice. For tests and hosts that do not deliver notices.
pub struct NullSink;

impl EventSink for NullSink {
    fn send(&self, _: Notice) {}
}

#[cfg(test)]
#[path = "notice_tests.rs"]
mod tests;
