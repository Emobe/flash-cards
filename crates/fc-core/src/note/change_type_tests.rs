//! Step 3.2e: `change_note_type`, the `transfer` event it writes, and what history and the
//! optimiser make of a transferred card (ADR 0014, decision 6).

use super::*;
use crate::collection::Collection;
use crate::deck::{default_deck, default_preset};
use crate::events::Event;
use crate::events::tests::Recorder;
use crate::notetype::{Kind, builtin};
use crate::scheduling::{CardState, Rating};
use crate::stats::HistoryKind;
use crate::study::answer_tests::{DAY, MINUTE, events, setup};
use crate::study::{CardEvent, EventKind};
use crate::sync::Selection;
use crate::sync::merge_tests::{Dev, START, assert_same, digest, pair, sync};

/// A note type's field IDs and template IDs, in order.
fn parts(c: &Collection, note_type: Id) -> (Vec<Id>, Vec<Id>) {
    let found = c.note_type(note_type).unwrap().unwrap();
    (
        found.fields.iter().map(|f| f.id).collect(),
        found.templates.iter().map(|t| t.id).collect(),
    )
}

fn add_basic(c: &Collection, front: &str) -> (Id, Id) {
    let (f, _) = parts(c, builtin::basic());
    let added = c
        .add_note(builtin::basic(), &[(f[0], front), (f[1], "back")])
        .unwrap();
    (added.id, added.cards[0])
}

/// The cards of a note as `(template, ordinal)` of the live ones, in order.
fn live(c: &Collection, note: Id) -> Vec<Card> {
    let mut cards = c.cards_of_note(note).unwrap();
    cards.sort_by_key(|card| (card.template, card.ordinal));
    cards
}

type Map = Vec<(Id, Option<Id>)>;

/// Maps Basic to Basic and reversed field for field and Card 1 to Card 1.
fn basic_to_reversed(c: &Collection) -> (Map, Map) {
    let (old_f, old_t) = parts(c, builtin::basic());
    let (new_f, new_t) = parts(c, builtin::basic_and_reversed());
    (
        vec![(new_f[0], Some(old_f[0])), (new_f[1], Some(old_f[1]))],
        vec![(new_t[0], Some(old_t[0])), (new_t[1], None)],
    )
}

fn change_to_reversed(c: &Collection, notes: &[Id]) -> TypeChange {
    let (fields, templates) = basic_to_reversed(c);
    c.change_note_type(notes, builtin::basic_and_reversed(), &fields, &templates)
        .unwrap()
}

/// The reverse of `basic_to_reversed`, as an undo writes it.
fn reversed_to_basic(c: &Collection, notes: &[Id]) -> TypeChange {
    let (old_f, old_t) = parts(c, builtin::basic());
    let (new_f, new_t) = parts(c, builtin::basic_and_reversed());
    c.change_note_type(
        notes,
        builtin::basic(),
        &[(old_f[0], Some(new_f[0])), (old_f[1], Some(new_f[1]))],
        &[(old_t[0], Some(new_t[0]))],
    )
    .unwrap()
}

fn schedule_of(c: &Collection, card: Id) -> crate::study::CardSchedule {
    c.card_schedule(card).unwrap()
}

fn transfers(c: &Collection, card: Id) -> Vec<CardEvent> {
    events(c, card)
        .into_iter()
        .filter(|e| e.kind == EventKind::Transfer)
        .collect()
}

fn easy(c: &Collection, card: Id) {
    c.answer(card, Rating::Easy, 2_000).unwrap();
}

fn stored_value(c: &Collection, note: Id, field: Id) -> Option<String> {
    c.conn
        .query_row(
            "SELECT value FROM note_field_value WHERE note = ?1 AND field = ?2",
            rusqlite::params![note, field],
            |row| row.get(0),
        )
        .ok()
}

// ---- The change ----

