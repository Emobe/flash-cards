use super::*;

const STEPS: Steps<'static> = Steps {
    learning: &[1, 10],
    relearning: &[10],
};

fn scheduler() -> Scheduler {
    Scheduler::new(None, 0.9).unwrap()
}

fn memory() -> Memory {
    Memory {
        stability: 10.96,
        difficulty: 5.0,
    }
}

fn card(state: CardState, step: u32) -> CardNow {
    CardNow {
        state,
        step,
        memory: (state != CardState::New).then(memory),
    }
}

/// The unfuzzed outcome of one answer.
fn after(card: CardNow, elapsed: u32, rating: Rating, steps: Steps<'_>) -> Outcome {
    preview(card, elapsed, steps, &scheduler()).unwrap()[rating as usize]
}

fn shape(o: Outcome) -> (CardState, u32, bool, u32) {
    match o.due {
        Due::Minutes(m) => (o.state, o.step, true, m),
        Due::Days(d) => (o.state, o.step, false, d),
    }
}

const ALL: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];

// ---- The table in ADR 0007, part 1, cell by cell ----

#[test]
fn a_new_card_on_again_goes_to_the_first_step() {
    let o = after(CardNow::NEW, 0, Rating::Again, STEPS);
    assert_eq!(shape(o), (CardState::Learning, 0, true, 1));
}

#[test]
fn a_new_card_on_hard_stays_on_the_first_step() {
    let o = after(CardNow::NEW, 0, Rating::Hard, STEPS);
    assert_eq!(shape(o), (CardState::Learning, 0, true, 1));
}

#[test]
fn a_new_card_on_good_goes_to_the_second_step() {
    let o = after(CardNow::NEW, 0, Rating::Good, STEPS);
    assert_eq!(shape(o), (CardState::Learning, 1, true, 10));
}

#[test]
fn a_new_card_on_easy_graduates() {
    let o = after(CardNow::NEW, 0, Rating::Easy, STEPS);
    let (state, step, minutes, days) = shape(o);
    assert_eq!((state, step, minutes), (CardState::Review, 0, false));
    assert!(days >= 8, "Easy on a new card is about 8 days, got {days}");
}

#[test]
fn a_learning_card_on_each_rating() {
    let first = card(CardState::Learning, 0);
    let second = card(CardState::Learning, 1);
    assert_eq!(
        shape(after(first, 0, Rating::Again, STEPS)),
        (CardState::Learning, 0, true, 1)
    );
    assert_eq!(
        shape(after(first, 0, Rating::Hard, STEPS)),
        (CardState::Learning, 0, true, 1)
    );
    assert_eq!(
        shape(after(first, 0, Rating::Good, STEPS)),
        (CardState::Learning, 1, true, 10)
    );
    assert_eq!(
        shape(after(second, 0, Rating::Again, STEPS)),
        (CardState::Learning, 0, true, 1)
    );
    assert_eq!(
        shape(after(second, 0, Rating::Hard, STEPS)),
        (CardState::Learning, 1, true, 10)
    );
    // Good on the last step graduates, Easy graduates from anywhere.
    for (c, rating) in [
        (second, Rating::Good),
        (first, Rating::Easy),
        (second, Rating::Easy),
    ] {
        let (state, step, minutes, days) = shape(after(c, 0, rating, STEPS));
        assert_eq!((state, step, minutes), (CardState::Review, 0, false));
        assert!(days >= 1);
    }
}

#[test]
fn a_review_card_on_each_rating() {
    let c = card(CardState::Review, 0);
    let lapse = after(c, 11, Rating::Again, STEPS);
    assert_eq!(shape(lapse), (CardState::Relearning, 0, true, 10));
    let [_, hard, good, easy] = ALL.map(|r| after(c, 11, r, STEPS));
    for o in [hard, good, easy] {
        assert_eq!(o.state, CardState::Review);
        assert_eq!(o.step, 0);
        assert!(matches!(o.due, Due::Days(_)));
    }
    let days = |o: Outcome| match o.due {
        Due::Days(d) => d,
        Due::Minutes(_) => unreachable!(),
    };
    assert!(days(hard) <= days(good) && days(good) < days(easy));
}

