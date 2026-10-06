//! Review history, daily counts, retention, the forecast and optimisation.

use std::sync::Arc;

use rusqlite::params;

use super::*;
use crate::clock::ManualClock;
use crate::collection::Collection;
use crate::deck::{PresetChange, default_deck, default_preset};
use crate::notetype::builtin;
use crate::scheduling::{
    CardState, Due, Rating, default_parameters, memory_from_history, parameter_set_id,
};
use crate::study::answer_tests::{DAY, HOUR, MINUTE, basic_card, good, setup};
use crate::study::fold_tests::{from_other_device, insert};
use crate::study::{Next, StudyError};

const DAY0: i64 = 19_675;

fn at(clock: &ManualClock, day: i64, hour: i64, minute: i64) {
    clock.set(day * DAY + hour * HOUR + minute * MINUTE);
}

fn answer(c: &Collection, card: Id, rating: Rating) {
    c.answer(card, rating, 2_000).unwrap();
}

fn basic_fields(c: &Collection) -> Vec<Id> {
    c.note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id)
        .collect()
}

/// A Basic card in a deck, and its note.
fn card_in(c: &Collection, deck: Id, front: &str) -> (Id, Id) {
    let f = basic_fields(c);
    let added = c
        .add_note_to_deck(deck, builtin::basic(), &[(f[0], front), (f[1], "back")])
        .unwrap();
    (added.cards[0], added.id)
}

/// A card that went through both learning steps on `day`, so it is a review card due 2 days later.
fn graduated(c: &Collection, clock: &ManualClock, card: Id, day: i64) {
    at(clock, day, 9, 0);
    good(c, card);
    at(clock, day, 9, 10);
    good(c, card);
    assert_eq!(c.card_schedule(card).unwrap().state, CardState::Review);
}

fn range(from: i64, to: i64) -> DayRange {
    DayRange { from, to }
}

// ---- Card history ----

#[test]
fn a_cards_history_lists_its_reviews_in_order_with_what_each_one_did() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    graduated(&c, &clock, card, DAY0);
    at(&clock, DAY0 + 2, 9, 0);
    answer(&c, card, Rating::Again);
    let history = c.card_history(card).unwrap();
    assert_eq!(history.len(), 3);
    let ratings: Vec<Rating> = history.iter().map(|e| e.rating).collect();
    assert_eq!(ratings, [Rating::Good, Rating::Good, Rating::Again]);
    assert_eq!(history[0].state_before, Some(CardState::New));
    assert_eq!(history[0].state, Some(CardState::Learning));
    assert_eq!(history[0].due, Some(Due::Minutes(10)));
    assert_eq!(history[1].state, Some(CardState::Review));
    assert_eq!(history[1].due, Some(Due::Days(2)));
    assert_eq!(history[2].state_before, Some(CardState::Review));
    assert_eq!(history[2].state, Some(CardState::Relearning));
    assert_eq!(history[2].due, Some(Due::Minutes(10)));
    assert_eq!(
        history
            .iter()
            .map(|e| e.days_since_previous)
            .collect::<Vec<_>>(),
        [None, Some(0), Some(2)]
    );
    assert_eq!(history[2].duration_ms, 2_000);
    assert_eq!(history[0].duration_ms, 3_000);
    assert!(history.iter().all(|e| e.memory.is_some()));
    assert_eq!(history[0].day, DAY0);
    assert!(history.windows(2).all(|w| w[0].time_ms < w[1].time_ms));
}

#[test]
fn an_undone_review_and_its_void_are_not_in_the_history_but_stay_stored() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    clock.advance(MINUTE);
    answer(&c, card, Rating::Again);
    c.undo_answer().unwrap().unwrap();
    assert_eq!(c.card_history(card).unwrap().len(), 1);
    assert_eq!(c.card_events(card).unwrap().len(), 3);
    // Answering after the undo adds to what is left.
    clock.advance(MINUTE);
    answer(&c, card, Rating::Hard);
    let ratings: Vec<Rating> = c
        .card_history(card)
        .unwrap()
        .iter()
        .map(|e| e.rating)
        .collect();
    assert_eq!(ratings, [Rating::Good, Rating::Hard]);
}

