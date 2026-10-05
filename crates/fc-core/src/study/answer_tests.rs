//! Answering: the events it writes, the cache it updates, and the day boundary.

use std::sync::Arc;

use super::*;
use crate::clock::{Clock, Host, ManualClock};
use crate::collection::Collection;
use crate::deck::{PresetChange, default_deck, default_preset};
use crate::notetype::builtin;
use crate::scheduling::{
    CardState, Due, Rating, Scheduler, default_parameters, fuzz, parameter_set_id, study_day,
};

const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;
const MINUTE: i64 = 60_000;
/// 2023-11-14 00:00 UTC, which is study day 19,675 at a midnight start and offset 0.
const DAY0: i64 = 19_675 * DAY;

fn host(clock: &Arc<ManualClock>) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes(*b"installation-one"),
    }
}

/// A collection whose clock is at 09:00 UTC on `DAY0`.
fn setup() -> (Collection, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(DAY0 + 9 * HOUR));
    let c = Collection::create(":memory:", host(&clock)).unwrap();
    (c, clock)
}

fn basic_card(c: &Collection, front: &str) -> Id {
    let f: Vec<Id> = c
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id)
        .collect();
    c.add_note(builtin::basic(), &[(f[0], front), (f[1], "back")])
        .unwrap()
        .cards[0]
}

fn events(c: &Collection, card: Id) -> Vec<CardEvent> {
    c.card_events(card).unwrap()
}

fn good(c: &Collection, card: Id) -> Answered {
    c.answer(card, Rating::Good, 3_000).unwrap()
}

// ---- One answer ----

#[test]
fn a_new_card_reads_as_new() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    let schedule = c.card_schedule(card).unwrap();
    assert_eq!(schedule.state, CardState::New);
    assert_eq!(schedule.answers, 0);
    assert_eq!(schedule.memory, None);
    assert!(events(&c, card).is_empty());
}

#[test]
fn the_first_good_answer_puts_the_card_on_its_second_step() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    let answered = good(&c, card);
    assert_eq!(answered.state, CardState::Learning);
    assert_eq!(answered.due_ms, Some(clock.now().unix_ms + 10 * MINUTE));
    assert_eq!(answered.due_day, None);
    assert_eq!(answered.interval_days, None);

    let schedule = c.card_schedule(card).unwrap();
    assert_eq!(schedule.state, CardState::Learning);
    assert_eq!(schedule.step, 1);
    assert_eq!(schedule.due_ms, answered.due_ms);
    assert_eq!(schedule.last_day, Some(19_675));
    assert_eq!((schedule.answers, schedule.lapses), (1, 0));
    assert!((schedule.memory.unwrap().stability - 2.3065).abs() < 1e-3);
}

#[test]
fn an_answer_records_every_input_and_result() {
    let (c, clock) = setup();
    clock.set_utc_offset_minutes(120);
    let card = basic_card(&c, "a");
    let answered = c.answer(card, Rating::Good, 4_321).unwrap();
    let [event] = events(&c, card).try_into().unwrap();
    assert_eq!(event.id, answered.event);
    assert_eq!(event.card, card);
    assert_eq!(event.kind, EventKind::Review);
    assert_eq!(event.time_ms, clock.now().unix_ms);
    assert_eq!(event.utc_offset, 120);
    assert_eq!(event.device, c.device_id().unwrap());
    assert_eq!(event.previous, None);
    assert_eq!(event.day, 19_675);
    assert_eq!(event.rating, Some(Rating::Good));
    assert_eq!(event.duration_ms, Some(4_321));
    assert_eq!(event.preset, Some(default_preset()));
    assert_eq!(event.desired_retention, Some(0.9));
    assert_eq!(
        event.parameters,
        Some(parameter_set_id(default_parameters()))
    );
    assert_eq!(event.steps.as_deref(), Some("1 10"));
    assert_eq!(event.state_before, Some(CardState::New));
    assert_eq!(event.state, Some(CardState::Learning));
    assert_eq!(event.step, Some(1));
    assert!(event.memory.is_some());
    assert_eq!(event.due_day, None);
    assert_eq!(event.due_ms, answered.due_ms);
    assert_eq!(event.target, None);
}