#[test]
fn a_relearning_card_on_each_rating() {
    let steps = Steps {
        learning: &[1, 10],
        relearning: &[10, 60],
    };
    let first = card(CardState::Relearning, 0);
    let second = card(CardState::Relearning, 1);
    assert_eq!(
        shape(after(first, 0, Rating::Again, steps)),
        (CardState::Relearning, 0, true, 10)
    );
    assert_eq!(
        shape(after(first, 0, Rating::Hard, steps)),
        (CardState::Relearning, 0, true, 10)
    );
    assert_eq!(
        shape(after(first, 0, Rating::Good, steps)),
        (CardState::Relearning, 1, true, 60)
    );
    assert_eq!(
        shape(after(second, 0, Rating::Again, steps)),
        (CardState::Relearning, 0, true, 10)
    );
    assert_eq!(
        shape(after(second, 0, Rating::Hard, steps)),
        (CardState::Relearning, 1, true, 60)
    );
    for (c, rating) in [
        (second, Rating::Good),
        (first, Rating::Easy),
        (second, Rating::Easy),
    ] {
        let (state, _, minutes, days) = shape(after(c, 0, rating, steps));
        assert_eq!((state, minutes), (CardState::Review, false));
        assert!(days >= 1);
    }
}

#[test]
fn every_answer_updates_the_memory_state() {
    // A same-day answer on a learning card still moves FSRS's memory (finding 2).
    let learning = card(CardState::Learning, 0);
    for rating in ALL {
        let o = after(learning, 0, rating, STEPS);
        assert_ne!(o.memory, memory(), "{rating:?}");
    }
    let new = after(CardNow::NEW, 0, Rating::Good, STEPS);
    assert!((new.memory.stability - 2.3065).abs() < 1e-3);
}

// ---- Edges ----

#[test]
fn empty_learning_steps_graduate_every_answer_on_a_new_card() {
    let steps = Steps {
        learning: &[],
        relearning: &[10],
    };
    for rating in ALL {
        let (state, step, minutes, days) = shape(after(CardNow::NEW, 0, rating, steps));
        assert_eq!(
            (state, step, minutes),
            (CardState::Review, 0, false),
            "{rating:?}"
        );
        assert!(days >= 1);
    }
}

#[test]
fn empty_relearning_steps_keep_a_lapse_a_review_card_with_a_shorter_interval() {
    let steps = Steps {
        learning: &[1, 10],
        relearning: &[],
    };
    let c = card(CardState::Review, 0);
    let lapse = after(c, 11, Rating::Again, steps);
    let good = after(c, 11, Rating::Good, steps);
    assert_eq!(lapse.state, CardState::Review);
    let (Due::Days(lapse_days), Due::Days(good_days)) = (lapse.due, good.due) else {
        panic!("a lapse with no relearning steps is due in days");
    };
    assert!(lapse_days >= 1 && lapse_days < good_days);
    // A card already relearning when the steps were removed graduates on any answer.
    let relearning = card(CardState::Relearning, 0);
    for rating in ALL {
        let o = after(relearning, 0, rating, steps);
        assert_eq!(o.state, CardState::Review, "{rating:?}");
        assert!(matches!(o.due, Due::Days(d) if d >= 1));
    }
}

#[test]
fn a_card_on_a_step_the_preset_no_longer_has_is_on_the_last_one() {
    let one = Steps {
        learning: &[5],
        relearning: &[7],
    };
    // Step 3 of a list that now has one step: Hard repeats the last step, Good graduates.
    let learning = card(CardState::Learning, 3);
    assert_eq!(
        shape(after(learning, 0, Rating::Hard, one)),
        (CardState::Learning, 0, true, 5)
    );
    assert_eq!(
        after(learning, 0, Rating::Good, one).state,
        CardState::Review
    );
    let relearning = card(CardState::Relearning, 9);
    assert_eq!(
        shape(after(relearning, 0, Rating::Hard, one)),
        (CardState::Relearning, 0, true, 7)
    );
    // A list that is longer than the card's step: Good goes to the next one.
    let long = Steps {
        learning: &[1, 10, 60],
        relearning: &[10],
    };
    let second = card(CardState::Learning, 1);
    assert_eq!(
        shape(after(second, 0, Rating::Good, long)),
        (CardState::Learning, 2, true, 60)
    );
}