#[test]
fn a_new_card_has_an_empty_history_and_a_missing_one_is_not_found() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    assert!(c.card_history(card).unwrap().is_empty());
    assert_eq!(
        c.card_history(Id::from_bytes([7; 16])),
        Err(StudyError::NotFound)
    );
}

#[test]
fn a_deleted_card_keeps_its_history_and_restoring_it_shows_it_again() {
    let (c, _) = setup();
    let (card, note) = card_in(&c, default_deck(), "a");
    good(&c, card);
    c.delete_note(note).unwrap();
    assert_eq!(c.card_history(card).unwrap().len(), 1);
    c.restore_note(note).unwrap();
    assert_eq!(c.card_history(card).unwrap().len(), 1);
}

#[test]
fn a_review_from_another_device_is_in_the_history_in_its_place() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    at(&clock, DAY0, 9, 0);
    good(&c, card);
    let first = c.card_events(card).unwrap()[0].clone();
    let remote = from_other_device(
        card,
        Id::new_v7(first.time_ms + 5_000, &[3; 10]),
        Some(first.id),
        None,
        Rating::Good,
        first.time_ms + 5_000,
        DAY0,
    );
    insert(&c, &remote);
    let history = c.card_history(card).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].event, remote.id);
    assert_ne!(history[1].device, history[0].device);
}

// ---- Daily counts and retention ----

/// Day 0: card x answered Good twice (new, then learning, ends as a review card), card y Again once
/// (new). Day 2: x Again (a review card, a lapse) and Good (relearning). 2 seconds each.
fn two_days() -> (Collection, Arc<ManualClock>, Id, Id) {
    let (c, clock) = setup();
    let x = basic_card(&c, "x");
    let y = basic_card(&c, "y");
    graduated(&c, &clock, x, DAY0);
    at(&clock, DAY0, 9, 20);
    answer(&c, y, Rating::Again);
    at(&clock, DAY0 + 2, 9, 0);
    answer(&c, x, Rating::Again);
    at(&clock, DAY0 + 2, 9, 10);
    answer(&c, x, Rating::Good);
    (c, clock, x, y)
}

#[test]
fn daily_counts_split_answers_by_what_the_card_was() {
    let (c, _, _, _) = two_days();
    let days = c.daily_counts(range(DAY0 - 1, DAY0 + 3), None).unwrap();
    let want = |day, new, learning, relearning, review, again, passed, time_ms: u64| DailyCount {
        day,
        new,
        learning,
        relearning,
        review,
        again,
        review_passed: passed,
        time_ms,
    };
    assert_eq!(
        days,
        [
            want(DAY0 - 1, 0, 0, 0, 0, 0, 0, 0),
            want(DAY0, 2, 1, 0, 0, 1, 0, 8_000),
            want(DAY0 + 1, 0, 0, 0, 0, 0, 0, 0),
            want(DAY0 + 2, 0, 0, 1, 1, 1, 0, 4_000),
            want(DAY0 + 3, 0, 0, 0, 0, 0, 0, 0),
        ]
    );
    assert_eq!(days[1].answers(), 3);
}

#[test]
fn an_undone_answer_is_not_counted() {
    let (c, _, _, _) = two_days();
    c.undo_answer().unwrap().unwrap();
    let days = c.daily_counts(range(DAY0 + 2, DAY0 + 2), None).unwrap();
    assert_eq!((days[0].relearning, days[0].review), (0, 1));
    assert_eq!(days[0].time_ms, 2_000);
}

#[test]
fn a_reversed_range_is_empty_and_a_huge_one_is_cut() {
    let (c, _, _, _) = two_days();
    assert!(c.daily_counts(range(5, 4), None).unwrap().is_empty());
    let long = c.daily_counts(range(i64::MIN, DAY0), None).unwrap();
    assert_eq!(long.len(), 36_525);
    assert_eq!(long.last().unwrap().day, DAY0);
    assert_eq!(long.last().unwrap().answers(), 3);
    assert_eq!(DayRange::last(DAY0, 30).from, DAY0 - 29);
}

