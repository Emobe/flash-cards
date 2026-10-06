//! Two collections making changes independently and merging them (ADR 0008, parts 3 to 6 and 11).
//! The helpers are shared with `tour_tests`.

use std::sync::Arc;

use rusqlite::types::Value;

use super::merge::{Column, Kind as Kind_};
use super::*;
use crate::clock::{Clock as _, Host, ManualClock};
use crate::collection::Collection;
use crate::deck::PresetChange;
use crate::id::Id;
use crate::notetype::{Kind, builtin};
use crate::scheduling::{Rating, default_parameters};
use crate::study::answer_tests::{DAY, DAY0, HOUR, MINUTE};

// ---- Helpers ----

pub(super) struct Dev {
    pub c: Collection,
    pub clock: Arc<ManualClock>,
}

impl Dev {
    pub(super) fn new(n: u8, now: i64) -> Self {
        let clock = Arc::new(ManualClock::new(now));
        let host = Host {
            clock: clock.clone(),
            installation_id: Id::from_bytes([n; 16]),
        };
        let c = Collection::create(":memory:", host).unwrap();
        Self { c, clock }
    }
}

pub(super) const START: i64 = DAY0 + 9 * HOUR;

/// Two devices whose IDs come from a fixed sequence, so a tie between equal clocks always breaks the
/// same way.
pub(super) fn pair() -> (Dev, Dev) {
    crate::id::seeded::seed(7);
    (Dev::new(1, START), Dev::new(2, START))
}

/// `a` and `b` give each other everything they have, once each way.
pub(super) fn sync(a: &Dev, b: &Dev) -> (MergeReport, MergeReport) {
    let into_b = b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
    let into_a = a.c.merge(&b.c.changes(Selection::All).unwrap()).unwrap();
    (into_a, into_b)
}

/// Syncs until nothing changes any more (reconcile writes can need a second round).
pub(super) fn settle(devs: &[&Dev]) {
    for _ in 0..6 {
        let before: Vec<String> = devs.iter().map(|d| digest(&d.c)).collect();
        for a in devs {
            for b in devs {
                if !std::ptr::eq(*a, *b) {
                    b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
                }
            }
        }
        let after: Vec<String> = devs.iter().map(|d| digest(&d.c)).collect();
        if before == after {
            return;
        }
    }
    panic!("the devices did not settle");
}

fn show(value: &Value) -> String {
    match value {
        Value::Blob(bytes) => bytes.iter().map(|b| format!("{b:02x}")).collect(),
        other => format!("{other:?}"),
    }
}

fn dump(c: &Collection, sql: &str) -> String {
    let mut statement = c.conn.prepare(sql).unwrap();
    let columns = statement.column_count();
    let rows = statement
        .query_map([], |row| {
            Ok((0..columns)
                .map(|i| show(&row.get::<_, Value>(i).unwrap()))
                .collect::<Vec<_>>()
                .join("|"))
        })
        .unwrap();
    rows.map(Result::unwrap).collect::<Vec<_>>().join("\n")
}

/// Everything two converged collections must share: every synced table, the clocks `(hlc, device)`,
/// the events, the schedule cache, and what the reading APIs return.
pub(super) fn digest(c: &Collection) -> String {
    let mut out = String::new();
    for table in c.schema.tables {
        out += &format!("## {}\n", table.table);
        out += &dump(c, &format!("SELECT * FROM \"{}\" ORDER BY id", table.table));
        out += "\n";
    }
    for table in DYNAMIC_TABLES {
        out += &format!("## {}\n", table.table);
        out += &dump(
            c,
            &format!("SELECT * FROM \"{}\" ORDER BY 1, 2", table.table),
        );
        out += "\n";
    }
    for table in APPEND_ONLY_TABLES {
        out += &format!("## {}\n", table.table);
        out += &dump(c, &format!("SELECT * FROM \"{}\" ORDER BY id", table.table));
        out += "\n";
    }
    out += "## clocks\n";
    out += &dump(
        c,
        "SELECT entity_type, entity_id, field, hlc, device FROM register_clock ORDER BY 1, 2, 3",
    );
    out += "\n## schedule\n";
    out += &dump(c, "SELECT * FROM card_schedule ORDER BY card");
    out += "\n## reading\n";
    out += &format!("{:?}\n", c.decks().unwrap());
    out += &format!("{:?}\n", c.deleted_decks().unwrap());
    out += &format!("{:?}\n", c.note_types().unwrap());
    out += &format!("{:?}\n", c.deleted_note_types().unwrap());
    out += &format!("{:?}\n", c.presets().unwrap());
    out += &format!("{:?}\n", c.deleted_presets().unwrap());
    out += &format!("{:?}\n", c.saved_searches().unwrap());
    out += &format!("{:?}\n", c.media_files().unwrap());
    out += &format!("{:?}\n", c.tags().unwrap());
    out += &format!("{:?}\n", c.deleted_notes().unwrap());
    for note_type in c.note_types().unwrap() {
        for note in c.notes(note_type.id).unwrap() {
            out += &format!("{note:?}\n");
            out += &format!("{:?}\n", c.cards_of_note(note.id).unwrap());
            out += &format!("{:?}\n", c.deleted_cards_of_note(note.id).unwrap());
            out += &format!("{:?}\n", c.note_tags(note.id).unwrap());
        }
    }
    out
}

pub(super) fn basic_fields(c: &Collection) -> (Id, Id) {
    let note_type = c.note_type(builtin::basic()).unwrap().unwrap();
    (note_type.fields[0].id, note_type.fields[1].id)
}