#[test]
fn graduating_never_gives_less_than_one_day() {
    // A same-day review right after a first answer has a tiny FSRS interval (finding 1).
    let first = after(CardNow::NEW, 0, Rating::Again, STEPS);
    let learning = CardNow {
        state: CardState::Learning,
        step: 0,
        memory: Some(first.memory),
    };
    let o = after(learning, 0, Rating::Easy, STEPS);
    assert!(matches!(o.due, Due::Days(d) if d >= 1));
    // Even with a made-up memory that FSRS would turn into a fraction of a day.
    let weak = CardNow {
        state: CardState::Learning,
        step: 1,
        memory: Some(Memory {
            stability: 0.01,
            difficulty: 9.0,
        }),
    };
    let o = after(
        weak,
        0,
        Rating::Good,
        Steps {
            learning: &[1],
            relearning: &[10],
        },
    );
    assert!(matches!(o.due, Due::Days(d) if d >= 1));
}

#[test]
fn an_interval_is_never_longer_than_the_maximum() {
    let strong = CardNow {
        state: CardState::Review,
        step: 0,
        memory: Some(Memory {
            stability: 3_000_000.0,
            difficulty: 1.0,
        }),
    };
    let steps = Steps {
        learning: &[1],
        relearning: &[10],
    };
    let o = after(strong, 36_500, Rating::Easy, steps);
    assert_eq!(o.due, Due::Days(MAX_INTERVAL_DAYS));
    // Fuzz cannot push it over either.
    for n in 0..200u8 {
        let seed = Id::new_v7(0, &[n; 10]);
        let o = answer(strong, 36_500, Rating::Easy, steps, &scheduler(), seed).unwrap();
        // Fuzz can only shorten it: 5% either side, cut off at the maximum.
        let Due::Days(days) = o.due else { panic!() };
        assert!(
            (34_675..=MAX_INTERVAL_DAYS).contains(&days),
            "seed {n}: {days}"
        );
    }
}

#[test]
fn the_card_state_codes_round_trip() {
    for state in [
        CardState::New,
        CardState::Learning,
        CardState::Review,
        CardState::Relearning,
    ] {
        assert_eq!(CardState::from_code(state.code()), Some(state));
    }
    assert_eq!(CardState::from_code(4), None);
    assert_eq!(CardState::from_code(-1), None);
    for rating in ALL {
        assert_eq!(
            Rating::from_number(i64::from(rating.number())),
            Some(rating)
        );
    }
    assert_eq!(Rating::from_number(0), None);
    assert_eq!(Rating::from_number(5), None);
}

// ---- A card through its first answers ----

/// Answers `ratings` in order with the steps `1 10`, each when due (learning steps the same day,
/// intervals after that many days). Returns each outcome, unfuzzed.
fn run(ratings: &[Rating]) -> Vec<Outcome> {
    let mut now = CardNow::NEW;
    let mut elapsed = 0;
    let mut out = Vec::new();
    for rating in ratings {
        let o = after(now, elapsed, *rating, STEPS);
        elapsed = match o.due {
            Due::Minutes(_) => 0,
            Due::Days(days) => days,
        };
        now = CardNow {
            state: o.state,
            step: o.step,
            memory: Some(o.memory),
        };
        out.push(o);
    }
    out
}

#[test]
fn five_good_answers_from_new_with_steps_one_and_ten() {
    let out = run(&[Rating::Good; 5]);
    let dues: Vec<Due> = out.iter().map(|o| o.due).collect();
    // Two same-day steps, then graduation at 2 days: FSRS counts the same-day answer without
    // inflating the interval (finding 1, S = 2.31), then the intervals grow as in step 0.5 (2, 11,
    // 46, 163).
    assert_eq!(
        dues,
        [
            Due::Minutes(10),
            Due::Days(2),
            Due::Days(PINNED[0]),
            Due::Days(PINNED[1]),
            Due::Days(PINNED[2]),
        ]
    );
    assert!((out[1].memory.stability - 2.3065).abs() < 1e-3);
}

const PINNED: [u32; 3] = [11, 46, 163];