#[test]
fn counts_can_be_limited_to_a_deck_and_what_is_inside_it() {
    let (c, clock) = setup();
    let parent = c.create_deck("Polish", None).unwrap();
    let child = c.create_deck("Food", Some(parent)).unwrap();
    let (a, _) = card_in(&c, parent, "a");
    let (b, note_b) = card_in(&c, child, "b");
    let (d, _) = card_in(&c, default_deck(), "d");
    at(&clock, DAY0, 9, 0);
    for card in [a, b, d] {
        answer(&c, card, Rating::Good);
    }
    let on_day = |deck| c.daily_counts(range(DAY0, DAY0), deck).unwrap()[0].answers();
    assert_eq!(
        (on_day(None), on_day(Some(parent)), on_day(Some(child))),
        (3, 2, 1)
    );
    assert_eq!(on_day(Some(default_deck())), 1);
    // A card that is deleted is not in a deck any more, but its answer is still in the total.
    c.delete_note(note_b).unwrap();
    assert_eq!(
        (on_day(None), on_day(Some(parent)), on_day(Some(child))),
        (3, 1, 0)
    );
    // A card moved to another deck counts there from then on.
    c.move_cards(&[a], child).unwrap();
    assert_eq!((on_day(Some(parent)), on_day(Some(child))), (1, 1));
    assert_eq!(
        c.daily_counts(range(DAY0, DAY0), Some(Id::from_bytes([9; 16]))),
        Err(StudyError::NotFound)
    );
}

#[test]
fn a_day_is_the_one_the_answer_was_written_on_whatever_changes_later() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    c.set_day_start_hour(4).unwrap();
    clock.set_utc_offset_minutes(-300);
    // 03:30 local with the day starting at 4 am is still the day before.
    clock.set(DAY0 * DAY + 3 * HOUR + 30 * MINUTE + 300 * MINUTE);
    answer(&c, card, Rating::Good);
    let day = |c: &Collection| {
        c.daily_counts(range(DAY0 - 2, DAY0 + 2), None)
            .unwrap()
            .iter()
            .filter(|d| d.answers() > 0)
            .map(|d| d.day)
            .collect::<Vec<_>>()
    };
    assert_eq!(day(&c), [DAY0 - 1]);
    c.set_day_start_hour(0).unwrap();
    clock.set_utc_offset_minutes(840);
    assert_eq!(day(&c), [DAY0 - 1]);
}

#[test]
fn retention_counts_answers_on_review_cards_that_were_not_again() {
    let (c, clock, x, y) = two_days();
    // x was a review card on day 2 and lapsed: 0 of 1.
    let all = range(DAY0, DAY0 + 3);
    let r = c.retention(all, None).unwrap();
    assert_eq!((r.reviews, r.passed, r.rate()), (1, 0, Some(0.0)));
    // Two more review answers, both remembered.
    graduated(&c, &clock, y, DAY0 + 3);
    for (card, day) in [(x, DAY0 + 4), (y, DAY0 + 5)] {
        at(&clock, day, 9, 0);
        answer(&c, card, Rating::Good);
    }
    let r = c.retention(range(DAY0, DAY0 + 6), None).unwrap();
    assert_eq!((r.reviews, r.passed), (3, 2));
    assert!((r.rate().unwrap() - 2.0 / 3.0).abs() < 1e-12);
    // Learning answers on their own are not retention.
    let r = c.retention(range(DAY0, DAY0), None).unwrap();
    assert_eq!((r.reviews, r.rate()), (0, None));
    assert_eq!(
        c.retention(range(5, 4), None).unwrap(),
        Retention::default()
    );
}

// ---- The due forecast ----

/// Review cards with these due days, in the Default deck, set straight in the cache.
fn cards_due(c: &Collection, clock: &ManualClock, due: &[i64]) -> Vec<Id> {
    due.iter()
        .enumerate()
        .map(|(i, due)| {
            let card = basic_card(c, &format!("card {i}"));
            graduated(c, clock, card, DAY0);
            c.conn
                .execute(
                    "UPDATE card_schedule SET due_day = ?1 WHERE card = ?2",
                    params![due, card],
                )
                .unwrap();
            card
        })
        .collect()
}

