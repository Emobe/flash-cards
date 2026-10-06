//! The queue: limits over nested decks, the order cards come in, siblings, suspend and bury.

use rusqlite::params;
use rusqlite::types::Value;

use std::sync::Arc;

use super::answer_tests::{DAY, HOUR, MINUTE, events, good, setup};
use super::fold_tests::{from_other_device, insert};
use super::*;
use crate::clock::{Clock, ManualClock};
use crate::collection::Collection;
use crate::deck::{PresetChange, default_deck, default_preset};
use crate::notetype::builtin;
use crate::scheduling::{CardState, Due, Rating, study_day};

fn basic_fields(c: &Collection) -> Vec<Id> {
    c.note_type(builtin::basic())
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id)
        .collect()
}

/// `n` Basic notes in `deck`, one card each, made one millisecond apart so their IDs (and so the
/// order new cards come in) follow the order here.
fn add(c: &Collection, clock: &ManualClock, deck: Id, n: usize) -> Vec<Id> {
    let f = basic_fields(c);
    (0..n)
        .map(|i| {
            let card = c
                .add_note_to_deck(
                    deck,
                    builtin::basic(),
                    &[(f[0], &format!("note {i}")), (f[1], "back")],
                )
                .unwrap()
                .cards[0];
            clock.advance(1);
            card
        })
        .collect()
}

fn counts(c: &Collection, deck: Id) -> (u32, u32, u32) {
    let k = c.study_counts(deck).unwrap();
    (k.new, k.learning, k.review)
}

fn limits(c: &Collection, preset: Id, new: u32, review: u32) {
    c.set_preset_options(
        preset,
        &PresetChange {
            new_per_day: Some(new),
            reviews_per_day: Some(review),
            ..Default::default()
        },
    )
    .unwrap();
}

/// A preset with these limits, used by `deck`.
fn preset_for(c: &Collection, deck: Id, new: u32, review: u32) -> Id {
    let preset = c.create_preset(&format!("preset {deck}")).unwrap();
    limits(c, preset, new, review);
    c.set_deck_preset(deck, preset).unwrap();
    preset
}

fn card_of(next: &Next) -> Option<Id> {
    match next {
        Next::Card { card, .. } => Some(*card),
        _ => None,
    }
}

fn shown(c: &Collection, deck: Id) -> Option<Id> {
    card_of(&c.next_card(deck).unwrap())
}

/// Answers the cards Easy and moves the clock on, so they are all review cards that are due.
fn make_reviews(c: &Collection, clock: &ManualClock, cards: &[Id]) {
    for card in cards {
        c.answer(*card, Rating::Easy, 1_000).unwrap();
    }
    clock.advance(60 * DAY);
}

fn sub_deck(c: &Collection, name: &str, parent: Id) -> Id {
    c.create_deck(name, Some(parent)).unwrap()
}

// ---- Limits ----

#[test]
fn a_deck_at_its_new_limit_shows_no_more_new_cards() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 5);
    limits(&c, default_preset(), 3, 200);
    assert_eq!(counts(&c, default_deck()), (3, 0, 0));
    good(&c, cards[0]);
    assert_eq!(counts(&c, default_deck()), (2, 1, 0));
    good(&c, cards[1]);
    good(&c, cards[2]);
    // The limit is spent: two new cards are left, and neither is shown.
    assert_eq!(counts(&c, default_deck()), (0, 3, 0));
    let next = c.next_card(default_deck()).unwrap();
    assert!(matches!(
        next,
        Next::Card {
            state: CardState::Learning,
            ..
        }
    ));
}

#[test]
fn a_deck_at_its_review_limit_shows_no_more_reviews() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 6);
    make_reviews(&c, &clock, &cards);
    limits(&c, default_preset(), 20, 4);
    assert_eq!(counts(&c, default_deck()), (0, 0, 4));
    good(&c, cards[0]);
    assert_eq!(counts(&c, default_deck()), (0, 0, 3));
    // A lapse is a review done today too, and the card moves to relearning.
    c.answer(cards[1], Rating::Again, 1_000).unwrap();
    assert_eq!(counts(&c, default_deck()), (0, 1, 2));
    c.answer(cards[2], Rating::Good, 1_000).unwrap();
    c.answer(cards[3], Rating::Good, 1_000).unwrap();
    assert_eq!(counts(&c, default_deck()), (0, 1, 0));
}

#[test]
fn new_and_review_limits_are_separate() {
    let (c, clock) = setup();
    let old = add(&c, &clock, default_deck(), 4);
    make_reviews(&c, &clock, &old);
    add(&c, &clock, default_deck(), 4);
    limits(&c, default_preset(), 2, 3);
    assert_eq!(counts(&c, default_deck()), (2, 0, 3));
}