#[test]
fn the_parameter_set_is_stored_once_and_has_the_same_id_everywhere() {
    let (a, _) = setup();
    let (b, _) = setup();
    for c in [&a, &b] {
        let card = basic_card(c, "a");
        good(c, card);
        good(c, card);
    }
    let rows = |c: &Collection| -> Vec<(Id, Vec<u8>)> {
        let mut statement = c
            .conn
            .prepare("SELECT id, values_f32 FROM fsrs_parameter_set")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(rows(&a).len(), 1);
    assert_eq!(rows(&a), rows(&b));
    assert_eq!(rows(&a)[0].0, parameter_set_id(default_parameters()));
    assert_eq!(rows(&a)[0].1.len(), 84);
}

#[test]
fn a_preset_with_its_own_parameters_records_them() {
    let (c, _) = setup();
    let mut own = default_parameters().to_vec();
    own[2] = 3.0;
    let text: Vec<String> = own.iter().map(f32::to_string).collect();
    c.write(|w| {
        w.set(
            "options_preset",
            default_preset(),
            "fsrs_parameters",
            rusqlite::types::Value::Text(text.join(" ")),
        )
    })
    .unwrap();
    let card = basic_card(&c, "a");
    good(&c, card);
    let [event] = events(&c, card).try_into().unwrap();
    assert_eq!(event.parameters, Some(parameter_set_id(&own)));
    assert_ne!(
        event.parameters,
        Some(parameter_set_id(default_parameters()))
    );
    // And the scheduler used them: S after a first Good is w[2].
    assert!((event.memory.unwrap().stability - 3.0).abs() < 1e-3);
}

#[test]
fn an_answer_uses_the_options_of_the_cards_deck() {
    let (c, _) = setup();
    let preset = c.create_preset("Slow").unwrap();
    c.set_preset_options(
        preset,
        &PresetChange {
            learning_steps: Some(vec![5, 60, 600]),
            desired_retention: Some(0.8),
            ..PresetChange::default()
        },
    )
    .unwrap();
    let deck = c.create_deck("Slow", None).unwrap();
    c.set_deck_preset(deck, preset).unwrap();
    let fields: Vec<Id> = c
        .note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id)
        .collect();
    let card = c
        .add_note_to_deck(
            deck,
            builtin::basic(),
            &[(fields[0], "x"), (fields[1], "y")],
        )
        .unwrap()
        .cards[0];
    let answered = c.answer(card, Rating::Good, 0).unwrap();
    assert_eq!(answered.state, CardState::Learning);
    let [event] = events(&c, card).try_into().unwrap();
    assert_eq!(event.preset, Some(preset));
    assert_eq!(event.desired_retention, Some(0.8));
    assert_eq!(event.steps.as_deref(), Some("5 60 600"));
    assert_eq!(event.due_ms, Some(event.time_ms + 60 * MINUTE));
}

#[test]
fn a_long_duration_is_recorded_as_one_hour() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    c.answer(card, Rating::Good, u32::MAX).unwrap();
    assert_eq!(events(&c, card)[0].duration_ms, Some(3_600_000));
}

#[test]
fn a_card_that_does_not_exist_or_is_deleted_cannot_be_answered() {
    let (c, _) = setup();
    assert_eq!(
        c.answer(Id::new_v7(0, &[1; 10]), Rating::Good, 0),
        Err(StudyError::NotFound)
    );
    let card = basic_card(&c, "a");
    let note = c.cards_of_note_for_test(card);
    c.delete_note(note).unwrap();
    assert_eq!(c.answer(card, Rating::Good, 0), Err(StudyError::NotFound));
    assert!(events(&c, card).is_empty());
    c.restore_note(note).unwrap();
    assert!(c.answer(card, Rating::Good, 0).is_ok());
}

impl Collection {
    /// The note a card belongs to.
    fn cards_of_note_for_test(&self, card: Id) -> Id {
        self.conn
            .query_row("SELECT note FROM card WHERE id = ?1", [card], |r| r.get(0))
            .unwrap()
    }
}

// ---- Learning, graduating and lapsing over days ----

