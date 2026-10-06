//! Backup, export and restore (step 1.13a).

use std::io::{Cursor, Read, Write};

use rusqlite::types::Value;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use super::format::{read_changes, write_changes};
use super::*;
use crate::collection::Collection;
use crate::id::Id;
use crate::scheduling::Rating;
use crate::study::answer_tests::MINUTE;
use crate::sync::merge_tests::{Dev, START, add, assert_same, digest, edit_front, front, pair};
use crate::sync::tour_tests::run_the_tour;
use crate::sync::{Changes, Clock, Hlc, RegisterChange, RowChange, Selection};

// ---- Helpers ----

/// What two collections with the same content share: the digest of the sync tests without the
/// clocks, since a restore writes the values again under new clocks.
fn state(c: &Collection) -> String {
    let all = digest(c);
    let (from, to) = (
        all.find("## clocks\n").unwrap(),
        all.find("## schedule\n").unwrap(),
    );
    without_empty_tags(&format!("{}{}", &all[..from], &all[to..]))
}

/// A tag that was taken off a note is a row with an empty value, which reads the same as no row.
fn without_empty_tags(digest: &str) -> String {
    let mut section = "";
    let mut kept = Vec::new();
    for line in digest.lines() {
        if line.starts_with("## ") {
            section = line;
        }
        if section == "## note_tag" && line.ends_with("|Text(\"\")") {
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

fn export(c: &Collection, options: &ExportOptions) -> (Vec<u8>, ExportSummary) {
    let mut out = Cursor::new(Vec::new());
    let summary = c.export_backup(&mut out, options).unwrap();
    (out.into_inner(), summary)
}

fn whole() -> ExportOptions {
    ExportOptions::default()
}

fn blobs(c: &Collection) -> Vec<(String, Vec<u8>)> {
    let mut statement = c
        .conn
        .prepare("SELECT hash, data FROM media_blob ORDER BY hash")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn names(bytes: &[u8]) -> Vec<String> {
    let archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    names.sort();
    names
}

/// Copies a backup, letting `change` replace (or drop, with `None`) the content of each entry.
fn rewrite(bytes: &[u8], change: impl Fn(&str, Vec<u8>) -> Option<Vec<u8>>) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().to_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if let Some(data) = change(&name, data) {
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&data).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

fn restore(c: &Collection, bytes: &[u8]) -> RestoreReport {
    c.restore_backup(Cursor::new(bytes)).unwrap()
}

fn card_events(c: &Collection) -> i64 {
    c.conn
        .query_row("SELECT COUNT(*) FROM card_event", [], |row| row.get(0))
        .unwrap()
}

// ---- The file ----

#[test]
fn a_backup_is_a_zip_with_a_manifest_the_changes_and_the_media() {
    let (a, _) = pair();
    run_the_tour(&a);
    let (bytes, summary) = export(&a.c, &whole());
    let names = names(&bytes);
    assert!(names.contains(&"manifest.json".to_owned()));
    assert!(names.contains(&"changes.jsonl".to_owned()));
    let media: Vec<_> = names.iter().filter(|n| n.starts_with("media/")).collect();
    assert_eq!(media.len() as u64, summary.manifest.media_files);
    assert_eq!(media.len(), blobs(&a.c).len());
    assert!(!media.is_empty());

    let manifest = read_manifest(Cursor::new(&bytes)).unwrap();
    assert_eq!(manifest, summary.manifest);
    assert_eq!(manifest.format_version, FORMAT_VERSION);
    assert_eq!(manifest.scope, ManifestScope::Collection);
    assert!(manifest.history);
    assert_eq!(manifest.device, a.c.device_id().unwrap());
    let changes = a.c.changes(Selection::All).unwrap();
    assert_eq!(manifest.registers, changes.registers.len() as u64);
    assert_eq!(manifest.rows, changes.rows.len() as u64);
}

#[test]
fn changes_survive_the_text_with_every_kind_of_value() {
    let device = Id::from_bytes([9; 16]);
    let at = |n: i64| Clock {
        hlc: Hlc::from_stored(n),
        device,
    };
    let id = |n: u8| Id::from_bytes([n; 16]);
    let changes = Changes {
        registers: vec![
            RegisterChange {
                entity: "note".into(),
                entity_id: id(1),
                field: "a \"quoted\"\nfield".into(),
                value: Value::Text("line one\nline two \u{1F600} \"q\" \\".into()),
                clock: at(5),
            },
            RegisterChange {
                entity: "x".into(),
                entity_id: id(2),
                field: "n".into(),
                value: Value::Integer(i64::MIN),
                clock: at(i64::MAX),
            },
            RegisterChange {
                entity: "x".into(),
                entity_id: id(2),
                field: "r".into(),
                value: Value::Real(0.1 + 0.2),
                clock: at(0),
            },
            RegisterChange {
                entity: "x".into(),
                entity_id: id(2),
                field: "whole".into(),
                value: Value::Real(3.0),
                clock: at(0),
            },
            RegisterChange {
                entity: "x".into(),
                entity_id: id(2),
                field: "inf".into(),
                value: Value::Real(f64::NEG_INFINITY),
                clock: at(0),
            },
            RegisterChange {
                entity: "x".into(),
                entity_id: id(2),
                field: "null".into(),
                value: Value::Null,
                clock: at(0),
            },
            RegisterChange {
                entity: "x".into(),
                entity_id: id(2),
                field: "blob".into(),
                value: Value::Blob(vec![0, 1, 254, 255]),
                clock: at(0),
            },
            RegisterChange {
                entity: "x".into(),
                entity_id: id(2),
                field: "empty blob".into(),
                value: Value::Blob(vec![]),
                clock: at(0),
            },
        ],
        rows: vec![RowChange {
            entity: "card_event".into(),
            id: id(3),
            columns: vec![
                ("card".into(), Value::Blob(id(4).as_bytes().to_vec())),
                ("kind".into(), Value::Text("answer".into())),
                ("card".into(), Value::Null),
            ],
        }],
    };
    let mut text = Vec::new();
    write_changes(&mut text, &changes).unwrap();
    let back = read_changes(Cursor::new(&text)).unwrap();
    assert_eq!(back, changes);
    // One JSON value per line.
    assert_eq!(text.iter().filter(|b| **b == b'\n').count(), 9);
}

#[test]
fn a_line_that_does_not_hold_together_is_damage_that_names_the_line() {
    for (line, why) in [
        ("{not json", "line 1"),
        ("[]", "not an object"),
        (r#"{"kind":"mystery"}"#, "unknown kind"),
        (r#"{"kind":"register","entity":"note"}"#, "`value`"),
        (
            r#"{"kind":"register","entity":"n","id":"nope","field":"f","value":1,"hlc":1,"device":"00000000-0000-0000-0000-000000000000"}"#,
            "not an ID",
        ),
        (
            r#"{"kind":"register","entity":"n","id":"00000000-0000-0000-0000-000000000001","field":"f","value":{"blob":"zz"},"hlc":1,"device":"00000000-0000-0000-0000-000000000000"}"#,
            "hexadecimal",
        ),
    ] {
        let Err(BackupError::Damaged(text)) = read_changes(Cursor::new(line.as_bytes())) else {
            panic!("`{line}` was accepted");
        };
        assert!(text.contains(why), "{text}");
    }
}

// ---- Restore into an empty collection ----

#[test]
fn a_whole_backup_restores_into_an_empty_collection() {
    let (a, _) = pair();
    run_the_tour(&a);
    let (bytes, _) = export(&a.c, &whole());

    let fresh = Dev::new(2, START);
    let report = restore(&fresh.c, &bytes);
    assert_eq!(report.removed, 0);
    assert!(report.rejected.is_empty());
    assert!(report.media_added > 0);
    assert_same(&state(&fresh.c), &state(&a.c), "restored");
    assert_eq!(blobs(&fresh.c), blobs(&a.c));
    assert_eq!(fresh.c.unsupported_features().unwrap(), ["future-thing"]);
    // The history came too, and the schedule was folded from it.
    assert!(card_events(&fresh.c) > 0);
    assert_eq!(card_events(&fresh.c), card_events(&a.c));
}

#[test]
fn a_restore_gives_the_collection_a_new_device_and_writes_new_changes() {
    let (a, _) = pair();
    run_the_tour(&a);
    let (bytes, _) = export(&a.c, &whole());
    let fresh = Dev::new(2, START);
    let before = fresh.c.device_id().unwrap();
    restore(&fresh.c, &bytes);
    let after = fresh.c.device_id().unwrap();
    assert_ne!(before, after);
    assert_ne!(after, a.c.device_id().unwrap());

    // Everything it wrote is for the next sync to push, under the new device.
    let unpushed = fresh.c.changes(Selection::Unpushed).unwrap();
    assert!(!unpushed.registers.is_empty());
    assert!(unpushed.registers.iter().all(|r| r.clock.device == after));
    assert!(!unpushed.rows.is_empty());
}

#[test]
fn restoring_what_the_collection_already_is_changes_nothing() {
    let (a, _) = pair();
    run_the_tour(&a);
    let (bytes, _) = export(&a.c, &whole());
    let before = digest(&a.c);
    let report = restore(&a.c, &bytes);
    assert_eq!(report.registers_written, 0);
    assert_eq!(report.rows_added, 0);
    assert_eq!(report.removed, 0);
    assert_eq!(report.media_added, 0);
    assert_same(&digest(&a.c), &before, "unchanged");
}

// ---- Restore over later work ----

#[test]
fn a_restore_puts_back_what_was_changed_after_the_backup() {
    let (a, _) = pair();
    let c = &a.c;
    let (kept, kept_card) = add(c, "kept", "k");
    let (edited, _) = add(c, "before", "b");
    let (doomed, _) = add(c, "doomed", "d");
    c.add_tags(&[kept], &["old"]).unwrap();
    c.answer(kept_card, Rating::Good, 1_000).unwrap();
    let (bytes, _) = export(c, &whole());

    a.clock.advance(5 * MINUTE);
    edit_front(c, edited, "after");
    c.delete_note(doomed).unwrap();
    let (added, _) = add(c, "new", "n");
    c.add_tags(&[kept], &["fresh"]).unwrap();
    c.remove_tags(&[kept], &["old"]).unwrap();
    c.answer(kept_card, Rating::Again, 1_000).unwrap();
    let events = card_events(c);

    a.clock.advance(MINUTE);
    let report = restore(c, &bytes);
    assert!(report.removed >= 2, "{report:?}");

    assert_eq!(front(c, edited), "before");
    assert!(!c.note(doomed).unwrap().unwrap().deleted);
    assert!(
        c.note(added).unwrap().unwrap().deleted,
        "added later: in the trash"
    );
    assert!(
        c.cards_of_note(added).unwrap().is_empty(),
        "its card went with it"
    );
    assert_eq!(c.note_tags(kept).unwrap(), ["old"]);
    // Reviews are never taken back.
    assert_eq!(card_events(c), events);
    // And the trash can bring the new note back.
    c.restore_note(added).unwrap();
    assert!(!c.note(added).unwrap().unwrap().deleted);
}

#[test]
fn a_restore_brings_back_every_value_and_trashes_what_is_new() {
    let (a, _) = pair();
    let c = &a.c;
    run_the_tour(&a);
    let (bytes, _) = export(c, &whole());
    let at_backup = state(c);

    a.clock.advance(5 * MINUTE);
    let (n, _) = add(c, "later", "x");
    edit_front(c, n, "edited");
    let d = c.create_deck("Later deck", None).unwrap();
    c.rename_deck(d, "Renamed").unwrap();
    c.set_day_start_hour(7).unwrap();
    c.add_media("later.png", b"later bytes").unwrap();
    a.clock.advance(MINUTE);
    restore(c, &bytes);

    // Everything that did not exist is in the trash, and everything that changed is back. The
    // setting was in the backup, so it is back too.
    assert!(c.note(n).unwrap().unwrap().deleted);
    assert!(c.deleted_decks().unwrap().iter().any(|k| k.id == d));
    assert_eq!(c.day_start_hour().unwrap(), 4);
    let now = state(c);
    for line in at_backup.lines().filter(|l| !l.is_empty()) {
        if line.starts_with("##") {
            continue;
        }
        assert!(
            now.contains(line),
            "a line of the backup is missing after the restore: {line}"
        );
    }
}

#[test]
fn a_restore_reaches_another_device_and_is_not_undone_by_it() {
    let (a, b) = pair();
    let (note, _) = add(&a.c, "original", "o");
    crate::sync::merge_tests::settle(&[&a, &b]);
    let (bytes, _) = export(&a.c, &whole());

    a.clock.advance(MINUTE);
    edit_front(&a.c, note, "changed on a");
    crate::sync::merge_tests::settle(&[&a, &b]);
    assert_eq!(front(&b.c, note), "changed on a");

    a.clock.advance(MINUTE);
    restore(&a.c, &bytes);
    assert_eq!(front(&a.c, note), "original");
    // The other device has the newer edit, and the restore still wins: it is a new write.
    crate::sync::merge_tests::settle(&[&a, &b]);
    assert_eq!(front(&b.c, note), "original");
    assert_eq!(front(&a.c, note), "original");
    assert_same(&digest(&a.c), &digest(&b.c), "converged");
}

#[test]
fn a_restore_leaves_what_it_does_not_know_about_the_edits_of_other_devices() {
    // B adds a note that A never saw, so A's restore has nothing to say about it.
    let (a, b) = pair();
    let (note, _) = add(&a.c, "one", "1");
    crate::sync::merge_tests::settle(&[&a, &b]);
    let (bytes, _) = export(&a.c, &whole());
    b.clock.advance(MINUTE);
    let (theirs, _) = add(&b.c, "theirs", "t");
    a.clock.advance(2 * MINUTE);
    restore(&a.c, &bytes);
    crate::sync::merge_tests::settle(&[&a, &b]);
    assert!(!b.c.note(theirs).unwrap().unwrap().deleted);
    assert!(!a.c.note(theirs).unwrap().unwrap().deleted);
    let _ = note;
}

// ---- History ----

#[test]
fn history_is_optional() {
    let (a, _) = pair();
    run_the_tour(&a);
    let with = export(&a.c, &whole()).1;
    let (bytes, without) = export(
        &a.c,
        &ExportOptions {
            history: false,
            ..whole()
        },
    );
    assert!(with.manifest.rows > 0);
    assert_eq!(without.manifest.rows, 0);
    assert!(!without.manifest.history);

    let fresh = Dev::new(2, START);
    restore(&fresh.c, &bytes);
    assert_eq!(card_events(&fresh.c), 0);
    let scheduled: i64 = fresh
        .c
        .conn
        .query_row("SELECT COUNT(*) FROM card_schedule", [], |row| row.get(0))
        .unwrap();
    assert_eq!(scheduled, 0, "every card reads as new");
    // The notes, the cards and the choices about them (suspended, buried) are all there.
    assert_eq!(
        fresh.c.note_types().unwrap().len(),
        a.c.note_types().unwrap().len()
    );
}

// ---- One deck ----

/// Two decks with notes, a parent and a child, media on one, a review, a note type.
fn two_decks(a: &Dev) -> (Id, Id, Id, Id, Id) {
    let c = &a.c;
    let (f, b) = crate::sync::merge_tests::basic_fields(c);
    let polish = c.create_deck("Polish", None).unwrap();
    let verbs = c.create_deck("Verbs", Some(polish)).unwrap();
    let other = c.create_deck("Other", None).unwrap();
    let photo = c.add_media("cat.png", b"cat bytes").unwrap();
    let front_with_media = format!("kot <img src=\"{}\">", photo.name);
    let in_polish = c
        .add_note_to_deck(
            polish,
            builtin_basic(),
            &[(f, front_with_media.as_str()), (b, "cat")],
        )
        .unwrap();
    c.add_note_to_deck(verbs, builtin_basic(), &[(f, "być"), (b, "to be")])
        .unwrap();
    let elsewhere = c
        .add_note_to_deck(other, builtin_basic(), &[(f, "pies"), (b, "dog")])
        .unwrap();
    c.add_media("dog.png", b"dog bytes").unwrap();
    c.set_note_fields(elsewhere.id, &[(f, "pies <img src=\"dog-x.png\">")])
        .unwrap();
    c.add_tags(&[in_polish.id, elsewhere.id], &["animals"])
        .unwrap();
    c.answer(in_polish.cards[0], Rating::Good, 1_000).unwrap();
    c.answer(elsewhere.cards[0], Rating::Good, 1_000).unwrap();
    (polish, verbs, other, in_polish.cards[0], elsewhere.cards[0])
}

fn builtin_basic() -> Id {
    crate::notetype::builtin::basic()
}

#[test]
fn a_deck_export_holds_the_deck_its_children_and_what_they_need() {
    let (a, _) = pair();
    let (polish, verbs, other, polish_card, other_card) = two_decks(&a);
    let (bytes, summary) = export(
        &a.c,
        &ExportOptions {
            scope: Scope::Deck(polish),
            history: true,
        },
    );
    assert_eq!(
        summary.manifest.scope,
        ManifestScope::Deck {
            id: polish,
            name: "Polish".into()
        }
    );
    assert_eq!(
        summary.manifest.media_files, 1,
        "only the picture the deck names"
    );
    assert_eq!(
        names(&bytes)
            .iter()
            .filter(|n| n.starts_with("media/"))
            .count(),
        1
    );

    let fresh = Dev::new(2, START);
    let report = fresh.c.import_backup(Cursor::new(&bytes)).unwrap();
    assert!(report.rejected.is_empty());
    let decks: Vec<String> = fresh
        .c
        .decks()
        .unwrap()
        .iter()
        .map(|d| d.path.clone())
        .collect();
    assert!(decks.contains(&"Polish".to_owned()), "{decks:?}");
    assert!(decks.contains(&"Polish::Verbs".to_owned()));
    assert!(!decks.contains(&"Other".to_owned()));
    let _ = (verbs, other);

    let notes: Vec<String> = fresh
        .c
        .notes(builtin_basic())
        .unwrap()
        .iter()
        .map(|n| n.fields[1].value.clone())
        .collect();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes.contains(&"cat".to_owned()) && notes.contains(&"to be".to_owned()));
    assert_eq!(blobs(&fresh.c).len(), 1);
    // Tags and review history of its own cards.
    assert_eq!(fresh.c.tags().unwrap().len(), 1);
    assert_eq!(card_events(&fresh.c), 1);
    assert_eq!(fresh.c.card_events(polish_card).unwrap().len(), 1);
    assert!(fresh.c.card_events(other_card).unwrap().is_empty());
}

#[test]
fn a_deck_export_without_history_has_no_events_and_a_deck_that_is_not_there_is_refused() {
    let (a, _) = pair();
    let (polish, ..) = two_decks(&a);
    let (_, summary) = export(
        &a.c,
        &ExportOptions {
            scope: Scope::Deck(polish),
            history: false,
        },
    );
    assert_eq!(summary.manifest.rows, 0);

    let mut out = Cursor::new(Vec::new());
    let gone = Id::from_bytes([7; 16]);
    assert_eq!(
        a.c.export_backup(
            &mut out,
            &ExportOptions {
                scope: Scope::Deck(gone),
                history: true
            }
        ),
        Err(BackupError::NoSuchDeck)
    );
}

#[test]
fn importing_a_deck_twice_changes_nothing_the_second_time_and_keeps_what_is_there() {
    let (a, _) = pair();
    let (polish, ..) = two_decks(&a);
    let (bytes, _) = export(
        &a.c,
        &ExportOptions {
            scope: Scope::Deck(polish),
            history: true,
        },
    );
    let target = Dev::new(2, START);
    let (own, _) = add(&target.c, "own note", "mine");
    target.c.import_backup(Cursor::new(&bytes)).unwrap();
    assert_eq!(front(&target.c, own), "own note", "what was there stays");

    let again = target.c.import_backup(Cursor::new(&bytes)).unwrap();
    assert_eq!(again.registers_written, 0, "{again:?}");
    assert_eq!(again.rows_added, 0);
    assert_eq!(again.media_added, 0);
    assert_eq!(again.removed, 0);
}

#[test]
fn an_import_is_written_as_new_changes_so_a_sync_will_push_it() {
    let (a, b) = pair();
    let (polish, ..) = two_decks(&a);
    let (bytes, _) = export(
        &a.c,
        &ExportOptions {
            scope: Scope::Deck(polish),
            history: true,
        },
    );
    b.c.import_backup(Cursor::new(&bytes)).unwrap();
    let unpushed = b.c.changes(Selection::Unpushed).unwrap();
    assert!(unpushed.registers.iter().any(|r| r.entity == "deck"));
    assert!(unpushed.rows.iter().any(|r| r.entity == "card_event"));
    assert!(
        unpushed
            .registers
            .iter()
            .all(|r| r.clock.device == b.c.device_id().unwrap())
    );
}

#[test]
fn a_deck_file_is_not_restored() {
    let (a, _) = pair();
    let (polish, ..) = two_decks(&a);
    let (bytes, _) = export(
        &a.c,
        &ExportOptions {
            scope: Scope::Deck(polish),
            history: true,
        },
    );
    let target = Dev::new(2, START);
    add(&target.c, "mine", "m");
    let before = digest(&target.c);
    assert_eq!(
        target.c.restore_backup(Cursor::new(&bytes)).unwrap_err(),
        BackupError::IsADeck
    );
    assert_same(&digest(&target.c), &before, "refused without a change");
}

// ---- Files that must be refused ----

#[test]
fn files_that_are_not_backups_or_are_damaged_are_refused_and_change_nothing() {
    let (a, _) = pair();
    run_the_tour(&a);
    let (good, _) = export(&a.c, &whole());
    let target = Dev::new(2, START);
    add(&target.c, "mine", "m");
    let before = digest(&target.c);
    let refuse = |bytes: &[u8]| target.c.restore_backup(Cursor::new(bytes)).unwrap_err();

    assert_eq!(refuse(b"not a zip at all"), BackupError::NotABackup);
    assert_eq!(refuse(b""), BackupError::NotABackup);
    assert_eq!(
        refuse(&good[..good.len() / 2]),
        BackupError::NotABackup,
        "cut short"
    );
    let no_manifest = rewrite(&good, |name, data| {
        (name != "manifest.json").then_some(data)
    });
    assert_eq!(refuse(&no_manifest), BackupError::NotABackup);
    let not_ours = rewrite(&good, |name, data| {
        if name == "manifest.json" {
            Some(br#"{"format":"something-else"}"#.to_vec())
        } else {
            Some(data)
        }
    });
    assert_eq!(refuse(&not_ours), BackupError::NotABackup);

    let newer = rewrite(&good, |name, data| {
        if name == "manifest.json" {
            let text = String::from_utf8(data).unwrap();
            Some(
                text.replace("\"format_version\": 1", "\"format_version\": 2")
                    .into_bytes(),
            )
        } else {
            Some(data)
        }
    });
    assert_eq!(
        refuse(&newer),
        BackupError::TooNew {
            found: 2,
            supported: 1
        }
    );

    let bad_line = rewrite(&good, |name, data| {
        if name == "changes.jsonl" {
            let mut data = data;
            data.extend_from_slice(b"{broken\n");
            Some(data)
        } else {
            Some(data)
        }
    });
    assert!(matches!(refuse(&bad_line), BackupError::Damaged(_)));
    let no_changes = rewrite(&good, |name, data| {
        (name != "changes.jsonl").then_some(data)
    });
    assert!(matches!(refuse(&no_changes), BackupError::Damaged(_)));

    // Media that does not match its name: found while the transaction is open, so it rolls back
    // what the changes had already written.
    let bad_media = rewrite(&good, |name, data| {
        if name.starts_with("media/") {
            let mut data = data;
            data.push(0);
            Some(data)
        } else {
            Some(data)
        }
    });
    assert!(matches!(refuse(&bad_media), BackupError::Damaged(_)));

    // The device ID changes only for a file that was read: all of the above were refused before
    // or rolled back, and nothing else changed.
    assert_same(
        &state(&target.c),
        &state_of_before(&before),
        "nothing changed",
    );
}

fn state_of_before(digest_text: &str) -> String {
    let (from, to) = (
        digest_text.find("## clocks\n").unwrap(),
        digest_text.find("## schedule\n").unwrap(),
    );
    without_empty_tags(&format!("{}{}", &digest_text[..from], &digest_text[to..]))
}

#[test]
fn what_this_build_does_not_know_is_kept_through_a_backup() {
    let (a, _) = pair();
    let id = Id::from_bytes([5; 16]);
    let device = Id::from_bytes([8; 16]);
    a.c.merge(&Changes {
        registers: vec![RegisterChange {
            entity: "gadget".into(),
            entity_id: id,
            field: "colour".into(),
            value: Value::Text("blue".into()),
            clock: Clock {
                hlc: Hlc::from_stored(77),
                device,
            },
        }],
        rows: vec![RowChange {
            entity: "gadget_log".into(),
            id,
            columns: vec![("what".into(), Value::Text("seen".into()))],
        }],
    })
    .unwrap();
    let (bytes, _) = export(&a.c, &whole());
    let fresh = Dev::new(2, START);
    restore(&fresh.c, &bytes);
    let all = fresh.c.changes(Selection::All).unwrap();
    assert!(
        all.registers
            .iter()
            .any(|r| r.entity == "gadget" && r.field == "colour")
    );
    assert!(all.rows.iter().any(|r| r.entity == "gadget_log"));
}

// ---- The documented format ----

#[test]
fn a_backup_written_by_hand_from_the_documentation_can_be_read() {
    // The example in `docs/backup-format.md`: one deck's worth of nothing, just a deck.
    let manifest = r#"{
  "format": "fc-backup",
  "format_version": 1,
  "app_version": "0.0.0",
  "storage_version": 11,
  "created_ms": 1760000000000,
  "device": "0194e8a0-0000-7000-8000-000000000001",
  "scope": { "kind": "collection" },
  "history": false,
  "registers": 4,
  "rows": 0,
  "media_files": 0,
  "future_key": "ignored"
}"#;
    let deck = "0194e8a0-0000-7000-8000-0000000000d1";
    let device = "0194e8a0-0000-7000-8000-000000000001";
    let lines: String = [
        ("name", r#""Hand made""#),
        ("parent", r#"{"blob":""}"#),
        ("options_preset", r#"{"blob":""}"#),
        ("deleted", "0"),
    ]
    .iter()
    .map(|(field, value)| {
        format!(
            "{{\"kind\":\"register\",\"entity\":\"deck\",\"id\":\"{deck}\",\"field\":\"{field}\",\"value\":{value},\"hlc\":1760000000000000,\"device\":\"{device}\"}}\n"
        )
    })
    .collect();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    out.start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    out.write_all(manifest.as_bytes()).unwrap();
    out.start_file("changes.jsonl", SimpleFileOptions::default())
        .unwrap();
    out.write_all(lines.as_bytes()).unwrap();
    let bytes = out.finish().unwrap().into_inner();

    let fresh = Dev::new(2, START);
    let report = fresh.c.import_backup(Cursor::new(&bytes)).unwrap();
    assert_eq!(report.registers_written, 4, "{report:?}");
    assert!(
        fresh
            .c
            .decks()
            .unwrap()
            .iter()
            .any(|d| d.path == "Hand made")
    );
}

#[test]
fn a_restore_never_trashes_the_built_ins() {
    // The backup is of a collection that has only the built-ins. Restoring it into one with more
    // trashes what the person made, and nothing they did not.
    let (a, b) = pair();
    let (bytes, _) = export(&a.c, &whole());
    let (note, _) = add(&b.c, "mine", "m");
    let mine =
        b.c.create_note_type("Mine", crate::notetype::Kind::Standard)
            .unwrap();
    b.clock.advance(MINUTE);
    restore(&b.c, &bytes);
    assert!(b.c.note(note).unwrap().unwrap().deleted);
    assert!(
        b.c.deleted_note_types()
            .unwrap()
            .iter()
            .any(|t| t.id == mine)
    );
    assert!(b.c.note_type(builtin_basic()).unwrap().is_some());
    assert!(
        b.c.decks()
            .unwrap()
            .iter()
            .any(|d| d.id == crate::deck::default_deck())
    );
    assert!(
        b.c.presets()
            .unwrap()
            .iter()
            .any(|p| p.id == crate::deck::default_preset())
    );
}

#[test]
fn a_field_and_a_template_added_after_the_backup_are_removed_by_a_restore() {
    let (a, _) = pair();
    let c = &a.c;
    let nt = c
        .create_note_type("Grow", crate::notetype::Kind::Standard)
        .unwrap();
    let fields = c.note_type(nt).unwrap().unwrap().fields;
    let values: Vec<(Id, &str)> = fields.iter().map(|f| (f.id, "v")).collect();
    let note = c.add_note(nt, &values).unwrap();
    let (bytes, _) = export(c, &whole());

    a.clock.advance(MINUTE);
    c.add_field(nt, "Extra").unwrap();
    c.add_template(nt, "Reverse", "{{Back}}", "{{Front}}")
        .unwrap();
    assert_eq!(c.cards_of_note(note.id).unwrap().len(), 2);
    a.clock.advance(MINUTE);
    restore(c, &bytes);
    let now = c.note_type(nt).unwrap().unwrap();
    assert_eq!(now.fields.len(), fields.len());
    assert_eq!(now.templates.len(), 1);
    assert_eq!(
        c.cards_of_note(note.id).unwrap().len(),
        1,
        "the extra card went with its template"
    );
}

/// Timings for the notes in `docs`: `cargo test --release -p fc-core --lib backup::tests::timing --
/// --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn timing_of_a_large_collection() {
    use std::time::Instant;
    let (a, _) = pair();
    let (f, b) = crate::sync::merge_tests::basic_fields(&a.c);
    let started = Instant::now();
    let mut cards = Vec::new();
    for n in 0..20_000 {
        let front =
            format!("front {n} <b>with</b> some html and words to make it a realistic size");
        let added =
            a.c.add_note(
                builtin_basic(),
                &[(f, front.as_str()), (b, "back text of the note")],
            )
            .unwrap();
        cards.push(added.cards[0]);
    }
    for card in cards.iter().take(10_000) {
        a.clock.advance(MINUTE);
        a.c.answer(*card, Rating::Good, 3_000).unwrap();
        a.clock.advance(MINUTE);
        a.c.answer(*card, Rating::Good, 3_000).unwrap();
    }
    for n in 0..200 {
        a.c.add_media(
            "pic.png",
            format!("media bytes {n}").repeat(2_000).as_bytes(),
        )
        .unwrap();
    }
    println!("built in {:?}", started.elapsed());

    let started = Instant::now();
    let (bytes, summary) = export(&a.c, &whole());
    println!(
        "export {:?}: {} registers, {} rows, {} media files, {} bytes",
        started.elapsed(),
        summary.manifest.registers,
        summary.manifest.rows,
        summary.manifest.media_files,
        bytes.len()
    );
    let started = Instant::now();
    let fresh = Dev::new(2, START);
    let report = restore(&fresh.c, &bytes);
    println!("restore into empty {:?}: {report:?}", started.elapsed());
    let started = Instant::now();
    let report = restore(&a.c, &bytes);
    println!(
        "restore of the same state {:?}: {report:?}",
        started.elapsed()
    );
}

// ---- Events ----

#[test]
fn a_restore_sends_one_merge_event_even_when_it_only_trashes_things() {
    use crate::events::Event;
    use crate::events::tests::Recorder;
    let (a, _) = pair();
    let (bytes, _) = export(&a.c, &whole());
    add(&a.c, "later", "l");
    a.clock.advance(MINUTE);
    let rec = Recorder::attach(&a.c);
    let report = restore(&a.c, &bytes);
    assert!(report.removed > 0);
    assert_eq!(report.registers_written, 0);
    let batch = rec.one();
    assert!(
        matches!(batch.as_slice(), [Event::MergeApplied { registers_applied, .. }] if *registers_applied as usize == report.removed),
        "{batch:?}"
    );

    // Nothing to do, nothing sent.
    let rec = Recorder::attach(&a.c);
    restore(&a.c, &bytes);
    assert!(rec.take().is_empty());
}

#[test]
fn a_refused_file_sends_no_event() {
    use crate::events::tests::Recorder;
    let (a, _) = pair();
    let rec = Recorder::attach(&a.c);
    assert!(a.c.restore_backup(Cursor::new(b"nope".as_slice())).is_err());
    assert!(rec.take().is_empty());
}
