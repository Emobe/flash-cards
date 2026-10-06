use std::sync::Arc;

use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, MIGRATIONS, Schema};
use crate::notetype::builtin;
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

use crate::id::Id;

fn note(c: &Collection, front: &str) -> Id {
    let note_type = c.note_type(builtin::basic()).unwrap().unwrap();
    let f: Vec<Id> = note_type.fields.iter().map(|f| f.id).collect();
    c.add_note(builtin::basic(), &[(f[0], front), (f[1], "back")])
        .unwrap()
        .id
}

/// Note IDs made in the same millisecond are in no particular order, so compare sorted.
fn sorted(mut ids: Vec<Id>) -> Vec<Id> {
    ids.sort();
    ids
}

fn names(tags: &[Tag]) -> Vec<&str> {
    tags.iter().map(|t| t.name.as_str()).collect()
}

fn remote_device() -> Id {
    Id::from_bytes(*b"remote-device-01")
}

/// What a merge will do in step 1.11: a `(note, tag)` register as it was written on another device
/// at time `at` (ms), so a later `at` is a later write.
fn remote_tag(c: &Collection, note: Id, tag: &str, present: bool, at: i64) {
    c.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    c.conn
        .execute(
            "INSERT INTO note_tag (note, tag, present) VALUES (?1, ?2, ?3)
             ON CONFLICT (note, tag) DO UPDATE SET present = excluded.present",
            rusqlite::params![note, tag, if present { "1" } else { "" }],
        )
        .unwrap();
    c.conn.execute("DELETE FROM write_guard", []).unwrap();
    c.conn
        .execute(
            "INSERT OR REPLACE INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES ('note_tag', ?1, ?2, ?3, ?4, 1)",
            rusqlite::params![note, tag, at << 16, remote_device()],
        )
        .unwrap();
}

fn stored(c: &Collection, note: Id) -> Vec<(String, String)> {
    let mut statement = c
        .conn
        .prepare("SELECT tag, present FROM note_tag WHERE note = ?1 ORDER BY tag")
        .unwrap();
    statement
        .query_map([note], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

// Adding, removing, setting.

#[test]
fn a_note_starts_with_no_tags_and_gains_and_loses_them() {
    let c = collection();
    let n = note(&c, "a");
    assert!(c.note_tags(n).unwrap().is_empty());
    assert_eq!(c.add_tags(&[n], &["verbs", "polish"]).unwrap(), 2);
    assert_eq!(c.note_tags(n).unwrap(), ["polish", "verbs"]);
    assert_eq!(c.remove_tags(&[n], &["verbs"]).unwrap(), 1);
    assert_eq!(c.note_tags(n).unwrap(), ["polish"]);
}

#[test]
fn adding_a_tag_a_note_has_writes_nothing() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["polish"]).unwrap();
    let before = c.register_clock("note_tag", n, "polish").unwrap();
    assert_eq!(c.add_tags(&[n], &["polish", "POLISH"]).unwrap(), 0);
    assert_eq!(c.register_clock("note_tag", n, "polish").unwrap(), before);
    assert_eq!(stored(&c, n).len(), 1);
}

#[test]
fn removing_a_tag_a_note_lacks_writes_nothing() {
    let c = collection();
    let n = note(&c, "a");
    assert_eq!(c.remove_tags(&[n], &["polish"]).unwrap(), 0);
    assert!(stored(&c, n).is_empty());
    assert!(c.register_clock("note_tag", n, "polish").unwrap().is_none());
}

#[test]
fn removing_keeps_the_row_as_absent_and_a_later_add_brings_it_back() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["polish"]).unwrap();
    c.remove_tags(&[n], &["polish"]).unwrap();
    assert_eq!(stored(&c, n), [("polish".to_owned(), String::new())]);
    c.add_tags(&[n], &["polish"]).unwrap();
    assert_eq!(c.note_tags(n).unwrap(), ["polish"]);
}

