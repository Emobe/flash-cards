use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use rusqlite::Transaction;
use rusqlite::types::Value;

use super::requires::TABLE as REQUIREMENT;
use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, CollectionError, MIGRATIONS, Migration};
use crate::id::Id;

// A test table, as step 1.2 and later steps will add real ones.

const ITEM: SyncedTable = SyncedTable {
    entity: "item",
    table: "test_item",
    registers: &["title", "body"],
};

fn v3(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE test_item (
            id BLOB PRIMARY KEY NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT ''
        ) WITHOUT ROWID",
    )?;
    install_guard(tx, &ITEM)
}

const WITH_ITEM: crate::collection::Schema = crate::collection::Schema {
    migrations: &[
        MIGRATIONS[0],
        MIGRATIONS[1],
        Migration {
            version: 3,
            apply: v3,
        },
    ],
    tables: &[REQUIREMENT, ITEM],
};

struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        Self(std::env::temp_dir().join(format!("fc-sync-{}-{n}.db", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(format!("{}-journal", self.path()));
    }
}

const START: i64 = 1_700_000_000_000;

fn host(clock: &Arc<ManualClock>, installation: &[u8; 16]) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes(*installation),
    }
}

fn new_collection(clock: &Arc<ManualClock>) -> Collection {
    Collection::create_with(":memory:", WITH_ITEM, host(clock, b"installation-one")).unwrap()
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn add_item(collection: &Collection, title: &str) -> Id {
    collection
        .write(|w| {
            let id = w.new_id()?;
            w.insert(
                "item",
                id,
                vec![("title", text(title)), ("body", text("body"))],
            )?;
            Ok(id)
        })
        .unwrap()
}

fn raw_error(collection: &Collection, sql: &str) -> String {
    collection.conn.execute(sql, []).unwrap_err().to_string()
}

// The schema checker: every table is classified, every column is a register.

#[test]
fn the_real_schema_has_every_table_classified() {
    let clock = Arc::new(ManualClock::new(START));
    let collection = Collection::create(":memory:", host(&clock, b"installation-one")).unwrap();
    check_schema(&collection.conn, SYNCED_TABLES).unwrap();
}

#[test]
fn the_test_schema_passes_the_checker() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    check_schema(&collection.conn, WITH_ITEM.tables).unwrap();
}

#[test]
fn the_checker_fails_for_a_table_nobody_classified() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    collection
        .conn
        .execute_batch("CREATE TABLE note (id BLOB PRIMARY KEY, front TEXT)")
        .unwrap();
    let problems = check_schema(&collection.conn, WITH_ITEM.tables).unwrap_err();
    assert!(
        problems.iter().any(|p| p.contains("`note`")),
        "{problems:?}"
    );
}

#[test]
fn the_checker_fails_for_a_column_that_is_not_a_register() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    collection
        .conn
        .execute_batch("ALTER TABLE test_item ADD COLUMN sneaky TEXT")
        .unwrap();
    let problems = check_schema(&collection.conn, WITH_ITEM.tables).unwrap_err();
    assert!(
        problems.iter().any(|p| p.contains("sneaky")),
        "{problems:?}"
    );
}

#[test]
fn the_checker_fails_for_a_synced_table_without_its_guard() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    collection
        .conn
        .execute_batch("DROP TRIGGER test_item_guard_update")
        .unwrap();
    let problems = check_schema(&collection.conn, WITH_ITEM.tables).unwrap_err();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("test_item_guard_update")),
        "{problems:?}"
    );
}

// The write path.