#[test]
fn basic_to_reversed_keeps_card_1s_schedule_and_card_2_is_new() {
    let (c, _) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    let before = schedule_of(&c, old_card);
    let rec = Recorder::attach(&c);

    let done = change_to_reversed(&c, &[note]);

    assert_eq!(done.from, Some(builtin::basic()));
    assert_eq!(done.notes, vec![note]);
    assert_eq!(done.cards_removed, vec![old_card]);
    assert_eq!(done.cards_added.len(), 2);
    assert_eq!(
        c.note(note).unwrap().unwrap().note_type,
        builtin::basic_and_reversed()
    );
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let cards = live(&c, note);
    assert_eq!(cards.len(), 2);
    let card_1 = cards.iter().find(|card| card.template == new_t[0]).unwrap();
    let card_2 = cards.iter().find(|card| card.template == new_t[1]).unwrap();

    // Card 1 has the schedule of the old card; the old card is in the trash with its history.
    let after = schedule_of(&c, card_1.id);
    assert_eq!(after.state, CardState::Review);
    assert_eq!(after.memory, before.memory);
    assert_eq!(after.due_day, before.due_day);
    assert_eq!(after.last_day, before.last_day);
    assert_eq!(done.carried, vec![(card_1.id, old_card)]);
    let [transfer] = transfers(&c, card_1.id).try_into().unwrap();
    assert_eq!(transfer.source, Some(old_card));
    assert_eq!(transfer.last_day, before.last_day);
    assert_eq!(events(&c, old_card).len(), 1);
    // Card 2 is new.
    assert_eq!(schedule_of(&c, card_2.id).state, CardState::New);
    assert!(events(&c, card_2.id).is_empty());
    // The cache is what the events fold to.
    let built = crate::study::fold_tests::snapshot(&c.conn);
    c.rebuild_schedule().unwrap();
    assert_eq!(crate::study::fold_tests::snapshot(&c.conn), built);

    let (new_f, _) = parts(&c, builtin::basic_and_reversed());
    assert_eq!(
        rec.one(),
        vec![Event::NoteEdited {
            note,
            fields: new_f,
            cards_added: done.cards_added.clone(),
            cards_removed: vec![old_card],
        }]
    );
}

#[test]
fn a_study_day_card_can_be_answered_after_the_change() {
    let (c, clock) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    let due = schedule_of(&c, old_card).due_day.unwrap();
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let card = live(&c, note)
        .into_iter()
        .find(|card| card.template == new_t[0])
        .unwrap();
    // Answered when due: the elapsed days come from the day of the old card's last answer.
    clock.advance((due - schedule_of(&c, card.id).last_day.unwrap()) * DAY);
    c.answer(card.id, Rating::Good, 2_000).unwrap();
    let after = schedule_of(&c, card.id);
    assert_eq!(after.state, CardState::Review);
    assert_eq!(after.answers, 1);
    let log = events(&c, card.id);
    assert_eq!(log.len(), 2);
    assert_eq!(log[1].previous, Some(log[0].id));
}

#[test]
fn the_values_are_mapped_and_the_old_ones_stay_stored() {
    let (c, _) = setup();
    let (note, _) = add_basic(&c, "kot");
    let (old_f, _) = parts(&c, builtin::basic());
    let (new_f, _) = parts(&c, builtin::basic_and_reversed());
    // Front and Back swap places.
    let done = c
        .change_note_type(
            &[note],
            builtin::basic_and_reversed(),
            &[(new_f[0], Some(old_f[1])), (new_f[1], Some(old_f[0]))],
            &[],
        )
        .unwrap();
    assert_eq!(done.cards_added.len(), 2);
    let found = c.note(note).unwrap().unwrap();
    let values: Vec<&str> = found.fields.iter().map(|f| f.value.as_str()).collect();
    assert_eq!(values, vec!["back", "kot"]);
    // The old values were not touched.
    assert_eq!(stored_value(&c, note, old_f[0]).as_deref(), Some("kot"));
    assert_eq!(stored_value(&c, note, old_f[1]).as_deref(), Some("back"));
}

