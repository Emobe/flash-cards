//! The backup methods through `dispatch`, as every host calls them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use fc_core::clock::ManualClock;
use fc_core::notetype::builtin;
use serde_json::{Value, json};

use super::*;

const T0: i64 = 1_791_363_600_000;

fn core() -> Core {
    let core = Core::new();
    let host = Host {
        clock: Arc::new(ManualClock::new(T0)),
        installation_id: Id::from_bytes([1; 16]),
    };
    core.open_collection(":memory:", host).unwrap();
    core
}

/// A core with two Basic notes, one of them in the deck "Polish". Returns the core and that deck.
fn core_with_notes() -> (Core, String) {
    let core = core();
    let deck = core
        .with_collection(|c| {
            let note_type = c.note_type(builtin::basic()).unwrap().unwrap();
            let fields = &note_type.fields;
            let deck = c.create_deck("Polish", None).unwrap();
            c.add_note_to_deck(
                deck,
                builtin::basic(),
                &[(fields[0].id, "dom"), (fields[1].id, "house")],
            )
            .unwrap();
            c.add_note(
                builtin::basic(),
                &[(fields[0].id, "kot"), (fields[1].id, "cat")],
            )
            .unwrap();
            deck.to_string()
        })
        .unwrap();
    (core, deck)
}

fn note_count(core: &Core) -> usize {
    core.with_collection(|c| c.notes(builtin::basic()).unwrap().len())
        .unwrap()
}

fn send(core: &Core, method: &str, input: Value, bytes: Option<&[u8]>) -> Result<Reply, ApiError> {
    let mut ctx = OpContext::uncancellable();
    if let Some(bytes) = bytes {
        ctx = ctx.with_attachment(bytes);
    }
    dispatch(core, method, input, &ctx)
}

fn export(core: &Core, deck: Option<&str>, history: bool) -> (Value, Vec<u8>) {
    let reply = send(
        core,
        "exportBackup",
        json!({ "deck": deck, "history": history }),
        None,
    )
    .unwrap();
    (reply.output, reply.attachment.unwrap())
}

#[test]
fn a_backup_exported_as_bytes_restores_into_another_collection() {
    let (a, _) = core_with_notes();
    let (summary, bytes) = export(&a, None, true);
    assert_eq!(summary["info"]["scope"], json!({ "kind": "collection" }));
    assert_eq!(summary["info"]["history"], true);
    assert_eq!(summary["info"]["createdMs"].as_f64(), Some(T0 as f64));
    assert!(summary["info"]["registers"].as_u64().unwrap() > 10);
    assert_eq!(&bytes[..2], b"PK");

    let b = core();
    assert_eq!(note_count(&b), 0);
    let info = send(&b, "readBackupInfo", Value::Null, Some(&bytes)).unwrap();
    assert_eq!(info.output, summary["info"]);
    // Reading the info changed nothing.
    assert_eq!(note_count(&b), 0);

    let restored = send(&b, "restoreBackup", Value::Null, Some(&bytes))
        .unwrap()
        .output;
    assert_eq!(note_count(&b), 2);
    assert!(restored["registersWritten"].as_u64().unwrap() > 10);
    assert_eq!(restored["removed"], 0);
    assert_eq!(restored["rejected"], json!([]));
    assert_eq!(restored["info"], summary["info"]);

    // Restoring it again is a no-op.
    let again = send(&b, "restoreBackup", Value::Null, Some(&bytes))
        .unwrap()
        .output;
    assert_eq!(again["registersWritten"], 0);
    assert_eq!(note_count(&b), 2);
}