#[test]
fn a_write_records_the_clock_of_every_register_it_changes() {
    let clock = Arc::new(ManualClock::new(START));
    let collection = new_collection(&clock);
    let device = collection.device_id().unwrap();
    let id = add_item(&collection, "one");

    let title = collection
        .register_clock("item", id, "title")
        .unwrap()
        .unwrap();
    let body = collection
        .register_clock("item", id, "body")
        .unwrap()
        .unwrap();
    assert_eq!(title.device, device);
    assert_eq!(body.device, device);
    assert!(!title.pushed && !body.pushed);
    assert_eq!(title.hlc.unix_ms(), START as u64);
    assert!(body.hlc > title.hlc, "each register gets its own stamp");

    clock.advance(10);
    collection
        .write(|w| w.set("item", id, "title", text("two")))
        .unwrap();
    let changed = collection
        .register_clock("item", id, "title")
        .unwrap()
        .unwrap();
    assert!(changed.hlc > body.hlc);
    assert_eq!(changed.hlc.unix_ms(), (START + 10) as u64);
    let untouched = collection
        .register_clock("item", id, "body")
        .unwrap()
        .unwrap();
    assert_eq!(untouched, body);
}

#[test]
fn a_new_write_marks_the_register_as_not_pushed_again() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    let id = add_item(&collection, "one");
    collection
        .conn
        .execute("UPDATE register_clock SET pushed = 1", [])
        .unwrap();
    assert!(
        collection
            .register_clock("item", id, "title")
            .unwrap()
            .unwrap()
            .pushed
    );
    collection
        .write(|w| w.set("item", id, "title", text("two")))
        .unwrap();
    assert!(
        !collection
            .register_clock("item", id, "title")
            .unwrap()
            .unwrap()
            .pushed
    );
}

#[test]
fn every_row_of_every_synced_table_has_a_clock_for_every_register() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    add_item(&collection, "one");
    add_item(&collection, "two");
    collection.require_feature("future-feature").unwrap();
    for table in WITH_ITEM.tables {
        for register in table.registers {
            let missing: i64 = collection
                .conn
                .query_row(
                    &format!(
                        "SELECT count(*) FROM \"{}\" t WHERE NOT EXISTS (
                           SELECT 1 FROM register_clock c
                           WHERE c.entity_type = ?1 AND c.entity_id = t.id AND c.field = ?2)",
                        table.table
                    ),
                    [table.entity, register],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(missing, 0, "{}.{register}", table.entity);
        }
    }
}

#[test]
fn a_failed_write_leaves_neither_data_nor_clocks_nor_a_moved_hlc() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    let before = collection.hlc().unwrap();
    let result: Result<(), CollectionError> = collection.write(|w| {
        let id = w.new_id()?;
        w.insert("item", id, vec![("title", text("x")), ("body", text("y"))])?;
        w.set("item", id, "nope", text("z"))
    });
    assert!(result.is_err());
    let count = |table: &str| -> i64 {
        collection
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count("test_item"), 0);
    assert_eq!(count("register_clock"), 0);
    assert_eq!(count("write_guard"), 0);
    assert_eq!(collection.hlc().unwrap(), before);
    // And the collection is still usable.
    add_item(&collection, "after");
}

#[test]
fn raw_sql_cannot_write_a_synced_table() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    let id = add_item(&collection, "one");
    // Row triggers fire per row, so there must be a row to protect.
    collection.require_feature("f").unwrap();
    let guarded = "outside the write path";
    for sql in [
        "INSERT INTO test_item (id, title, body) VALUES (x'00', 'a', 'b')",
        "UPDATE test_item SET title = 'changed'",
        "INSERT INTO requirement (id, feature, active) VALUES (x'00', 'f', 1)",
        "UPDATE requirement SET active = 0",
    ] {
        assert!(raw_error(&collection, sql).contains(guarded), "{sql}");
    }
    assert!(raw_error(&collection, "DELETE FROM test_item").contains("never hard-deleted"));
    assert!(raw_error(&collection, "DELETE FROM requirement").contains("never hard-deleted"));
    // Nothing changed.
    let title: String = collection
        .conn
        .query_row("SELECT title FROM test_item WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(title, "one");
}

#[test]
fn the_guard_is_closed_again_after_a_write() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    add_item(&collection, "one");
    assert!(
        raw_error(&collection, "UPDATE test_item SET title = 'x'")
            .contains("outside the write path")
    );
}