/// A Basic note. Returns the note and its card.
pub(super) fn add(c: &Collection, front: &str, back: &str) -> (Id, Id) {
    let (f, b) = basic_fields(c);
    let added = c
        .add_note(builtin::basic(), &[(f, front), (b, back)])
        .unwrap();
    (added.id, added.cards[0])
}

pub(super) fn front(c: &Collection, note: Id) -> String {
    c.note(note).unwrap().unwrap().fields[0].value.clone()
}

pub(super) fn back(c: &Collection, note: Id) -> String {
    c.note(note).unwrap().unwrap().fields[1].value.clone()
}

pub(super) fn edit_front(c: &Collection, note: Id, value: &str) {
    let (f, _) = basic_fields(c);
    c.set_note_fields(note, &[(f, value)]).unwrap();
}

pub(super) fn edit_back(c: &Collection, note: Id, value: &str) {
    let (_, b) = basic_fields(c);
    c.set_note_fields(note, &[(b, value)]).unwrap();
}

fn count(c: &Collection, sql: &str) -> i64 {
    c.conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn events_of(c: &Collection, card: Id) -> usize {
    c.card_events(card).unwrap().len()
}

fn all_cards(c: &Collection) -> usize {
    c.search("", &crate::search::SearchOptions::default())
        .unwrap()
        .total as usize
}

// ---- Changes ----

#[test]
fn changes_all_has_every_register_and_unpushed_has_only_local_writes() {
    let (a, b) = pair();
    let (note, _) = add(&a.c, "front", "back");
    a.clock.advance(MINUTE);
    let all = a.c.changes(Selection::All).unwrap();
    let unpushed = a.c.changes(Selection::Unpushed).unwrap();
    assert!(
        all.registers.len() > unpushed.registers.len(),
        "seeds are in All only"
    );
    assert!(
        unpushed
            .registers
            .iter()
            .any(|r| r.entity == "note" && r.entity_id == note && r.field == "deleted")
    );
    // A batch that came from elsewhere is not unpushed.
    b.c.merge(&all).unwrap();
    let from_elsewhere = b.c.changes(Selection::Unpushed).unwrap();
    assert!(from_elsewhere.registers.is_empty(), "{from_elsewhere:?}");
    assert!(from_elsewhere.rows.is_empty());
}

#[test]
fn a_register_with_no_clock_is_not_sent() {
    let (a, _) = pair();
    let (_, card) = add(&a.c, "x", "y");
    // Rows made before v8 had `suspended` with no clock: the same default on every device.
    a.c.conn
        .execute(
            "DELETE FROM register_clock WHERE entity_type = 'card' AND field = 'suspended'",
            [],
        )
        .unwrap();
    let all = a.c.changes(Selection::All).unwrap();
    assert!(
        !all.registers
            .iter()
            .any(|r| r.entity == "card" && r.entity_id == card && r.field == "suspended")
    );
    assert!(
        all.registers
            .iter()
            .any(|r| r.entity == "card" && r.entity_id == card && r.field == "deleted")
    );
}

#[test]
fn seeds_are_sent_and_change_nothing() {
    let (a, b) = pair();
    let before = digest(&b.c);
    let all = a.c.changes(Selection::All).unwrap();
    let seeds = all
        .registers
        .iter()
        .filter(|r| r.clock.hlc == Hlc::ZERO)
        .count();
    assert!(seeds > 0);
    let report = b.c.merge(&all).unwrap();
    assert_eq!(report.registers_applied, 0);
    assert_eq!(report.registers_ignored, all.registers.len());
    assert_eq!(digest(&b.c), before);
}

#[test]
fn changes_carry_the_columns_of_unknown_rows_and_unknown_registers() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    crate::study::answer_tests::good(&a.c, card);
    let mut all = a.c.changes(Selection::All).unwrap();
    // An event with a column and a kind from a newer app, and a register of a new entity type.
    let mut event = all
        .rows
        .iter()
        .find(|r| r.entity == "card_event")
        .unwrap()
        .clone();
    event.id = Id::from_bytes([9; 16]);
    event
        .columns
        .push(("from_the_future".to_owned(), Value::Text("keep".to_owned())));
    all.rows.push(event);
    let future = RegisterChange {
        entity: "gadget".to_owned(),
        entity_id: Id::from_bytes([3; 16]),
        field: "colour".to_owned(),
        value: Value::Text("red".to_owned()),
        clock: Clock {
            hlc: Hlc::from_parts(START, 1),
            device: Id::from_bytes([5; 16]),
        },
    };
    all.registers.push(future.clone());
    all.rows.push(RowChange {
        entity: "gizmo".to_owned(),
        id: Id::from_bytes([4; 16]),
        columns: vec![("n".to_owned(), Value::Integer(3))],
    });
    let report = b.c.merge(&all).unwrap();
    assert_eq!((report.unknown_registers, report.unknown_rows), (1, 2));
    let again = b.c.changes(Selection::All).unwrap();
    assert!(again.registers.contains(&future));
    let kept = again
        .rows
        .iter()
        .find(|r| r.id == Id::from_bytes([9; 16]))
        .unwrap();
    assert_eq!(
        kept.columns.last().unwrap(),
        &("from_the_future".to_owned(), Value::Text("keep".to_owned()))
    );
    assert!(again.rows.iter().any(|r| r.entity == "gizmo"));
    // None of it is for a push.
    let unpushed = b.c.changes(Selection::Unpushed).unwrap();
    assert!(unpushed.rows.is_empty() && unpushed.registers.is_empty());
}

// ---- The merge rule ----

