//! Set due date, reset to new and their undo (ADR 0014, decision 5): what they write, how the fold
//! settles them against answers from another device, and what history and the optimiser see.

use std::collections::BTreeSet;

use super::answer_tests::{DAY, DAY0, HOUR, MINUTE, basic_card, events, good, setup};
use super::fold::{ParameterSets, fold};
use super::fold_tests::{from_other_device, insert, snapshot};
use super::schedule::read_cached;
use super::*;
use crate::collection::Collection;
use crate::deck::{default_deck, default_preset};
use crate::scheduling::{CardState, Rating, day_in_range};
use crate::stats::{DayRange, HistoryKind};

const TODAY: i64 = DAY0 / DAY;

/// A card answered Easy once today: a review card with a memory, last answered today.
fn review_card(c: &Collection, front: &str) -> Id {
    let card = basic_card(c, front);
    c.answer(card, Rating::Easy, 2_000).unwrap();
    assert_eq!(c.card_schedule(card).unwrap().state, CardState::Review);
    card
}

fn event_count(c: &Collection) -> i64 {
    c.conn
        .query_row("SELECT count(*) FROM card_event", [], |r| r.get(0))
        .unwrap()
}

// ---- Set due date ----

#[test]
fn set_due_makes_a_review_card_due_on_that_day_and_keeps_its_memory_and_last_answer() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    let before = c.card_schedule(card).unwrap();
    clock.advance(2 * DAY);
    let done = c.set_due(&[card], 3, 3).unwrap();
    assert_eq!(done.skipped_new, 0);
    let [id] = done.events.try_into().unwrap();

    let after = c.card_schedule(card).unwrap();
    assert_eq!(after.state, CardState::Review);
    assert_eq!(after.due_day, Some(TODAY + 2 + 3));
    assert_eq!(after.memory, before.memory);
    assert_eq!(after.last_day, Some(TODAY));
    assert_eq!(
        (after.answers, after.lapses),
        (before.answers, before.lapses)
    );

    let event = events(&c, card).pop().unwrap();
    assert_eq!(event.id, id);
    assert_eq!(event.kind, EventKind::SetDue);
    assert_eq!(event.day, TODAY + 2);
    assert_eq!(event.state_before, Some(CardState::Review));
    assert_eq!(event.state, Some(CardState::Review));
    assert_eq!(event.step, Some(0));
    assert_eq!(event.memory, before.memory);
    assert_eq!(event.due_day, Some(TODAY + 5));
    assert_eq!(event.last_day, Some(TODAY));
    assert_eq!(event.rating, None);
    assert_eq!(event.previous, Some(events(&c, card)[0].id));
    // The cache it wrote is what the fold makes of the events.
    let built = snapshot(&c.conn);
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
}

#[test]
fn a_learning_card_becomes_a_review_card_due_today() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    let before = c.card_schedule(card).unwrap();
    assert_eq!((before.state, before.step), (CardState::Learning, 1));
    c.set_due(&[card], 0, 0).unwrap();
    let after = c.card_schedule(card).unwrap();
    assert_eq!((after.state, after.step), (CardState::Review, 0));
    assert_eq!(after.due_day, Some(TODAY));
    assert_eq!(after.due_ms, None);
    assert_eq!(after.memory, before.memory);
    // Due now: the queue offers it as a review.
    assert_eq!(c.study_counts(default_deck()).unwrap().review, 1);
}

#[test]
fn new_cards_are_left_as_they_are_and_counted() {
    let (c, _) = setup();
    let new = basic_card(&c, "new");
    let studied = review_card(&c, "studied");
    let done = c.set_due(&[new, studied, new], 1, 1).unwrap();
    assert_eq!(done.events.len(), 1);
    assert_eq!(done.skipped_new, 1);
    assert!(events(&c, new).is_empty());
    assert_eq!(c.card_schedule(new).unwrap().state, CardState::New);
}