#[test]
fn a_field_mapped_to_nothing_is_emptied_and_one_not_listed_keeps_its_value() {
    let (c, _) = setup();
    let (note, _) = add_basic(&c, "kot");
    let (old_f, _) = parts(&c, builtin::basic());
    let (new_f, _) = parts(&c, builtin::basic_and_reversed());
    c.change_note_type(
        &[note],
        builtin::basic_and_reversed(),
        &[(new_f[0], Some(old_f[0])), (new_f[1], None)],
        &[],
    )
    .unwrap();
    assert_eq!(c.note(note).unwrap().unwrap().fields[1].value, "");
    // Back there and on again with Back not listed: the value it had comes back, hidden until now.
    reversed_to_basic(&c, &[note]);
    c.set_note_fields(note, &[(old_f[1], "answer")]).unwrap();
    c.change_note_type(
        &[note],
        builtin::basic_and_reversed(),
        &[(new_f[0], Some(old_f[0]))],
        &[],
    )
    .unwrap();
    assert_eq!(c.note(note).unwrap().unwrap().fields[1].value, "");
}

#[test]
fn deck_and_suspension_are_carried() {
    let (c, _) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    let deck = c.create_deck("Polish", None).unwrap();
    c.move_cards(&[old_card], deck).unwrap();
    c.suspend_cards(&[old_card]).unwrap();
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let cards = live(&c, note);
    let card_1 = cards.iter().find(|card| card.template == new_t[0]).unwrap();
    assert_eq!(card_1.deck, deck);
    assert!(
        c.suspended_among(&[card_1.id])
            .unwrap()
            .contains(&card_1.id)
    );
    // Card 2 is not carried from anything: it goes where the note's cards were, and is not suspended.
    let card_2 = cards.iter().find(|card| card.template == new_t[1]).unwrap();
    assert_eq!(card_2.deck, deck);
    assert!(c.suspended_among(&[card_2.id]).unwrap().is_empty());
}

#[test]
fn several_notes_change_in_one_write_and_one_note_event_each() {
    let (c, _) = setup();
    let notes: Vec<(Id, Id)> = (0..5).map(|n| add_basic(&c, &format!("n{n}"))).collect();
    easy(&c, notes[0].1);
    let rec = Recorder::attach(&c);
    let ids: Vec<Id> = notes.iter().map(|(n, _)| *n).collect();
    let done = change_to_reversed(&c, &ids);
    assert_eq!(done.notes.len(), 5);
    assert_eq!(done.cards_added.len(), 10);
    assert_eq!(done.carried.len(), 1);
    assert_eq!(rec.one().len(), 5);
}