#[test]
fn removing_a_parent_does_not_remove_what_is_inside_it() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["lang", "lang::polish"]).unwrap();
    c.remove_tags(&[n], &["lang"]).unwrap();
    assert_eq!(c.note_tags(n).unwrap(), ["lang::polish"]);
}

#[test]
fn a_change_to_several_notes_is_one_all_or_nothing_write() {
    let c = collection();
    let a = note(&c, "a");
    let b = note(&c, "b");
    c.delete_note(b).unwrap();
    assert_eq!(c.add_tags(&[a, b], &["x"]), Err(TagError::NotFound));
    assert!(c.note_tags(a).unwrap().is_empty());
    assert_eq!(
        c.add_tags(&[a, Id::from_bytes([9; 16])], &["x"]),
        Err(TagError::NotFound)
    );
    assert_eq!(c.remove_tags(&[b], &["x"]), Err(TagError::NotFound));
    assert_eq!(c.set_note_tags(b, &["x"]), Err(TagError::NotFound));
}

#[test]
fn a_bad_tag_changes_nothing() {
    let c = collection();
    let n = note(&c, "a");
    assert_eq!(c.add_tags(&[n], &["fine", " "]), Err(TagError::Empty));
    assert_eq!(c.add_tags(&[n], &[""]), Err(TagError::Empty));
    assert!(matches!(
        c.add_tags(&[n], &["two words"]),
        Err(TagError::Whitespace(_))
    ));
    assert!(matches!(
        c.add_tags(&[n], &["tab\there"]),
        Err(TagError::Whitespace(_))
    ));
    for bad in ["lang::", "::lang", "a::::b", "::"] {
        assert!(
            matches!(c.add_tags(&[n], &[bad]), Err(TagError::EmptyPart(_))),
            "{bad}"
        );
    }
    assert!(c.note_tags(n).unwrap().is_empty());
    assert!(stored(&c, n).is_empty());
    for error in [
        TagError::Empty,
        TagError::Whitespace("a b".into()),
        TagError::EmptyPart("a::".into()),
    ] {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn surrounding_space_is_trimmed() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["  polish "]).unwrap();
    assert_eq!(c.note_tags(n).unwrap(), ["polish"]);
}

#[test]
fn set_note_tags_adds_the_missing_and_removes_the_rest() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["a", "b", "c"]).unwrap();
    let kept = c.register_clock("note_tag", n, "b").unwrap();
    c.set_note_tags(n, &["b", "C", "d", "d"]).unwrap();
    assert_eq!(c.note_tags(n).unwrap(), ["b", "c", "d"]);
    assert_eq!(
        c.register_clock("note_tag", n, "b").unwrap(),
        kept,
        "b is not rewritten"
    );
    c.set_note_tags(n, &[]).unwrap();
    assert!(c.note_tags(n).unwrap().is_empty());
}

// Spelling and case.

#[test]
fn a_tag_takes_the_spelling_the_collection_already_uses() {
    let c = collection();
    let a = note(&c, "a");
    let b = note(&c, "b");
    c.add_tags(&[a], &["polish"]).unwrap();
    c.add_tags(&[b], &["POLISH"]).unwrap();
    assert_eq!(c.note_tags(b).unwrap(), ["polish"]);
    assert_eq!(stored(&c, b), [("polish".to_owned(), "1".to_owned())]);
    c.add_tags(&[b], &["Lang::Polish"]).unwrap();
    c.add_tags(&[a], &["LANG::polish::verbs"]).unwrap();
    assert_eq!(c.note_tags(a).unwrap(), ["Lang::Polish::verbs", "polish"]);
}

#[test]
fn tags_with_polish_letters_match_ignoring_case() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["żółć"]).unwrap();
    assert_eq!(c.add_tags(&[n], &["ŻÓŁĆ"]).unwrap(), 0);
    assert_eq!(c.notes_with_tag("Żółć", false).unwrap(), [n]);
}