#[test]
fn learning_cards_are_not_limited() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 5);
    limits(&c, default_preset(), 5, 0);
    for card in &cards {
        c.answer(*card, Rating::Again, 1_000).unwrap();
    }
    clock.advance(2 * MINUTE);
    assert_eq!(counts(&c, default_deck()), (0, 5, 0));
}

#[test]
fn a_limit_of_zero_shows_nothing_new() {
    let (c, clock) = setup();
    add(&c, &clock, default_deck(), 3);
    limits(&c, default_preset(), 0, 200);
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    assert_eq!(
        c.next_card(default_deck()).unwrap(),
        Next::Done {
            counts: Counts::default()
        }
    );
}

#[test]
fn undoing_an_answer_gives_the_budget_back() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 3);
    limits(&c, default_preset(), 1, 200);
    good(&c, cards[0]);
    assert_eq!(counts(&c, default_deck()), (0, 1, 0));
    c.undo_answer().unwrap().unwrap();
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
    assert_eq!(shown(&c, default_deck()), Some(cards[0]));
}

#[test]
fn a_parent_limit_stops_a_child() {
    let (c, clock) = setup();
    let parent = c.create_deck("Polish", None).unwrap();
    let child = sub_deck(&c, "Food", parent);
    preset_for(&c, parent, 2, 200);
    let cards = add(&c, &clock, child, 5);
    // The child is on the Default preset (20 a day), but its parent allows 2.
    assert_eq!(counts(&c, parent), (2, 0, 0));
    assert_eq!(counts(&c, child), (2, 0, 0));
    good(&c, cards[0]);
    assert_eq!(counts(&c, child), (1, 1, 0));
    assert_eq!(counts(&c, parent), (1, 1, 0));
}

#[test]
fn a_child_limit_stops_it_inside_its_parent() {
    let (c, clock) = setup();
    let parent = c.create_deck("Polish", None).unwrap();
    let child = sub_deck(&c, "Food", parent);
    preset_for(&c, child, 2, 200);
    add(&c, &clock, child, 5);
    let own = add(&c, &clock, parent, 4);
    assert_eq!(counts(&c, parent), (6, 0, 0));
    assert_eq!(counts(&c, child), (2, 0, 0));
    good(&c, own[0]);
    assert_eq!(counts(&c, parent), (5, 1, 0));
    assert_eq!(counts(&c, child), (2, 0, 0));
}

#[test]
fn studying_a_sub_deck_when_the_parent_is_spent_shows_nothing_new() {
    let (c, clock) = setup();
    let parent = c.create_deck("Polish", None).unwrap();
    let child = sub_deck(&c, "Food", parent);
    preset_for(&c, parent, 2, 200);
    let own = add(&c, &clock, parent, 3);
    add(&c, &clock, child, 3);
    assert_eq!(counts(&c, child), (2, 0, 0));
    good(&c, own[0]);
    good(&c, own[1]);
    assert_eq!(counts(&c, child), (0, 0, 0));
    assert_eq!(counts(&c, parent), (0, 2, 0));
    assert_eq!(shown(&c, child), None);
}

#[test]
fn sub_decks_share_their_parents_budget() {
    let (c, clock) = setup();
    let parent = c.create_deck("Polish", None).unwrap();
    let food = sub_deck(&c, "Food", parent);
    let verbs = sub_deck(&c, "Verbs", parent);
    preset_for(&c, parent, 2, 200);
    let in_food = add(&c, &clock, food, 3);
    add(&c, &clock, verbs, 3);
    assert_eq!(counts(&c, food), (2, 0, 0));
    assert_eq!(counts(&c, verbs), (2, 0, 0));
    good(&c, in_food[0]);
    good(&c, in_food[1]);
    assert_eq!(counts(&c, verbs), (0, 0, 0));
}

#[test]
fn a_deck_on_another_preset_uses_its_own_limits() {
    let (c, clock) = setup();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", None).unwrap();
    preset_for(&c, a, 1, 200);
    preset_for(&c, b, 4, 200);
    add(&c, &clock, a, 5);
    add(&c, &clock, b, 5);
    assert_eq!(counts(&c, a), (1, 0, 0));
    assert_eq!(counts(&c, b), (4, 0, 0));
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
}