#[test]
fn a_card_with_no_schedule_has_nothing_to_carry() {
    let (c, _) = setup();
    let (note, _) = add_basic(&c, "kot");
    let done = change_to_reversed(&c, &[note]);
    assert!(done.carried.is_empty());
    let count: i64 = c
        .conn
        .query_row("SELECT count(*) FROM card_event", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

// ---- Refused ----

#[test]
fn a_change_that_cannot_be_made_writes_nothing() {
    let (a, _) = pair();
    let c = &a.c;
    let (basic_note, _) = add_basic(c, "kot");
    let (old_f, old_t) = parts(c, builtin::basic());
    let (new_f, new_t) = parts(c, builtin::basic_and_reversed());
    let reversed = c
        .add_note(
            builtin::basic_and_reversed(),
            &[(new_f[0], "pies"), (new_f[1], "dog")],
        )
        .unwrap();
    let before = digest(c);
    let ok_fields = [(new_f[0], Some(old_f[0])), (new_f[1], Some(old_f[1]))];

    // Notes of two note types.
    assert_eq!(
        c.change_note_type(&[basic_note, reversed.id], builtin::cloze(), &[], &[]),
        Err(NoteError::MixedTypes)
    );
    // The type they already have.
    assert_eq!(
        c.change_note_type(&[basic_note], builtin::basic(), &[], &[]),
        Err(NoteError::SameType)
    );
    // A note that is not there.
    assert_eq!(
        c.change_note_type(
            &[Id::from_bytes([0xee; 16])],
            builtin::basic_and_reversed(),
            &ok_fields,
            &[]
        ),
        Err(NoteError::NotFound)
    );
    // A field or template of another note type.
    assert_eq!(
        c.change_note_type(
            &[basic_note],
            builtin::basic_and_reversed(),
            &[(old_f[0], Some(old_f[0]))],
            &[]
        ),
        Err(NoteError::UnknownField)
    );
    assert_eq!(
        c.change_note_type(
            &[basic_note],
            builtin::basic_and_reversed(),
            &ok_fields,
            &[(new_t[0], Some(new_t[1]))]
        ),
        Err(NoteError::UnknownField)
    );
    // One old card for two new ones.
    assert_eq!(
        c.change_note_type(
            &[basic_note],
            builtin::basic_and_reversed(),
            &ok_fields,
            &[(new_t[0], Some(old_t[0])), (new_t[1], Some(old_t[0]))]
        ),
        Err(NoteError::DuplicateSource)
    );
    // A note left with no cards: both notes are fine but the second has an empty first field.
    let empty = c
        .add_note(builtin::basic(), &[(old_f[0], "x"), (old_f[1], "")])
        .unwrap();
    assert_eq!(
        c.change_note_type(
            &[basic_note, empty.id],
            builtin::basic_and_reversed(),
            &[(new_f[0], Some(old_f[1])), (new_f[1], Some(old_f[1]))],
            &[]
        ),
        Err(NoteError::LeavesNoCards { notes: 1 })
    );
    // Only the note added for the last case differs from the start.
    assert_ne!(digest(c), before);
    assert_eq!(
        c.note(basic_note).unwrap().unwrap().note_type,
        builtin::basic()
    );
    assert_eq!(
        c.note(empty.id).unwrap().unwrap().note_type,
        builtin::basic()
    );
}

#[test]
fn a_refused_change_leaves_the_digest_alone() {
    let (a, _) = pair();
    let c = &a.c;
    let (note, _) = add_basic(c, "kot");
    let (new_f, _) = parts(c, builtin::basic_and_reversed());
    let before = digest(c);
    let rec = Recorder::attach(c);
    c.change_note_type(
        &[note],
        builtin::basic_and_reversed(),
        &[(new_f[0], None), (new_f[1], None)],
        &[],
    )
    .unwrap_err();
    assert_eq!(digest(c), before);
    assert!(rec.take().is_empty());
    // An empty list is a change of nothing.
    assert_eq!(
        c.change_note_type(&[], builtin::basic_and_reversed(), &[], &[]),
        Ok(TypeChange::default())
    );
}

// ---- Undo: a change back ----

#[test]
fn changing_back_restores_the_old_cards_and_carries_study_done_meanwhile() {
    let (c, clock) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    let original = schedule_of(&c, old_card);
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let card_1 = live(&c, note)
        .into_iter()
        .find(|card| card.template == new_t[0])
        .unwrap()
        .id;

    // Nothing studied meanwhile: the old card comes back as it was, with no new event.
    let events_before = events(&c, old_card).len();
    let back = reversed_to_basic(&c, &[note]);
    assert_eq!(back.cards_added, vec![old_card]);
    assert_eq!(back.cards_removed.len(), 2);
    assert!(back.carried.is_empty());
    assert_eq!(events(&c, old_card).len(), events_before);
    assert_eq!(schedule_of(&c, old_card), original);

    // Again, and this time Card 1 is studied before the change back.
    change_to_reversed(&c, &[note]);
    clock.advance(3 * DAY);
    c.answer(card_1, Rating::Good, 2_000).unwrap();
    let studied = schedule_of(&c, card_1);
    let back = reversed_to_basic(&c, &[note]);
    assert_eq!(back.carried, vec![(old_card, card_1)]);
    let now = schedule_of(&c, old_card);
    assert_eq!(now.memory, studied.memory);
    assert_eq!(now.due_day, studied.due_day);
    assert_eq!(now.last_day, studied.last_day);
    assert_eq!(transfers(&c, old_card).len(), 1);
}

#[test]
fn start_as_new_resets_a_card_that_comes_back_with_a_schedule() {
    let (c, _) = setup();
    let (note, _) = add_basic(&c, "kot");
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let card_2 = live(&c, note)
        .into_iter()
        .find(|card| card.template == new_t[1])
        .unwrap()
        .id;
    easy(&c, card_2);
    reversed_to_basic(&c, &[note]);
    // Card 2 listed as "start as new" comes back reset...
    let (fields, templates) = basic_to_reversed(&c);
    let done = c
        .change_note_type(&[note], builtin::basic_and_reversed(), &fields, &templates)
        .unwrap();
    assert_eq!(done.reset, vec![card_2]);
    assert_eq!(schedule_of(&c, card_2).state, CardState::New);
    // ...with its history kept, and the reset in it.
    let history = c.card_history(card_2).unwrap();
    assert_eq!(
        history.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![HistoryKind::Answer, HistoryKind::Reset]
    );
    // Not listed: it comes back as it was.
    easy(&c, card_2);
    let studied = schedule_of(&c, card_2);
    reversed_to_basic(&c, &[note]);
    let done = c
        .change_note_type(&[note], builtin::basic_and_reversed(), &fields[..], &[])
        .unwrap();
    assert!(done.reset.is_empty());
    assert_eq!(schedule_of(&c, card_2), studied);
}

// ---- Cloze ----

#[test]
fn cloze_to_cloze_carries_by_cloze_number() {
    let (c, _) = setup();
    let other = c.create_note_type("Cloze 2", Kind::Cloze).unwrap();
    let (old_f, old_t) = parts(&c, builtin::cloze());
    let (new_f, new_t) = parts(&c, other);
    let added = c
        .add_note(
            builtin::cloze(),
            &[(old_f[0], "{{c1::kot}} and {{c2::pies}}")],
        )
        .unwrap();
    let cards = live(&c, added.id);
    assert_eq!(cards.len(), 2);
    let second = cards.iter().find(|card| card.ordinal == 2).unwrap().id;
    easy(&c, second);
    let before = schedule_of(&c, second);

    let done = c
        .change_note_type(
            &[added.id],
            other,
            &[(new_f[0], Some(old_f[0])), (new_f[1], Some(old_f[1]))],
            &[(new_t[0], Some(old_t[0]))],
        )
        .unwrap();

    assert_eq!(done.carried.len(), 1);
    let now = live(&c, added.id);
    assert_eq!(now.len(), 2);
    let carried = now.iter().find(|card| card.ordinal == 2).unwrap();
    assert_eq!(schedule_of(&c, carried.id).due_day, before.due_day);
    let first = now.iter().find(|card| card.ordinal == 1).unwrap();
    assert_eq!(schedule_of(&c, first.id).state, CardState::New);
}

#[test]
fn between_a_standard_and_a_cloze_type_every_card_starts_new() {
    let (c, _) = setup();
    let (note, old_card) = add_basic(&c, "{{c1::kot}}");
    easy(&c, old_card);
    let (old_f, old_t) = parts(&c, builtin::basic());
    let (new_f, new_t) = parts(&c, builtin::cloze());
    let done = c
        .change_note_type(
            &[note],
            builtin::cloze(),
            &[(new_f[0], Some(old_f[0])), (new_f[1], Some(old_f[1]))],
            &[(new_t[0], Some(old_t[0]))],
        )
        .unwrap();
    assert!(done.carried.is_empty());
    assert_eq!(done.cards_added.len(), 1);
    assert_eq!(schedule_of(&c, done.cards_added[0]).state, CardState::New);
}

// ---- Merge ----

#[test]
fn a_type_change_merges_into_another_collection_with_the_same_cards_and_schedules() {
    let (a, b) = pair();
    let (note, old_card) = {
        let (f, _) = parts(&a.c, builtin::basic());
        let added =
            a.c.add_note(builtin::basic(), &[(f[0], "kot"), (f[1], "cat")])
                .unwrap();
        (added.id, added.cards[0])
    };
    sync(&a, &b);
    easy(&a.c, old_card);
    sync(&a, &b);
    change_to_reversed(&a.c, &[note]);
    sync(&a, &b);
    assert_same(&digest(&a.c), &digest(&b.c), "after the change");
    assert_eq!(live(&b.c, note).len(), 2);
    assert!(b.c.card_history(live(&b.c, note)[0].id).is_ok());

    // Undone on one device, and the other follows. It also holds for a fresh collection.
    reversed_to_basic(&a.c, &[note]);
    sync(&a, &b);
    assert_same(&digest(&a.c), &digest(&b.c), "after the change back");
    let fresh = Dev::new(3, START);
    fresh
        .c
        .merge(&a.c.changes(Selection::All).unwrap())
        .unwrap();
    assert_same(&digest(&a.c), &digest(&fresh.c), "a new device");
}

#[test]
fn a_change_made_on_two_devices_settles() {
    let (a, b) = pair();
    let f = parts(&a.c, builtin::basic()).0;
    let added =
        a.c.add_note(builtin::basic(), &[(f[0], "kot"), (f[1], "cat")])
            .unwrap();
    sync(&a, &b);
    easy(&a.c, added.cards[0]);
    change_to_reversed(&a.c, &[added.id]);
    // The other device studies the old card and changes the type too, without seeing any of it.
    easy(&b.c, added.cards[0]);
    change_to_reversed(&b.c, &[added.id]);
    crate::sync::merge_tests::settle(&[&a, &b]);
    assert_same(&digest(&a.c), &digest(&b.c), "two devices");
    assert_eq!(live(&a.c, added.id).len(), 2);
}

// ---- History and the optimiser ----

#[test]
fn history_of_a_transferred_card_includes_the_old_cards() {
    let (c, clock) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    clock.advance(DAY);
    c.answer(old_card, Rating::Good, 2_000).unwrap();
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let card = live(&c, note)
        .into_iter()
        .find(|card| card.template == new_t[0])
        .unwrap()
        .id;
    clock.advance(DAY);
    c.answer(card, Rating::Good, 2_000).unwrap();

    let history = c.card_history(card).unwrap();
    assert_eq!(
        history.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![
            HistoryKind::Answer,
            HistoryKind::Answer,
            HistoryKind::Transfer,
            HistoryKind::Answer
        ]
    );
    // The days between answers are counted across the card change.
    assert_eq!(history[3].days_since_previous, Some(1));
    // The old card keeps only its own.
    assert_eq!(c.card_history(old_card).unwrap().len(), 2);
}

#[test]
fn history_across_a_change_and_back_counts_each_event_once() {
    let (c, clock) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let card = live(&c, note)
        .into_iter()
        .find(|card| card.template == new_t[0])
        .unwrap()
        .id;
    clock.advance(DAY);
    c.answer(card, Rating::Good, 2_000).unwrap();
    reversed_to_basic(&c, &[note]);
    let history = c.card_history(old_card).unwrap();
    let answers = history
        .iter()
        .filter(|e| e.kind == HistoryKind::Answer)
        .count();
    assert_eq!(answers, 2);
    let mut ids: Vec<Id> = history.iter().map(|e| e.event).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), history.len());
}