#[test]
fn two_spellings_that_merged_in_show_once_in_the_smallest_spelling_and_remove_together() {
    let c = collection();
    let n = note(&c, "a");
    remote_tag(&c, n, "polish", true, START + 1);
    remote_tag(&c, n, "Polish", true, START + 2);
    assert_eq!(c.note_tags(n).unwrap(), ["Polish"]);
    let tags = c.tags().unwrap();
    assert_eq!(names(&tags), ["Polish"]);
    assert_eq!(tags[0].notes, 1);
    assert_eq!(c.remove_tags(&[n], &["polish"]).unwrap(), 2);
    assert!(c.note_tags(n).unwrap().is_empty());
}

#[test]
fn the_shown_spelling_does_not_depend_on_the_order_things_arrived_in() {
    let orders: [&[(&str, i64)]; 3] = [
        &[("Lang::x", 1), ("lang::Y", 2), ("LANG::z", 3)],
        &[("LANG::z", 1), ("lang::Y", 2), ("Lang::x", 3)],
        &[("lang::Y", 1), ("Lang::x", 2), ("LANG::z", 3)],
    ];
    let mut seen = Vec::new();
    for order in orders {
        let c = collection();
        let n = note(&c, "a");
        for (tag, at) in order {
            remote_tag(&c, n, tag, true, START + at);
        }
        seen.push((names(&c.tags().unwrap()).join(","), c.note_tags(n).unwrap()));
    }
    assert!(seen.windows(2).all(|pair| pair[0] == pair[1]), "{seen:?}");
    assert_eq!(seen[0].0, "LANG,LANG::x,LANG::Y,LANG::z");
}

// Reading.

#[test]
fn the_tag_tree_lists_parents_before_children_with_counts() {
    let c = collection();
    let (a, b, d) = (note(&c, "a"), note(&c, "b"), note(&c, "d"));
    c.add_tags(&[a], &["lang::polish", "lang::polish::verbs", "food"])
        .unwrap();
    c.add_tags(&[b], &["lang::polish", "lang::german"]).unwrap();
    c.add_tags(&[d], &["lang"]).unwrap();
    let tags = c.tags().unwrap();
    let rows: Vec<_> = tags
        .iter()
        .map(|t| (t.name.as_str(), t.depth, t.notes, t.total))
        .collect();
    assert_eq!(
        rows,
        [
            ("food", 0, 1, 1),
            ("lang", 0, 1, 3),
            ("lang::german", 1, 1, 1),
            ("lang::polish", 1, 2, 2),
            ("lang::polish::verbs", 2, 1, 1),
        ]
    );
}

#[test]
fn a_parent_that_only_exists_because_of_a_child_has_no_notes_of_its_own() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["a::b::c"]).unwrap();
    let rows: Vec<_> = c
        .tags()
        .unwrap()
        .iter()
        .map(|t| (t.name.clone(), t.notes, t.total))
        .collect();
    assert_eq!(
        rows,
        [
            ("a".to_owned(), 0, 1),
            ("a::b".to_owned(), 0, 1),
            ("a::b::c".to_owned(), 1, 1)
        ]
    );
}

#[test]
fn a_tag_with_no_live_note_is_not_listed_and_comes_back_with_the_note() {
    let c = collection();
    let (a, b) = (note(&c, "a"), note(&c, "b"));
    c.add_tags(&[a], &["keep"]).unwrap();
    c.add_tags(&[b], &["gone", "keep"]).unwrap();
    c.delete_note(b).unwrap();
    assert_eq!(names(&c.tags().unwrap()), ["keep"]);
    assert_eq!(c.tags().unwrap()[0].notes, 1);
    assert_eq!(
        c.note_tags(b).unwrap(),
        ["gone", "keep"],
        "the trash keeps its tags"
    );
    assert_eq!(c.notes_with_tag("gone", false).unwrap(), []);
    c.restore_note(b).unwrap();
    assert_eq!(names(&c.tags().unwrap()), ["gone", "keep"]);
}

#[test]
fn a_tag_whose_note_has_not_arrived_is_hidden() {
    let c = collection();
    remote_tag(&c, Id::from_bytes([5; 16]), "early", true, START + 1);
    assert!(c.tags().unwrap().is_empty());
}