#[test]
fn limits_that_are_not_spent_by_sub_decks_when_the_switch_is_off() {
    let (c, clock) = setup();
    let parent = c.create_deck("Languages", None).unwrap();
    let child = sub_deck(&c, "Polish", parent);
    preset_for(&c, parent, 2, 200);
    c.set_deck_limits_include_subdecks(parent, false).unwrap();
    let own = add(&c, &clock, parent, 3);
    let inside = add(&c, &clock, child, 5);

    // The parent limits its own cards to 2, and its sub-deck is neither capped nor counted by it.
    assert_eq!(counts(&c, parent), (7, 0, 0));
    assert_eq!(counts(&c, child), (5, 0, 0));
    good(&c, inside[0]);
    good(&c, inside[1]);
    good(&c, inside[2]);
    assert_eq!(counts(&c, parent), (4, 3, 0));
    assert_eq!(counts(&c, child), (2, 3, 0));
    good(&c, own[0]);
    good(&c, own[1]);
    // Its own limit is spent and its sub-deck is not affected.
    assert_eq!(counts(&c, parent), (2, 5, 0));
    assert_eq!(counts(&c, child), (2, 3, 0));
}

#[test]
fn a_deck_above_still_counts_everything_inside_it_past_a_deck_whose_switch_is_off() {
    let (c, clock) = setup();
    let top = c.create_deck("Top", None).unwrap();
    let middle = sub_deck(&c, "Middle", top);
    let bottom = sub_deck(&c, "Bottom", middle);
    preset_for(&c, top, 4, 200);
    c.set_deck_limits_include_subdecks(middle, false).unwrap();
    add(&c, &clock, bottom, 8);
    assert_eq!(counts(&c, bottom), (4, 0, 0));
    assert_eq!(counts(&c, middle), (4, 0, 0));
}

#[test]
fn the_switch_is_on_for_a_new_deck_and_changing_it_to_the_same_writes_nothing() {
    let (c, _) = setup();
    let deck = c.create_deck("A", None).unwrap();
    assert!(c.deck(deck).unwrap().unwrap().limits_include_subdecks);
    let before = c
        .register_clock("deck", deck, "limits_include_subdecks")
        .unwrap();
    c.set_deck_limits_include_subdecks(deck, true).unwrap();
    assert_eq!(
        c.register_clock("deck", deck, "limits_include_subdecks")
            .unwrap(),
        before
    );
    c.set_deck_limits_include_subdecks(deck, false).unwrap();
    assert!(!c.deck(deck).unwrap().unwrap().limits_include_subdecks);
    assert!(
        c.deck(default_deck())
            .unwrap()
            .unwrap()
            .limits_include_subdecks
    );
    assert!(
        c.set_deck_limits_include_subdecks(Id::from_bytes([1; 16]), false)
            .is_err()
    );
}

#[test]
fn cards_deleted_with_their_deck_or_note_are_not_in_the_queue() {
    let (c, clock) = setup();
    let deck = c.create_deck("Gone", None).unwrap();
    let cards = add(&c, &clock, deck, 2);
    add(&c, &clock, default_deck(), 1);
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
    assert_eq!(counts(&c, deck), (2, 0, 0));
    c.delete_deck(deck).unwrap();
    assert_eq!(c.study_counts(deck), Err(StudyError::NotFound));
    assert_eq!(c.next_card(deck), Err(StudyError::NotFound));
    assert_eq!(cards.len(), 2);
    let all: u32 = c.deck_counts().unwrap().iter().map(|d| d.counts.new).sum();
    assert_eq!(all, 1);
}

// ---- Order ----

#[test]
fn new_cards_come_in_the_order_their_notes_were_added() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 4);
    for card in &cards {
        assert_eq!(shown(&c, default_deck()), Some(*card));
        good(&c, *card);
    }
}