fn change(entity: &str, id: Id, field: &str, value: Value, hlc: Hlc, device: u8) -> RegisterChange {
    RegisterChange {
        entity: entity.to_owned(),
        entity_id: id,
        field: field.to_owned(),
        value,
        clock: Clock {
            hlc,
            device: Id::from_bytes([device; 16]),
        },
    }
}

fn at(ms: i64) -> Hlc {
    Hlc::from_parts(ms, 0)
}

fn batch(registers: Vec<RegisterChange>) -> Changes {
    Changes {
        registers,
        rows: Vec::new(),
    }
}

#[test]
fn the_higher_clock_wins_and_equal_or_lower_changes_nothing() {
    let (a, _) = pair();
    let (note, _) = add(&a.c, "mine", "b");
    let (f, _) = basic_fields(&a.c);
    let name = f.to_string();
    let local = a.c.register_clock("note", note, &name).unwrap().unwrap();
    // Lower: ignored.
    let lower = change(
        "note",
        note,
        &name,
        Value::Text("old".into()),
        Hlc::from_stored(local.hlc.to_stored() - 1),
        0,
    );
    let report = a.c.merge(&batch(vec![lower])).unwrap();
    assert_eq!((report.registers_applied, report.registers_ignored), (0, 1));
    assert_eq!(front(&a.c, note), "mine");
    // Equal: ignored.
    let equal = change(
        "note",
        note,
        &name,
        Value::Text("same".into()),
        local.hlc,
        0,
    );
    let equal = RegisterChange {
        clock: Clock {
            hlc: local.hlc,
            device: local.device,
        },
        ..equal
    };
    assert_eq!(a.c.merge(&batch(vec![equal])).unwrap().registers_applied, 0);
    assert_eq!(front(&a.c, note), "mine");
    // Higher: wins, and the clock is the remote one, already pushed.
    let higher = change(
        "note",
        note,
        &name,
        Value::Text("new".into()),
        at(START + DAY),
        9,
    );
    let report = a.c.merge(&batch(vec![higher])).unwrap();
    assert_eq!(report.registers_applied, 1);
    assert_eq!(front(&a.c, note), "new");
    let clock = a.c.register_clock("note", note, &name).unwrap().unwrap();
    assert_eq!(
        (clock.hlc, clock.device, clock.pushed),
        (at(START + DAY), Id::from_bytes([9; 16]), true)
    );
}

#[test]
fn equal_hlcs_are_decided_by_the_device() {
    let (a, _) = pair();
    let (note, _) = add(&a.c, "mine", "b");
    let (f, _) = basic_fields(&a.c);
    let name = f.to_string();
    let local = a.c.register_clock("note", note, &name).unwrap().unwrap();
    let low = RegisterChange {
        clock: Clock {
            hlc: local.hlc,
            device: Id::from_bytes([0; 16]),
        },
        ..change("note", note, &name, Value::Text("low".into()), local.hlc, 0)
    };
    let high = RegisterChange {
        clock: Clock {
            hlc: local.hlc,
            device: Id::from_bytes([255; 16]),
        },
        ..change(
            "note",
            note,
            &name,
            Value::Text("high".into()),
            local.hlc,
            0,
        )
    };
    a.c.merge(&batch(vec![low])).unwrap();
    assert_eq!(front(&a.c, note), "mine");
    a.c.merge(&batch(vec![high])).unwrap();
    assert_eq!(front(&a.c, note), "high");
}

