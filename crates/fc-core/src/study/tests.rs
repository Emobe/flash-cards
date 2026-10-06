use std::sync::Arc;

use rusqlite::params;
use rusqlite::types::Value;

use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, MIGRATIONS, Schema};
use crate::deck::{PresetChange, default_preset};
use crate::sync::{SYNCED_TABLES, check_schema};

const START: i64 = 1_700_000_000_000;

fn host(clock: &Arc<ManualClock>) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes(*b"installation-one"),
    }
}

fn collection() -> Collection {
    let clock = Arc::new(ManualClock::new(START));
    Collection::create(":memory:", host(&clock)).unwrap()
}

/// All 21 columns of a card event, for tests that write one by hand.
fn event_values(card: Id) -> Vec<(&'static str, Value)> {
    CARD_EVENT
        .columns
        .iter()
        .map(|column| {
            let value = match *column {
                "card" => Value::Blob(card.as_bytes().to_vec()),
                "kind" => Value::Text("review".to_owned()),
                "time_ms" | "day" | "utc_offset" => Value::Integer(0),
                "device" => Value::Blob(vec![0; 16]),
                _ => Value::Null,
            };
            (*column, value)
        })
        .collect()
}

fn event_id(n: u8) -> Id {
    Id::new_v7(START, &[n; 10])
}

// ---- Settings ----

#[test]
fn the_day_starts_at_midnight_unless_changed() {
    let c = collection();
    assert_eq!(c.day_start_hour().unwrap(), 0);
    c.set_day_start_hour(4).unwrap();
    assert_eq!(c.day_start_hour().unwrap(), 4);
    c.set_day_start_hour(23).unwrap();
    assert_eq!(c.day_start_hour().unwrap(), 23);
    c.set_day_start_hour(0).unwrap();
    assert_eq!(c.day_start_hour().unwrap(), 0);
}

#[test]
fn a_start_hour_outside_the_day_is_refused() {
    let c = collection();
    assert_eq!(c.set_day_start_hour(24), Err(StudyError::StartHour));
    assert_eq!(c.day_start_hour().unwrap(), 0);
}

#[test]
fn setting_the_same_hour_again_writes_nothing() {
    let c = collection();
    c.set_day_start_hour(4).unwrap();
    let id = Id::new_v5(SETTING_NAMESPACE, b"day_start_hour");
    let first = c.register_clock("collection_setting", id, "value").unwrap();
    assert!(first.is_some());
    c.set_day_start_hour(4).unwrap();
    assert_eq!(
        c.register_clock("collection_setting", id, "value").unwrap(),
        first
    );
}