#[test]
fn a_range_spreads_the_cards_over_its_days_from_each_event_id() {
    let (c, _) = setup();
    let cards: Vec<Id> = (0..60)
        .map(|n| review_card(&c, &format!("card {n}")))
        .collect();
    let done = c.set_due(&cards, 10, 14).unwrap();
    assert_eq!(done.events.len(), 60);
    let mut days = BTreeSet::new();
    for (card, id) in cards.iter().zip(&done.events) {
        let due = c.card_schedule(*card).unwrap().due_day.unwrap();
        assert!((TODAY + 10..=TODAY + 14).contains(&due), "{due}");
        // The day is the event ID's: the same on every device that folds it.
        assert_eq!(due, TODAY + i64::from(day_in_range(10, 14, *id)));
        days.insert(due);
    }
    assert_eq!(days.len(), 5, "{days:?}");
    let built = snapshot(&c.conn);
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
}

#[test]
fn day_in_range_is_even_and_repeatable() {
    let mut seen = [0u32; 7];
    for n in 0..7_000u64 {
        let mut random = [0u8; 10];
        random[2..].copy_from_slice(&n.wrapping_mul(0x9e37_79b9_7f4a_7c15).to_be_bytes());
        let id = Id::new_v7(1_700_000_000_000, &random);
        let day = day_in_range(3, 9, id);
        assert_eq!(day, day_in_range(3, 9, id));
        seen[(day - 3) as usize] += 1;
    }
    for count in seen {
        assert!((800..1_200).contains(&count), "{seen:?}");
    }
    let id = Id::new_v7(5, &[1; 10]);
    assert_eq!(day_in_range(4, 4, id), 4);
    assert_eq!(day_in_range(0, 0, id), 0);
}

#[test]
fn a_bad_range_or_a_deleted_card_writes_nothing() {
    let (c, _) = setup();
    let card = review_card(&c, "a");
    let gone = review_card(&c, "b");
    let note = c.notes_of_cards(&[gone]).unwrap()[0];
    c.delete_notes(&[note]).unwrap();
    let before = event_count(&c);
    assert_eq!(c.set_due(&[card], 5, 3), Err(StudyError::DueRange));
    assert_eq!(c.set_due(&[card], 0, 36_501), Err(StudyError::DueRange));
    assert_eq!(c.set_due(&[card, gone], 1, 1), Err(StudyError::NotFound));
    assert_eq!(c.reset_cards(&[card, gone]), Err(StudyError::NotFound));
    assert_eq!(event_count(&c), before);
    assert!(c.set_due(&[card], 36_500, 36_500).is_ok());
}

#[test]
fn the_next_answer_sees_the_real_time_since_the_last_answer() {
    let (c, clock) = setup();
    let moved = review_card(&c, "moved");
    let left = review_card(&c, "left");
    assert_eq!(
        c.card_schedule(moved).unwrap().memory,
        c.card_schedule(left).unwrap().memory
    );
    c.set_due(&[moved], 1, 1).unwrap();
    // Both are answered three days on: FSRS sees three days for both, not one for the moved card.
    clock.advance(3 * DAY);
    c.answer(moved, Rating::Good, 1_000).unwrap();
    c.answer(left, Rating::Good, 1_000).unwrap();
    let (m, l) = (
        c.card_schedule(moved).unwrap(),
        c.card_schedule(left).unwrap(),
    );
    assert_eq!(m.memory, l.memory);
    assert_eq!(m.last_day, Some(TODAY + 3));
    assert_eq!(m.answers, 2);
}

// ---- Reset ----

#[test]
fn reset_makes_the_card_new_and_keeps_its_history_and_counts() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    clock.advance(9 * DAY);
    c.answer(card, Rating::Again, 1_000).unwrap();
    let before = c.card_schedule(card).unwrap();
    assert_eq!((before.answers, before.lapses), (2, 1));
    let done = c.reset_cards(&[card]).unwrap();
    assert_eq!(done.events.len(), 1);

    let after = c.card_schedule(card).unwrap();
    assert_eq!(after.state, CardState::New);
    assert_eq!(after.step, 0);
    assert_eq!(after.memory, None);
    assert_eq!(after.due_day, None);
    assert_eq!(after.due_ms, None);
    assert_eq!(after.last_day, None);
    assert_eq!((after.answers, after.lapses), (2, 1));
    let event = events(&c, card).pop().unwrap();
    assert_eq!(event.kind, EventKind::Reset);
    assert_eq!(event.state_before, Some(CardState::Relearning));
    assert_eq!(event.state, Some(CardState::New));
    assert_eq!(
        (event.memory, event.due_day, event.due_ms),
        (None, None, None)
    );
    assert_eq!(event.last_day, Some(TODAY + 9));
    let built = snapshot(&c.conn);
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
    assert_eq!(c.study_counts(default_deck()).unwrap().new, 1);
}