#[test]
fn the_cards_of_a_note_come_in_template_order_and_cloze_cards_by_number() {
    let (c, _) = setup();
    let f = c
        .note_type(builtin::basic_and_reversed())
        .unwrap()
        .unwrap()
        .fields;
    let both = c
        .add_note(
            builtin::basic_and_reversed(),
            &[(f[0].id, "a"), (f[1].id, "b")],
        )
        .unwrap();
    assert_eq!(shown(&c, default_deck()), Some(both.cards[0]));
    let position = |card: Id| -> String {
        c.conn
            .query_row(
                "SELECT t.position FROM card k JOIN template t ON t.id = k.template WHERE k.id = ?1",
                [card],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert!(position(both.cards[0]) < position(both.cards[1]));

    let (c, _) = setup();
    let field = c.note_type(builtin::cloze()).unwrap().unwrap().fields[0].id;
    let cloze = c
        .add_note(
            builtin::cloze(),
            &[(field, "{{c2::two}} {{c1::one}} {{c3::three}}")],
        )
        .unwrap();
    assert_eq!(cloze.cards.len(), 3);
    let ordinal = |card: Id| -> i64 {
        c.conn
            .query_row("SELECT ordinal FROM card WHERE id = ?1", [card], |row| {
                row.get(0)
            })
            .unwrap()
    };
    let lowest = cloze.cards.iter().map(|card| ordinal(*card)).min().unwrap();
    assert_eq!(ordinal(shown(&c, default_deck()).unwrap()), lowest);
}

#[test]
fn learning_cards_that_are_due_come_before_reviews() {
    let (c, clock) = setup();
    let old = add(&c, &clock, default_deck(), 3);
    make_reviews(&c, &clock, &old);
    let fresh = add(&c, &clock, default_deck(), 1);
    c.answer(fresh[0], Rating::Again, 1_000).unwrap();
    // Not due yet (a minute), so a review is shown, learning counts as left today.
    assert_eq!(counts(&c, default_deck()), (0, 1, 3));
    assert_ne!(shown(&c, default_deck()), Some(fresh[0]));
    clock.advance(2 * MINUTE);
    assert_eq!(shown(&c, default_deck()), Some(fresh[0]));
}

#[test]
fn reviews_come_by_due_day_and_new_cards_are_spread_among_them() {
    let (c, clock) = setup();
    let old = add(&c, &clock, default_deck(), 6);
    make_reviews(&c, &clock, &old);
    add(&c, &clock, default_deck(), 2);

    let mut order = Vec::new();
    while let Next::Card {
        card,
        state: state @ (CardState::New | CardState::Review),
        ..
    } = c.next_card(default_deck()).unwrap()
    {
        order.push(state);
        good(&c, card);
    }
    use CardState::{New, Review};
    // Eight cards: the two new ones sit at 1/4 and 3/4 of the way through the six reviews, and a
    // review comes first when the positions are equal.
    assert_eq!(
        order,
        [Review, Review, New, Review, Review, Review, New, Review]
    );
}

#[test]
fn only_new_cards_or_only_reviews_are_shown_in_their_own_order() {
    let (c, clock) = setup();
    let old = add(&c, &clock, default_deck(), 3);
    make_reviews(&c, &clock, &old);
    // Easy intervals are fuzzed, so the due days differ a little: by due day, then card ID.
    let mut expected: Vec<(i64, Id)> = old
        .iter()
        .map(|card| (c.card_schedule(*card).unwrap().due_day.unwrap(), *card))
        .collect();
    expected.sort();
    let mut seen = Vec::new();
    while let Some(card) = shown(&c, default_deck()) {
        seen.push(card);
        good(&c, card);
        assert!(seen.len() <= 3);
    }
    assert_eq!(
        seen,
        expected.iter().map(|(_, card)| *card).collect::<Vec<_>>()
    );
}

#[test]
fn a_more_overdue_review_comes_first() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 2);
    c.answer(cards[0], Rating::Easy, 1_000).unwrap();
    clock.advance(5 * DAY);
    c.answer(cards[1], Rating::Easy, 1_000).unwrap();
    clock.advance(60 * DAY);
    // Both are due; the first has been due for longer.
    assert_eq!(shown(&c, default_deck()), Some(cards[0]));
    good(&c, cards[0]);
    assert_eq!(shown(&c, default_deck()), Some(cards[1]));
}