#[test]
fn the_optimiser_trains_on_the_old_cards_answers_once() {
    let (c, clock) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    for _ in 0..3 {
        c.answer(old_card, Rating::Good, 2_000).unwrap();
        clock.advance(DAY);
    }
    let before = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((before.cards, before.reviews), (1, 3));

    change_to_reversed(&c, &[note]);
    // The old card is in the trash, the new one has its history.
    let after = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((after.cards, after.reviews), (1, 3));
    assert_eq!(after.items, before.items);

    // Changing back: the old card is live again, and the new ones are gone. Still once.
    reversed_to_basic(&c, &[note]);
    let back = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((back.cards, back.reviews), (1, 3));
}

#[test]
fn the_optimiser_follows_a_chain_of_changes_and_a_reset() {
    let (c, clock) = setup();
    let other = c.create_note_type("Basic 2", Kind::Standard).unwrap();
    let (note, old_card) = add_basic(&c, "kot");
    for _ in 0..2 {
        c.answer(old_card, Rating::Good, 2_000).unwrap();
        clock.advance(DAY);
    }
    change_to_reversed(&c, &[note]);
    // Reversed to Basic 2: Card 1 to Card 1.
    let (r_f, r_t) = parts(&c, builtin::basic_and_reversed());
    let (o_f, o_t) = parts(&c, other);
    c.change_note_type(
        &[note],
        other,
        &[(o_f[0], Some(r_f[0])), (o_f[1], Some(r_f[1]))],
        &[(o_t[0], Some(r_t[0]))],
    )
    .unwrap();
    let card = live(&c, note)[0].id;
    c.answer(card, Rating::Good, 2_000).unwrap();
    let data = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((data.cards, data.reviews), (1, 3));
    // A reset on the last card starts the history again.
    c.reset_cards(&[card]).unwrap();
    let data = c.optimisation_data(default_preset()).unwrap();
    assert_eq!(data.cards, 0);
}