#[test]
fn searching_by_tag_finds_notes_with_it_and_optionally_what_is_inside_it() {
    let c = collection();
    let (a, b, d, e) = (note(&c, "a"), note(&c, "b"), note(&c, "d"), note(&c, "e"));
    c.add_tags(&[a], &["lang"]).unwrap();
    c.add_tags(&[b], &["lang::polish"]).unwrap();
    c.add_tags(&[d], &["language"]).unwrap();
    c.add_tags(&[e], &["lang::polish", "lang::german"]).unwrap();
    assert_eq!(c.notes_with_tag("lang", false).unwrap(), [a]);
    assert_eq!(
        c.notes_with_tag("LANG", true).unwrap(),
        sorted(vec![a, b, e])
    );
    assert_eq!(
        c.notes_with_tag("lang::polish", true).unwrap(),
        sorted(vec![b, e])
    );
    assert_eq!(c.notes_with_tag("lan", true).unwrap(), []);
    assert_eq!(c.notes_with_tag("nothing", true).unwrap(), []);
    assert_eq!(
        c.notes_with_tag("bad tag", true),
        Err(TagError::Whitespace("bad tag".into()))
    );
}

// Renaming.

#[test]
fn renaming_a_tag_changes_it_on_every_note() {
    let c = collection();
    let (a, b) = (note(&c, "a"), note(&c, "b"));
    c.add_tags(&[a, b], &["verbs", "keep"]).unwrap();
    assert_eq!(c.rename_tag("verbs", "grammar").unwrap(), 2);
    assert_eq!(c.note_tags(a).unwrap(), ["grammar", "keep"]);
    assert_eq!(c.note_tags(b).unwrap(), ["grammar", "keep"]);
    assert_eq!(c.notes_with_tag("verbs", true).unwrap(), []);
}

#[test]
fn renaming_a_parent_renames_what_is_inside_it_and_nothing_that_only_starts_the_same() {
    let c = collection();
    let (a, b, d) = (note(&c, "a"), note(&c, "b"), note(&c, "d"));
    c.add_tags(&[a], &["lang", "lang::polish::verbs"]).unwrap();
    c.add_tags(&[b], &["lang::german"]).unwrap();
    c.add_tags(&[d], &["language", "languages::x", "Lang2"])
        .unwrap();
    assert_eq!(c.rename_tag("LANG", "speech").unwrap(), 2);
    assert_eq!(c.note_tags(a).unwrap(), ["speech", "speech::polish::verbs"]);
    assert_eq!(c.note_tags(b).unwrap(), ["speech::german"]);
    assert_eq!(
        c.note_tags(d).unwrap(),
        ["Lang2", "language", "languages::x"]
    );
    let tags = c.tags().unwrap();
    assert_eq!(
        names(&tags),
        [
            "Lang2",
            "language",
            "languages",
            "languages::x",
            "speech",
            "speech::german",
            "speech::polish",
            "speech::polish::verbs"
        ]
    );
}

#[test]
fn renaming_a_child_moves_it_to_another_parent() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["lang::polish", "lang::polish::verbs"])
        .unwrap();
    c.rename_tag("lang::polish", "learning::pl").unwrap();
    assert_eq!(
        c.note_tags(n).unwrap(),
        ["learning::pl", "learning::pl::verbs"]
    );
}

#[test]
fn renaming_onto_an_existing_tag_merges_the_notes() {
    let c = collection();
    let (a, b) = (note(&c, "a"), note(&c, "b"));
    c.add_tags(&[a], &["old", "new"]).unwrap();
    c.add_tags(&[b], &["old"]).unwrap();
    assert_eq!(c.rename_tag("old", "NEW").unwrap(), 2);
    assert_eq!(c.note_tags(a).unwrap(), ["new"]);
    assert_eq!(c.note_tags(b).unwrap(), ["new"]);
    assert_eq!(c.notes_with_tag("new", false).unwrap(), sorted(vec![a, b]));
    assert_eq!(c.tags().unwrap()[0].notes, 2);
}

