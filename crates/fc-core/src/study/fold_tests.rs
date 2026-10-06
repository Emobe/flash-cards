//! The fold and the cache: it must always equal what answering built, and it must settle events
//! that two devices wrote without seeing each other the same way everywhere.

use std::sync::Arc;

use rusqlite::Connection;

use super::answer_tests::{DAY, HOUR, MINUTE, basic_card, events, good, host, setup};
use super::schedule::{Cached, compute, read_cached};
use super::*;
use crate::clock::{Clock, ManualClock};
use crate::collection::Collection;
use crate::deck::default_preset;
use crate::scheduling::{CardState, Rating, Scheduler, Steps, default_parameters};

/// Every cache row, in a fixed order, as text (floats and all), for comparing two caches.
pub(super) fn snapshot(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare(
            "SELECT hex(card), state, step, due_day, due_ms, stability, difficulty, last_day,
                    hex(last_event), answers, lapses
             FROM card_schedule ORDER BY card",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok(format!(
                "{:?}",
                (
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<f64>>(5)?.map(f64::to_bits),
                    row.get::<_, Option<f64>>(6)?.map(f64::to_bits),
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                )
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// A small deterministic generator.
struct Random(u64);

impl Random {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

fn rating(random: &mut Random) -> Rating {
    match random.next(20) {
        0..=2 => Rating::Again,
        3..=4 => Rating::Hard,
        5..=16 => Rating::Good,
        _ => Rating::Easy,
    }
}

#[test]
fn rebuilding_gives_exactly_the_cache_that_answering_built() {
    let (c, clock) = setup();
    let cards: Vec<Id> = (0..25)
        .map(|n| basic_card(&c, &format!("card {n}")))
        .collect();
    let mut random = Random(42);
    for round in 0..900 {
        let card = cards[random.next(cards.len() as u64) as usize];
        let schedule = c.card_schedule(card).unwrap();
        // Answer when due (or at once for a new card), sometimes a bit late, sometimes early.
        let due = match (schedule.due_ms, schedule.due_day) {
            (Some(ms), _) => ms,
            (None, Some(day)) => day * DAY + 9 * HOUR,
            (None, None) => clock.now().unix_ms,
        };
        let jitter = random.next(3 * HOUR as u64) as i64 - HOUR;
        clock.set(clock.now().unix_ms.max(due + jitter));
        // Now and then the device's time zone changes.
        if random.next(40) == 0 {
            clock.set_utc_offset_minutes([-720, -60, 0, 60, 330, 840][random.next(6) as usize]);
        }
        let rating = rating(&mut random);
        c.answer(card, rating, random.next(20_000) as u32).unwrap();
        if round % 300 == 299 {
            let built = snapshot(&c.conn);
            c.rebuild_schedule().unwrap();
            assert_eq!(snapshot(&c.conn), built, "after {round} answers");
        }
    }
    let built = snapshot(&c.conn);
    assert_eq!(built.len(), cards.len());
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
    // The run had learning, relearning and review cards in it, and lapses.
    let lapses: i64 = c
        .conn
        .query_row("SELECT sum(lapses) FROM card_schedule", [], |r| r.get(0))
        .unwrap();
    assert!(lapses > 10, "{lapses}");
    let states: i64 = c
        .conn
        .query_row("SELECT count(DISTINCT state) FROM card_schedule", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(states >= 2);
}

#[test]
fn a_missing_or_damaged_cache_is_rebuilt() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    let built = snapshot(&c.conn);
    c.conn.execute("DELETE FROM card_schedule", []).unwrap();
    assert_eq!(c.card_schedule(card).unwrap().state, CardState::New);
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
    c.conn
        .execute("UPDATE card_schedule SET due_ms = 1, answers = 99", [])
        .unwrap();
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
    // A cache row for a card with no events (left over) is gone after a rebuild.
    c.conn
        .execute(
            "INSERT INTO card_schedule (card, state, last_day, last_event) VALUES (?1, 2, 0, ?1)",
            [Id::new_v7(0, &[3; 10])],
        )
        .unwrap();
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
}

#[test]
fn a_cache_from_another_version_of_the_fold_is_rebuilt_when_the_collection_opens() {
    let path = std::env::temp_dir().join(format!("fc-fold-open-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = Arc::new(ManualClock::new(19_675 * DAY + 9 * HOUR));
    let c = Collection::create(location, host(&clock)).unwrap();
    let card = basic_card(&c, "a");
    good(&c, card);
    let built = snapshot(&c.conn);
    c.conn
        .execute("UPDATE card_schedule SET due_ms = 1, answers = 99", [])
        .unwrap();
    c.close().unwrap();

    // Same version in `meta`: the cache is trusted, as it must be, or opening would be slow.
    let c = Collection::open(location, host(&clock)).unwrap();
    assert_ne!(snapshot(&c.conn), built);
    c.conn
        .execute(
            "UPDATE meta SET value = '0' WHERE key = 'schedule_cache_version'",
            [],
        )
        .unwrap();
    c.close().unwrap();

    let c = Collection::open(location, host(&clock)).unwrap();
    assert_eq!(snapshot(&c.conn), built);
    let version: String = c
        .conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schedule_cache_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, "1");
    c.close().unwrap();
    let _ = std::fs::remove_file(&path);
}

// ---- Events from another device ----

/// An event as another device would have written it, built on `previous`, with its result worked
/// out by that device from the card as it saw it (`seen`).
#[allow(clippy::too_many_arguments)]
pub(super) fn from_other_device(
    card: Id,
    id: Id,
    previous: Option<Id>,
    seen: Option<&Cached>,
    rating: Rating,
    time_ms: i64,
    day: i64,
) -> CardEvent {
    let steps = Steps {
        learning: &[1, 10],
        relearning: &[10],
    };
    let scheduler = Scheduler::new(None, 0.9).unwrap();
    let applied = compute(seen, day, time_ms, rating, steps, &scheduler, id).unwrap();
    CardEvent {
        id,
        card,
        kind: EventKind::Review,
        time_ms,
        utc_offset: 0,
        device: Id::new_v7(1, &[9; 10]),
        previous,
        day,
        rating: Some(rating),
        duration_ms: Some(1_000),
        preset: Some(default_preset()),
        desired_retention: Some(0.9),
        parameters: Some(crate::scheduling::parameter_set_id(default_parameters())),
        steps: Some("1 10|10".to_owned()),
        state_before: Some(applied.state_before),
        state: Some(applied.state),
        step: Some(applied.step),
        memory: Some(applied.memory),
        due_day: applied.due_day,
        due_ms: applied.due_ms,
        target: None,
    }
}

pub(super) fn insert(c: &Collection, event: &CardEvent) {
    c.write(|w| w.insert_row("card_event", event.id, event.values()))
        .unwrap();
}

#[test]
fn two_events_on_the_same_previous_are_settled_by_recomputing_the_later_one() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    let first = events(&c, card)[0].clone();
    let after_first = read_cached(&c.conn, card).unwrap().unwrap();

    // This device answers Good ten minutes later. The other device, which only had the first
    // answer too, answered Again at 5 minutes: it sorts before ours.
    clock.advance(10 * MINUTE);
    good(&c, card);
    let mine = events(&c, card)[1].clone();
    let theirs = from_other_device(
        card,
        Id::new_v7(first.time_ms + 5 * MINUTE, &[5; 10]),
        Some(first.id),
        Some(&after_first),
        Rating::Again,
        first.time_ms + 5 * MINUTE,
        first.day,
    );
    assert!(theirs.time_ms < mine.time_ms);
    insert(&c, &theirs);
    c.rebuild_schedule().unwrap();

    // Folded: first, theirs (built on first, so as recorded), then mine, which built on first and
    // not on theirs, so it is computed again from the card as theirs left it.
    let after_theirs = after_event(&after_first, &theirs);
    let scheduler = Scheduler::new(None, 0.9).unwrap();
    let steps = Steps {
        learning: &[1, 10],
        relearning: &[10],
    };
    let again = compute(
        Some(&after_theirs),
        mine.day,
        mine.time_ms,
        Rating::Good,
        steps,
        &scheduler,
        mine.id,
    )
    .unwrap();
    let got = read_cached(&c.conn, card).unwrap().unwrap();
    assert_eq!(got.state, again.state);
    assert_eq!(got.step, again.step);
    assert_eq!(got.memory, again.memory);
    assert_eq!(got.due_ms, again.due_ms);
    assert_eq!(got.last_event, mine.id);
    assert_eq!(got.answers, 3);
    // And it is not what this device saw before the other answer arrived.
    assert_ne!(got.memory, mine.memory.unwrap());
}

fn after_event(before: &Cached, event: &CardEvent) -> Cached {
    super::schedule::after(
        Some(before),
        event.id,
        event.day,
        event.rating,
        &event.applied().unwrap(),
    )
}

#[test]
fn the_fold_does_not_depend_on_the_order_events_arrived_in() {
    let (a, _) = setup();
    let (b, _) = setup();
    // Two collections get the same events for one card, inserted in opposite orders.
    let card = Id::new_v7(0, &[7; 10]);
    let first = from_other_device(
        card,
        Id::new_v7(100, &[1; 10]),
        None,
        None,
        Rating::Good,
        100,
        5,
    );
    let seen = after_event_new(&first);
    let x = from_other_device(
        card,
        Id::new_v7(200, &[2; 10]),
        Some(first.id),
        Some(&seen),
        Rating::Good,
        200,
        5,
    );
    let y = from_other_device(
        card,
        Id::new_v7(300, &[3; 10]),
        Some(first.id),
        Some(&seen),
        Rating::Hard,
        300,
        5,
    );
    for event in [&first, &x, &y] {
        insert(&a, event);
    }
    for event in [&y, &first, &x] {
        insert(&b, event);
    }
    a.rebuild_schedule().unwrap();
    b.rebuild_schedule().unwrap();
    let rows = |c: &Collection| read_cached(&c.conn, card).unwrap().unwrap();
    assert_eq!(rows(&a), rows(&b));
    assert_eq!(rows(&a).last_event, y.id);
    assert_eq!(rows(&a).answers, 3);
}

fn after_event_new(event: &CardEvent) -> Cached {
    super::schedule::after(
        None,
        event.id,
        event.day,
        event.rating,
        &event.applied().unwrap(),
    )
}

#[test]
fn an_event_whose_parameter_set_is_missing_keeps_its_recorded_result() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    let first = events(&c, card)[0].clone();
    let mut other = from_other_device(
        card,
        Id::new_v7(first.time_ms + 1_000, &[4; 10]),
        None, // does not build on the first
        None,
        Rating::Good,
        first.time_ms + 1_000,
        first.day,
    );
    // Parameters this device was never sent.
    other.parameters = Some(Id::new_v7(0, &[8; 10]));
    insert(&c, &other);
    c.rebuild_schedule().unwrap();
    let got = read_cached(&c.conn, card).unwrap().unwrap();
    // The review is not lost: it counts, and the card ends in the state it recorded.
    assert_eq!(got.answers, 2);
    assert_eq!(got.last_event, other.id);
    assert_eq!(got.state, other.state.unwrap());
}

#[test]
fn an_unknown_kind_that_builds_on_the_last_event_applies_its_recorded_result() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    let first = events(&c, card)[0].clone();
    let mut set_due = from_other_device(
        card,
        Id::new_v7(first.time_ms + 1_000, &[6; 10]),
        Some(first.id),
        None,
        Rating::Good,
        first.time_ms + 1_000,
        first.day,
    );
    set_due.kind = EventKind::Unknown("set_due".to_owned());
    set_due.rating = None;
    set_due.state = Some(CardState::Review);
    set_due.step = Some(0);
    set_due.due_day = Some(99_999);
    set_due.due_ms = None;
    insert(&c, &set_due);
    c.rebuild_schedule().unwrap();
    let got = read_cached(&c.conn, card).unwrap().unwrap();
    assert_eq!(got.state, CardState::Review);
    assert_eq!(got.due_day, Some(99_999));
    assert_eq!(got.last_event, set_due.id);
    assert_eq!(got.answers, 1, "an unknown kind is not an answer");

    // A review built on it starts from what it recorded.
    let mut next = from_other_device(
        card,
        Id::new_v7(first.time_ms + 2_000, &[7; 10]),
        Some(set_due.id),
        Some(&got),
        Rating::Good,
        first.time_ms + 2_000,
        first.day + 1,
    );
    next.previous = Some(set_due.id);
    insert(&c, &next);
    c.rebuild_schedule().unwrap();
    let got = read_cached(&c.conn, card).unwrap().unwrap();
    assert_eq!(got.last_event, next.id);
    assert_eq!(got.answers, 2);

    // One that does not build on the last event cannot be recomputed, so it is kept but ignored.
    let mut stray = set_due.clone();
    stray.id = Id::new_v7(first.time_ms + 3_000, &[8; 10]);
    stray.time_ms = first.time_ms + 3_000;
    stray.previous = Some(first.id);
    stray.due_day = Some(1);
    insert(&c, &stray);
    c.rebuild_schedule().unwrap();
    assert_eq!(
        read_cached(&c.conn, card).unwrap().unwrap().last_event,
        next.id
    );
    assert_eq!(events(&c, card).len(), 4);
}