// ---- The fold ----

#[test]
fn a_transfer_is_applied_as_recorded_even_when_it_did_not_build_on_the_last_event() {
    let (c, clock) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    let source_row = schedule_of(&c, old_card);
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let card = live(&c, note)
        .into_iter()
        .find(|card| card.template == new_t[0])
        .unwrap()
        .id;
    // Another device answered the new card, building on nothing, before it saw the transfer.
    clock.advance(MINUTE);
    let transfer = transfers(&c, card).pop().unwrap();
    let mut early = transfer.clone();
    early.id = Id::new_v7(transfer.time_ms - 1, &[3; 10]);
    early.kind = EventKind::Review;
    early.rating = Some(Rating::Again);
    early.time_ms = transfer.time_ms - 1;
    early.state_before = Some(CardState::New);
    early.source = None;
    early.memory = Some(crate::scheduling::Memory {
        stability: 1.0,
        difficulty: 5.0,
    });
    early.due_day = Some(1);
    early.last_day = None;
    crate::study::fold_tests::insert(&c, &early);
    c.rebuild_schedule().unwrap();
    // The transfer sorts after it and does not build on it: the recorded result is still taken.
    let after = schedule_of(&c, card);
    assert_eq!(after.memory, source_row.memory);
    assert_eq!(after.due_day, source_row.due_day);
    assert_eq!(after.answers, 1);
}

#[test]
fn a_later_event_blocks_the_void_of_an_earlier_one_across_a_transfer() {
    let (c, _) = setup();
    let (note, old_card) = add_basic(&c, "kot");
    easy(&c, old_card);
    let review = events(&c, old_card)[0].id;
    change_to_reversed(&c, &[note]);
    let (_, new_t) = parts(&c, builtin::basic_and_reversed());
    let card = live(&c, note)
        .into_iter()
        .find(|card| card.template == new_t[0])
        .unwrap()
        .id;
    // Studied meanwhile, so changing back writes a transfer onto the old card.
    c.answer(card, Rating::Hard, 2_000).unwrap();
    reversed_to_basic(&c, &[note]);
    assert_eq!(transfers(&c, old_card).len(), 1);
    let voided = c.void_events(&[review]).unwrap();
    assert_eq!((voided.voided.len(), voided.refused), (0, 1));
}

#[test]
fn default_deck_is_kept_when_the_note_has_no_other() {
    let (c, _) = setup();
    let (note, _) = add_basic(&c, "kot");
    change_to_reversed(&c, &[note]);
    assert!(
        live(&c, note)
            .iter()
            .all(|card| card.deck == default_deck())
    );
}
