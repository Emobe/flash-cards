//! Undo: a void event, so nothing is deleted and the fold does the rest.

use super::answer_tests::{DAY, HOUR, MINUTE, basic_card, events, good, setup};
use super::fold_tests::{from_other_device, insert, snapshot};
use super::schedule::read_cached;
use super::*;
use crate::clock::Clock;
use crate::scheduling::{CardState, Rating};

#[test]
fn there_is_nothing_to_undo_in_a_new_collection() {
    let (c, _) = setup();
    assert_eq!(c.undo_answer(), Ok(None));
    let card = basic_card(&c, "a");
    assert_eq!(c.undo_answer(), Ok(None));
    assert!(events(&c, card).is_empty());
}

#[test]
fn undoing_the_first_answer_makes_the_card_new_again_and_keeps_the_review() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    let answered = good(&c, card);
    let undone = c.undo_answer().unwrap().unwrap();
    assert_eq!(undone.card, card);
    assert_eq!(undone.event, answered.event);
    assert_eq!(undone.rating, Rating::Good);
    assert_eq!(undone.schedule.state, CardState::New);
    assert_eq!(undone.schedule.answers, 0);
    assert_eq!(c.card_schedule(card).unwrap(), undone.schedule);
    assert!(read_cached(&c.conn, card).unwrap().is_none());

    // The review is still stored, and the void points at it.
    let all = events(&c, card);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kind, EventKind::Review);
    assert_eq!(all[0].id, answered.event);
    assert_eq!(all[1].kind, EventKind::Void);
    assert_eq!(all[1].target, Some(answered.event));
    assert_eq!(all[1].previous, Some(answered.event));
    assert_eq!(all[1].device, c.device_id().unwrap());
    assert!(all[1].time_ms > all[0].time_ms);
    assert_eq!(all[1].rating, None);
}

#[test]
fn undoing_a_later_answer_puts_state_due_and_memory_back() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    clock.advance(10 * MINUTE);
    good(&c, card);
    clock.set(19_677 * DAY + 9 * HOUR);
    let before = read_cached(&c.conn, card).unwrap().unwrap();
    let snapshot_before = snapshot(&c.conn);
    assert_eq!(before.state, CardState::Review);

    // A lapse, then undone: everything is as it was, lapse count included.
    c.answer(card, Rating::Again, 0).unwrap();
    assert_eq!(c.card_schedule(card).unwrap().lapses, 1);
    clock.advance(MINUTE);
    c.undo_answer().unwrap().unwrap();
    assert_eq!(read_cached(&c.conn, card).unwrap().unwrap(), before);
    assert_eq!(snapshot(&c.conn), snapshot_before);
    // And it is what a rebuild gives.
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), snapshot_before);
}

#[test]
fn undoing_again_goes_back_one_more_answer_and_then_runs_out() {
    let (c, clock) = setup();
    let a = basic_card(&c, "a");
    let b = basic_card(&c, "b");
    let first = good(&c, a);
    clock.advance(MINUTE);
    let second = good(&c, b);
    clock.advance(MINUTE);
    let third = good(&c, a);
    assert_eq!(c.undo_answer().unwrap().unwrap().event, third.event);
    assert_eq!(c.undo_answer().unwrap().unwrap().event, second.event);
    assert_eq!(c.card_schedule(b).unwrap().state, CardState::New);
    assert_eq!(c.card_schedule(a).unwrap().answers, 1);
    assert_eq!(c.undo_answer().unwrap().unwrap().event, first.event);
    assert_eq!(c.card_schedule(a).unwrap().state, CardState::New);
    assert_eq!(c.undo_answer(), Ok(None));
    // All three reviews and three voids are stored.
    let count: i64 = c
        .conn
        .query_row("SELECT count(*) FROM card_event", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 6);
}

#[test]
fn answering_again_after_an_undo_builds_on_what_came_before_the_voided_review() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    clock.advance(MINUTE);
    let voided = good(&c, card);
    c.undo_answer().unwrap().unwrap();
    clock.advance(MINUTE);
    let redone = c.answer(card, Rating::Easy, 0).unwrap();
    let all = events(&c, card);
    let first = &all[0];
    let new = all.iter().find(|e| e.id == redone.event).unwrap();
    assert_eq!(new.previous, Some(first.id));
    assert_ne!(new.previous, Some(voided.event));
    let schedule = c.card_schedule(card).unwrap();
    assert_eq!(schedule.answers, 2);
    assert_eq!(
        read_cached(&c.conn, card).unwrap().unwrap().last_event,
        redone.event
    );
    let built = snapshot(&c.conn);
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
    // The undone review can be undone only once, and the new one is now the newest.
    assert_eq!(c.undo_answer().unwrap().unwrap().event, redone.event);
    assert_eq!(c.undo_answer().unwrap().unwrap().event, first.id);
    assert_eq!(c.undo_answer(), Ok(None));
}