#[test]
fn the_write_path_rejects_mistakes_instead_of_writing_unclocked_data() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    let id = add_item(&collection, "one");
    let fails = |f: &dyn Fn(&mut WriteTx<'_>) -> Result<(), CollectionError>| {
        assert!(collection.write(|w| f(w)).is_err());
    };
    let other = Id::from_bytes([9; 16]);
    fails(&|w| w.insert("item", other, vec![("title", text("only"))]));
    fails(&|w| {
        w.insert(
            "item",
            other,
            vec![
                ("title", text("a")),
                ("body", text("b")),
                ("extra", text("c")),
            ],
        )
    });
    fails(&|w| {
        w.insert(
            "item",
            other,
            vec![("title", text("a")), ("title", text("a"))],
        )
    });
    fails(&|w| w.insert("nothing", other, vec![]));
    fails(&|w| w.set("item", id, "extra", text("x")));
    fails(&|w| w.set("item", other, "title", text("x")));
    let count: i64 = collection
        .conn
        .query_row("SELECT count(*) FROM test_item", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

// The HLC.

#[test]
fn the_hlc_is_saved_and_never_runs_backwards_across_a_reopen() {
    let db = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    let collection =
        Collection::create_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    let id = add_item(&collection, "one");
    let first = collection
        .register_clock("item", id, "body")
        .unwrap()
        .unwrap()
        .hlc;
    assert_eq!(collection.hlc().unwrap(), first);
    collection.close().unwrap();

    // The user puts the clock back by a day.
    clock.set(START - 86_400_000);
    let reopened =
        Collection::open_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    assert_eq!(reopened.hlc().unwrap(), first);
    reopened
        .write(|w| w.set("item", id, "title", text("two")))
        .unwrap();
    let second = reopened
        .register_clock("item", id, "title")
        .unwrap()
        .unwrap()
        .hlc;
    assert!(second > first);
}

#[test]
fn writes_after_receiving_a_change_are_stamped_after_it() {
    let clock = Arc::new(ManualClock::new(START));
    let collection = new_collection(&clock);
    let ahead = Hlc::from_parts(START + 3_600_000, 5);
    collection.observe_hlc(ahead).unwrap();
    let id = add_item(&collection, "one");
    assert!(
        collection
            .register_clock("item", id, "title")
            .unwrap()
            .unwrap()
            .hlc
            > ahead
    );
    // Observing something older changes nothing.
    let last = collection.hlc().unwrap();
    collection.observe_hlc(Hlc::from_parts(START, 0)).unwrap();
    assert_eq!(collection.hlc().unwrap(), last);
}

// The device ID.

#[test]
fn a_new_collection_has_a_device_id_that_survives_reopening() {
    let db = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    let collection =
        Collection::create_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    let device = collection.device_id().unwrap();
    assert_eq!(device.as_bytes()[6] >> 4, 7);
    assert_eq!(collection.info().unwrap().device_id, device);
    collection.close().unwrap();

    let reopened =
        Collection::open_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    assert_eq!(reopened.device_id().unwrap(), device);
}

#[test]
fn a_copied_collection_gets_a_new_device_id() {
    let original = TempDb::new();
    let copy = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    let collection = Collection::create_with(
        original.path(),
        WITH_ITEM,
        host(&clock, b"installation-one"),
    )
    .unwrap();
    let id = add_item(&collection, "one");
    let device = collection.device_id().unwrap();
    collection.close().unwrap();
    std::fs::copy(&original.0, &copy.0).unwrap();

    // The copy is opened by another installation (another machine).
    let elsewhere =
        Collection::open_with(copy.path(), WITH_ITEM, host(&clock, b"installation-two")).unwrap();
    let new_device = elsewhere.device_id().unwrap();
    assert_ne!(new_device, device);
    elsewhere
        .write(|w| w.set("item", id, "title", text("edited on the copy")))
        .unwrap();
    let clock_row = elsewhere
        .register_clock("item", id, "title")
        .unwrap()
        .unwrap();
    assert_eq!(clock_row.device, new_device);
    // Old writes keep the old device: history is not rewritten.
    let old = elsewhere
        .register_clock("item", id, "body")
        .unwrap()
        .unwrap();
    assert_eq!(old.device, device);
    elsewhere.close().unwrap();

    // The original, still on its own installation, keeps its ID.
    let again = Collection::open_with(
        original.path(),
        WITH_ITEM,
        host(&clock, b"installation-one"),
    )
    .unwrap();
    assert_eq!(again.device_id().unwrap(), device);
}

#[test]
fn a_restore_or_import_can_regenerate_the_device_id() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    let before = collection.device_id().unwrap();
    let after = collection.regenerate_device_id().unwrap();
    assert_ne!(before, after);
    assert_eq!(collection.device_id().unwrap(), after);
    let id = add_item(&collection, "one");
    assert_eq!(
        collection
            .register_clock("item", id, "title")
            .unwrap()
            .unwrap()
            .device,
        after
    );
}