#[test]
fn a_setting_has_the_same_id_on_every_device() {
    let a = collection();
    let b = collection();
    a.set_day_start_hour(5).unwrap();
    b.set_day_start_hour(6).unwrap();
    let id_of = |c: &Collection| -> Id {
        c.conn
            .query_row("SELECT id FROM collection_setting", [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(id_of(&a), id_of(&b));
}

#[test]
fn a_stored_start_hour_that_is_not_an_hour_reads_as_midnight() {
    let c = collection();
    c.set_day_start_hour(4).unwrap();
    for bad in ["25", "-1", "noon", ""] {
        c.write(|w| {
            w.set(
                "collection_setting",
                Id::new_v5(SETTING_NAMESPACE, b"day_start_hour"),
                "value",
                Value::Text(bad.to_owned()),
            )
        })
        .unwrap();
        assert_eq!(c.day_start_hour().unwrap(), 0, "{bad:?}");
    }
}

// ---- Append-only tables ----

#[test]
fn an_event_is_added_through_the_write_path_and_listed_as_not_pushed() {
    let c = collection();
    let id = event_id(1);
    let added = c
        .write(|w| w.insert_row("card_event", id, event_values(event_id(9))))
        .unwrap();
    assert!(added);
    let count: i64 = c
        .conn
        .query_row("SELECT count(*) FROM card_event", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let unpushed: i64 = c
        .conn
        .query_row(
            "SELECT count(*) FROM unpushed_row WHERE entity_type = 'card_event' AND row_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(unpushed, 1);
}

#[test]
fn adding_the_same_event_again_changes_nothing() {
    let c = collection();
    let id = event_id(1);
    let first = c
        .write(|w| w.insert_row("card_event", id, event_values(event_id(9))))
        .unwrap();
    // The same ID with other content: the first row stands, as on a device that gets it twice.
    let mut other = event_values(event_id(8));
    other.iter_mut().find(|(k, _)| *k == "kind").unwrap().1 = Value::Text("void".to_owned());
    let second = c.write(|w| w.insert_row("card_event", id, other)).unwrap();
    assert!(first && !second);
    let kind: String = c
        .conn
        .query_row("SELECT kind FROM card_event", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kind, "review");
}

#[test]
fn an_event_cannot_be_inserted_outside_the_write_path() {
    let c = collection();
    let result = c.conn.execute(
        "INSERT INTO card_event (id, card, kind, time_ms, utc_offset, device, day)
         VALUES (?1, ?2, 'review', 0, 0, x'00', 0)",
        params![event_id(1), event_id(2)],
    );
    assert!(result.is_err());
    let result = c.conn.execute(
        "INSERT INTO fsrs_parameter_set (id, values_f32) VALUES (?1, x'00')",
        params![event_id(1)],
    );
    assert!(result.is_err());
}

#[test]
fn an_event_is_never_changed_or_deleted_even_inside_the_write_path() {
    let c = collection();
    let id = event_id(1);
    c.write(|w| w.insert_row("card_event", id, event_values(event_id(9))))
        .unwrap();
    let update = c.write(|w| {
        w.local()
            .execute("UPDATE card_event SET kind = 'void'", [])
            .map_err(Into::into)
    });
    assert!(update.is_err());
    let delete = c.write(|w| {
        w.local()
            .execute("DELETE FROM card_event", [])
            .map_err(Into::into)
    });
    assert!(delete.is_err());
    // And outside it.
    assert!(
        c.conn
            .execute("UPDATE card_event SET kind = 'void'", [])
            .is_err()
    );
    assert!(c.conn.execute("DELETE FROM card_event", []).is_err());
    assert!(
        c.conn
            .execute("UPDATE fsrs_parameter_set SET id = id", [])
            .is_ok()
    ); // no rows yet
    let kind: String = c
        .conn
        .query_row("SELECT kind FROM card_event", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kind, "review");
}

#[test]
fn a_parameter_set_row_is_immutable_too() {
    let c = collection();
    let id = event_id(3);
    c.write(|w| {
        w.insert_row(
            "fsrs_parameter_set",
            id,
            vec![("values_f32", Value::Blob(vec![1, 2, 3, 4]))],
        )
    })
    .unwrap();
    assert!(
        c.conn
            .execute("UPDATE fsrs_parameter_set SET values_f32 = x'00'", [])
            .is_err()
    );
    assert!(
        c.conn
            .execute("DELETE FROM fsrs_parameter_set", [])
            .is_err()
    );
}

#[test]
fn an_append_only_row_needs_every_column_and_no_others() {
    let c = collection();
    let mut missing = event_values(event_id(9));
    missing.pop();
    assert!(
        c.write(|w| w.insert_row("card_event", event_id(1), missing))
            .is_err()
    );
    let mut extra = event_values(event_id(9));
    extra.push(("flavour", Value::Null));
    assert!(
        c.write(|w| w.insert_row("card_event", event_id(1), extra))
            .is_err()
    );
    assert!(
        c.write(|w| w.insert_row("note", event_id(1), vec![]))
            .is_err()
    );
}

#[test]
fn the_schema_check_covers_the_new_tables() {
    let c = collection();
    check_schema(&c.conn, SYNCED_TABLES).unwrap();
    // A card_event column that the declaration does not know is a problem.
    let clock = Arc::new(ManualClock::new(START));
    let broken = Collection::create(":memory:", host(&clock)).unwrap();
    broken
        .conn
        .execute_batch("ALTER TABLE card_event ADD COLUMN surprise INTEGER")
        .unwrap();
    let problems = check_schema(&broken.conn, SYNCED_TABLES).unwrap_err();
    assert!(
        problems.iter().any(|p| p.contains("card_event")),
        "{problems:?}"
    );
    broken
        .conn
        .execute_batch("DROP TRIGGER fsrs_parameter_set_guard_update")
        .unwrap();
    let problems = check_schema(&broken.conn, SYNCED_TABLES).unwrap_err();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("fsrs_parameter_set_guard_update")),
        "{problems:?}"
    );
}

// ---- Preset registers ----

#[test]
fn the_default_preset_has_relearning_steps_and_default_parameters() {
    let c = collection();
    let preset = c.preset(default_preset()).unwrap().unwrap();
    assert_eq!(preset.relearning_steps, vec![10]);
    assert!(preset.fsrs_parameters.is_empty());
    assert_eq!(preset.learning_steps, vec![1, 10]);
}

#[test]
fn a_new_preset_has_relearning_steps_and_a_clock_for_every_register() {
    let c = collection();
    let id = c.create_preset("Languages").unwrap();
    let preset = c.preset(id).unwrap().unwrap();
    assert_eq!(preset.relearning_steps, vec![10]);
    assert!(preset.fsrs_parameters.is_empty());
    for field in ["relearning_steps", "fsrs_parameters"] {
        assert!(
            c.register_clock("options_preset", id, field)
                .unwrap()
                .is_some()
        );
        assert!(
            c.register_clock("options_preset", default_preset(), field)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn relearning_steps_change_like_learning_steps() {
    let c = collection();
    let id = c.create_preset("Languages").unwrap();
    c.set_preset_options(
        id,
        &PresetChange {
            relearning_steps: Some(vec![5, 30]),
            ..PresetChange::default()
        },
    )
    .unwrap();
    assert_eq!(c.preset(id).unwrap().unwrap().relearning_steps, vec![5, 30]);
    c.set_preset_options(
        id,
        &PresetChange {
            relearning_steps: Some(vec![]),
            ..PresetChange::default()
        },
    )
    .unwrap();
    assert!(c.preset(id).unwrap().unwrap().relearning_steps.is_empty());
    let refused = c.set_preset_options(
        id,
        &PresetChange {
            relearning_steps: Some(vec![0]),
            ..PresetChange::default()
        },
    );
    assert!(refused.is_err());
    assert!(c.preset(id).unwrap().unwrap().relearning_steps.is_empty());
}

fn set_parameters(c: &Collection, text: &str) -> Vec<f32> {
    c.write(|w| {
        w.set(
            "options_preset",
            default_preset(),
            "fsrs_parameters",
            Value::Text(text.to_owned()),
        )
    })
    .unwrap();
    c.preset(default_preset()).unwrap().unwrap().fsrs_parameters
}

#[test]
fn stored_fsrs_parameters_read_back_and_bad_ones_read_as_the_defaults() {
    let c = collection();
    let defaults = crate::scheduling::default_parameters();
    let text: Vec<String> = defaults.iter().map(f32::to_string).collect();
    assert_eq!(set_parameters(&c, &text.join(" ")), defaults);
    // Seventeen numbers (FSRS 4.5) are filled to 21 the way `fsrs` fills them.
    let old: Vec<String> = (0..17)
        .map(|n| (1.0 + n as f32 / 10.0).to_string())
        .collect();
    assert_eq!(set_parameters(&c, &old.join(" ")).len(), 21);
    // Wrong count, not numbers, not finite: the defaults.
    assert!(set_parameters(&c, "1 2 3").is_empty());
    assert!(set_parameters(&c, "a b c").is_empty());
    let nan: Vec<String> = (0..21).map(|_| "NaN".to_owned()).collect();
    assert!(set_parameters(&c, &nan.join(" ")).is_empty());
    assert!(set_parameters(&c, "").is_empty());
}

// ---- Upgrade ----

#[test]
fn a_version_6_collection_upgrades_and_keeps_its_presets() {
    const V6: Schema = Schema {
        migrations: &[
            MIGRATIONS[0],
            MIGRATIONS[1],
            MIGRATIONS[2],
            MIGRATIONS[3],
            MIGRATIONS[4],
            MIGRATIONS[5],
        ],
        tables: &[
            SYNCED_TABLES[0],
            SYNCED_TABLES[1],
            SYNCED_TABLES[2],
            SYNCED_TABLES[3],
            SYNCED_TABLES[4],
            SYNCED_TABLES[5],
            SYNCED_TABLES[6],
            SYNCED_TABLES[7],
        ],
    };
    let path = std::env::temp_dir().join(format!("fc-study-v6-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = Arc::new(ManualClock::new(START));
    let old = Collection::create_with(location, V6, host(&clock)).unwrap();
    // The current code cannot read presets in the old layout, so write one by hand.
    let mine = Id::new_v7(START, &[5; 10]);
    old.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    old.conn
        .execute(
            "INSERT INTO options_preset (id, name, new_per_day) VALUES (?1, 'Mine', 7)",
            [mine],
        )
        .unwrap();
    old.conn.execute("DELETE FROM write_guard", []).unwrap();
    assert!(
        old.conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE name = 'card_event'",
                [],
                |_| Ok(())
            )
            .is_err()
    );
    old.close().unwrap();

    let upgraded = Collection::open(location, host(&clock)).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 9);
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    let kept = upgraded.preset(mine).unwrap().unwrap();
    assert_eq!(kept.new_per_day, 7);
    assert_eq!(kept.relearning_steps, vec![10]);
    assert!(kept.fsrs_parameters.is_empty());
    assert_eq!(upgraded.day_start_hour().unwrap(), 0);
    // The upgraded Default preset has clocks for the new registers, so a sync can compare them.
    for field in ["relearning_steps", "fsrs_parameters"] {
        let clock = upgraded
            .register_clock("options_preset", default_preset(), field)
            .unwrap()
            .unwrap();
        assert!(clock.pushed);
    }
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&path);
}