#[test]
fn the_forecast_counts_overdue_today_and_the_next_thirty_days() {
    let (c, clock) = setup();
    let today = DAY0 + 10;
    cards_due(
        &c,
        &clock,
        &[
            today - 3,
            today - 1,
            today,
            today,
            today + 1,
            today + 29,
            today + 30,
            today + 31,
        ],
    );
    // A card in its first learning step, due in a minute (today).
    let learning = basic_card(&c, "learning");
    at(&clock, today, 9, 0);
    answer(&c, learning, Rating::Good);
    let f = c.due_forecast(30, None).unwrap();
    assert_eq!(f.today, today);
    assert_eq!((f.overdue, f.learning_today), (2, 1));
    assert_eq!(f.days.len(), 31);
    assert_eq!(
        f.days[0],
        ForecastDay {
            day: today,
            reviews: 2
        }
    );
    assert_eq!(
        f.days[30],
        ForecastDay {
            day: today + 30,
            reviews: 1
        }
    );
    let by_day: Vec<u32> = f.days.iter().map(|d| d.reviews).collect();
    assert_eq!(by_day.iter().sum::<u32>(), 5);
    assert_eq!((by_day[1], by_day[29]), (1, 1));
    assert!(f.days.windows(2).all(|w| w[1].day == w[0].day + 1));
    // A shorter window cuts the later days.
    assert_eq!(c.due_forecast(1, None).unwrap().days.len(), 2);
    assert_eq!(c.due_forecast(0, None).unwrap().days.len(), 1);
}

#[test]
fn the_forecast_leaves_out_suspended_and_deleted_cards_but_not_buried_ones() {
    let (c, clock) = setup();
    let today = DAY0 + 10;
    let cards = cards_due(&c, &clock, &[today + 1; 4]);
    at(&clock, today, 9, 0);
    c.suspend_cards(&[cards[0]]).unwrap();
    c.bury_cards(&[cards[1]]).unwrap();
    let note: Vec<u8> = c
        .conn
        .query_row("SELECT note FROM card WHERE id = ?1", [cards[2]], |r| {
            r.get(0)
        })
        .unwrap();
    c.delete_note(Id::from_bytes(note.try_into().unwrap()))
        .unwrap();
    let f = c.due_forecast(5, None).unwrap();
    assert_eq!(f.days[1].reviews, 2);
    c.unsuspend_cards(&[cards[0]]).unwrap();
    assert_eq!(c.due_forecast(5, None).unwrap().days[1].reviews, 3);
}

#[test]
fn the_forecast_can_be_limited_to_a_deck() {
    let (c, clock) = setup();
    let today = DAY0 + 10;
    let cards = cards_due(&c, &clock, &[today, today + 2, today + 2]);
    let parent = c.create_deck("Polish", None).unwrap();
    let child = c.create_deck("Food", Some(parent)).unwrap();
    c.move_cards(&[cards[1]], parent).unwrap();
    c.move_cards(&[cards[2]], child).unwrap();
    at(&clock, today, 9, 0);
    let days = |deck| {
        c.due_forecast(3, deck)
            .unwrap()
            .days
            .iter()
            .map(|d| d.reviews)
            .collect::<Vec<_>>()
    };
    assert_eq!(days(None), [1, 0, 2, 0]);
    assert_eq!(days(Some(parent)), [0, 0, 2, 0]);
    assert_eq!(days(Some(child)), [0, 0, 1, 0]);
    assert_eq!(days(Some(default_deck())), [1, 0, 0, 0]);
}

#[test]
fn what_the_forecast_says_is_what_the_queue_offers_on_that_day() {
    let (c, clock) = setup();
    let today = DAY0 + 10;
    cards_due(
        &c,
        &clock,
        &[today, today + 1, today + 1, today + 3, today + 3, today + 3],
    );
    at(&clock, today, 9, 0);
    let forecast = c.due_forecast(5, None).unwrap();
    for day in &forecast.days {
        at(&clock, day.day, 9, 0);
        let counts = c.study_counts(default_deck()).unwrap();
        // Nothing was answered, so the cards due on earlier days are overdue by then.
        let due_by_then: u32 = forecast
            .days
            .iter()
            .take_while(|d| d.day <= day.day)
            .map(|d| d.reviews)
            .sum();
        assert_eq!(counts.review, due_by_then, "day {}", day.day);
    }
}

