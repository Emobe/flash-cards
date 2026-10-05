//! Time and host facts, supplied by the host (ADR 0006, section 2). `fc-core` never reads the
//! clock itself (a clippy rule in `clippy.toml` enforces it), so native, web and tests all go
//! through `Clock`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};

use crate::id::Id;

/// What the clock says right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Milliseconds since the Unix epoch, UTC.
    pub unix_ms: i64,
    /// Minutes the local time is ahead of UTC (negative when behind), at this moment.
    pub utc_offset_minutes: i32,
}

/// Supplied by the host: `fc-native` reads the system clock, `fc-wasm` uses `Date.now()` and
/// `getTimezoneOffset()`.
pub trait Clock: Send + Sync + fmt::Debug {
    fn now(&self) -> Reading;
}

/// A clock that only moves when told to. For tests and tools.
#[derive(Debug, Default)]
pub struct ManualClock {
    unix_ms: AtomicI64,
    utc_offset_minutes: AtomicI32,
}

impl ManualClock {
    pub fn new(unix_ms: i64) -> Self {
        Self {
            unix_ms: AtomicI64::new(unix_ms),
            utc_offset_minutes: AtomicI32::new(0),
        }
    }

    pub fn set(&self, unix_ms: i64) {
        self.unix_ms.store(unix_ms, Ordering::SeqCst);
    }

    pub fn advance(&self, ms: i64) {
        self.unix_ms.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn set_utc_offset_minutes(&self, minutes: i32) {
        self.utc_offset_minutes.store(minutes, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Reading {
        Reading {
            unix_ms: self.unix_ms.load(Ordering::SeqCst),
            utc_offset_minutes: self.utc_offset_minutes.load(Ordering::SeqCst),
        }
    }
}

/// Everything a host gives the core when it opens a collection.
#[derive(Debug, Clone)]
pub struct Host {
    pub clock: Arc<dyn Clock>,
    /// Identifies this installation of the app. The host stores it outside the collection, so a
    /// copied collection file is recognised as a copy and gets a new device ID (ADR 0006,
    /// section 1). Any value that differs from the one saved with the collection counts.
    pub installation_id: Id,
}