#[test]
fn a_register_for_a_row_that_does_not_exist_makes_the_row_with_defaults() {
    let (a, _) = pair();
    let deck = Id::from_bytes([8; 16]);
    let report =
        a.c.merge(&batch(vec![change(
            "deck",
            deck,
            "name",
            Value::Text("Remote".into()),
            at(START),
            9,
        )]))
        .unwrap();
    assert_eq!(report.registers_applied, 1);
    let found = a.c.deck(deck).unwrap().unwrap();
    assert_eq!(found.name, "Remote");
    assert!(!found.deleted);
    // Only the register that arrived has a clock.
    assert!(a.c.register_clock("deck", deck, "name").unwrap().is_some());
    assert!(
        a.c.register_clock("deck", deck, "parent")
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_hlc_moves_to_the_highest_received_clock_even_when_it_loses() {
    let (a, _) = pair();
    let (note, _) = add(&a.c, "mine", "b");
    let (f, _) = basic_fields(&a.c);
    let far = at(START + 3 * HOUR);
    // This register loses (it is older than the local one... made so by editing later locally).
    a.clock.set(START + 10 * DAY);
    edit_front(&a.c, note, "later");
    let loses = change(
        "note",
        note,
        &f.to_string(),
        Value::Text("x".into()),
        far,
        9,
    );
    a.c.merge(&batch(vec![loses])).unwrap();
    assert!(a.c.hlc().unwrap() >= at(START + 10 * DAY));
    // A clock hours ahead, which wins, moves the HLC and the next local write goes after it.
    let ahead = at(START + 30 * DAY);
    let wins = change(
        "note",
        note,
        &f.to_string(),
        Value::Text("y".into()),
        ahead,
        9,
    );
    a.c.merge(&batch(vec![wins])).unwrap();
    assert_eq!(a.c.hlc().unwrap(), ahead);
    edit_front(&a.c, note, "mine again");
    let clock =
        a.c.register_clock("note", note, &f.to_string())
            .unwrap()
            .unwrap();
    assert!(clock.hlc > ahead);
    assert_eq!(front(&a.c, note), "mine again");
}

#[test]
fn a_failed_merge_changes_nothing() {
    let (a, _) = pair();
    let (note, _) = add(&a.c, "mine", "b");
    let (f, _) = basic_fields(&a.c);
    let before = digest(&a.c);
    let wins = change(
        "note",
        note,
        &f.to_string(),
        Value::Text("new".into()),
        at(START + DAY),
        9,
    );
    let breaks = change(
        "deck",
        Id::from_bytes([1; 16]),
        "name",
        Value::Text("ok".into()),
        at(START),
        9,
    );
    a.c.conn
        .execute_batch("ALTER TABLE deck RENAME TO deck_gone")
        .unwrap();
    assert!(a.c.merge(&batch(vec![wins, breaks])).is_err());
    a.c.conn
        .execute_batch("ALTER TABLE deck_gone RENAME TO deck")
        .unwrap();
    assert_eq!(digest(&a.c), before);
}

// ---- Rejected values ----

#[test]
fn values_that_do_not_fit_are_rejected_and_the_rest_applies() {
    let (a, b) = pair();
    let (note, card) = add(&a.c, "x", "y");
    crate::study::answer_tests::good(&a.c, card);
    let mut batch_ = a.c.changes(Selection::All).unwrap();
    let later = at(START + DAY);
    batch_
        .registers
        .push(change("card", card, "deleted", Value::Null, later, 9));
    batch_.registers.push(change(
        "card",
        card,
        "suspended",
        Value::Text("yes".into()),
        later,
        9,
    ));
    batch_.registers.push(change(
        "deck",
        Id::from_bytes([7; 16]),
        "name",
        Value::Text("Fine".into()),
        later,
        9,
    ));
    let mut event = batch_
        .rows
        .iter()
        .find(|r| r.entity == "card_event")
        .unwrap()
        .clone();
    event.id = Id::from_bytes([6; 16]);
    for (name, value) in &mut event.columns {
        if name == "time_ms" {
            *value = Value::Text("noon".into());
        }
    }
    batch_.rows.push(event);
    let report = b.c.merge(&batch_).unwrap();
    assert_eq!(report.rejected.len(), 3, "{:?}", report.rejected);
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.field.as_deref() == Some("deleted"))
    );
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.field.as_deref() == Some("suspended"))
    );
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.field.is_none() && r.reason.contains("time_ms"))
    );
    // The rest of the batch applied: the note, the card and the valid deck.
    assert_eq!(front(&b.c, note), "x");
    assert_eq!(
        b.c.deck(Id::from_bytes([7; 16])).unwrap().unwrap().name,
        "Fine"
    );
    assert_eq!(events_of(&b.c, card), 1);
    assert_eq!(
        count(&b.c, "SELECT COUNT(*) FROM card WHERE suspended <> 0"),
        0
    );
    assert!(
        b.c.register_clock("card", card, "deleted")
            .unwrap()
            .is_some()
    );
}

#[test]
fn an_integer_fits_a_real_column_and_nothing_else_does() {
    let column = Column {
        kind: Kind_::Real,
        not_null: true,
    };
    assert!(column.check(&Value::Integer(1)).is_ok());
    assert!(column.check(&Value::Real(1.5)).is_ok());
    assert!(column.check(&Value::Text("1".into())).is_err());
    let integer = Column {
        kind: Kind_::Integer,
        not_null: false,
    };
    assert!(integer.check(&Value::Real(1.5)).is_err());
    assert!(integer.check(&Value::Null).is_ok());
}

// ---- Rows of ADR 0006, section 11 ----