#[test]
fn a_reset_card_is_studied_as_a_new_card() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    let fresh = basic_card(&c, "fresh");
    c.reset_cards(&[card]).unwrap();
    clock.advance(DAY);
    good(&c, card);
    good(&c, fresh);
    let (r, f) = (
        c.card_schedule(card).unwrap(),
        c.card_schedule(fresh).unwrap(),
    );
    assert_eq!((r.state, r.step), (CardState::Learning, 1));
    assert_eq!(r.memory, f.memory);
    assert_eq!(r.due_ms, f.due_ms);
    assert_eq!(r.answers, 2);
}

#[test]
fn reset_skips_cards_that_are_already_new() {
    let (c, _) = setup();
    let new = basic_card(&c, "new");
    let card = review_card(&c, "a");
    let done = c.reset_cards(&[new, card]).unwrap();
    assert_eq!((done.events.len(), done.skipped_new), (1, 1));
    let again = c.reset_cards(&[card]).unwrap();
    assert_eq!((again.events.len(), again.skipped_new), (0, 1));
}

// ---- Undo ----

#[test]
fn voiding_the_events_puts_every_card_back_as_it_was() {
    let (c, clock) = setup();
    let a = review_card(&c, "a");
    let b = basic_card(&c, "b");
    good(&c, b);
    clock.advance(HOUR);
    let built = snapshot(&c.conn);
    let due = c.set_due(&[a, b], 4, 8).unwrap();
    assert_ne!(snapshot(&c.conn), built);
    let voided = c.void_events(&due.events).unwrap();
    assert_eq!(voided.voided.len(), 2);
    assert_eq!(voided.refused, 0);
    assert_eq!(snapshot(&c.conn), built);

    let reset = c.reset_cards(&[a, b]).unwrap();
    c.void_events(&reset.events).unwrap();
    assert_eq!(snapshot(&c.conn), built);
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);

    // Voiding again does nothing.
    let twice = c.void_events(&reset.events).unwrap();
    assert!(twice.voided.is_empty());
    assert_eq!(twice.refused, 0);
    assert_eq!(
        c.void_events(&[Id::new_v7(1, &[1; 10])]),
        Err(StudyError::NotFound)
    );
}

#[test]
fn a_card_answered_since_is_not_put_back() {
    let (c, clock) = setup();
    let a = review_card(&c, "a");
    let b = review_card(&c, "b");
    let done = c.set_due(&[a, b], 0, 0).unwrap();
    clock.advance(MINUTE);
    good(&c, a);
    let after_answer = c.card_schedule(a).unwrap();
    let voided = c.void_events(&done.events).unwrap();
    assert_eq!(voided.refused, 1);
    assert_eq!(voided.voided, vec![done.events[1]]);
    assert_eq!(c.card_schedule(a).unwrap(), after_answer);
}

#[test]
fn an_answer_cannot_be_undone_after_a_later_set_due_date_or_reset() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    clock.advance(MINUTE);
    c.set_due(&[card], 2, 2).unwrap();
    assert_eq!(c.undo_answer(), Err(StudyError::LaterAnswer));

    let other = review_card(&c, "b");
    clock.advance(MINUTE);
    c.reset_cards(&[other]).unwrap();
    assert_eq!(c.undo_answer(), Err(StudyError::LaterAnswer));
}

// ---- The fold, with another device ----

