//! Delivers core notices to the webview's channel, with progress throttling. One channel per
//! webview label: subscribing again (a page reload) replaces the old one.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use fc_api::{EventSink, Notice};
use tauri::ipc::Channel;

/// Progress for one operation is passed on at most this often, except its final update.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Where notices end up. A Tauri `Channel` in the app, a recorder in tests.
pub trait Subscriber: Send {
    /// Returns false when the subscriber is gone and should be dropped.
    fn deliver(&self, notice: &Notice) -> bool;
}

impl Subscriber for Channel<Notice> {
    fn deliver(&self, notice: &Notice) -> bool {
        self.send(notice.clone()).is_ok()
    }
}

pub struct NoticeHub<S> {
    subscribers: Mutex<HashMap<String, S>>,
    last_progress: Mutex<HashMap<u32, Instant>>,
}

impl<S> Default for NoticeHub<S> {
    fn default() -> Self {
        Self {
            subscribers: Mutex::default(),
            last_progress: Mutex::default(),
        }
    }
}

impl<S: Subscriber> NoticeHub<S> {
    /// Registers the notice channel for a webview, replacing any earlier one.
    pub fn subscribe(&self, label: &str, subscriber: S) {
        self.subscribers
            .lock()
            .expect("subscribers lock")
            .insert(label.to_owned(), subscriber);
    }

    /// Forgets throttling state once `op` has finished.
    pub fn finish(&self, op: u32) {
        self.last_progress
            .lock()
            .expect("progress lock")
            .remove(&op);
    }

    fn allow_progress(&self, op: u32, is_final: bool, now: Instant) -> bool {
        let mut last = self.last_progress.lock().expect("progress lock");
        let due = last
            .get(&op)
            .is_none_or(|t| now.saturating_duration_since(*t) >= PROGRESS_INTERVAL);
        if due || is_final {
            last.insert(op, now);
            true
        } else {
            false
        }
    }

    fn send_at(&self, notice: &Notice, now: Instant) {
        if let Notice::Progress { op, progress } = notice {
            let is_final = progress.total == Some(progress.done);
            if !self.allow_progress(*op, is_final, now) {
                return;
            }
        }
        // Notices sent while nothing is subscribed are dropped: the UI refetches on (re)subscribe.
        self.subscribers
            .lock()
            .expect("subscribers lock")
            .retain(|_, s| s.deliver(notice));
    }
}

impl<S: Subscriber + Sync> EventSink for NoticeHub<S> {
    fn send(&self, notice: Notice) {
        self.send_at(&notice, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fc_api::{CoreEvent, Progress};

    use super::*;

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<Notice>>>);

    impl Subscriber for Recorder {
        fn deliver(&self, notice: &Notice) -> bool {
            self.0.lock().unwrap().push(notice.clone());
            true
        }
    }

    fn progress(op: u32, done: u32, total: Option<u32>) -> Notice {
        Notice::Progress {
            op,
            progress: Progress {
                done,
                total,
                message: None,
            },
        }
    }

    fn event(message: &str) -> Notice {
        Notice::Event {
            event: CoreEvent::Debug {
                message: message.to_owned(),
            },
        }
    }

    #[test]
    fn progress_is_throttled_but_the_final_update_always_arrives_in_order() {
        let hub = NoticeHub::default();
        let rec = Recorder::default();
        hub.subscribe("main", rec.clone());
        let t0 = Instant::now();
        for done in 1..=5 {
            hub.send_at(
                &progress(1, done, Some(5)),
                t0 + Duration::from_millis(u64::from(done)),
            );
        }
        let seen = rec.0.lock().unwrap().clone();
        assert_eq!(seen, vec![progress(1, 1, Some(5)), progress(1, 5, Some(5))]);
        hub.send_at(&progress(1, 6, None), t0 + PROGRESS_INTERVAL * 2);
        assert_eq!(rec.0.lock().unwrap().len(), 3);
    }

    #[test]
    fn throttling_is_per_operation() {
        let hub = NoticeHub::default();
        let rec = Recorder::default();
        hub.subscribe("main", rec.clone());
        let t0 = Instant::now();
        hub.send_at(&progress(1, 1, None), t0);
        hub.send_at(&progress(2, 1, None), t0);
        assert_eq!(rec.0.lock().unwrap().len(), 2);
    }

    #[test]
    fn events_are_never_throttled() {
        let hub = NoticeHub::default();
        let rec = Recorder::default();
        hub.subscribe("main", rec.clone());
        let t0 = Instant::now();
        hub.send_at(&event("a"), t0);
        hub.send_at(&event("b"), t0);
        assert_eq!(rec.0.lock().unwrap().len(), 2);
    }

    #[test]
    fn resubscribing_replaces_the_channel() {
        let hub = NoticeHub::default();
        let (old, new) = (Recorder::default(), Recorder::default());
        let t0 = Instant::now();
        hub.subscribe("main", old.clone());
        hub.send_at(&event("before"), t0);
        hub.subscribe("main", new.clone());
        hub.send_at(&event("after"), t0);
        assert_eq!(*old.0.lock().unwrap(), vec![event("before")]);
        assert_eq!(*new.0.lock().unwrap(), vec![event("after")]);
    }

    #[test]
    fn notices_with_no_subscriber_are_dropped_without_error() {
        let hub: NoticeHub<Recorder> = NoticeHub::default();
        hub.send_at(&event("nobody"), Instant::now());
        let late = Recorder::default();
        hub.subscribe("main", late.clone());
        assert!(late.0.lock().unwrap().is_empty());
    }

    #[test]
    fn a_dead_subscriber_is_dropped() {
        struct Dead;
        impl Subscriber for Dead {
            fn deliver(&self, _: &Notice) -> bool {
                false
            }
        }
        let hub = NoticeHub::default();
        hub.subscribe("main", Dead);
        hub.send_at(&event("x"), Instant::now());
        assert!(hub.subscribers.lock().unwrap().is_empty());
    }
}