#[test]
fn a_1_1a_collection_is_upgraded_and_gets_a_device_id() {
    let db = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    const BASELINE: crate::collection::Schema = crate::collection::Schema {
        migrations: &[MIGRATIONS[0]],
        tables: &[],
    };
    let baseline = BASELINE;
    let old =
        Collection::create_with(db.path(), baseline, host(&clock, b"installation-one")).unwrap();
    old.conn
        .execute("INSERT INTO meta VALUES ('word', 'czesc')", [])
        .unwrap();
    // A real 1.1a file never had these (the baseline schema here is only the same tables).
    old.conn
        .execute(
            "DELETE FROM meta WHERE key IN ('device_id', 'installation_id')",
            [],
        )
        .unwrap();
    assert!(old.device_id().is_err());
    old.close().unwrap();

    let upgraded = Collection::open(db.path(), host(&clock, b"installation-one")).unwrap();
    let info = upgraded.info().unwrap();
    assert_eq!(info.schema_version, info.supported_schema_version);
    assert!(upgraded.device_id().is_ok());
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    let word: String = upgraded
        .conn
        .query_row("SELECT value FROM meta WHERE key = 'word'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(word, "czesc");
    // And it can be written through the real write path.
    upgraded.require_feature("future-feature").unwrap();
}

// The unknown-data store.

fn unknown(field: &str, value: Value, hlc: Hlc, device: u8) -> UnknownRegister {
    UnknownRegister {
        entity_type: "addon.example.thing".to_owned(),
        entity_id: Id::from_bytes([5; 16]),
        field: field.to_owned(),
        value,
        hlc,
        device: Id::from_bytes([device; 16]),
    }
}

#[test]
fn unknown_registers_are_kept_exactly_as_received() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    let hlc = Hlc::from_parts(START, 0);
    let values = [
        ("text", text("zażółć")),
        ("int", Value::Integer(-7)),
        ("real", Value::Real(0.1)),
        ("blob", Value::Blob(vec![0, 1, 255])),
        ("null", Value::Null),
    ];
    for (field, value) in &values {
        assert!(
            collection
                .store_unknown_register(&unknown(field, value.clone(), hlc, 1))
                .unwrap()
        );
    }
    let stored = collection
        .unknown_registers("addon.example.thing", Id::from_bytes([5; 16]))
        .unwrap();
    for (field, value) in &values {
        let found = stored.iter().find(|r| &r.field == field).unwrap();
        assert_eq!(&found.value, value, "{field}");
    }
}

#[test]
fn unknown_registers_merge_the_same_in_any_order_and_twice() {
    let a = unknown("f", text("a"), Hlc::from_parts(100, 0), 1);
    let b = unknown("f", text("b"), Hlc::from_parts(200, 0), 2);
    // Same time, different devices: the higher device ID wins.
    let c = unknown("f", text("c"), Hlc::from_parts(200, 0), 3);
    let clock = Arc::new(ManualClock::new(START));
    let mut results = Vec::new();
    for order in [[&a, &b, &c], [&c, &b, &a], [&b, &a, &c], [&a, &c, &b]] {
        let collection = new_collection(&clock);
        for register in order {
            collection.store_unknown_register(register).unwrap();
        }
        // Applying them all again, as an interrupted sync would, changes nothing.
        for register in order {
            assert!(!collection.store_unknown_register(register).unwrap());
        }
        results.push(
            collection
                .unknown_registers("addon.example.thing", Id::from_bytes([5; 16]))
                .unwrap(),
        );
    }
    assert!(results.iter().all(|r| r == &results[0]));
    assert_eq!(results[0][0].value, text("c"));
}

