//! A month of studying, simulated: 200 notes in two nested decks, a person who studies every
//! morning until the queue says Done. The table of what happened each day is pinned, and the same
//! month is run in other time zones, across a daylight saving change and with another start hour:
//! the table must not change.

use std::sync::Arc;

use super::answer_tests::{DAY, HOUR, MINUTE, host};
use super::fold_tests::snapshot;
use super::*;
use crate::clock::{Clock, ManualClock};
use crate::collection::Collection;
use crate::deck::default_deck;
use crate::notetype::builtin;
use crate::scheduling::{CardState, Rating, study_day};

/// 2023-11-14 (study day 19,675 at a midnight start and offset 0).
const FIRST_DAY: i64 = 19_675;
const DAYS: usize = 30;
const NOTES: usize = 200;

/// One day: what the queue held when the person sat down (new, learning, review), and how many new
/// cards were started, reviews done and learning or relearning answers given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Day {
    morning: (u32, u32, u32),
    introduced: u32,
    reviews: u32,
    learning: u32,
}

struct Setup {
    /// The device's UTC offset in minutes on each day, to model travel and daylight saving.
    offset: fn(usize) -> i32,
    start_hour: u8,
    /// When the person sits down, in minutes after local midnight.
    session: i64,
}

fn rating(index: usize, answer: u32) -> Rating {
    // SplitMix64 of (card number, answer number): 85% Good, 10% Again, 3% Hard, 2% Easy.
    let mut z = ((index as u64) << 20 | u64::from(answer)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    match z % 100 {
        0..=9 => Rating::Again,
        10..=12 => Rating::Hard,
        13..=14 => Rating::Easy,
        _ => Rating::Good,
    }
}

fn run(setup: &Setup) -> Vec<Day> {
    // The same IDs on every run, so the fuzz is the same: the notes are made at one fixed time,
    // whatever the time zone, and every random number comes from one sequence.
    crate::id::seeded::seed(2026);
    let clock = Arc::new(ManualClock::new((FIRST_DAY - 10) * DAY));
    let c = Collection::create(":memory:", host(&clock)).unwrap();
    c.set_day_start_hour(setup.start_hour).unwrap();
    let parent = c.create_deck("Polish", None).unwrap();
    let child = c.create_deck("Food", Some(parent)).unwrap();
    let fields: Vec<Id> = c
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id)
        .collect();
    let mut cards = Vec::new();
    for i in 0..NOTES {
        let deck = if i % 2 == 0 { parent } else { child };
        let added = c
            .add_note_to_deck(
                deck,
                builtin::basic(),
                &[(fields[0], &format!("word {i}")), (fields[1], "slowo")],
            )
            .unwrap();
        cards.push(added.cards[0]);
        clock.advance(1);
    }
    let index = |card: Id| cards.iter().position(|c| *c == card).unwrap();

    let mut days = Vec::new();
    for d in 0..DAYS {
        let offset = (setup.offset)(d);
        clock.set_utc_offset_minutes(offset);
        clock.set(
            (FIRST_DAY + d as i64) * DAY + setup.session * MINUTE - i64::from(offset) * MINUTE,
        );
        let today = study_day(clock.now().unix_ms, offset, setup.start_hour);
        let k = c.study_counts(parent).unwrap();
        let mut day = Day {
            morning: (k.new, k.learning, k.review),
            introduced: 0,
            reviews: 0,
            learning: 0,
        };
        for _ in 0..5_000 {
            match c.next_card(parent).unwrap() {
                Next::Card { card, state, .. } => {
                    let schedule = c.card_schedule(card).unwrap();
                    match state {
                        CardState::New => day.introduced += 1,
                        CardState::Review => {
                            // Nothing is shown before its day.
                            assert!(schedule.due_day.unwrap() <= today, "day {d}");
                            day.reviews += 1;
                        }
                        CardState::Learning | CardState::Relearning => {
                            assert!(
                                schedule.due_ms.unwrap() <= clock.now().unix_ms + LEARN_AHEAD_MS,
                                "day {d}"
                            );
                            day.learning += 1;
                        }
                    }
                    c.answer(card, rating(index(card), schedule.answers), 2_000)
                        .unwrap();
                }
                Next::Waiting { until_ms, .. } => clock.set(until_ms),
                Next::Done { .. } => break,
            }
        }
        assert_eq!(
            c.next_card(parent).unwrap(),
            Next::Done {
                counts: Counts::default()
            },
            "day {d} did not finish"
        );
        // The limits of the Default preset, step 1.5.
        assert!(
            day.introduced <= 20,
            "day {d}: {} new cards",
            day.introduced
        );
        assert!(day.reviews <= 200, "day {d}: {} reviews", day.reviews);
        days.push(day);
    }

    // Every card was introduced by day 10.
    let introduced: u32 = days.iter().take(10).map(|d| d.introduced).sum();
    assert_eq!(introduced as usize, NOTES);
    // What was cached while answering is what the events add up to.
    let built = snapshot(&c.conn);
    c.rebuild_schedule().unwrap();
    assert_eq!(snapshot(&c.conn), built);
    days
}

fn midnight_utc(_: usize) -> i32 {
    0
}

fn rows(days: &[Day]) -> Vec<[u32; 6]> {
    days.iter()
        .map(|d| {
            [
                d.morning.0,
                d.morning.1,
                d.morning.2,
                d.introduced,
                d.reviews,
                d.learning,
            ]
        })
        .collect()
}