#[test]
fn a_learning_card_due_within_twenty_minutes_is_shown_early_when_nothing_else_is_left() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 2);
    good(&c, cards[0]);
    // The other new card goes first, though the learning card is only ten minutes away.
    assert_eq!(shown(&c, default_deck()), Some(cards[1]));
    clock.advance(MINUTE);
    good(&c, cards[1]);
    match c.next_card(default_deck()).unwrap() {
        Next::Card { card, state, .. } => {
            assert_eq!(card, cards[0]);
            assert_eq!(state, CardState::Learning);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_learning_card_further_away_is_waited_for() {
    let (c, clock) = setup();
    c.set_preset_options(
        default_preset(),
        &PresetChange {
            learning_steps: Some(vec![30]),
            ..Default::default()
        },
    )
    .unwrap();
    let cards = add(&c, &clock, default_deck(), 1);
    let due = clock.now().unix_ms + 30 * MINUTE;
    c.answer(cards[0], Rating::Again, 1_000).unwrap();
    let counts_now = Counts {
        new: 0,
        learning: 1,
        review: 0,
    };
    assert_eq!(
        c.next_card(default_deck()).unwrap(),
        Next::Waiting {
            until_ms: due,
            counts: counts_now
        }
    );
    clock.advance(9 * MINUTE);
    assert!(matches!(
        c.next_card(default_deck()).unwrap(),
        Next::Waiting { .. }
    ));
    // Twenty minutes ahead of the due time, it is shown.
    clock.advance(MINUTE + 1);
    assert_eq!(shown(&c, default_deck()), Some(cards[0]));
}

#[test]
fn a_learning_card_due_tomorrow_is_not_waited_for_today() {
    let (c, clock) = setup();
    c.set_preset_options(
        default_preset(),
        &PresetChange {
            learning_steps: Some(vec![1440]),
            ..Default::default()
        },
    )
    .unwrap();
    let cards = add(&c, &clock, default_deck(), 1);
    c.answer(cards[0], Rating::Again, 1_000).unwrap();
    assert_eq!(
        c.next_card(default_deck()).unwrap(),
        Next::Done {
            counts: Counts::default()
        }
    );
    clock.advance(DAY);
    assert_eq!(shown(&c, default_deck()), Some(cards[0]));
}

#[test]
fn an_empty_deck_is_done() {
    let (c, _) = setup();
    assert_eq!(
        c.next_card(default_deck()).unwrap(),
        Next::Done {
            counts: Counts::default()
        }
    );
    assert_eq!(
        c.study_counts(Id::from_bytes([3; 16])),
        Err(StudyError::NotFound)
    );
}

#[test]
fn the_next_card_says_what_each_answer_would_do() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 1);
    match c.next_card(default_deck()).unwrap() {
        Next::Card {
            card,
            deck,
            state,
            previews,
            counts,
        } => {
            assert_eq!(card, cards[0]);
            assert_eq!(deck, default_deck());
            assert_eq!(state, CardState::New);
            // Steps are 1 and 10 minutes: Again and Hard stay on the first step, Good moves to the
            // second, Easy graduates, to a whole number of days (finding 1 of ADR 0007: 8 days).
            assert_eq!(previews[0], Due::Minutes(1));
            assert_eq!(previews[1], Due::Minutes(1));
            assert_eq!(previews[2], Due::Minutes(10));
            assert!(matches!(previews[3], Due::Days(d) if d >= 5));
            assert_eq!(counts.new, 1);
        }
        other => panic!("{other:?}"),
    }
    // Nothing was written by looking.
    assert!(events(&c, cards[0]).is_empty());
}

// ---- Siblings ----

fn reversed_note(c: &Collection) -> Vec<Id> {
    let f = c
        .note_type(builtin::basic_and_reversed())
        .unwrap()
        .unwrap()
        .fields;
    c.add_note(
        builtin::basic_and_reversed(),
        &[(f[0].id, "a"), (f[1].id, "b")],
    )
    .unwrap()
    .cards
}

#[test]
fn a_new_sibling_is_held_until_tomorrow() {
    let (c, clock) = setup();
    let cards = reversed_note(&c);
    assert_eq!(counts(&c, default_deck()), (2, 0, 0));
    good(&c, cards[0]);
    assert_eq!(counts(&c, default_deck()), (0, 1, 0));
    clock.advance(DAY);
    // Tomorrow the sibling is back (and the first card is due as learning).
    assert_eq!(counts(&c, default_deck()), (1, 1, 0));
}

#[test]
fn a_review_sibling_is_held_until_tomorrow() {
    let (c, clock) = setup();
    let cards = reversed_note(&c);
    make_reviews(&c, &clock, &cards);
    assert_eq!(counts(&c, default_deck()), (0, 0, 2));
    good(&c, cards[0]);
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    assert_eq!(shown(&c, default_deck()), None);
}

#[test]
fn undoing_the_answer_lifts_the_hold() {
    let (c, _) = setup();
    let cards = reversed_note(&c);
    good(&c, cards[0]);
    assert_eq!(counts(&c, default_deck()).0, 0);
    c.undo_answer().unwrap().unwrap();
    assert_eq!(counts(&c, default_deck()).0, 2);
}

#[test]
fn learning_siblings_are_not_held() {
    let (c, clock) = setup();
    let cards = reversed_note(&c);
    // Both are answered directly (the queue would hold the second one back).
    c.answer(cards[1], Rating::Again, 1_000).unwrap();
    c.answer(cards[0], Rating::Again, 1_000).unwrap();
    clock.advance(2 * MINUTE);
    assert_eq!(counts(&c, default_deck()), (0, 2, 0));
    assert!(shown(&c, default_deck()).is_some());
}