#[test]
fn storing_unknown_data_moves_the_hlc_and_survives_a_reopen() {
    let db = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    let collection =
        Collection::create_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    let ahead = Hlc::from_parts(START + 1_000_000, 0);
    collection
        .store_unknown_register(&unknown("f", text("x"), ahead, 9))
        .unwrap();
    assert_eq!(collection.hlc().unwrap(), ahead);
    collection.close().unwrap();
    let reopened =
        Collection::open_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    let stored = reopened
        .unknown_registers("addon.example.thing", Id::from_bytes([5; 16]))
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].value, text("x"));
}

#[test]
fn known_registers_are_not_accepted_as_unknown() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    assert!(collection.knows_register("item", "title"));
    assert!(
        !collection.knows_register("item", "colour"),
        "a new field of a known type"
    );
    assert!(!collection.knows_register("thing", "title"), "a new type");
    let mut known = unknown("title", text("x"), Hlc::from_parts(1, 0), 1);
    known.entity_type = "item".to_owned();
    assert!(collection.store_unknown_register(&known).is_err());
    // A new field of a known type is fine.
    known.field = "colour".to_owned();
    assert!(collection.store_unknown_register(&known).unwrap());
}

// Collection-level `requires`.

#[test]
fn a_collection_needing_an_unknown_feature_opens_and_reports_it() {
    let db = TempDb::new();
    let clock = Arc::new(ManualClock::new(START));
    let collection =
        Collection::create_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    assert!(collection.info().unwrap().unsupported_features.is_empty());
    collection.require_feature("sync-format-2").unwrap();
    collection.require_feature("alpha-feature").unwrap();
    assert_eq!(
        collection.info().unwrap().unsupported_features,
        ["alpha-feature", "sync-format-2"]
    );
    collection.close().unwrap();

    // It still opens: local study carries on, sync is what pauses (ADR 0006, section 10).
    let reopened =
        Collection::open_with(db.path(), WITH_ITEM, host(&clock, b"installation-one")).unwrap();
    assert_eq!(
        reopened.unsupported_features().unwrap(),
        ["alpha-feature", "sync-format-2"]
    );
    add_item(&reopened, "still works");
}

#[test]
fn requiring_a_feature_goes_through_the_write_path_and_is_idempotent() {
    let clock = Arc::new(ManualClock::new(START));
    let collection = new_collection(&clock);
    collection.require_feature("f").unwrap();
    let id = Id::new_v5(Id::from_bytes(*b"fc-requirement-1"), b"f");
    let first = collection
        .register_clock("requirement", id, "active")
        .unwrap()
        .unwrap();
    assert_eq!(first.device, collection.device_id().unwrap());
    assert!(
        collection
            .register_clock("requirement", id, "feature")
            .unwrap()
            .is_some()
    );
    clock.advance(5);
    collection.require_feature("f").unwrap();
    let second = collection
        .register_clock("requirement", id, "active")
        .unwrap()
        .unwrap();
    assert_eq!(first, second, "already required: nothing to write");
    assert_eq!(collection.unsupported_features().unwrap(), ["f"]);
}

#[test]
fn a_requirement_row_that_arrived_without_its_name_is_ignored() {
    let collection = new_collection(&Arc::new(ManualClock::new(START)));
    collection
        .conn
        .execute_batch(
            "INSERT INTO write_guard VALUES (1);
             INSERT INTO requirement (id, active) VALUES (x'01020304050607080910111213141516', 1);
             DELETE FROM write_guard;",
        )
        .unwrap();
    assert!(collection.unsupported_features().unwrap().is_empty());
}
