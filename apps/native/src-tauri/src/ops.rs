//! Tracks running operations so `cancel(op)` can reach them, including a cancel that arrives
//! before its call does (`docs/adr/0002-ui-core-bridge.md`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A cancel for an operation that never starts is forgotten after this long. IDs restart at 1
/// when the page reloads, so a stale entry must not outlive the reload for long.
const EARLY_CANCEL_TTL: Duration = Duration::from_secs(60);

struct Entry {
    flag: Arc<AtomicBool>,
    /// Set when `cancel` created the entry before the call started.
    early_since: Option<Instant>,
}

#[derive(Default)]
pub struct Operations {
    entries: Mutex<HashMap<u32, Entry>>,
}

impl Operations {
    /// Registers `op` and returns its cancel flag. Already set if `cancel(op)` came first.
    pub fn start(&self, op: u32, now: Instant) -> Arc<AtomicBool> {
        let mut entries = self.entries.lock().expect("operations lock");
        prune(&mut entries, now);
        let entry = entries.entry(op).or_insert_with(|| Entry {
            flag: Arc::new(AtomicBool::new(false)),
            early_since: None,
        });
        entry.early_since = None;
        Arc::clone(&entry.flag)
    }

    /// Requests cancellation of `op`, whether or not it has started.
    pub fn cancel(&self, op: u32, now: Instant) {
        let mut entries = self.entries.lock().expect("operations lock");
        prune(&mut entries, now);
        let entry = entries.entry(op).or_insert_with(|| Entry {
            flag: Arc::new(AtomicBool::new(false)),
            early_since: Some(now),
        });
        entry.flag.store(true, Ordering::SeqCst);
    }

    /// Forgets `op` once its call has finished.
    pub fn finish(&self, op: u32) {
        self.entries.lock().expect("operations lock").remove(&op);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().expect("operations lock").len()
    }
}

fn prune(entries: &mut HashMap<u32, Entry>, now: Instant) {
    entries.retain(|_, e| {
        e.early_since
            .is_none_or(|since| now.saturating_duration_since(since) < EARLY_CANCEL_TTL)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_after_start_sets_the_flag() {
        let ops = Operations::default();
        let now = Instant::now();
        let flag = ops.start(1, now);
        assert!(!flag.load(Ordering::SeqCst));
        ops.cancel(1, now);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn cancel_before_start_is_remembered() {
        let ops = Operations::default();
        let now = Instant::now();
        ops.cancel(2, now);
        assert!(ops.start(2, now).load(Ordering::SeqCst));
    }

    #[test]
    fn cancelling_one_op_leaves_others_alone() {
        let ops = Operations::default();
        let now = Instant::now();
        let a = ops.start(1, now);
        let b = ops.start(2, now);
        ops.cancel(1, now);
        assert!(a.load(Ordering::SeqCst));
        assert!(!b.load(Ordering::SeqCst));
    }

    #[test]
    fn finished_ops_are_forgotten() {
        let ops = Operations::default();
        let now = Instant::now();
        ops.start(1, now);
        ops.finish(1);
        assert_eq!(ops.len(), 0);
        // A late cancel for a finished op creates an early entry, which expires.
        ops.cancel(1, now);
        assert_eq!(ops.len(), 1);
        ops.cancel(9, now + EARLY_CANCEL_TTL);
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn a_stale_early_cancel_does_not_hit_a_later_op_with_the_same_id() {
        let ops = Operations::default();
        let now = Instant::now();
        ops.cancel(3, now);
        let later = now + EARLY_CANCEL_TTL + Duration::from_secs(1);
        assert!(!ops.start(3, later).load(Ordering::SeqCst));
    }
}