#[test]
fn an_answer_from_another_device_before_a_set_due_date_gives_it_the_memory() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    let first = events(&c, card)[0].clone();
    let seen = read_cached(&c.conn, card).unwrap().unwrap();
    clock.advance(2 * DAY);
    c.set_due(&[card], 5, 5).unwrap();
    // The other device answered at day 1, built on the same first answer. It sorts before the set
    // due date, which is then computed again on top of it.
    let theirs = from_other_device(
        card,
        Id::new_v7(first.time_ms + DAY, &[5; 10]),
        Some(first.id),
        Some(&seen),
        Rating::Hard,
        first.time_ms + DAY,
        TODAY + 1,
    );
    insert(&c, &theirs);
    c.rebuild_schedule().unwrap();
    let got = c.card_schedule(card).unwrap();
    assert_eq!(got.state, CardState::Review);
    assert_eq!(got.due_day, Some(TODAY + 2 + 5));
    assert_eq!(got.memory, theirs.memory);
    assert_eq!(got.last_day, Some(TODAY + 1));
    assert_eq!(got.answers, 2);
}

#[test]
fn an_answer_from_another_device_after_a_set_due_date_is_computed_on_it() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    let first = events(&c, card)[0].clone();
    let seen = read_cached(&c.conn, card).unwrap().unwrap();
    c.set_due(&[card], 1, 1).unwrap();
    let after_set = read_cached(&c.conn, card).unwrap().unwrap();
    // The other device did not have the set due date: its answer at day 3 built on the first one.
    let theirs = from_other_device(
        card,
        Id::new_v7(first.time_ms + 3 * DAY, &[6; 10]),
        Some(first.id),
        Some(&seen),
        Rating::Good,
        first.time_ms + 3 * DAY,
        TODAY + 3,
    );
    insert(&c, &theirs);
    c.rebuild_schedule().unwrap();
    let got = read_cached(&c.conn, card).unwrap().unwrap();
    // Recomputed from the card as the set due date left it, which kept the memory and last day, so
    // the answer comes out as it did there.
    assert_eq!(after_set.memory, seen.memory);
    assert_eq!(got.last_event, theirs.id);
    assert_eq!(got.memory, theirs.memory);
    assert_eq!(got.answers, 2);
    let _ = clock;
}

#[test]
fn a_reset_from_another_device_is_kept_under_a_later_concurrent_answer() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    let first = events(&c, card)[0].clone();
    let fresh = basic_card(&c, "fresh");
    let seen = read_cached(&c.conn, card).unwrap().unwrap();
    clock.advance(HOUR);
    c.reset_cards(&[card]).unwrap();
    // An answer built on the first one, written after the reset on a device that did not have it:
    // the card is new when the fold reaches it, so it is answered as a new card.
    let theirs = from_other_device(
        card,
        Id::new_v7(first.time_ms + 2 * HOUR, &[7; 10]),
        Some(first.id),
        Some(&seen),
        Rating::Good,
        first.time_ms + 2 * HOUR,
        TODAY,
    );
    insert(&c, &theirs);
    c.rebuild_schedule().unwrap();
    clock.set(first.time_ms + 2 * HOUR);
    good(&c, fresh);
    let (got, f) = (
        c.card_schedule(card).unwrap(),
        c.card_schedule(fresh).unwrap(),
    );
    assert_eq!((got.state, got.step), (CardState::Learning, 1));
    assert_eq!(got.memory, f.memory);
    assert_eq!(got.answers, 2);
}

#[test]
fn a_set_due_date_on_a_card_that_is_new_when_folded_does_nothing() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    let answer = events(&c, card)[0].id;
    clock.advance(MINUTE);
    let done = c.set_due(&[card], 3, 3).unwrap();
    // Another device voided the answer the set due date built on: the card is new again, so the set
    // due date has no memory to keep and leaves it new.
    let mut void = events(&c, card)[0].clone();
    void.id = Id::new_v7(void.time_ms + 2 * MINUTE, &[8; 10]);
    void.kind = EventKind::Void;
    void.time_ms += 2 * MINUTE;
    void.target = Some(answer);
    void.previous = Some(done.events[0]);
    void.rating = None;
    insert(&c, &void);
    c.rebuild_schedule().unwrap();
    let got = c.card_schedule(card).unwrap();
    assert_eq!(got.state, CardState::New);
    assert_eq!(got.answers, 0);
}