#[test]
fn siblings_are_not_held_when_the_preset_says_so() {
    let (c, _) = setup();
    c.set_preset_options(
        default_preset(),
        &PresetChange {
            space_siblings: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!c.preset(default_preset()).unwrap().unwrap().space_siblings);
    let cards = reversed_note(&c);
    good(&c, cards[0]);
    assert_eq!(counts(&c, default_deck()), (1, 1, 0));
    assert_eq!(shown(&c, default_deck()), Some(cards[1]));
}

#[test]
fn a_held_sibling_does_not_use_the_days_limit() {
    let (c, clock) = setup();
    let cards = reversed_note(&c);
    let others = add(&c, &clock, default_deck(), 3);
    limits(&c, default_preset(), 2, 200);
    good(&c, cards[0]);
    // One new card done today, the sibling held, so one more new card fits.
    assert_eq!(counts(&c, default_deck()), (1, 1, 0));
    assert_eq!(shown(&c, default_deck()), Some(others[0]));
}

#[test]
fn space_siblings_is_on_for_a_new_preset_and_writes_only_a_change() {
    let (c, _) = setup();
    let preset = c.create_preset("Mine").unwrap();
    assert!(c.preset(preset).unwrap().unwrap().space_siblings);
    let before = c
        .register_clock("options_preset", preset, "space_siblings")
        .unwrap();
    c.set_preset_options(
        preset,
        &PresetChange {
            space_siblings: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        c.register_clock("options_preset", preset, "space_siblings")
            .unwrap(),
        before
    );
}

// ---- Suspend and bury ----

#[test]
fn a_suspended_card_is_out_of_the_queue_until_it_is_unsuspended() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 2);
    c.suspend_cards(&[cards[0]]).unwrap();
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
    assert_eq!(shown(&c, default_deck()), Some(cards[1]));
    clock.advance(10 * DAY);
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
    c.unsuspend_cards(&[cards[0]]).unwrap();
    assert_eq!(counts(&c, default_deck()), (2, 0, 0));
}

#[test]
fn a_suspended_review_card_and_a_suspended_learning_card_are_out_too() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 2);
    c.answer(cards[0], Rating::Easy, 1_000).unwrap();
    c.answer(cards[1], Rating::Again, 1_000).unwrap();
    clock.advance(60 * DAY);
    assert_eq!(counts(&c, default_deck()), (0, 1, 1));
    c.suspend_cards(&cards).unwrap();
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    assert_eq!(
        c.next_card(default_deck()).unwrap(),
        Next::Done {
            counts: Counts::default()
        }
    );
}

#[test]
fn suspending_writes_only_the_cards_that_change() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 2);
    c.suspend_cards(&[cards[0]]).unwrap();
    let first = c.register_clock("card", cards[0], "suspended").unwrap();
    let second = c.register_clock("card", cards[1], "suspended").unwrap();
    clock.advance(1_000);
    c.suspend_cards(&cards).unwrap();
    assert_eq!(
        c.register_clock("card", cards[0], "suspended").unwrap(),
        first
    );
    assert_ne!(
        c.register_clock("card", cards[1], "suspended").unwrap(),
        second
    );
    let before = c.register_clock("card", cards[1], "suspended").unwrap();
    c.unsuspend_cards(&[cards[0], cards[0]]).unwrap();
    c.suspend_cards(&[cards[1]]).unwrap();
    assert_eq!(
        c.register_clock("card", cards[1], "suspended").unwrap(),
        before
    );
}

#[test]
fn suspending_a_deleted_or_missing_card_is_refused_and_nothing_is_written() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 1);
    let missing = Id::from_bytes([8; 16]);
    let before = c.register_clock("card", cards[0], "suspended").unwrap();
    assert_eq!(
        c.suspend_cards(&[cards[0], missing]),
        Err(StudyError::NotFound)
    );
    assert_eq!(c.bury_cards(&[missing]), Err(StudyError::NotFound));
    assert_eq!(c.unbury_cards(&[missing]), Err(StudyError::NotFound));
    assert_eq!(c.unsuspend_cards(&[missing]), Err(StudyError::NotFound));
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
    assert_eq!(
        c.register_clock("card", cards[0], "suspended").unwrap(),
        before
    );
}

#[test]
fn a_buried_card_is_back_the_next_day_with_nothing_written() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 2);
    c.bury_cards(&[cards[0]]).unwrap();
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
    let written = c.register_clock("card", cards[0], "buried_until").unwrap();
    clock.advance(DAY);
    assert_eq!(counts(&c, default_deck()), (2, 0, 0));
    assert_eq!(
        c.register_clock("card", cards[0], "buried_until").unwrap(),
        written
    );
}