#[test]
fn offline_for_weeks() {
    let (a, b) = pair();
    let notes: Vec<Id> = (0..20)
        .map(|i| add(&a.c, &format!("n{i}"), "b").0)
        .collect();
    sync(&a, &b);
    // B works alone for three weeks, 200 edits.
    for i in 0..200 {
        b.clock.advance(2 * HOUR + 3 * MINUTE * (i % 7));
        edit_front(&b.c, notes[(i % 20) as usize], &format!("b{i}"));
    }
    assert!(b.clock.now().unix_ms - START > 17 * DAY);
    // A edits a few of the same notes later than all of that.
    a.clock.set(b.clock.now().unix_ms + DAY);
    for note in &notes[..5] {
        edit_front(&a.c, *note, "from A");
    }
    sync(&a, &b);
    settle(&[&a, &b]);
    for note in &notes[..5] {
        assert_eq!(front(&a.c, *note), "from A");
        assert_eq!(front(&b.c, *note), "from A");
    }
    for (i, note) in notes.iter().enumerate().skip(5) {
        let last = (0..200).rev().find(|n| n % 20 == i).unwrap();
        assert_eq!(front(&a.c, *note), format!("b{last}"));
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn the_same_review_card_answered_on_two_devices_counts_about_once() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    crate::study::answer_tests::good(&a.c, card);
    a.clock.advance(11 * MINUTE);
    crate::study::answer_tests::good(&a.c, card);
    // A control that is answered once, on the same day.
    sync(&a, &b);
    let control = Dev::new(3, START);
    control
        .c
        .merge(&a.c.changes(Selection::All).unwrap())
        .unwrap();
    a.clock.advance(5 * DAY);
    b.clock.set(a.clock.now().unix_ms + 5 * MINUTE);
    control.clock.set(a.clock.now().unix_ms);
    a.c.answer(card, Rating::Good, 2_000).unwrap();
    b.c.answer(card, Rating::Good, 2_000).unwrap();
    control.c.answer(card, Rating::Good, 2_000).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    assert_eq!(events_of(&a.c, card), 4);
    assert_eq!(events_of(&b.c, card), 4);
    let (sa, sb) = (
        a.c.card_schedule(card).unwrap(),
        b.c.card_schedule(card).unwrap(),
    );
    assert_eq!(sa, sb);
    let once = control.c.card_schedule(card).unwrap();
    let (due, once_due) = (sa.due_day.unwrap(), once.due_day.unwrap());
    // A second answer minutes after the first changes the interval by a day or so, not by a
    // whole new review.
    assert!((due - once_due).abs() <= 2, "{due} against {once_due}");
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn a_new_card_answered_on_two_devices_is_replayed_in_order() {
    // ADR 0007, part 6: a concurrent event is recomputed from the state the fold has reached, so
    // two first answers make two steps.
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    sync(&a, &b);
    crate::study::answer_tests::good(&a.c, card);
    b.clock.advance(5 * MINUTE);
    b.c.answer(card, Rating::Good, 2_000).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    assert_eq!(events_of(&a.c, card), 2);
    assert_eq!(events_of(&b.c, card), 2);
    assert_eq!(
        a.c.card_schedule(card).unwrap(),
        b.c.card_schedule(card).unwrap()
    );
    assert_eq!(a.c.card_schedule(card).unwrap().answers, 2);
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn good_on_one_device_and_again_on_another_counts_a_lapse() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    crate::study::answer_tests::good(&a.c, card);
    a.clock.advance(11 * MINUTE);
    crate::study::answer_tests::good(&a.c, card);
    assert_eq!(
        a.c.card_schedule(card).unwrap().state,
        crate::scheduling::CardState::Review
    );
    sync(&a, &b);
    a.clock.advance(5 * DAY);
    b.clock.set(a.clock.now().unix_ms + 5 * MINUTE);
    a.c.answer(card, Rating::Good, 2_000).unwrap();
    b.c.answer(card, Rating::Again, 2_000).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    let schedule = a.c.card_schedule(card).unwrap();
    assert_eq!(schedule.lapses, 1);
    assert_eq!(schedule, b.c.card_schedule(card).unwrap());
    assert_eq!(events_of(&a.c, card), 4);
}

#[test]
fn an_undo_that_has_synced() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    sync(&a, &b);
    crate::study::answer_tests::good(&a.c, card);
    sync(&a, &b);
    assert_eq!(b.c.card_schedule(card).unwrap().answers, 1);
    a.clock.advance(MINUTE);
    a.c.undo_answer().unwrap().unwrap();
    sync(&a, &b);
    let schedule = b.c.card_schedule(card).unwrap();
    assert_eq!(schedule.answers, 0);
    assert_eq!(schedule.state, crate::scheduling::CardState::New);
    // The review stays stored, with the void beside it.
    assert_eq!(events_of(&b.c, card), 2);
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn the_same_field_on_two_devices_the_later_edit_wins() {
    let (a, b) = pair();
    let (note, _) = add(&a.c, "x", "y");
    sync(&a, &b);
    a.clock.advance(HOUR);
    edit_front(&a.c, note, "from A");
    b.clock.advance(2 * HOUR);
    edit_front(&b.c, note, "from B");
    sync(&a, &b);
    assert_eq!(front(&a.c, note), "from B");
    assert_eq!(front(&b.c, note), "from B");
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn different_fields_and_tags_of_one_note_are_both_kept() {
    let (a, b) = pair();
    let (note, _) = add(&a.c, "x", "y");
    sync(&a, &b);
    a.clock.advance(HOUR);
    edit_front(&a.c, note, "front A");
    a.c.add_tags(&[note], &["alpha"]).unwrap();
    b.clock.advance(HOUR);
    edit_back(&b.c, note, "back B");
    b.c.add_tags(&[note], &["beta"]).unwrap();
    sync(&a, &b);
    for d in [&a, &b] {
        assert_eq!(front(&d.c, note), "front A");
        assert_eq!(back(&d.c, note), "back B");
        assert_eq!(d.c.note_tags(note).unwrap(), ["alpha", "beta"]);
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn an_edit_on_one_device_and_a_delete_on_another() {
    let (a, b) = pair();
    let (note, _) = add(&a.c, "x", "y");
    sync(&a, &b);
    a.clock.advance(HOUR);
    edit_front(&a.c, note, "edited");
    b.clock.advance(2 * HOUR);
    b.c.delete_note(note).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        assert_eq!(d.c.deleted_notes().unwrap().len(), 1);
        assert!(d.c.note_type(builtin::basic()).unwrap().is_some());
        assert_eq!(d.c.notes(builtin::basic()).unwrap().len(), 0);
    }
    a.c.restore_note(note).unwrap();
    assert_eq!(front(&a.c, note), "edited");
    sync(&a, &b);
    assert_eq!(front(&b.c, note), "edited");
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn a_review_on_one_device_and_a_delete_on_another() {
    let (a, b) = pair();
    let (note, card) = add(&a.c, "x", "y");
    sync(&a, &b);
    crate::study::answer_tests::good(&a.c, card);
    b.clock.advance(HOUR);
    b.c.delete_note(note).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        assert_eq!(d.c.deleted_cards_of_note(note).unwrap().len(), 1);
        assert!(d.c.cards_of_note(note).unwrap().is_empty());
        assert_eq!(events_of(&d.c, card), 1);
    }
    b.c.restore_note(note).unwrap();
    assert_eq!(b.c.card_schedule(card).unwrap().answers, 1);
    sync(&a, &b);
    assert_eq!(a.c.cards_of_note(note).unwrap().len(), 1);
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn a_note_added_to_a_deck_that_is_deleted_elsewhere() {
    let (a, b) = pair();
    let deck = a.c.create_deck("Gone", None).unwrap();
    let (f, back_) = basic_fields(&a.c);
    let old: Vec<Id> = (0..2)
        .map(|i| {
            a.c.add_note_to_deck(
                deck,
                builtin::basic(),
                &[(f, &format!("old{i}")), (back_, "b")],
            )
            .unwrap()
            .id
        })
        .collect();
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.delete_deck(deck).unwrap();
    b.clock.advance(2 * HOUR);
    let new =
        b.c.add_note_to_deck(deck, builtin::basic(), &[(f, "new"), (back_, "b")])
            .unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        let decks = d.c.decks().unwrap();
        assert!(decks.iter().any(|x| x.id == deck), "the deck reads as live");
        assert_eq!(d.c.cards_of_note(new.id).unwrap().len(), 1);
        for note in &old {
            assert!(d.c.cards_of_note(*note).unwrap().is_empty());
        }
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn a_note_type_deleted_while_notes_are_added_elsewhere() {
    let (a, b) = pair();
    let note_type = a.c.create_note_type("Mine", Kind::Standard).unwrap();
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.delete_note_type(note_type).unwrap();
    b.clock.advance(2 * HOUR);
    let fields = b.c.note_type(note_type).unwrap().unwrap().fields;
    let added =
        b.c.add_note(note_type, &[(fields[0].id, "q"), (fields[1].id, "a")])
            .unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        assert!(d.c.note_types().unwrap().iter().any(|t| t.id == note_type));
        assert!(
            !d.c.deleted_note_types()
                .unwrap()
                .iter()
                .any(|t| t.id == note_type)
        );
        assert_eq!(d.c.cards_of_note(added.id).unwrap().len(), 1);
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn a_field_removed_while_a_value_is_edited() {
    let (a, b) = pair();
    let note_type = a.c.create_note_type("Mine", Kind::Standard).unwrap();
    let extra = a.c.add_field(note_type, "Extra").unwrap();
    let fields = a.c.note_type(note_type).unwrap().unwrap().fields;
    let note =
        a.c.add_note(
            note_type,
            &[(fields[0].id, "q"), (fields[1].id, "a"), (extra, "one")],
        )
        .unwrap()
        .id;
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.remove_field(note_type, extra).unwrap();
    b.clock.advance(2 * HOUR);
    b.c.set_note_fields(note, &[(extra, "two")]).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        assert_eq!(d.c.note(note).unwrap().unwrap().fields.len(), 2, "hidden");
        d.c.restore_field(note_type, extra).unwrap();
        let shown = d.c.note(note).unwrap().unwrap();
        assert_eq!(
            shown.fields.last().unwrap().value,
            "two",
            "the edit was kept"
        );
    }
}

#[test]
fn a_template_removed_while_a_card_is_reviewed() {
    let (a, b) = pair();
    let note_type = a.c.create_note_type("Mine", Kind::Standard).unwrap();
    let second =
        a.c.add_template(note_type, "Reverse", "{{Back}}", "{{Front}}")
            .unwrap();
    let fields = a.c.note_type(note_type).unwrap().unwrap().fields;
    let note =
        a.c.add_note(note_type, &[(fields[0].id, "q"), (fields[1].id, "a")])
            .unwrap();
    assert_eq!(note.cards.len(), 2);
    let reverse =
        a.c.cards_of_note(note.id)
            .unwrap()
            .into_iter()
            .find(|c| c.template == second)
            .unwrap()
            .id;
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.remove_template(note_type, second).unwrap();
    b.clock.advance(2 * HOUR);
    b.c.answer(reverse, Rating::Good, 1_000).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        assert!(
            d.c.deleted_cards_of_note(note.id)
                .unwrap()
                .iter()
                .any(|c| c.id == reverse)
        );
        assert_eq!(events_of(&d.c, reverse), 1);
        d.c.restore_template(note_type, second).unwrap();
        assert!(
            d.c.cards_of_note(note.id)
                .unwrap()
                .iter()
                .any(|c| c.id == reverse)
        );
        assert_eq!(d.c.card_schedule(reverse).unwrap().answers, 1);
    }
}

#[test]
fn suspend_on_one_device_and_a_review_on_another() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.suspend_cards(&[card]).unwrap();
    b.clock.advance(2 * HOUR);
    b.c.answer(card, Rating::Good, 1_000).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        assert_eq!(count(&d.c, "SELECT suspended FROM card"), 1);
        assert_eq!(d.c.card_schedule(card).unwrap().answers, 1);
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn deck_options_changed_on_two_devices_merge_field_by_field() {
    let (a, b) = pair();
    let preset = a.c.create_preset("Mine").unwrap();
    sync(&a, &b);
    let mut x = default_parameters().to_vec();
    let mut y = x.clone();
    x[0] += 0.01;
    y[0] += 0.02;
    a.clock.advance(HOUR);
    a.c.set_preset_options(
        preset,
        &PresetChange {
            new_per_day: Some(5),
            fsrs_parameters: Some(x),
            ..Default::default()
        },
    )
    .unwrap();
    b.clock.advance(2 * HOUR);
    b.c.set_preset_options(
        preset,
        &PresetChange {
            reviews_per_day: Some(50),
            fsrs_parameters: Some(y.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    sync(&a, &b);
    for d in [&a, &b] {
        let found = d.c.preset(preset).unwrap().unwrap();
        assert_eq!((found.new_per_day, found.reviews_per_day), (5, 50));
        assert_eq!(found.fsrs_parameters, y, "the parameters win whole");
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn two_devices_move_decks_into_each_other() {
    let (a, b) = pair();
    let x = a.c.create_deck("X", None).unwrap();
    let y = a.c.create_deck("Y", None).unwrap();
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.move_deck(x, Some(y)).unwrap();
    b.clock.advance(2 * HOUR);
    b.c.move_deck(y, Some(x)).unwrap();
    sync(&a, &b);
    let paths = |d: &Dev| {
        d.c.decks()
            .unwrap()
            .iter()
            .map(|k| k.path.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(paths(&a), paths(&b));
    assert_eq!(a.c.decks().unwrap().len(), 3);
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn two_collections_created_separately_share_the_built_ins() {
    let (a, b) = pair();
    a.c.create_deck("Spanish", None).unwrap();
    b.clock.advance(MINUTE);
    b.c.create_deck("Spanish", None).unwrap();
    sync(&a, &b);
    for d in [&a, &b] {
        assert_eq!(d.c.note_types().unwrap().len(), 3);
        assert_eq!(d.c.presets().unwrap().len(), 1);
        let names: Vec<String> =
            d.c.decks()
                .unwrap()
                .iter()
                .map(|k| k.display_name.clone())
                .collect();
        assert_eq!(names.iter().filter(|n| *n == "Default").count(), 1);
        assert!(names.contains(&"Spanish".to_owned()) && names.contains(&"Spanish (2)".to_owned()));
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn a_device_clock_wrong_by_hours() {
    let (a, b) = pair();
    let (note, _) = add(&a.c, "x", "y");
    sync(&a, &b);
    a.clock.advance(HOUR);
    edit_front(&a.c, note, "A, on time");
    // B is three hours ahead: its edit, made earlier in real time, wins.
    b.clock.set(a.clock.now().unix_ms - 30 * MINUTE + 3 * HOUR);
    edit_front(&b.c, note, "B, ahead");
    sync(&a, &b);
    assert_eq!(front(&a.c, note), "B, ahead");
    let (f, _) = basic_fields(&a.c);
    let received =
        a.c.register_clock("note", note, &f.to_string())
            .unwrap()
            .unwrap()
            .hlc;
    // A's next write beats every clock it received, although its own clock is behind.
    edit_front(&a.c, note, "A, after the merge");
    let mine =
        a.c.register_clock("note", note, &f.to_string())
            .unwrap()
            .unwrap()
            .hlc;
    assert!(mine > received);
    sync(&a, &b);
    assert_eq!(front(&b.c, note), "A, after the merge");
}

#[test]
fn applying_half_a_batch_and_then_all_of_it_equals_applying_it_once() {
    let (a, _) = pair();
    for i in 0..10 {
        let (_, card) = add(&a.c, &format!("n{i}"), "b");
        if i % 3 == 0 {
            a.clock.advance(MINUTE);
            crate::study::answer_tests::good(&a.c, card);
        }
    }
    let all = a.c.changes(Selection::All).unwrap();
    let half = Changes {
        registers: all.registers[..all.registers.len() / 2].to_vec(),
        rows: all.rows[..all.rows.len() / 2].to_vec(),
    };
    let once = Dev::new(3, START);
    once.c.merge(&all).unwrap();
    let twice = Dev::new(4, START);
    twice.c.merge(&half).unwrap();
    twice.c.merge(&all).unwrap();
    assert_eq!(digest(&once.c), digest(&twice.c));
    // And again changes nothing.
    let before = digest(&twice.c);
    let report = twice.c.merge(&all).unwrap();
    assert_eq!(
        (
            report.registers_applied,
            report.rows_added,
            report.notes_reconciled,
            report.cards_rebuilt
        ),
        (0, 0, 0, 0)
    );
    assert!(report.rejected.is_empty());
    assert_eq!(digest(&twice.c), before);
}

#[test]
fn an_empty_collection_given_all_the_changes_equals_the_original() {
    let (a, _) = pair();
    let (note, card) = add(&a.c, "x", "y");
    a.c.add_tags(&[note], &["t"]).unwrap();
    crate::study::answer_tests::good(&a.c, card);
    a.c.create_deck("D", None).unwrap();
    let fresh = Dev::new(3, START);
    fresh
        .c
        .merge(&a.c.changes(Selection::All).unwrap())
        .unwrap();
    assert_eq!(digest(&fresh.c), digest(&a.c));
}

#[test]
fn media_added_on_both_devices_is_one_file_and_a_used_file_is_alive() {
    let (a, b) = pair();
    let bytes = b"the same picture";
    let on_a = a.c.add_media("pic.png", bytes).unwrap();
    b.clock.advance(MINUTE);
    let on_b = b.c.add_media("pic.png", bytes).unwrap();
    assert_eq!(on_a.name, on_b.name);
    sync(&a, &b);
    assert_eq!(a.c.media_files().unwrap().len(), 1);
    assert_eq!(b.c.media_files().unwrap().len(), 1);
    // A deletes it as unused while B adds a note that names it.
    a.clock.advance(HOUR);
    assert_eq!(a.c.delete_unused_media().unwrap().len(), 1);
    b.clock.advance(2 * HOUR);
    add(&b.c, &format!("<img src=\"{}\">", on_a.name), "b");
    sync(&a, &b);
    for d in [&a, &b] {
        let files = d.c.media_files().unwrap();
        assert_eq!(files.len(), 1);
        assert!(
            !files[0].deleted,
            "a deleted file that is still named is alive"
        );
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn a_card_that_arrives_before_its_note_is_hidden_and_then_shown() {
    let (a, b) = pair();
    let (note, card) = add(&a.c, "x", "y");
    let all = a.c.changes(Selection::All).unwrap();
    let (cards, rest): (Vec<_>, Vec<_>) = all
        .registers
        .into_iter()
        .partition(|r| r.entity == "card" && r.entity_id == card);
    assert!(!cards.is_empty());
    b.c.merge(&batch(cards)).unwrap();
    assert_eq!(all_cards(&b.c), 0, "no note yet, so no card");
    b.c.merge(&Changes {
        registers: rest,
        rows: all.rows,
    })
    .unwrap();
    assert_eq!(all_cards(&b.c), 1);
    assert_eq!(front(&b.c, note), "x");
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn merge_time_reconcile_brings_back_a_card_neither_device_deleted_alone() {
    // ADR 0008, finding 3. The reverse card needs the Back field.
    let (a, b) = pair();
    let (f, back_) = {
        let t =
            a.c.note_type(builtin::basic_and_reversed())
                .unwrap()
                .unwrap();
        (t.fields[0].id, t.fields[1].id)
    };
    let note =
        a.c.add_note(builtin::basic_and_reversed(), &[(f, "x"), (back_, "y")])
            .unwrap();
    assert_eq!(note.cards.len(), 2);
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.set_note_fields(note.id, &[(back_, "")]).unwrap();
    assert_eq!(a.c.cards_of_note(note.id).unwrap().len(), 1);
    b.clock.advance(2 * HOUR);
    b.c.set_note_fields(note.id, &[(back_, "z")]).unwrap();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        assert_eq!(d.c.note(note.id).unwrap().unwrap().fields[1].value, "z");
        assert_eq!(
            d.c.cards_of_note(note.id).unwrap().len(),
            2,
            "the reverse card is back"
        );
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn merge_time_reconcile_gives_new_notes_a_template_added_elsewhere() {
    let (a, b) = pair();
    let note_type = a.c.create_note_type("Mine", Kind::Standard).unwrap();
    sync(&a, &b);
    a.clock.advance(HOUR);
    a.c.add_template(note_type, "Reverse", "{{Back}}", "{{Front}}")
        .unwrap();
    b.clock.advance(2 * HOUR);
    let fields = b.c.note_type(note_type).unwrap().unwrap().fields;
    let notes: Vec<Id> = (0..3)
        .map(|i| {
            b.c.add_note(
                note_type,
                &[(fields[0].id, &format!("q{i}")), (fields[1].id, "a")],
            )
            .unwrap()
            .id
        })
        .collect();
    sync(&a, &b);
    settle(&[&a, &b]);
    for d in [&a, &b] {
        for note in &notes {
            assert_eq!(d.c.cards_of_note(*note).unwrap().len(), 2);
        }
    }
    assert_eq!(digest(&a.c), digest(&b.c));
}

#[test]
fn an_unknown_event_kind_is_kept_relayed_and_does_not_break_the_fold() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    crate::study::answer_tests::good(&a.c, card);
    let mut all = a.c.changes(Selection::All).unwrap();
    let mut event = all
        .rows
        .iter()
        .find(|r| r.entity == "card_event")
        .unwrap()
        .clone();
    event.id = Id::from_bytes([9; 16]);
    for (name, value) in &mut event.columns {
        match name.as_str() {
            "kind" => *value = Value::Text("hologram".into()),
            "time_ms" => *value = Value::Integer(START + HOUR),
            "previous" => *value = Value::Null,
            _ => {}
        }
    }
    event
        .columns
        .push(("shape".into(), Value::Text("cube".into())));
    all.rows.push(event.clone());
    b.c.merge(&all).unwrap();
    assert_eq!(events_of(&b.c, card), 2);
    // Relayed unchanged to a third collection.
    let third = Dev::new(3, START);
    third
        .c
        .merge(&b.c.changes(Selection::All).unwrap())
        .unwrap();
    let relayed = third.c.changes(Selection::All).unwrap();
    let kept = relayed.rows.iter().find(|r| r.id == event.id).unwrap();
    for column in &event.columns {
        assert!(kept.columns.contains(column), "{column:?}");
    }
    assert_eq!(kept.columns.len(), event.columns.len());
    // The fold gives the same answer everywhere.
    assert_eq!(
        b.c.card_schedule(card).unwrap(),
        third.c.card_schedule(card).unwrap()
    );
}

#[test]
fn a_parameter_set_that_arrives_after_its_events_rebuilds_their_cards() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    let preset = crate::deck::default_preset();
    let mut params = default_parameters().to_vec();
    params[0] += 0.5;
    a.c.set_preset_options(
        preset,
        &PresetChange {
            fsrs_parameters: Some(params),
            ..Default::default()
        },
    )
    .unwrap();
    crate::study::answer_tests::good(&a.c, card);
    a.clock.advance(11 * MINUTE);
    crate::study::answer_tests::good(&a.c, card);
    let all = a.c.changes(Selection::All).unwrap();
    let (sets, rest): (Vec<_>, Vec<_>) = all
        .rows
        .into_iter()
        .partition(|r| r.entity == "fsrs_parameter_set");
    assert_eq!(sets.len(), 1);
    b.c.merge(&Changes {
        registers: all.registers,
        rows: rest,
    })
    .unwrap();
    let report =
        b.c.merge(&Changes {
            registers: vec![],
            rows: sets,
        })
        .unwrap();
    assert_eq!(report.cards_rebuilt, 1);
    assert_eq!(
        b.c.card_schedule(card).unwrap(),
        a.c.card_schedule(card).unwrap()
    );
    assert_eq!(digest(&a.c), digest(&b.c));
}