#[test]
fn a_kind_the_fold_does_not_know_with_the_same_columns_gives_the_same_card() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    clock.advance(2 * DAY);
    c.set_due(&[card], 4, 6).unwrap();
    clock.advance(DAY);
    good(&c, card);
    clock.advance(DAY);
    c.reset_cards(&[card]).unwrap();
    let sets = ParameterSets::load(&c.conn).unwrap();
    let known = events(&c, card);
    let unknown: Vec<CardEvent> = known
        .iter()
        .cloned()
        .map(|mut e| {
            if matches!(e.kind, EventKind::SetDue | EventKind::Reset) {
                e.kind = EventKind::Unknown(format!("{}-from-later", e.kind.as_text()));
            }
            e
        })
        .collect();
    assert_eq!(fold(&unknown, &sets), fold(&known, &sets));
    assert_eq!(fold(&known, &sets), read_cached(&c.conn, card).unwrap());
}

#[test]
fn both_kinds_and_their_undo_merge_into_another_collection() {
    use crate::sync::Selection;
    use crate::sync::merge_tests::{Dev, add, digest, pair, sync};
    let (a, b) = pair();
    let (_, one) = add(&a.c, "one", "1");
    let (_, two) = add(&a.c, "two", "2");
    let (_, three) = add(&a.c, "three", "3");
    for card in [one, two, three] {
        a.c.answer(card, Rating::Easy, 1_000).unwrap();
    }
    a.clock.advance(DAY);
    a.c.set_due(&[one], 2, 9).unwrap();
    a.c.reset_cards(&[two]).unwrap();
    let undone = a.c.set_due(&[three], 1, 1).unwrap();
    a.c.void_events(&undone.events).unwrap();
    sync(&a, &b);
    assert_eq!(digest(&a.c), digest(&b.c));
    for card in [one, two, three] {
        assert_eq!(
            b.c.card_schedule(card).unwrap(),
            a.c.card_schedule(card).unwrap()
        );
    }
    // A new device that gets everything at once folds the same cards.
    let fresh = Dev::new(3, a.c.now_ms());
    fresh
        .c
        .merge(&a.c.changes(Selection::All).unwrap())
        .unwrap();
    assert_eq!(snapshot(&fresh.c.conn), snapshot(&a.c.conn));
}

// ---- History and statistics ----

#[test]
fn history_lists_both_kinds_as_entries_that_are_not_answers() {
    let (c, clock) = setup();
    let card = review_card(&c, "a");
    clock.advance(DAY);
    c.set_due(&[card], 3, 3).unwrap();
    clock.advance(2 * DAY);
    good(&c, card);
    clock.advance(DAY);
    c.reset_cards(&[card]).unwrap();
    let undone = c.set_due(&[review_card(&c, "b")], 1, 1).unwrap();
    c.void_events(&undone.events).unwrap();

    let history = c.card_history(card).unwrap();
    let kinds: Vec<HistoryKind> = history.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            HistoryKind::Answer,
            HistoryKind::SetDue,
            HistoryKind::Answer,
            HistoryKind::Reset
        ]
    );
    assert_eq!(history[1].rating, None);
    assert_eq!(history[1].days_since_previous, None);
    assert_eq!(history[1].due, Some(crate::scheduling::Due::Days(3)));
    // The answer after the set due date counts its days from the answer before it.
    assert_eq!(history[2].days_since_previous, Some(3));
    assert_eq!(history[3].state, Some(CardState::New));
    // An undone set due date is not in the history.
    let b = c.card_events(undone.events[0]).unwrap_or_default();
    assert!(b.is_empty());
}

#[test]
fn daily_counts_and_retention_count_only_answers() {
    let (c, clock) = setup();
    let cards: Vec<Id> = (0..4).map(|n| review_card(&c, &format!("{n}"))).collect();
    let range = DayRange::last(TODAY, 1);
    let counts = c.daily_counts(range, None).unwrap();
    let retention = c.retention(range, None).unwrap();
    c.set_due(&cards[..2], 0, 0).unwrap();
    c.reset_cards(&cards[2..]).unwrap();
    clock.advance(MINUTE);
    assert_eq!(c.daily_counts(range, None).unwrap(), counts);
    assert_eq!(c.daily_counts(range, Some(default_deck())).unwrap(), counts);
    assert_eq!(c.retention(range, None).unwrap(), retention);
}

