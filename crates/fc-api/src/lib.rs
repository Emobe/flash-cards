//! The core's public API: a list of methods, one dispatcher for every host (Tauri, web, CLI), and
//! the TypeScript bindings generated from the same list. No Tauri or platform dependency.
//! See `docs/adr/0002-ui-core-bridge.md`.

mod backup;
#[cfg(test)]
mod backup_tests;
mod bindings;
mod collection;
mod context;
#[cfg(debug_assertions)]
mod debug;
mod decks;
mod error;
mod examples;
mod notes;
mod notice;
#[cfg(debug_assertions)]
mod sample_cards;
mod spike_scheduling;
mod study;

#[cfg(not(target_arch = "wasm32"))]
pub mod autobackup;
pub use bindings::generate_bindings;
pub use context::OpContext;
pub use error::{ApiError, ErrorKind};
pub use fc_core::Core;
pub use fc_core::clock::{Clock, Host, Reading};
pub use fc_core::id::Id;
pub use notice::{
    CoreEvent, EventRating, EventSink, Notice, NullSink, Progress, SessionEndReason,
    SessionSummary, forward_events,
};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use ts_rs::TS;

/// One API method. Implement it, then add the type to the `methods!` list below.
pub trait Method {
    /// camelCase name used on the wire.
    const NAME: &'static str;
    /// Whether the method reads the request's attachment (`OpContext::attachment`).
    const ATTACHMENT_IN: bool = false;
    /// Whether the method sets a reply attachment (`OpContext::set_reply_attachment`).
    const ATTACHMENT_OUT: bool = false;
    type Input: DeserializeOwned + TS + 'static;
    type Output: Serialize + TS + 'static;

    fn call(core: &Core, input: Self::Input, ctx: &OpContext) -> Result<Self::Output, ApiError>;
}

/// What a method returned: JSON output and an optional attachment.
#[derive(Debug, PartialEq)]
pub struct Reply {
    pub output: Value,
    pub attachment: Option<Vec<u8>>,
}

fn run<M: Method>(core: &Core, input: Value, ctx: &OpContext) -> Result<Reply, ApiError> {
    let input: M::Input = serde_json::from_value(input).map_err(|e| {
        ApiError::invalid_input(format!(
            "The app sent a request to \"{}\" that this version could not read ({e}). Update the app and try again.",
            M::NAME
        ))
    })?;
    let output = M::call(core, input, ctx)?;
    let output = serde_json::to_value(output).map_err(|_| ApiError::internal())?;
    Ok(Reply {
        output,
        attachment: ctx.take_reply_attachment(),
    })
}

/// Registers every method: generates `dispatch` and the TypeScript `Methods` map from one list.
/// Methods in the `native` list exist only where there is a file system (not on wasm). Methods in
/// the `debug` list exist only when `debug_assertions` is on.
macro_rules! methods {
    (
        always: [$($m:ty),* $(,)?],
        native: [$($n:ty),* $(,)?],
        debug: [$($d:ty),* $(,)?] $(,)?
    ) => {
        /// Runs the named method. Every host calls this.
        pub fn dispatch(
            core: &Core,
            method: &str,
            input: Value,
            ctx: &OpContext,
        ) -> Result<Reply, ApiError> {
            $( if method == <$m as Method>::NAME { return run::<$m>(core, input, ctx); } )*
            #[cfg(not(target_arch = "wasm32"))]
            {
                $( if method == <$n as Method>::NAME { return run::<$n>(core, input, ctx); } )*
            }
            #[cfg(debug_assertions)]
            {
                $( if method == <$d as Method>::NAME { return run::<$d>(core, input, ctx); } )*
            }
            Err(ApiError::unknown_method(method))
        }

        /// Collects every API type, and the `Methods` map, as `(file name, contents)`.
        pub(crate) fn collect_bindings(
            cfg: &ts_rs::Config,
        ) -> std::collections::BTreeMap<String, String> {
            let mut out = bindings::Collector::new(cfg);
            out.add::<ApiError>();
            out.add::<Notice>();
            $( out.add::<<$m as Method>::Input>(); out.add::<<$m as Method>::Output>(); )*
            #[cfg(not(target_arch = "wasm32"))]
            {
                $( out.add::<<$n as Method>::Input>(); out.add::<<$n as Method>::Output>(); )*
            }
            #[cfg(debug_assertions)]
            {
                $( out.add::<<$d as Method>::Input>(); out.add::<<$d as Method>::Output>(); )*
            }
            #[allow(unused_mut)]
            let mut entries = vec![$( bindings::methods_entry::<$m>(cfg) ),*];
            #[cfg(not(target_arch = "wasm32"))]
            {
                $( entries.push(bindings::methods_entry::<$n>(cfg)); )*
            }
            #[cfg(debug_assertions)]
            {
                $( entries.push(bindings::methods_entry::<$d>(cfg)); )*
            }
            out.finish(entries)
        }
    };
}