#[test]
fn a_deck_file_is_imported_not_restored() {
    let (a, deck) = core_with_notes();
    let (summary, bytes) = export(&a, Some(&deck), false);
    assert_eq!(
        summary["info"]["scope"],
        json!({ "kind": "deck", "id": deck, "name": "Polish" })
    );
    assert_eq!(summary["info"]["history"], false);

    let b = core();
    let err = send(&b, "restoreBackup", Value::Null, Some(&bytes)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    assert!(err.message.contains("import it instead"), "{}", err.message);
    assert_eq!(note_count(&b), 0);

    send(&b, "importBackup", Value::Null, Some(&bytes)).unwrap();
    assert_eq!(note_count(&b), 1);
    // A second import adds nothing.
    let again = send(&b, "importBackup", Value::Null, Some(&bytes))
        .unwrap()
        .output;
    assert_eq!(again["registersWritten"], 0);
    assert_eq!(note_count(&b), 1);
}

#[test]
fn bad_requests_get_readable_errors_and_change_nothing() {
    let (a, deck) = core_with_notes();
    let (_, good) = export(&a, None, true);
    let b = core();

    for method in ["restoreBackup", "importBackup", "readBackupInfo"] {
        let err = send(&b, method, Value::Null, None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput, "{method}");
        let err = send(&b, method, Value::Null, Some(b"not a zip")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput, "{method}");
        assert!(
            err.message.contains("Nothing was changed"),
            "{}",
            err.message
        );
    }
    // Cut short.
    let err = send(
        &b,
        "restoreBackup",
        Value::Null,
        Some(&good[..good.len() / 2]),
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    assert_eq!(note_count(&b), 0);

    let err = send(
        &a,
        "exportBackup",
        json!({ "deck": "nope", "history": true }),
        None,
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    let other_deck = Id::from_bytes([9; 16]).to_string();
    let err = send(
        &a,
        "exportBackup",
        json!({ "deck": other_deck, "history": true }),
        None,
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
    assert_ne!(other_deck, deck);

    let none = Core::new();
    for (method, bytes) in [
        ("exportBackup", None),
        ("restoreBackup", Some(&good[..])),
        ("getBackupSettings", None),
    ] {
        let input = if method == "exportBackup" {
            json!({ "deck": null, "history": true })
        } else {
            Value::Null
        };
        let err = send(&none, method, input, bytes).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound, "{method}");
        assert_eq!(err.message, "No collection is open.");
    }
}

#[test]
fn a_cancelled_call_does_nothing() {
    let (a, _) = core_with_notes();
    let (_, bytes) = export(&a, None, true);
    let b = core();
    let cancelled = || true;
    let ctx = OpContext::new(&cancelled).with_attachment(&bytes);
    let err = dispatch(&b, "restoreBackup", Value::Null, &ctx).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Cancelled);
    assert_eq!(note_count(&b), 0);
}

#[test]
fn the_settings_default_change_and_are_validated() {
    let core = core();
    let defaults = json!({ "intervalHours": 24, "keep": 5, "lastError": null });
    assert_eq!(
        send(&core, "getBackupSettings", Value::Null, None)
            .unwrap()
            .output,
        defaults
    );

    let set = send(
        &core,
        "setBackupSettings",
        json!({ "intervalHours": 0, "keep": 9 }),
        None,
    )
    .unwrap()
    .output;
    assert_eq!(
        set,
        json!({ "intervalHours": 0, "keep": 9, "lastError": null })
    );
    assert_eq!(
        send(&core, "getBackupSettings", Value::Null, None)
            .unwrap()
            .output,
        set
    );

    for input in [
        json!({ "intervalHours": 24, "keep": 0 }),
        json!({ "intervalHours": 24, "keep": 101 }),
        json!({ "intervalHours": 99999, "keep": 5 }),
    ] {
        let err = send(&core, "setBackupSettings", input, None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }
    assert_eq!(
        send(&core, "getBackupSettings", Value::Null, None)
            .unwrap()
            .output,
        set
    );
}

#[test]
fn a_restore_tells_the_ui_the_collection_changed() {
    use std::sync::Mutex;
    #[derive(Default)]
    struct Sink(Mutex<Vec<Notice>>);
    impl EventSink for Sink {
        fn send(&self, notice: Notice) {
            self.0.lock().unwrap().push(notice);
        }
    }
    let (a, _) = core_with_notes();
    let (_, bytes) = export(&a, None, true);
    let b = core();
    let sink = Arc::new(Sink::default());
    forward_events(&b, sink.clone());
    send(&b, "restoreBackup", Value::Null, Some(&bytes)).unwrap();
    let seen = sink.0.lock().unwrap();
    assert!(
        seen.iter().any(|n| matches!(
            n,
            Notice::Event {
                event: CoreEvent::MergeApplied { .. }
            }
        )),
        "{seen:?}"
    );
}

mod files {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("fc-api-backup-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str) -> String {
            self.0.join(name).to_str().unwrap().to_owned()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_backup_file_round_trips_and_matches_the_bytes_methods() {
        let dir = TempDir::new();
        let (a, deck) = core_with_notes();
        let path = dir.file("all.fcbackup");
        let out = send(
            &a,
            "exportBackupToFile",
            json!({ "path": path, "deck": null, "history": true }),
            None,
        )
        .unwrap();
        assert!(out.attachment.is_none());
        let (summary, bytes) = export(&a, None, true);
        assert_eq!(out.output, summary);
        // Both families write the same file.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        let b = core();
        let info = send(&b, "readBackupFileInfo", json!({ "path": path }), None).unwrap();
        assert_eq!(info.output, summary["info"]);
        send(&b, "restoreBackupFromFile", json!({ "path": path }), None).unwrap();
        assert_eq!(note_count(&b), 2);

        let deck_path = dir.file("deck.fcbackup");
        send(
            &a,
            "exportBackupToFile",
            json!({ "path": deck_path, "deck": deck, "history": false }),
            None,
        )
        .unwrap();
        let c = core();
        let err = send(
            &c,
            "restoreBackupFromFile",
            json!({ "path": deck_path }),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        send(
            &c,
            "importBackupFromFile",
            json!({ "path": deck_path }),
            None,
        )
        .unwrap();
        assert_eq!(note_count(&c), 1);
    }

    #[test]
    fn an_export_never_replaces_a_file_and_never_leaves_a_partial_one() {
        let dir = TempDir::new();
        let (a, _) = core_with_notes();
        let taken = dir.file("taken.fcbackup");
        std::fs::write(&taken, b"mine").unwrap();
        let err = send(
            &a,
            "exportBackupToFile",
            json!({ "path": taken, "deck": null, "history": true }),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("exists already"), "{}", err.message);
        assert_eq!(std::fs::read(&taken).unwrap(), b"mine");

        // A deck that is not there fails after the file was started.
        let failed = dir.file("failed.fcbackup");
        let err = send(
            &a,
            "exportBackupToFile",
            json!({ "path": failed, "deck": Id::from_bytes([9; 16]).to_string(), "history": true }),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        let left: Vec<_> = std::fs::read_dir(&dir.0).unwrap().collect();
        assert_eq!(left.len(), 1, "{left:?}");

        // A folder that is not there.
        let err = send(
            &a,
            "exportBackupToFile",
            json!({ "path": dir.file("no/such/folder.fcbackup"), "deck": null, "history": true }),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Unavailable);
    }

    #[test]
    fn reading_a_file_that_is_missing_or_not_a_backup_is_a_readable_error() {
        let dir = TempDir::new();
        let b = core();
        for method in [
            "readBackupFileInfo",
            "restoreBackupFromFile",
            "importBackupFromFile",
        ] {
            let err = send(
                &b,
                method,
                json!({ "path": dir.file("none.fcbackup") }),
                None,
            )
            .unwrap_err();
            assert_eq!(err.kind, ErrorKind::NotFound, "{method}");
            assert!(err.message.contains("There is no file"), "{}", err.message);

            let junk = dir.file("junk.fcbackup");
            std::fs::write(&junk, b"junk").unwrap();
            let err = send(&b, method, json!({ "path": junk }), None).unwrap_err();
            assert_eq!(err.kind, ErrorKind::InvalidInput, "{method}");
        }
        assert_eq!(note_count(&b), 0);
    }

    #[test]
    fn listing_backing_up_now_and_restoring_from_the_list() {
        let dir = TempDir::new();
        let clock = Arc::new(ManualClock::new(T0));
        let core = Core::new();
        let host = Host {
            clock: clock.clone(),
            installation_id: Id::from_bytes([1; 16]),
        };
        core.open_collection(":memory:", host).unwrap();
        // No folder set: a readable error.
        let error = send(&core, "listBackups", Value::Null, None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unavailable);
        core.set_backup_dir(dir.0.join("backups"));
        let listed = |core: &Core| {
            send(core, "listBackups", Value::Null, None).unwrap().output["backups"]
                .as_array()
                .unwrap()
                .clone()
        };
        assert!(listed(&core).is_empty());

        let add = |kind: &str| {
            core.with_collection(|c| {
                let nt = c.note_type(builtin::basic()).unwrap().unwrap();
                c.add_note(
                    builtin::basic(),
                    &[(nt.fields[0].id, kind), (nt.fields[1].id, "x")],
                )
                .unwrap();
            })
            .unwrap();
        };
        add("one");
        let first = send(&core, "backupNow", Value::Null, None).unwrap().output;
        assert_eq!(first["restorable"], true);
        assert_eq!(first["createdMs"], T0 as f64);
        // The same second again would be the same file name.
        assert!(send(&core, "backupNow", Value::Null, None).is_err());
        clock.advance(2_000);
        add("two");
        send(&core, "backupNow", Value::Null, None).unwrap();
        let backups = listed(&core);
        assert_eq!(backups.len(), 2);
        assert_eq!(backups[1]["name"], first["name"]);
        assert_eq!(note_count(&core), 2);

        // A name that is not in the listing is refused, a path most of all.
        for name in ["../x.fcbackup", "nothing.fcbackup"] {
            let error =
                send(&core, "restoreListedBackup", json!({ "name": name }), None).unwrap_err();
            assert_eq!(error.kind, ErrorKind::NotFound);
        }

        clock.advance(2_000);
        send(
            &core,
            "restoreListedBackup",
            json!({ "name": first["name"] }),
            None,
        )
        .unwrap();
        assert_eq!(note_count(&core), 1);
        // The restore first made a backup of the state it replaced.
        assert_eq!(listed(&core).len(), 3);
        let undo = listed(&core)[0]["name"].clone();
        send(&core, "restoreListedBackup", json!({ "name": undo }), None).unwrap();
        assert_eq!(note_count(&core), 2);
    }

    #[test]
    fn keep_limits_the_list_and_a_damaged_file_is_not_restorable() {
        let dir = TempDir::new();
        let clock = Arc::new(ManualClock::new(T0));
        let core = Core::new();
        let host = Host {
            clock: clock.clone(),
            installation_id: Id::from_bytes([1; 16]),
        };
        core.open_collection(":memory:", host).unwrap();
        let backups = dir.0.join("backups");
        core.set_backup_dir(backups.clone());
        send(
            &core,
            "setBackupSettings",
            json!({ "intervalHours": 24, "keep": 2 }),
            None,
        )
        .unwrap();
        for _ in 0..3 {
            send(&core, "backupNow", Value::Null, None).unwrap();
            clock.advance(2_000);
        }
        let list = |core: &Core| send(core, "listBackups", Value::Null, None).unwrap().output;
        assert_eq!(list(&core)["backups"].as_array().unwrap().len(), 2);
        std::fs::write(backups.join("backup-2099-01-01-000000.fcbackup"), b"junk").unwrap();
        let entries = list(&core)["backups"].clone();
        assert_eq!(entries[0]["restorable"], false);
        assert_eq!(entries[0]["createdMs"], Value::Null);
        let error = send(
            &core,
            "restoreListedBackup",
            json!({ "name": entries[0]["name"] }),
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidInput);
    }
}