#[test]
fn the_optimiser_starts_a_card_after_its_last_reset() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    for _ in 0..3 {
        good(&c, card);
        clock.advance(2 * DAY);
    }
    let before = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((before.cards, before.reviews), (1, 3));
    c.reset_cards(&[card]).unwrap();
    let after_reset = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((after_reset.cards, after_reset.reviews), (0, 0));
    clock.advance(DAY);
    good(&c, card);
    clock.advance(DAY);
    good(&c, card);
    let after = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((after.cards, after.reviews), (1, 2));
    assert_eq!(after.histories[0][0].day, 0);
    assert_eq!(after.histories[0][1].day, 1);
    // An undone reset is not a reset.
    let other = basic_card(&c, "b");
    good(&c, other);
    clock.advance(2 * DAY);
    good(&c, other);
    let reset = c.reset_cards(&[other]).unwrap();
    assert_eq!(c.optimisation_data(default_preset()).unwrap().reviews, 2);
    c.void_events(&reset.events).unwrap();
    let undone = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((undone.cards, undone.reviews), (2, 4));
}

// ---- The upgrade ----

#[test]
fn a_version_11_collection_upgrades_keeps_its_schedule_and_adopts_the_new_columns() {
    use crate::collection::{MIGRATIONS, Schema};
    use crate::sync::SYNCED_TABLES;
    let v11 = Schema {
        migrations: &MIGRATIONS[..11],
        tables: SYNCED_TABLES,
    };
    let path = std::env::temp_dir().join(format!("fc-reschedule-v11-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = std::sync::Arc::new(crate::clock::ManualClock::new(DAY0 + 9 * HOUR));
    let host = super::answer_tests::host(&clock);
    let old = Collection::create_with(location, v11, host.clone()).unwrap();
    let card = basic_card(&old, "a");
    // An answer as the version-11 build wrote it, and a `last_day` it kept as unknown data (a newer
    // device sent it).
    let id = Id::new_v7(DAY0 + 9 * HOUR, &[4; 10]);
    let event = from_other_device(card, id, None, None, Rating::Easy, DAY0 + 9 * HOUR, TODAY);
    let values: Vec<_> = event
        .values()
        .into_iter()
        .filter(|(c, _)| !["source", "last_day"].contains(c))
        .collect();
    let names: String = values.iter().map(|(c, _)| format!(", {c}")).collect();
    let marks = ", ?".repeat(values.len());
    old.conn
        .execute_batch("INSERT INTO write_guard (id) VALUES (1)")
        .unwrap();
    old.conn
        .execute(
            &format!("INSERT INTO card_event (id{names}) VALUES (?{marks})"),
            rusqlite::params_from_iter(
                std::iter::once(rusqlite::types::Value::Blob(id.as_bytes().to_vec()))
                    .chain(values.into_iter().map(|(_, v)| v)),
            ),
        )
        .unwrap();
    old.conn.execute_batch("DELETE FROM write_guard").unwrap();
    old.conn
        .execute(
            "INSERT INTO unknown_row_value (entity_type, row_id, column, value)
             VALUES ('card_event', ?1, 'last_day', 19000)",
            [id],
        )
        .unwrap();
    old.rebuild_schedule().unwrap();
    let schedule = old.card_schedule(card).unwrap();
    old.close().unwrap();

    let upgraded = Collection::open(location, host).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 12);
    crate::sync::check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    assert_eq!(upgraded.card_schedule(card).unwrap(), schedule);
    let [stored] = upgraded.card_events(card).unwrap().try_into().unwrap();
    assert_eq!(stored.last_day, Some(19_000));
    assert_eq!(stored.source, None);
    let kept: i64 = upgraded
        .conn
        .query_row("SELECT count(*) FROM unknown_row_value", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 0);
    // And it can set a due date and reset at once.
    upgraded.set_due(&[card], 2, 2).unwrap();
    upgraded.reset_cards(&[card]).unwrap();
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&path);
}