methods! {
    always: [
        examples::GetCoreInfo,
        examples::ExampleDivide,
        collection::GetCollectionInfo,
        decks::GetDeckList,
        study::StartStudySession,
        study::EndStudySession,
        study::NextCard,
        study::RenderCard,
        study::GetMedia,
        notes::GetNoteTypes,
        notes::GetTags,
        notes::FindDuplicates,
        notes::AddNote,
        notes::AddMedia,
        study::AnswerCard,
        study::UndoAnswer,
        spike_scheduling::SpikeSchedule,
        spike_scheduling::SpikeOptimise,
        backup::ExportBackup,
        backup::ReadBackupInfo,
        backup::RestoreBackup,
        backup::ImportBackup,
        backup::GetBackupSettings,
        backup::SetBackupSettings,
    ],
    native: [
        backup::files::ExportBackupToFile,
        backup::files::ReadBackupFileInfo,
        backup::files::RestoreBackupFromFile,
        backup::files::ImportBackupFromFile,
    ],
    debug: [
        debug::DebugSlow,
        debug::DebugEchoBytes,
        debug::DebugEmitEvent,
        debug::DebugPanic,
        sample_cards::DebugAddSampleCards,
    ],
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(method: &str, input: Value) -> Result<Value, ApiError> {
        dispatch(&Core::new(), method, input, &OpContext::uncancellable()).map(|reply| reply.output)
    }

    #[test]
    fn get_core_info_returns_the_core_version() {
        let out = call("getCoreInfo", Value::Null).unwrap();
        assert_eq!(out, json!({ "coreVersion": fc_core::version() }));
    }

    #[test]
    fn example_divide_returns_the_quotient() {
        let out = call("exampleDivide", json!({ "dividend": 7, "divisor": 2 })).unwrap();
        assert_eq!(out, json!({ "quotient": 3.5 }));
    }

    #[test]
    fn dividing_by_zero_is_a_readable_invalid_input_error() {
        let err = call("exampleDivide", json!({ "dividend": 1, "divisor": 0 })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert_eq!(
            err.message,
            "Can't divide by zero. Enter a divisor other than 0."
        );
    }

    #[test]
    fn spike_schedule_returns_one_review_per_rating() {
        let out = call(
            "spikeSchedule",
            json!({ "ratings": ["good", "good", "again"], "desiredRetention": 0.9 }),
        )
        .unwrap();
        let reviews = out["reviews"].as_array().unwrap();
        assert_eq!(reviews.len(), 3);
        assert_eq!(reviews[0]["intervalDays"], 2);
        assert_eq!(reviews[1]["intervalDays"], 11);
    }

    #[test]
    fn spike_schedule_rejects_a_retention_outside_the_range() {
        let err = call(
            "spikeSchedule",
            json!({ "ratings": ["good"], "desiredRetention": 0.2 }),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn spike_optimise_returns_21_parameters() {
        let out = call("spikeOptimise", json!({ "cards": 100 })).unwrap();
        assert_eq!(out["parameters"].as_array().unwrap().len(), 21);
        let err = call("spikeOptimise", json!({ "cards": 0 })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn collection_info_needs_an_open_collection() {
        let err = call("getCollectionInfo", Value::Null).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(err.message, "No collection is open.");
    }

    #[test]
    fn collection_info_describes_the_open_collection() {
        let core = Core::new();
        let host = Host {
            clock: std::sync::Arc::new(fc_core::clock::ManualClock::new(1_700_000_000_000)),
            installation_id: Id::from_bytes(*b"installation-one"),
        };
        core.open_collection(":memory:", host).unwrap();
        let ctx = OpContext::uncancellable();
        let out = dispatch(&core, "getCollectionInfo", Value::Null, &ctx).unwrap();
        let device_id = out.output["deviceId"].as_str().unwrap().to_owned();
        assert!(device_id.parse::<Id>().is_ok(), "{device_id}");
        // The newest version changes with every migration, so ask the core for it.
        let newest = core
            .with_collection(|c| c.info().unwrap().supported_schema_version)
            .unwrap();
        assert_eq!(
            out.output,
            json!({
                "schemaVersion": newest,
                "supportedSchemaVersion": newest,
                "createdBy": fc_core::version(),
                "deviceId": device_id,
                "unsupportedFeatures": [],
            })
        );
    }

    #[test]
    fn collection_errors_become_readable_api_errors() {
        use fc_core::collection::CollectionError as E;
        let cases = [
            (E::NotFound, ErrorKind::NotFound),
            (E::AlreadyExists, ErrorKind::InvalidInput),
            (E::NotACollection, ErrorKind::InvalidInput),
            (
                E::TooNew {
                    found: 9,
                    supported: 1,
                },
                ErrorKind::UpdateRequired,
            ),
            (E::InUse, ErrorKind::Unavailable),
        ];
        for (error, kind) in cases {
            let message = error.to_string();
            let api = ApiError::from(error);
            assert_eq!(api.kind, kind);
            assert_eq!(api.message, message);
        }
        let hidden = ApiError::from(E::Storage("disk I/O error at /secret".to_owned()));
        assert_eq!(hidden, ApiError::internal());
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "debugPanic")]
    fn debug_panic_panics() {
        let _ = call("debugPanic", Value::Null);
    }

    #[test]
    fn unknown_method_is_reported() {
        let err = call("nope", Value::Null).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnknownMethod);
        assert!(err.message.contains("nope"));
    }

    #[test]
    fn malformed_input_is_a_readable_invalid_input_error() {
        let err = call("exampleDivide", json!({ "dividend": "x" })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("exampleDivide"));
    }

    #[test]
    fn error_json_has_kind_and_message_only() {
        let err = ApiError::new(ErrorKind::NotFound, "Gone.");
        assert_eq!(
            serde_json::to_value(err).unwrap(),
            json!({ "kind": "notFound", "message": "Gone." })
        );
    }

    #[test]
    fn checkpoint_reports_cancellation() {
        assert!(OpContext::uncancellable().checkpoint().is_ok());
        let err = OpContext::new(&|| true).checkpoint().unwrap_err();
        assert_eq!(err.kind, ErrorKind::Cancelled);
    }

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<Notice>>);

    impl EventSink for RecordingSink {
        fn send(&self, notice: Notice) {
            self.0.lock().unwrap().push(notice);
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn progress_notices_arrive_in_order_tagged_with_the_op() {
        let sink = RecordingSink::default();
        let ctx = OpContext::uncancellable().with_sink(&sink, Some(7));
        let reply = dispatch(
            &Core::new(),
            "debugSlow",
            json!({ "steps": 3, "stepMs": 0 }),
            &ctx,
        )
        .unwrap();
        assert_eq!(reply.output, json!({ "completed": 3 }));
        let seen: Vec<_> = sink.0.lock().unwrap().clone();
        let expected: Vec<_> = (1..=3)
            .map(|done| Notice::Progress {
                op: 7,
                progress: Progress {
                    done,
                    total: Some(3),
                    message: None,
                },
            })
            .collect();
        assert_eq!(seen, expected);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn progress_without_an_op_is_dropped() {
        let sink = RecordingSink::default();
        let ctx = OpContext::uncancellable().with_sink(&sink, None);
        dispatch(
            &Core::new(),
            "debugSlow",
            json!({ "steps": 2, "stepMs": 0 }),
            &ctx,
        )
        .unwrap();
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn cancelling_before_start_stops_at_the_first_checkpoint() {
        let cancelled = || true;
        let sink = RecordingSink::default();
        let ctx = OpContext::new(&cancelled).with_sink(&sink, Some(1));
        let err = dispatch(
            &Core::new(),
            "debugSlow",
            json!({ "steps": 5, "stepMs": 0 }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Cancelled);
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn cancelling_mid_run_stops_at_the_next_checkpoint() {
        // The sink flips the flag after the second progress notice.
        struct CancelAfterTwo<'a>(&'a AtomicBool, Mutex<u32>);
        impl EventSink for CancelAfterTwo<'_> {
            fn send(&self, _: Notice) {
                let mut count = self.1.lock().unwrap();
                *count += 1;
                if *count == 2 {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
        }
        let flag = AtomicBool::new(false);
        let sink = CancelAfterTwo(&flag, Mutex::new(0));
        let cancelled = || flag.load(Ordering::SeqCst);
        let ctx = OpContext::new(&cancelled).with_sink(&sink, Some(1));
        let err = dispatch(
            &Core::new(),
            "debugSlow",
            json!({ "steps": 10, "stepMs": 0 }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Cancelled);
        assert_eq!(*sink.1.lock().unwrap(), 2);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn attachment_round_trips_through_dispatch() {
        let bytes = [1u8, 2, 3, 255];
        let ctx = OpContext::uncancellable().with_attachment(&bytes);
        let reply = dispatch(&Core::new(), "debugEchoBytes", Value::Null, &ctx).unwrap();
        assert_eq!(reply.output, json!({ "length": 4 }));
        assert_eq!(reply.attachment.as_deref(), Some(&bytes[..]));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_missing_attachment_is_a_readable_error() {
        let err = call("debugEchoBytes", Value::Null).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_emit_event_sends_an_event_notice() {
        let sink = RecordingSink::default();
        let ctx = OpContext::uncancellable().with_sink(&sink, None);
        dispatch(
            &Core::new(),
            "debugEmitEvent",
            json!({ "message": "hi" }),
            &ctx,
        )
        .unwrap();
        assert_eq!(
            *sink.0.lock().unwrap(),
            vec![Notice::Event {
                event: CoreEvent::Debug {
                    message: "hi".into()
                }
            }]
        );
    }

    #[test]
    fn notice_json_shapes() {
        let progress = Notice::Progress {
            op: 3,
            progress: Progress {
                done: 1,
                total: None,
                message: Some("x".into()),
            },
        };
        assert_eq!(
            serde_json::to_value(progress).unwrap(),
            json!({ "type": "progress", "op": 3,
                    "progress": { "done": 1, "total": null, "message": "x" } })
        );
        let event = Notice::Event {
            event: CoreEvent::Debug {
                message: "m".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({ "type": "event", "event": { "kind": "debug", "message": "m" } })
        );
    }
}