#[test]
fn undo_never_writes_a_void_earlier_than_the_event_it_follows() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    let review_time = events(&c, card)[0].time_ms;
    clock.set(review_time - HOUR);
    c.undo_answer().unwrap().unwrap();
    assert_eq!(events(&c, card)[1].time_ms, review_time + 1);
}

#[test]
fn an_answer_from_another_device_is_not_undone_here() {
    let (c, clock) = setup();
    let mine = basic_card(&c, "mine");
    let theirs = basic_card(&c, "theirs");
    let own = good(&c, mine);
    clock.advance(MINUTE);
    // Another device's review of another card is newer than ours.
    let remote = from_other_device(
        theirs,
        Id::new_v7(clock.now().unix_ms, &[5; 10]),
        None,
        None,
        Rating::Good,
        clock.now().unix_ms,
        19_675,
    );
    insert(&c, &remote);
    c.rebuild_schedule().unwrap();
    assert_eq!(c.undo_answer().unwrap().unwrap().event, own.event);
    assert_eq!(c.card_schedule(theirs).unwrap().answers, 1);
    assert_eq!(c.undo_answer(), Ok(None));
}

#[test]
fn undo_is_refused_when_the_card_was_answered_again_elsewhere_afterwards() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    let own = good(&c, card);
    let own_event = events(&c, card)[0].clone();
    let seen = read_cached(&c.conn, card).unwrap().unwrap();
    // After a merge, a later review of the same card from another device, built on ours.
    let later = from_other_device(
        card,
        Id::new_v7(own_event.time_ms + 5 * MINUTE, &[6; 10]),
        Some(own.event),
        Some(&seen),
        Rating::Good,
        own_event.time_ms + 5 * MINUTE,
        own_event.day,
    );
    insert(&c, &later);
    c.rebuild_schedule().unwrap();
    let before = snapshot(&c.conn);
    clock.advance(10 * MINUTE);
    assert_eq!(c.undo_answer(), Err(StudyError::LaterAnswer));
    // Nothing changed.
    assert_eq!(snapshot(&c.conn), before);
    assert_eq!(events(&c, card).len(), 2);
    assert!(
        StudyError::LaterAnswer
            .to_string()
            .contains("cannot be undone")
    );
}

#[test]
fn undo_is_refused_when_a_remote_event_built_on_the_answer_even_with_an_earlier_time() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    let own = good(&c, card);
    let own_event = events(&c, card)[0].clone();
    let seen = read_cached(&c.conn, card).unwrap().unwrap();
    // The other device's clock is behind: its event sorts before ours but was built on it.
    let skewed = from_other_device(
        card,
        Id::new_v7(own_event.time_ms - HOUR, &[4; 10]),
        Some(own.event),
        Some(&seen),
        Rating::Good,
        own_event.time_ms - HOUR,
        own_event.day,
    );
    insert(&c, &skewed);
    assert_eq!(c.undo_answer(), Err(StudyError::LaterAnswer));
}

#[test]
fn a_remote_event_that_is_itself_voided_does_not_block_undo() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    let own = good(&c, card);
    let own_event = events(&c, card)[0].clone();
    let seen = read_cached(&c.conn, card).unwrap().unwrap();
    let later = from_other_device(
        card,
        Id::new_v7(own_event.time_ms + 5 * MINUTE, &[6; 10]),
        Some(own.event),
        Some(&seen),
        Rating::Good,
        own_event.time_ms + 5 * MINUTE,
        own_event.day,
    );
    insert(&c, &later);
    // The other device took its answer back.
    let mut void = later.clone();
    void.id = Id::new_v7(own_event.time_ms + 6 * MINUTE, &[7; 10]);
    void.time_ms = own_event.time_ms + 6 * MINUTE;
    void.kind = EventKind::Void;
    void.target = Some(later.id);
    void.rating = None;
    void.state_before = None;
    void.state = None;
    void.step = None;
    void.memory = None;
    void.due_day = None;
    void.due_ms = None;
    insert(&c, &void);
    c.rebuild_schedule().unwrap();
    clock.advance(10 * MINUTE);
    let undone = c.undo_answer().unwrap().unwrap();
    assert_eq!(undone.event, own.event);
    assert_eq!(c.card_schedule(card).unwrap().state, CardState::New);
}

#[test]
fn undo_still_works_for_a_card_deleted_since() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    let answered = good(&c, card);
    let note: Id = c
        .conn
        .query_row("SELECT note FROM card WHERE id = ?1", [card], |r| r.get(0))
        .unwrap();
    c.delete_note(note).unwrap();
    assert_eq!(c.undo_answer().unwrap().unwrap().event, answered.event);
}
