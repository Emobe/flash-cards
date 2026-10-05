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

/// Something that happened in the core. The UI treats it as "something changed, refetch", never as
/// the only source of truth. Step 1.12 replaces the placeholder variant.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CoreEvent {
    /// Placeholder, used by the debug-only `debugEmitEvent` method.
    Debug { message: String },
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