#[test]
fn renaming_can_only_change_the_case() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["polish", "polish::verbs"]).unwrap();
    assert_eq!(c.rename_tag("polish", "Polish").unwrap(), 1);
    assert_eq!(c.note_tags(n).unwrap(), ["Polish", "Polish::verbs"]);
}

#[test]
fn renaming_to_the_same_name_changes_nothing() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["polish"]).unwrap();
    let before = c.register_clock("note_tag", n, "polish").unwrap();
    assert_eq!(c.rename_tag("polish", "polish").unwrap(), 0);
    assert_eq!(c.register_clock("note_tag", n, "polish").unwrap(), before);
}

#[test]
fn a_tag_can_be_renamed_to_inside_itself() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["a", "a::c"]).unwrap();
    c.rename_tag("a", "a::b").unwrap();
    assert_eq!(c.note_tags(n).unwrap(), ["a::b", "a::b::c"]);
}

#[test]
fn renaming_a_tag_nobody_has_is_refused() {
    let c = collection();
    let (n, gone) = (note(&c, "a"), note(&c, "b"));
    c.add_tags(&[gone], &["trash"]).unwrap();
    c.delete_note(gone).unwrap();
    assert_eq!(
        c.rename_tag("nope", "x"),
        Err(TagError::NoSuchTag("nope".into()))
    );
    assert_eq!(
        c.rename_tag("trash", "x"),
        Err(TagError::NoSuchTag("trash".into()))
    );
    assert_eq!(
        c.rename_tag("a", "b c"),
        Err(TagError::Whitespace("b c".into()))
    );
    assert!(c.note_tags(n).unwrap().is_empty());
}

#[test]
fn a_rename_reaches_notes_in_the_trash_so_a_restore_does_not_bring_the_old_name_back() {
    let c = collection();
    let (a, b) = (note(&c, "a"), note(&c, "b"));
    c.add_tags(&[a, b], &["old"]).unwrap();
    c.delete_note(b).unwrap();
    assert_eq!(c.rename_tag("old", "new").unwrap(), 1);
    assert_eq!(c.note_tags(b).unwrap(), ["new"]);
    c.restore_note(b).unwrap();
    assert_eq!(names(&c.tags().unwrap()), ["new"]);
}

#[test]
fn a_rename_writes_only_the_pairs_that_change() {
    let c = collection();
    let (a, b) = (note(&c, "a"), note(&c, "b"));
    c.add_tags(&[a], &["lang::x", "other"]).unwrap();
    c.add_tags(&[b], &["other"]).unwrap();
    let untouched_a = c.register_clock("note_tag", a, "other").unwrap();
    let untouched_b = c.register_clock("note_tag", b, "other").unwrap();
    c.rename_tag("lang", "speech").unwrap();
    assert_eq!(
        c.register_clock("note_tag", a, "other").unwrap(),
        untouched_a
    );
    assert_eq!(
        c.register_clock("note_tag", b, "other").unwrap(),
        untouched_b
    );
    assert!(stored(&c, b).iter().all(|(tag, _)| tag == "other"));
}

#[test]
fn deleting_a_tag_removes_it_and_what_is_inside_it_from_every_note() {
    let c = collection();
    let (a, b, gone) = (note(&c, "a"), note(&c, "b"), note(&c, "c"));
    c.add_tags(&[a], &["lang", "lang::x", "keep"]).unwrap();
    c.add_tags(&[b], &["lang::y", "language"]).unwrap();
    c.add_tags(&[gone], &["lang"]).unwrap();
    c.delete_note(gone).unwrap();
    assert_eq!(c.delete_tag("Lang").unwrap(), 2);
    assert_eq!(c.note_tags(a).unwrap(), ["keep"]);
    assert_eq!(c.note_tags(b).unwrap(), ["language"]);
    assert!(c.note_tags(gone).unwrap().is_empty());
    assert_eq!(
        c.delete_tag("lang"),
        Err(TagError::NoSuchTag("lang".into()))
    );
}

// Merging and sync discipline.