/// The 30 days, one row each: new, learning and review cards waiting in the morning, then new cards
/// started, reviews done and learning answers given.
const MONTH: [[u32; 6]; DAYS] = [
    [20, 0, 0, 20, 0, 27],
    [20, 0, 3, 20, 3, 24],
    [20, 0, 19, 20, 19, 30],
    [20, 0, 26, 20, 26, 25],
    [20, 0, 20, 20, 20, 29],
    [20, 0, 28, 20, 28, 33],
    [20, 0, 32, 20, 32, 30],
    [20, 0, 33, 20, 33, 31],
    [20, 0, 29, 20, 29, 32],
    [20, 0, 36, 20, 36, 40],
    [0, 0, 35, 0, 35, 4],
    [0, 0, 28, 0, 28, 3],
    [0, 0, 28, 0, 28, 3],
    [0, 0, 24, 0, 24, 3],
    [0, 0, 23, 0, 23, 4],
    [0, 0, 32, 0, 32, 3],
    [0, 0, 29, 0, 29, 5],
    [0, 0, 27, 0, 27, 1],
    [0, 0, 16, 0, 16, 3],
    [0, 0, 29, 0, 29, 4],
    [0, 0, 20, 0, 20, 3],
    [0, 0, 28, 0, 28, 5],
    [0, 0, 19, 0, 19, 2],
    [0, 0, 12, 0, 12, 1],
    [0, 0, 7, 0, 7, 0],
    [0, 0, 9, 0, 9, 3],
    [0, 0, 12, 0, 12, 1],
    [0, 0, 8, 0, 8, 2],
    [0, 0, 7, 0, 7, 0],
    [0, 0, 8, 0, 8, 1],
];

#[test]
fn a_month_of_studying_gives_the_pinned_table() {
    let days = run(&Setup {
        offset: midnight_utc,
        start_hour: 0,
        session: 9 * 60,
    });
    assert_eq!(rows(&days), MONTH);
}

fn utc_minus_12(_: usize) -> i32 {
    -720
}

fn utc_minus_5(_: usize) -> i32 {
    -300
}

fn utc_plus_5_30(_: usize) -> i32 {
    330
}

fn utc_plus_14(_: usize) -> i32 {
    840
}

#[test]
fn the_table_does_not_change_in_other_time_zones() {
    let base = rows(&run(&Setup {
        offset: midnight_utc,
        start_hour: 0,
        session: 9 * 60,
    }));
    for offset in [utc_minus_12, utc_minus_5, utc_plus_5_30, utc_plus_14] {
        let found = rows(&run(&Setup {
            offset,
            start_hour: 0,
            session: 9 * 60,
        }));
        assert_eq!(found, base, "offset {}", offset(0));
    }
}

#[test]
fn the_table_does_not_change_across_a_daylight_saving_change() {
    fn spring_forward(day: usize) -> i32 {
        if day < 12 { 60 } else { 120 }
    }
    fn fall_back(day: usize) -> i32 {
        if day < 20 { 120 } else { 60 }
    }
    let base = rows(&run(&Setup {
        offset: midnight_utc,
        start_hour: 0,
        session: 9 * 60,
    }));
    for offset in [spring_forward as fn(usize) -> i32, fall_back] {
        let found = rows(&run(&Setup {
            offset,
            start_hour: 0,
            session: 9 * 60,
        }));
        assert_eq!(found, base);
    }
}

#[test]
fn the_table_does_not_change_with_another_start_hour() {
    let base = rows(&run(&Setup {
        offset: midnight_utc,
        start_hour: 0,
        session: 9 * 60,
    }));
    // The day starts at 4 am, so a session at 03:30 still belongs to the day before.
    let early = rows(&run(&Setup {
        offset: midnight_utc,
        start_hour: 4,
        session: 3 * 60 + 30,
    }));
    assert_eq!(early, base);
    // And a late-night session with the day starting at 4 am.
    let late = rows(&run(&Setup {
        offset: midnight_utc,
        start_hour: 4,
        session: 22 * 60,
    }));
    assert_eq!(late, base);
}

#[test]
fn a_person_who_skips_days_comes_back_to_a_pile_of_reviews_that_the_limit_caps() {
    crate::id::seeded::seed(11);
    let clock = Arc::new(ManualClock::new(FIRST_DAY * DAY + 9 * HOUR));
    let c = Collection::create(":memory:", host(&clock)).unwrap();
    let fields: Vec<Id> = c
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id)
        .collect();
    let mut cards = Vec::new();
    for i in 0..120 {
        cards.push(
            c.add_note(
                builtin::basic(),
                &[(fields[0], &format!("word {i}")), (fields[1], "b")],
            )
            .unwrap()
            .cards[0],
        );
        clock.advance(1);
    }
    // Today: 120 new cards and a limit of 20 a day. Learn 20, finish their steps.
    c.set_preset_options(
        crate::deck::default_preset(),
        &crate::deck::PresetChange {
            new_per_day: Some(120),
            reviews_per_day: Some(30),
            ..Default::default()
        },
    )
    .unwrap();
    while let Next::Card { card, .. } = c.next_card(default_deck()).unwrap() {
        c.answer(card, Rating::Easy, 1_000).unwrap();
    }
    // Two months away: all 120 are overdue, and 30 are offered.
    clock.advance(60 * DAY);
    let k = c.study_counts(default_deck()).unwrap();
    assert_eq!((k.new, k.learning, k.review), (0, 0, 30));
    for _ in 0..30 {
        let Next::Card { card, .. } = c.next_card(default_deck()).unwrap() else {
            panic!("expected a card");
        };
        c.answer(card, Rating::Good, 1_000).unwrap();
    }
    assert_eq!(
        c.next_card(default_deck()).unwrap(),
        Next::Done {
            counts: Counts::default()
        }
    );
    // Tomorrow another 30 of the pile.
    clock.advance(DAY);
    assert_eq!(c.study_counts(default_deck()).unwrap().review, 30);
}