#[test]
fn a_lapse_in_the_middle_of_a_run_sends_the_card_back_through_relearning() {
    let out = run(&[
        Rating::Good,
        Rating::Good,
        Rating::Good,
        Rating::Again,
        Rating::Good,
        Rating::Good,
    ]);
    let dues: Vec<Due> = out.iter().map(|o| o.due).collect();
    assert_eq!(dues[3], Due::Minutes(10));
    assert_eq!(out[3].state, CardState::Relearning);
    assert_eq!(out[4].state, CardState::Review);
    assert!(matches!(dues[4], Due::Days(d) if d >= 1));
    assert!(matches!((dues[2], dues[5]), (Due::Days(a), Due::Days(b)) if b < a * 2));
}

// ---- Previews and answers agree ----

#[test]
fn the_previews_are_the_answers_without_fuzz() {
    let steps = STEPS;
    for c in [
        CardNow::NEW,
        card(CardState::Learning, 1),
        card(CardState::Review, 0),
        card(CardState::Relearning, 0),
    ] {
        let previews = preview(c, 5, steps, &scheduler()).unwrap();
        for (rating, preview) in ALL.iter().zip(previews) {
            // Same card, every rating, a seed that gives no fuzz shift is not guaranteed, so
            // compare everything but the days of a review.
            let answered =
                answer(c, 5, *rating, steps, &scheduler(), Id::new_v7(0, &[1; 10])).unwrap();
            assert_eq!(answered.state, preview.state);
            assert_eq!(answered.step, preview.step);
            assert_eq!(answered.memory, preview.memory);
            match (answered.due, preview.due) {
                (Due::Minutes(a), Due::Minutes(b)) => assert_eq!(a, b),
                (Due::Days(a), Due::Days(b)) => {
                    assert_eq!(a, fuzz(b, Id::new_v7(0, &[1; 10])));
                }
                other => panic!("{other:?}"),
            }
        }
    }
}

// ---- Fuzz ----

/// Ten bytes from a small generator, for IDs that differ in the part the fuzz reads.
fn ids(count: u32) -> impl Iterator<Item = Id> {
    let mut seed: u64 = 0x1234_5678_9abc_def0;
    (0..count).map(move |_| {
        let mut bytes = [0u8; 10];
        for byte in &mut bytes {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            *byte = (seed >> 56) as u8;
        }
        Id::new_v7(1_700_000_000_000, &bytes)
    })
}

#[test]
fn intervals_under_three_days_are_not_fuzzed() {
    for interval in [1, 2] {
        for id in ids(50) {
            assert_eq!(fuzz(interval, id), interval);
        }
    }
}

#[test]
fn fuzz_stays_within_five_percent_and_hits_every_day_in_the_range() {
    // (interval, expected range) with r = max(1, round(5%)), never below 2.
    for (interval, low, high) in [
        (3, 2, 4),
        (10, 9, 11),
        (20, 19, 21),
        (30, 28, 32),
        (100, 95, 105),
        (1000, 950, 1050),
    ] {
        let seen: std::collections::BTreeSet<u32> =
            ids(5000).map(|id| fuzz(interval, id)).collect();
        assert_eq!(seen.iter().next(), Some(&low), "interval {interval}");
        assert_eq!(seen.iter().next_back(), Some(&high), "interval {interval}");
        assert_eq!(seen.len() as u32, high - low + 1, "interval {interval}");
    }
}

#[test]
fn the_same_event_id_always_gives_the_same_day() {
    for id in ids(100) {
        assert_eq!(fuzz(100, id), fuzz(100, id));
    }
    // And it reads the random tail of the ID, not the time: same tail, other time, same day.
    let a = Id::new_v7(1_000, &[7; 10]);
    let b = Id::new_v7(9_999_999, &[7; 10]);
    assert_eq!(fuzz(100, a), fuzz(100, b));
}

#[test]
fn fuzz_is_roughly_even() {
    let mut counts = [0u32; 11];
    for id in ids(22_000) {
        counts[(fuzz(100, id) - 95) as usize] += 1;
    }
    // 2,000 each in expectation.
    for (offset, count) in counts.iter().enumerate() {
        assert!(
            (1_700..=2_300).contains(count),
            "day {}: {count}",
            95 + offset
        );
    }
}