#[test]
fn different_tags_added_to_one_note_on_two_devices_both_stay() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["local"]).unwrap();
    remote_tag(&c, n, "remote", true, START + 10);
    assert_eq!(c.note_tags(n).unwrap(), ["local", "remote"]);
}

#[test]
fn the_later_write_to_the_same_tag_wins() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["t"]).unwrap();
    // Another device removed it later, so it is gone, and a still later add there brings it back.
    remote_tag(&c, n, "t", false, START + 1_000_000);
    assert!(c.note_tags(n).unwrap().is_empty());
    remote_tag(&c, n, "t", true, START + 2_000_000);
    assert_eq!(c.note_tags(n).unwrap(), ["t"]);
}

#[test]
fn every_tag_register_has_a_clock_and_is_not_pushed() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["a", "b::c"]).unwrap();
    c.rename_tag("a", "z").unwrap();
    for tag in ["a", "z", "b::c"] {
        let clock = c.register_clock("note_tag", n, tag).unwrap();
        assert!(clock.is_some_and(|clock| !clock.pushed), "{tag}");
    }
}

#[test]
fn a_tag_register_is_known_and_other_registers_are_not_confused_with_it() {
    let c = collection();
    assert!(c.knows_register("note_tag", "anything::at-all"));
    assert!(c.knows_register("note_tag", "żółć"));
    assert!(!c.knows_register("card", "anything"));
}

#[test]
fn the_database_refuses_raw_writes_to_note_tag() {
    let c = collection();
    let n = note(&c, "a");
    c.add_tags(&[n], &["t"]).unwrap();
    for sql in [
        "INSERT INTO note_tag (note, tag, present) VALUES (x'00', 'x', '1')",
        "UPDATE note_tag SET present = ''",
        "DELETE FROM note_tag",
    ] {
        let error = c.conn.execute(sql, []).unwrap_err().to_string();
        assert!(
            error.contains("write path") || error.contains("hard-deleted"),
            "{sql}: {error}"
        );
    }
}

#[test]
fn the_schema_check_passes_with_note_tag() {
    let c = collection();
    check_schema(&c.conn, SYNCED_TABLES).unwrap();
}

#[test]
fn a_version_5_collection_upgrades_and_keeps_its_notes() {
    const V5: Schema = Schema {
        migrations: &[
            MIGRATIONS[0],
            MIGRATIONS[1],
            MIGRATIONS[2],
            MIGRATIONS[3],
            MIGRATIONS[4],
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
    let path = std::env::temp_dir().join(format!("fc-tag-v5-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = Arc::new(ManualClock::new(START));
    let old = Collection::create_with(location, V5, host(&clock)).unwrap();
    assert!(
        old.conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE name = 'note_tag'",
                [],
                |_| Ok(())
            )
            .is_err()
    );
    // A note as step 1.5 wrote it. (The note API reads columns that came later, so it cannot run on
    // this old layout.)
    let n = Id::from_bytes([9; 16]);
    old.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    old.conn
        .execute(
            "INSERT INTO note (id, note_type, deleted) VALUES (?1, ?2, 0)",
            rusqlite::params![n, crate::notetype::builtin::basic()],
        )
        .unwrap();
    old.conn.execute("DELETE FROM write_guard", []).unwrap();
    old.close().unwrap();

    let upgraded = Collection::open(location, host(&clock)).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 10);
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    assert!(upgraded.note(n).unwrap().is_some());
    assert!(upgraded.note_tags(n).unwrap().is_empty());
    upgraded.add_tags(&[n], &["after"]).unwrap();
    assert_eq!(upgraded.note_tags(n).unwrap(), ["after"]);
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_tag_readers_never_fail_whatever_merged_in() {
    let c = collection();
    let n = note(&c, "a");
    for (i, tag) in [
        "", " ", "a b", "::", "a::", "::a", "A", "a", "a::B", "A::b", "é", "É",
    ]
    .into_iter()
    .enumerate()
    {
        remote_tag(&c, n, tag, true, START + i as i64);
    }
    c.tags().unwrap();
    c.note_tags(n).unwrap();
    c.notes_with_tag("a", true).unwrap();
}