#[test]
fn burying_lasts_to_the_end_of_the_study_day() {
    let (c, clock) = setup();
    c.set_day_start_hour(4).unwrap();
    let cards = add(&c, &clock, default_deck(), 1);
    // 09:00. The study day ends at 04:00 tomorrow.
    c.bury_cards(&cards).unwrap();
    clock.advance(18 * 3_600_000);
    // 03:00 tomorrow: still the same study day.
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    clock.advance(2 * 3_600_000);
    // 05:00.
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
}

#[test]
fn unburying_brings_a_card_back_today() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 3);
    c.bury_cards(&cards).unwrap();
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    c.unbury_cards(&[cards[0]]).unwrap();
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
    // Burying again after unburying works, and unburying a card that is not buried writes nothing.
    c.bury_cards(&[cards[0]]).unwrap();
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    clock.advance(DAY);
    let before = c.register_clock("card", cards[0], "buried_until").unwrap();
    c.unbury_cards(&[cards[0]]).unwrap();
    assert_eq!(
        c.register_clock("card", cards[0], "buried_until").unwrap(),
        before
    );
}

#[test]
fn unburying_a_deck_brings_back_everything_buried_in_it_and_below() {
    let (c, clock) = setup();
    let parent = c.create_deck("P", None).unwrap();
    let child = sub_deck(&c, "C", parent);
    let outside = add(&c, &clock, default_deck(), 1);
    let own = add(&c, &clock, parent, 1);
    let inside = add(&c, &clock, child, 2);
    let all: Vec<Id> = outside.iter().chain(&own).chain(&inside).copied().collect();
    c.bury_cards(&all).unwrap();
    assert_eq!(counts(&c, parent), (0, 0, 0));
    c.unbury_deck(parent).unwrap();
    assert_eq!(counts(&c, parent), (3, 0, 0));
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    assert_eq!(
        c.unbury_deck(Id::from_bytes([4; 16])),
        Err(StudyError::NotFound)
    );
}

#[test]
fn a_suspended_or_buried_card_can_still_be_answered() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 2);
    c.suspend_cards(&[cards[0]]).unwrap();
    c.bury_cards(&[cards[1]]).unwrap();
    good(&c, cards[0]);
    good(&c, cards[1]);
    assert_eq!(
        c.card_schedule(cards[0]).unwrap().state,
        CardState::Learning
    );
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
}

/// A register as another device wrote it later than anything here.
fn remote_set(c: &Collection, card: Id, field: &str, value: i64) {
    c.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    c.conn
        .execute(
            &format!("UPDATE card SET {field} = ?1 WHERE id = ?2"),
            params![Value::Integer(value), card],
        )
        .unwrap();
    c.conn.execute("DELETE FROM write_guard", []).unwrap();
    c.conn
        .execute(
            "INSERT OR REPLACE INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES ('card', ?1, ?2, ?3, ?4, 1)",
            params![card, field, i64::MAX >> 1, Id::from_bytes(*b"remote-device-01")],
        )
        .unwrap();
}

#[test]
fn suspended_on_another_device_and_reviewed_here_keeps_both() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 1);
    good(&c, cards[0]);
    remote_set(&c, cards[0], "suspended", 1);
    c.rebuild_schedule().unwrap();
    assert_eq!(
        c.card_schedule(cards[0]).unwrap().state,
        CardState::Learning
    );
    clock.advance(DAY);
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    c.unsuspend_cards(&cards).unwrap();
    assert_eq!(counts(&c, default_deck()), (0, 1, 0));
}

#[test]
fn suspended_here_and_reviewed_on_another_device_keeps_both() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 1);
    c.suspend_cards(&cards).unwrap();
    let theirs = from_other_device(
        cards[0],
        Id::new_v7(clock.now().unix_ms, &[6; 10]),
        None,
        None,
        Rating::Good,
        clock.now().unix_ms,
        study_day(clock.now().unix_ms, 0, 0),
    );
    insert(&c, &theirs);
    c.rebuild_schedule().unwrap();
    assert_eq!(
        c.card_schedule(cards[0]).unwrap().state,
        CardState::Learning
    );
    clock.advance(DAY);
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    c.unsuspend_cards(&cards).unwrap();
    assert_eq!(counts(&c, default_deck()), (0, 1, 0));
}

#[test]
fn a_card_buried_on_another_device_for_a_later_day_stays_buried_until_then() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 1);
    let today = c.today().unwrap();
    remote_set(&c, cards[0], "buried_until", today + 3);
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    clock.advance(3 * DAY);
    assert_eq!(counts(&c, default_deck()), (0, 0, 0));
    clock.advance(DAY);
    assert_eq!(counts(&c, default_deck()), (1, 0, 0));
}