#[test]
fn five_good_answers_follow_the_steps_and_then_the_intervals() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    let first = good(&c, card);
    assert_eq!(first.due_ms, Some(clock.now().unix_ms + 10 * MINUTE));

    clock.advance(10 * MINUTE);
    let second = good(&c, card);
    // Graduated, after 2 days (no fuzz under 3 days), FSRS counting the same-day answer without
    // inflating the interval (finding 1 of ADR 0007).
    assert_eq!(second.state, CardState::Review);
    assert_eq!(second.interval_days, Some(2));
    assert_eq!(second.due_day, Some(19_677));

    // The third answer, 2 days later, is the 11 days of step 0.5 before fuzz (no fuzz was applied
    // to the 2 days, so the elapsed time is exactly 2). The later ones start from a fuzzed
    // elapsed time, so each is checked against the scheduler run on the card's own memory.
    let scheduler = Scheduler::new(None, 0.9).unwrap();
    let mut day = 19_677;
    for (n, expected) in [Some(11u32), None, None].into_iter().enumerate() {
        clock.set(day * DAY + 9 * HOUR);
        let before = c.card_schedule(card).unwrap();
        let answered = good(&c, card);
        let elapsed = (day - before.last_day.unwrap()) as u32;
        let unfuzzed =
            scheduler.next(before.memory, elapsed).unwrap()[Rating::Good as usize].interval_days;
        if let Some(expected) = expected {
            assert_eq!(unfuzzed, expected);
        } else {
            // About 46 and 163, give or take the fuzz of the answer before.
            let about = [46u32, 163][n - 1];
            assert!(
                unfuzzed.abs_diff(about) <= about / 10,
                "{unfuzzed} vs {about}"
            );
        }
        let interval = answered.interval_days.unwrap();
        assert_eq!(interval, fuzz(unfuzzed, answered.event));
        assert_eq!(answered.due_day, Some(day + i64::from(interval)));
        day += i64::from(interval);
    }
    let schedule = c.card_schedule(card).unwrap();
    assert_eq!((schedule.answers, schedule.lapses), (5, 0));
    let all = events(&c, card);
    assert_eq!(all.len(), 5);
    // Each event built on the one before it.
    assert_eq!(all[0].previous, None);
    for pair in all.windows(2) {
        assert_eq!(pair[1].previous, Some(pair[0].id));
    }
}

#[test]
fn again_on_a_review_card_is_a_lapse_that_goes_through_the_relearning_steps() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    clock.advance(10 * MINUTE);
    good(&c, card);
    clock.set(19_677 * DAY + 9 * HOUR);
    let lapse = c.answer(card, Rating::Again, 0).unwrap();
    assert_eq!(lapse.state, CardState::Relearning);
    assert_eq!(lapse.due_ms, Some(clock.now().unix_ms + 10 * MINUTE));
    let schedule = c.card_schedule(card).unwrap();
    assert_eq!((schedule.answers, schedule.lapses), (3, 1));
    let last = events(&c, card).pop().unwrap();
    assert_eq!(last.state_before, Some(CardState::Review));
    assert_eq!(last.steps.as_deref(), Some("10"));

    clock.advance(10 * MINUTE);
    let back = good(&c, card);
    assert_eq!(back.state, CardState::Review);
    assert!(back.interval_days.unwrap() >= 1);
    assert_eq!(c.card_schedule(card).unwrap().lapses, 1);
}

#[test]
fn a_preset_without_steps_graduates_at_once() {
    let (c, _) = setup();
    c.set_preset_options(
        default_preset(),
        &PresetChange {
            learning_steps: Some(vec![]),
            relearning_steps: Some(vec![]),
            ..PresetChange::default()
        },
    )
    .unwrap();
    let card = basic_card(&c, "a");
    let answered = good(&c, card);
    assert_eq!(answered.state, CardState::Review);
    assert_eq!(answered.interval_days, Some(2));
    let [event] = events(&c, card).try_into().unwrap();
    assert_eq!(event.steps.as_deref(), Some(""));
}

// ---- Event time ----

#[test]
fn an_event_is_never_earlier_than_the_one_it_builds_on() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    good(&c, card);
    let first = events(&c, card)[0].time_ms;
    // The clock is set back an hour (a wrong setting, or travelling with a bad clock).
    clock.set(first - HOUR);
    good(&c, card);
    let all = events(&c, card);
    assert_eq!(all[1].time_ms, first + 1);
    assert_eq!(all[1].previous, Some(all[0].id));
    // Still the same one-millisecond step on a third answer at the same clock.
    good(&c, card);
    assert_eq!(events(&c, card)[2].time_ms, first + 2);
}

// ---- The day boundary ----

/// The unix time of a local wall-clock moment: day `day`, `hour`:`minute`, at `offset` minutes
/// ahead of UTC.
fn local(day: i64, hour: i64, minute: i64, offset: i32) -> i64 {
    day * DAY + hour * HOUR + minute * MINUTE - i64::from(offset) * MINUTE
}

#[test]
fn an_answer_just_before_and_after_the_boundary_belongs_to_different_days() {
    for (start_hour, offset) in [(0u8, 0), (4, 0), (23, 0), (0, -720), (4, 840), (23, -720)] {
        let (c, clock) = setup();
        c.set_day_start_hour(start_hour).unwrap();
        clock.set_utc_offset_minutes(offset);
        let before = basic_card(&c, "before");
        let after = basic_card(&c, "after");
        // Local `start_hour`:00 on day 20,000, less and plus a minute.
        let h = i64::from(start_hour);
        clock.set(local(20_000, h, 0, offset) - MINUTE);
        c.answer(before, Rating::Good, 0).unwrap();
        clock.set(local(20_000, h, 0, offset));
        c.answer(after, Rating::Good, 0).unwrap();
        assert_eq!(
            events(&c, before)[0].day,
            19_999,
            "start {start_hour} offset {offset}"
        );
        assert_eq!(events(&c, after)[0].day, 20_000);
        assert_eq!(c.card_schedule(after).unwrap().last_day, Some(20_000));
    }
}