// ---- Optimisation ----

/// A small deterministic generator.
struct Random(u64);

impl Random {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// `cards` new cards in the Default deck studied every day for `days` days by someone who
/// remembers about 85% of the time.
fn study_for(c: &Collection, clock: &ManualClock, cards: usize, days: i64) {
    for i in 0..cards {
        basic_card(c, &format!("card {i}"));
    }
    let mut random = Random(7);
    for day in 0..days {
        at(clock, DAY0 + day, 9, 0);
        for _ in 0..1_000 {
            match c.next_card(default_deck()).unwrap() {
                Next::Card { card, .. } => {
                    let rating = match random.below(100) {
                        0..=11 => Rating::Again,
                        12..=16 => Rating::Hard,
                        17..=93 => Rating::Good,
                        _ => Rating::Easy,
                    };
                    answer(c, card, rating);
                }
                Next::Waiting { until_ms, .. } => clock.set(until_ms),
                Next::Done { .. } => break,
            }
        }
    }
}

#[test]
fn the_optimiser_trains_on_the_history_and_a_preset_can_keep_the_result() {
    let (c, clock) = setup();
    study_for(&c, &clock, 40, 60);
    let data = c.optimisation_data(default_preset()).unwrap();
    assert_eq!(data.cards, 40);
    assert!(data.items <= data.reviews - data.cards);
    assert!(data.items > MIN_TRAINING_ITEMS * 3, "{} items", data.items);
    let total: u32 = c
        .daily_counts(range(DAY0, DAY0 + 59), None)
        .unwrap()
        .iter()
        .map(DailyCount::answers)
        .sum();
    assert_eq!(total, data.reviews);

    let tuned = data.optimise().unwrap();
    assert_eq!((tuned.cards, tuned.reviews), (data.cards, data.reviews));
    assert_eq!(tuned.parameters.len(), 21);
    assert!(tuned.parameters.iter().all(|p| p.is_finite()));
    assert_ne!(tuned.parameters, default_parameters());
    // The scheduler takes them, and the one-call version gives the same.
    crate::scheduling::Scheduler::new(Some(&tuned.parameters), 0.9).unwrap();
    assert_eq!(c.optimise_preset(default_preset()).unwrap(), tuned);

    // Nothing is stored until the host says so.
    assert!(
        c.preset(default_preset())
            .unwrap()
            .unwrap()
            .fsrs_parameters
            .is_empty()
    );
    let change = PresetChange {
        fsrs_parameters: Some(tuned.parameters.clone()),
        ..Default::default()
    };
    c.set_preset_options(default_preset(), &change).unwrap();
    let stored = c.preset(default_preset()).unwrap().unwrap().fsrs_parameters;
    assert_eq!(stored, tuned.parameters);
    // The next answer is scheduled with them and records which set it used.
    at(&clock, DAY0 + 200, 9, 0);
    let Next::Card { card, .. } = c.next_card(default_deck()).unwrap() else {
        panic!("a card is due after a long wait");
    };
    answer(&c, card, Rating::Good);
    let last = c.card_events(card).unwrap().pop().unwrap();
    assert_eq!(last.parameters, Some(parameter_set_id(&tuned.parameters)));
    // And they can be taken back.
    let reset = PresetChange {
        fsrs_parameters: Some(Vec::new()),
        ..Default::default()
    };
    c.set_preset_options(default_preset(), &reset).unwrap();
    assert!(
        c.preset(default_preset())
            .unwrap()
            .unwrap()
            .fsrs_parameters
            .is_empty()
    );
}

#[test]
fn too_little_history_is_refused_with_the_counts() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    for _ in 0..3 {
        good(&c, card);
        clock.advance(MINUTE);
    }
    assert_eq!(
        c.optimise_preset(default_preset()),
        Err(StudyError::NotEnoughHistory {
            items: 0,
            needed: MIN_TRAINING_ITEMS
        })
    );
    let empty = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((empty.cards, empty.reviews), (1, 3));
    assert_eq!(
        c.optimisation_data(Id::from_bytes([5; 16])).err(),
        Some(StudyError::NotFound)
    );
}