// ---- Counts for every deck ----

#[test]
fn every_deck_has_counts_in_tree_order() {
    let (c, clock) = setup();
    let parent = c.create_deck("A", None).unwrap();
    let child = sub_deck(&c, "B", parent);
    add(&c, &clock, parent, 2);
    add(&c, &clock, child, 3);
    let list = c.deck_counts().unwrap();
    let ids: Vec<Id> = list.iter().map(|d| d.deck).collect();
    assert_eq!(ids, [parent, child, default_deck()]);
    let news: Vec<u32> = list.iter().map(|d| d.counts.new).collect();
    assert_eq!(news, [5, 3, 0]);
}

#[test]
fn looking_at_the_queue_writes_nothing() {
    let (c, clock) = setup();
    let cards = add(&c, &clock, default_deck(), 3);
    good(&c, cards[0]);
    let count = |c: &Collection| -> i64 {
        c.conn
            .query_row("SELECT count(*) FROM register_clock", [], |r| r.get(0))
            .unwrap()
    };
    let before = (count(&c), events(&c, cards[0]).len());
    for _ in 0..3 {
        c.deck_counts().unwrap();
        c.next_card(default_deck()).unwrap();
    }
    assert_eq!((count(&c), events(&c, cards[0]).len()), before);
}

// ---- Upgrade ----

#[test]
fn a_version_7_collection_upgrades_and_its_cards_are_in_the_queue() {
    use crate::collection::{MIGRATIONS, Schema};
    use crate::sync::{SYNCED_TABLES, check_schema};

    let v7 = Schema {
        migrations: &MIGRATIONS[..7],
        tables: SYNCED_TABLES,
    };
    let path = std::env::temp_dir().join(format!("fc-queue-v7-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = Arc::new(ManualClock::new(super::answer_tests::DAY0 + 9 * HOUR));
    let old = Collection::create_with(location, v7, super::answer_tests::host(&clock)).unwrap();
    // A note with a card that was answered, as step 1.7a left it (the current code cannot read the
    // old layout, so write the rows by hand).
    let note = Id::from_bytes([7; 16]);
    let template = old.note_type(builtin::basic()).unwrap().unwrap().templates[0].id;
    let card = crate::note::card_id(note, template, 0);
    let today = study_day(clock.now().unix_ms, 0, 0);
    old.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    old.conn
        .execute(
            "INSERT INTO note (id, note_type, deleted) VALUES (?1, ?2, 0)",
            params![note, builtin::basic()],
        )
        .unwrap();
    old.conn
        .execute(
            "INSERT INTO card (id, note, template, ordinal, deleted) VALUES (?1, ?2, ?3, 0, 0)",
            params![card, note, template],
        )
        .unwrap();
    old.conn.execute("DELETE FROM write_guard", []).unwrap();
    old.conn
        .execute(
            "INSERT INTO card_schedule (card, state, step, due_day, stability, difficulty, last_day,
                                         last_event, answers, lapses)
             VALUES (?1, 2, 0, ?2, 5.0, 5.0, ?3, ?4, 1, 0)",
            params![card, today - 1, today - 6, Id::from_bytes([1; 16])],
        )
        .unwrap();
    assert!(old.conn.prepare("SELECT suspended FROM card").is_err());
    old.close().unwrap();

    let upgraded = Collection::open(location, super::answer_tests::host(&clock)).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 10);
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    // The old card is awake and due, the Default deck and preset have the new settings, and the
    // seeded rows have clocks for the new registers, so a sync can compare them.
    assert_eq!(counts(&upgraded, default_deck()), (0, 0, 1));
    assert_eq!(shown(&upgraded, default_deck()), Some(card));
    assert!(
        upgraded
            .deck(default_deck())
            .unwrap()
            .unwrap()
            .limits_include_subdecks
    );
    assert!(
        upgraded
            .preset(default_preset())
            .unwrap()
            .unwrap()
            .space_siblings
    );
    for (entity, id, field) in [
        ("deck", default_deck(), "limits_include_subdecks"),
        ("options_preset", default_preset(), "space_siblings"),
    ] {
        assert!(
            upgraded
                .register_clock(entity, id, field)
                .unwrap()
                .unwrap()
                .pushed
        );
    }
    upgraded.suspend_cards(&[card]).unwrap();
    assert_eq!(counts(&upgraded, default_deck()), (0, 0, 0));
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&path);
}
