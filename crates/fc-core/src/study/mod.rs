//! Answering cards, the schedule and the day boundary (ADR 0007). Step 1.7a.
//!
//! A review is an immutable **card event** (ADR 0006, section 3), merged between devices by union.
//! Each event records its inputs and its result. A card's due date, memory and state are a local
//! cache, `card_schedule`, folded from its events and rebuildable at any time.
//!
//! Three synced tables: `collection_setting` (one row per setting, registers `key` and `value`) and
//! the append-only `card_event` and `fsrs_parameter_set`. The local `card_schedule` holds the cache.

mod answer;
#[cfg(test)]
pub(crate) mod answer_tests;
mod error;
mod event;
mod fold;
#[cfg(test)]
pub(crate) mod fold_tests;
mod queue;
#[cfg(test)]
mod queue_tests;
mod schedule;
mod settings;
#[cfg(test)]
mod simulation_tests;
mod suspend;
#[cfg(test)]
mod tests;
mod undo;
#[cfg(test)]
mod undo_tests;

use crate::id::Id;
use crate::sync::{AppendOnlyTable, SyncedTable};

pub use answer::Answered;
pub use error::StudyError;
pub use event::{CardEvent, EventKind};
pub use queue::{Counts, DeckCounts, LEARN_AHEAD_MS, Next};
pub use schedule::CardSchedule;
pub use undo::Undone;

pub(crate) use fold::{ParameterSets, check_cache, rebuild_card};
pub use settings::DEFAULT_DAY_START_HOUR;

/// The sync entity types and register and column names are part of the sync format and never change
/// (ADR 0006, section 10).
pub const SETTING: SyncedTable = SyncedTable {
    entity: "collection_setting",
    table: "collection_setting",
    registers: &["key", "value"],
};

/// A setting's ID is `UUIDv5(SETTING_NAMESPACE, key)`, so two devices that set the same setting
/// write the same entity.
pub(crate) const SETTING_NAMESPACE: Id = Id::from_bytes(*b"fc-setting-ids-1");

/// One answer, or a void of one. Columns a kind does not use are null. `day` is the study day the
/// event belongs to, worked out when it was written from its time, its UTC offset and the start hour
/// then, so recomputing it never depends on a setting that has changed since.
pub const CARD_EVENT: AppendOnlyTable = AppendOnlyTable {
    entity: "card_event",
    table: "card_event",
    columns: &[
        "card",
        "kind",
        "time_ms",
        "utc_offset",
        "device",
        "previous",
        "day",
        "rating",
        "duration_ms",
        "preset",
        "desired_retention",
        "parameters",
        "steps",
        "state_before",
        "state",
        "step",
        "stability",
        "difficulty",
        "due_day",
        "due_ms",
        "target",
    ],
};

/// A set of 21 FSRS parameters, stored once and pointed at by the events that used it.
pub const PARAMETER_SET: AppendOnlyTable = AppendOnlyTable {
    entity: "fsrs_parameter_set",
    table: "fsrs_parameter_set",
    columns: &["values_f32"],
};