#[test]
fn each_preset_trains_on_its_own_cards_and_undone_reviews_are_left_out() {
    let (c, clock) = setup();
    let other = c.create_preset("Other").unwrap();
    let deck = c.create_deck("Other deck", None).unwrap();
    c.set_deck_preset(deck, other).unwrap();
    let (a, _) = card_in(&c, default_deck(), "a");
    let (b, note_b) = card_in(&c, deck, "b");
    let (d, _) = card_in(&c, deck, "d");
    at(&clock, DAY0, 9, 0);
    for card in [a, b, b, d] {
        answer(&c, card, Rating::Good);
        clock.advance(MINUTE);
    }
    let shape = |preset| {
        let t = c.optimisation_data(preset).unwrap();
        (t.cards, t.reviews, t.items)
    };
    assert_eq!(shape(default_preset()), (1, 1, 0));
    assert_eq!(shape(other), (2, 3, 0));
    // An undone review is not training data.
    c.undo_answer().unwrap().unwrap();
    assert_eq!(shape(other), (1, 2, 0));
    // A deleted card is not either, and moving a card moves its history.
    c.delete_note(note_b).unwrap();
    assert_eq!(shape(other), (0, 0, 0));
    c.move_cards(&[a], deck).unwrap();
    assert_eq!(shape(default_preset()), (0, 0, 0));
    assert_eq!(shape(other).0, 1);
}

#[test]
fn the_history_the_optimiser_gets_is_the_one_the_scheduler_answered_from() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    // Learning steps, a lapse and a relearning step, hard and easy answers, across several days.
    let steps = [
        (0, 9, 0, Rating::Good),
        (0, 9, 10, Rating::Again),
        (0, 9, 11, Rating::Good),
        (0, 9, 21, Rating::Good),
        (2, 9, 0, Rating::Good),
        (7, 9, 0, Rating::Hard),
        (12, 9, 0, Rating::Again),
        (12, 9, 10, Rating::Good),
        (14, 9, 0, Rating::Easy),
        (30, 9, 0, Rating::Good),
    ];
    for (day, hour, minute, rating) in steps {
        at(&clock, DAY0 + day, hour, minute);
        answer(&c, card, rating);
    }
    let data = c.optimisation_data(default_preset()).unwrap();
    assert_eq!((data.cards, data.reviews), (1, steps.len() as u32));
    let history = &data.histories[0];
    let history_days: Vec<u32> = history.iter().map(|r| r.day).collect();
    assert_eq!(history_days, [0, 0, 0, 0, 2, 7, 12, 12, 14, 30]);
    // FSRS, given the whole history at once, ends where the scheduler did answer by answer.
    let kept = c.card_schedule(card).unwrap().memory.unwrap();
    let derived = memory_from_history(default_parameters(), history);
    assert!(
        (kept.stability - derived.stability).abs() < 1e-3 * derived.stability
            && (kept.difficulty - derived.difficulty).abs() < 1e-3,
        "{kept:?} vs {derived:?}"
    );
}

#[test]
fn fsrs_parameters_are_checked_when_a_preset_stores_them() {
    let (c, _) = setup();
    let set = |p: Vec<f32>| {
        c.set_preset_options(
            default_preset(),
            &PresetChange {
                fsrs_parameters: Some(p),
                ..Default::default()
            },
        )
    };
    assert_eq!(
        set(vec![1.0, 2.0, 3.0]),
        Err(crate::deck::DeckError::Parameters)
    );
    let mut with_nan = default_parameters().to_vec();
    with_nan[3] = f32::NAN;
    assert_eq!(set(with_nan), Err(crate::deck::DeckError::Parameters));
    assert!(
        c.preset(default_preset())
            .unwrap()
            .unwrap()
            .fsrs_parameters
            .is_empty()
    );
    // A set from an older version with 19 numbers is filled to 21.
    set(default_parameters()[..19].to_vec()).unwrap();
    assert_eq!(
        c.preset(default_preset())
            .unwrap()
            .unwrap()
            .fsrs_parameters
            .len(),
        21
    );
}
