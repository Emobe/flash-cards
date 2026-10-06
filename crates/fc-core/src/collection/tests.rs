use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use rusqlite::{Connection, Transaction};

use super::migrate::{APPLICATION_ID, MIGRATIONS, Migration};
use super::*;
use crate::clock::ManualClock;

/// A database file in the temp directory, removed (with its journal) when dropped.
struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        Self(std::env::temp_dir().join(format!("fc-collection-{}-{n}.db", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn journal(&self) -> PathBuf {
        PathBuf::from(format!("{}-journal", self.path()))
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.journal());
    }
}

fn pragma(path: &str, name: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, name, |r| r.get(0))
        .unwrap()
}

// Test migrations. A "v1" collection has the real baseline plus nothing; "v2" adds a table and
// rewrites existing data, as a real migration would.

fn v1(tx: &Transaction) -> rusqlite::Result<()> {
    (MIGRATIONS[0].apply)(tx)
}

fn v2(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE extra (n INTEGER NOT NULL)")?;
    tx.execute("UPDATE meta SET value = upper(value)", [])?;
    Ok(())
}

fn v2_fails(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE half_done (n INTEGER)")?;
    tx.execute_batch("THIS IS NOT SQL")
}

const fn schema(migrations: &'static [Migration]) -> Schema {
    Schema {
        migrations,
        tables: &[],
    }
}

fn host() -> Host {
    Host {
        clock: Arc::new(ManualClock::new(1_700_000_000_000)),
        installation_id: Id::from_bytes(*b"installation-one"),
    }
}

const V1: Schema = schema(&[Migration {
    version: 1,
    apply: v1,
}]);
const V2: Schema = schema(&[
    Migration {
        version: 1,
        apply: v1,
    },
    Migration {
        version: 2,
        apply: v2,
    },
]);
const V2_FAILING: Schema = schema(&[
    Migration {
        version: 1,
        apply: v1,
    },
    Migration {
        version: 2,
        apply: v2_fails,
    },
]);

#[test]
fn migrations_start_at_1_and_have_no_gaps() {
    for (index, migration) in MIGRATIONS.iter().enumerate() {
        assert_eq!(migration.version as usize, index + 1);
    }
}

#[test]
fn a_created_collection_can_be_closed_and_reopened() {
    let db = TempDb::new();
    let collection = Collection::create(db.path(), host()).unwrap();
    let info = collection.info().unwrap();
    assert_eq!(info.schema_version, latest(MIGRATIONS));
    assert_eq!(info.supported_schema_version, latest(MIGRATIONS));
    assert_eq!(info.created_by, crate::version());
    assert!(info.unsupported_features.is_empty());
    collection.close().unwrap();

    let reopened = Collection::open(db.path(), host()).unwrap();
    assert_eq!(reopened.info().unwrap(), info);
    reopened.close().unwrap();
    assert_eq!(
        pragma(db.path(), "application_id"),
        i64::from(APPLICATION_ID)
    );
}