#[test]
fn a_review_is_due_on_a_study_day_wherever_the_device_is() {
    let (c, clock) = setup();
    clock.set_utc_offset_minutes(840);
    let card = basic_card(&c, "a");
    clock.set(local(20_000, 9, 0, 840));
    good(&c, card);
    clock.advance(10 * MINUTE);
    let graduated = good(&c, card);
    // Due on day 20,002 for good: the due day is a number, not a time, so flying to the other side
    // of the world does not move it.
    assert_eq!(graduated.due_day, Some(20_002));
    clock.set_utc_offset_minutes(-720);
    assert_eq!(c.card_schedule(card).unwrap().due_day, Some(20_002));
}

#[test]
fn travelling_west_counts_as_the_same_day() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    // Answered at 01:00 on day 20,000 at UTC+14, which is 11:00 UTC the day before.
    clock.set_utc_offset_minutes(840);
    clock.set(local(20_000, 1, 0, 840));
    good(&c, card);
    assert_eq!(c.card_schedule(card).unwrap().last_day, Some(20_000));
    // 30 minutes later, in a time zone 26 hours behind (UTC-12): it is day 19,998 there.
    clock.set_utc_offset_minutes(-720);
    clock.advance(30 * MINUTE);
    assert_eq!(study_day(clock.now().unix_ms, -720, 0), 19_998);
    // The learning step is due on a time, so answer it when due; "today" is before the last
    // answer's day, which counts as 0 elapsed days and so the same FSRS result as same day.
    clock.advance(10 * MINUTE);
    let answered = good(&c, card);
    assert_eq!(answered.state, CardState::Review);
    let all = events(&c, card);
    assert_eq!(all[1].day, 19_998);
    assert_eq!(c.card_schedule(card).unwrap().last_day, Some(19_998));
    // The graduating interval is the same 2 days it would have been the same day (elapsed 0, not
    // negative), counted from the day the answer was made on.
    assert_eq!(answered.interval_days, Some(2));
    assert_eq!(answered.due_day, Some(20_000));
}

#[test]
fn a_dst_change_in_the_middle_of_a_run_does_not_disturb_the_days() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    // Offset +60 until 01:00 UTC on day 20,000, then +120 (clocks go forward an hour).
    clock.set_utc_offset_minutes(60);
    clock.set(20_000 * DAY - 2 * HOUR);
    good(&c, card);
    let first_day = events(&c, card)[0].day;
    assert_eq!(first_day, 19_999);
    clock.set_utc_offset_minutes(120);
    clock.set(20_000 * DAY + 5 * HOUR);
    good(&c, card);
    let second = events(&c, card)[1].clone();
    assert_eq!(second.day, 20_000);
    assert_eq!(second.utc_offset, 120);
    // Graduated, with one day elapsed since the first answer (day 19,999 to 20,000), counted from
    // the study day of the answer. The interval is the scheduler's for one elapsed day.
    assert_eq!(second.state, Some(CardState::Review));
    let scheduler = Scheduler::new(None, 0.9).unwrap();
    let first_memory = events(&c, card)[0].memory;
    let unfuzzed = scheduler.next(first_memory, 1).unwrap()[Rating::Good as usize].interval_days;
    assert_eq!(
        second.due_day,
        Some(20_000 + i64::from(fuzz(unfuzzed, second.id)))
    );
}

#[test]
fn changing_the_start_hour_keeps_the_stored_due_days() {
    let (c, clock) = setup();
    let card = basic_card(&c, "a");
    clock.set(local(20_000, 9, 0, 0));
    good(&c, card);
    clock.advance(10 * MINUTE);
    let graduated = good(&c, card);
    assert_eq!(graduated.due_day, Some(20_002));
    c.set_day_start_hour(4).unwrap();
    assert_eq!(c.card_schedule(card).unwrap().due_day, Some(20_002));
    // The next answer, at 03:00 on day 20,002 (still day 20,001 now), takes its day from the new
    // start hour.
    clock.set(local(20_002, 3, 0, 0));
    let answered = good(&c, card);
    let last = events(&c, card).pop().unwrap();
    assert_eq!(last.day, 20_001);
    assert_eq!(answered.state, CardState::Review);
}

#[test]
fn the_default_deck_exists_for_these_tests() {
    let (c, _) = setup();
    assert_eq!(c.decks().unwrap()[0].id, default_deck());
    assert_eq!(Due::Days(1), Due::Days(1));
}