#[test]
fn creating_over_an_existing_collection_is_refused_and_keeps_its_data() {
    let db = TempDb::new();
    let collection = Collection::create(db.path(), host()).unwrap();
    collection
        .conn
        .execute("INSERT INTO meta VALUES ('kept', 'yes')", [])
        .unwrap();
    collection.close().unwrap();

    assert_eq!(
        Collection::create(db.path(), host()).unwrap_err(),
        CollectionError::AlreadyExists
    );
    let reopened = Collection::open(db.path(), host()).unwrap();
    let kept: String = reopened
        .conn
        .query_row("SELECT value FROM meta WHERE key = 'kept'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kept, "yes");
}

#[test]
fn opening_a_missing_collection_is_not_found_and_creates_nothing() {
    let db = TempDb::new();
    assert_eq!(
        Collection::open(db.path(), host()).unwrap_err(),
        CollectionError::NotFound
    );
    assert!(!db.0.exists());
}

#[test]
fn open_or_create_creates_once_then_opens() {
    let db = TempDb::new();
    let first = Collection::open_or_create(db.path(), host()).unwrap();
    first
        .conn
        .execute("INSERT INTO meta VALUES ('k', 'v')", [])
        .unwrap();
    first.close().unwrap();
    let second = Collection::open_or_create(db.path(), host()).unwrap();
    let count: u32 = second
        .conn
        .query_row("SELECT count(*) FROM meta WHERE key = 'k'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn an_in_memory_collection_works() {
    let collection = Collection::create(":memory:", host()).unwrap();
    assert_eq!(
        collection.info().unwrap().schema_version,
        latest(MIGRATIONS)
    );
}

#[test]
fn a_file_that_is_not_a_database_is_not_a_collection() {
    let db = TempDb::new();
    std::fs::write(
        &db.0,
        b"This is just some text, not a SQLite database at all.",
    )
    .unwrap();
    let before = std::fs::read(&db.0).unwrap();
    assert_eq!(
        Collection::open(db.path(), host()).unwrap_err(),
        CollectionError::NotACollection
    );
    assert_eq!(
        Collection::create(db.path(), host()).unwrap_err(),
        CollectionError::NotACollection
    );
    assert_eq!(std::fs::read(&db.0).unwrap(), before);
}

#[test]
fn another_sqlite_database_is_not_a_collection_and_is_left_alone() {
    let db = TempDb::new();
    Connection::open(db.path())
        .unwrap()
        .execute_batch("CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('mine')")
        .unwrap();
    let before = std::fs::read(&db.0).unwrap();
    assert_eq!(
        Collection::open(db.path(), host()).unwrap_err(),
        CollectionError::NotACollection
    );
    assert_eq!(
        Collection::create(db.path(), host()).unwrap_err(),
        CollectionError::NotACollection
    );
    assert_eq!(std::fs::read(&db.0).unwrap(), before);
}

#[test]
fn a_newer_collection_is_refused_without_being_modified() {
    let db = TempDb::new();
    Collection::create(db.path(), host())
        .unwrap()
        .close()
        .unwrap();
    Connection::open(db.path())
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let before = std::fs::read(&db.0).unwrap();

    let error = Collection::open(db.path(), host()).unwrap_err();
    assert_eq!(
        error,
        CollectionError::TooNew {
            found: 99,
            supported: latest(MIGRATIONS)
        }
    );
    assert!(error.to_string().contains("Update the app"));
    assert_eq!(std::fs::read(&db.0).unwrap(), before);
    assert!(!db.journal().exists());
}

#[test]
fn an_older_collection_is_upgraded_and_keeps_its_data() {
    let db = TempDb::new();
    let old = Collection::create_with(db.path(), V1, host()).unwrap();
    assert_eq!(old.schema_version, 1);
    old.conn
        .execute("INSERT INTO meta VALUES ('word', 'czesc')", [])
        .unwrap();
    old.close().unwrap();
    assert_eq!(pragma(db.path(), "user_version"), 1);

    let upgraded = Collection::open_with(db.path(), V2, host()).unwrap();
    assert_eq!(upgraded.schema_version, 2);
    assert_eq!(pragma(db.path(), "user_version"), 2);
    // The migration ran (new table, rewritten data) and the old data survived.
    upgraded
        .conn
        .execute("INSERT INTO extra VALUES (1)", [])
        .unwrap();
    let word: String = upgraded
        .conn
        .query_row("SELECT value FROM meta WHERE key = 'word'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(word, "CZESC");
}

#[test]
fn a_collection_that_is_current_is_not_migrated_again() {
    let db = TempDb::new();
    Collection::create_with(db.path(), V2, host())
        .unwrap()
        .close()
        .unwrap();
    // `v2` upper-cases `meta`, so a second run would be visible: it would fail on `CREATE TABLE`.
    Collection::open_with(db.path(), V2, host())
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn a_failing_migration_leaves_the_old_collection_untouched() {
    let db = TempDb::new();
    Collection::create_with(db.path(), V1, host())
        .unwrap()
        .close()
        .unwrap();
    let before = std::fs::read(&db.0).unwrap();

    let error = Collection::open_with(db.path(), V2_FAILING, host()).unwrap_err();
    assert!(matches!(error, CollectionError::Storage(_)));
    assert_eq!(std::fs::read(&db.0).unwrap(), before);
    assert_eq!(pragma(db.path(), "user_version"), 1);

    // Still a working v1 collection, and a later fixed migration succeeds.
    Collection::open_with(db.path(), V1, host())
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(
        Collection::open_with(db.path(), V2, host())
            .unwrap()
            .schema_version,
        2
    );
}

#[test]
fn a_failing_first_migration_leaves_an_empty_file_that_can_be_created_again() {
    let db = TempDb::new();
    let failing: &'static [Migration] = &[Migration {
        version: 1,
        apply: v2_fails,
    }];
    assert!(matches!(
        Collection::create_with(db.path(), schema(failing), host()).unwrap_err(),
        CollectionError::Storage(_)
    ));
    assert_eq!(pragma(db.path(), "application_id"), 0);
    assert_eq!(pragma(db.path(), "user_version"), 0);
    Collection::create(db.path(), host())
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn a_collection_open_elsewhere_is_in_use() {
    let db = TempDb::new();
    let first = Collection::create(db.path(), host()).unwrap();
    // Hold a write lock, as another program in the middle of a write would.
    first.conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert_eq!(
        Collection::open(db.path(), host()).unwrap_err(),
        CollectionError::InUse
    );
    first.conn.execute_batch("ROLLBACK").unwrap();
    Collection::open(db.path(), host()).unwrap();
}
