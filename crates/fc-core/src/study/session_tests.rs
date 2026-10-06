//! Study sessions (ADR 0009, part 5).

use super::*;
use crate::deck::default_deck;
use crate::events::Event;
use crate::events::tests::Recorder;
use crate::scheduling::Rating;
use crate::study::answer_tests::{MINUTE, basic_card, setup};

fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}

#[test]
fn starting_a_session_emits_and_returns_its_id() {
    let (c, _) = setup();
    let rec = Recorder::attach(&c);
    let session = c.start_study_session(None).unwrap();
    assert_eq!(
        rec.one(),
        vec![Event::StudySessionStarted {
            session,
            deck: None
        }]
    );
    let deck = c.create_deck("French", None).unwrap();
    let second = c.start_study_session(Some(deck)).unwrap();
    assert_ne!(session, second);
}

#[test]
fn the_summary_counts_answers_not_undone() {
    let (c, clock) = setup();
    let (a, b, d) = (
        basic_card(&c, "a"),
        basic_card(&c, "b"),
        basic_card(&c, "c"),
    );
    // An answer before the session does not count.
    c.answer(a, Rating::Good, 1_000).unwrap();
    clock.advance(MINUTE);
    let session = c.start_study_session(None).unwrap();
    let rec = Recorder::attach(&c);
    clock.advance(MINUTE);
    c.answer(b, Rating::Again, 2_000).unwrap();
    clock.advance(MINUTE);
    c.answer(d, Rating::Good, 3_000).unwrap();
    clock.advance(MINUTE);
    c.answer(a, Rating::Again, 4_000).unwrap();
    c.undo_answer().unwrap().unwrap(); // takes back a's Again
    rec.take();
    clock.advance(MINUTE);
    let summary = c.end_study_session(session).unwrap().unwrap();
    assert_eq!(
        summary,
        SessionSummary {
            answered: 2,
            again: 1,
            studied_ms: 5_000,
            elapsed_ms: u32::try_from(4 * MINUTE).unwrap(),
        }
    );
    assert_eq!(
        rec.one(),
        vec![Event::StudySessionEnded {
            session,
            deck: None,
            reason: EndReason::Ended,
            summary,
        }]
    );
}

#[test]
fn ending_a_session_twice_or_a_wrong_one_does_nothing() {
    let (c, _) = setup();
    let session = c.start_study_session(None).unwrap();
    let rec = Recorder::attach(&c);
    assert_eq!(c.end_study_session(id(9)).unwrap(), None);
    assert!(c.end_study_session(session).unwrap().is_some());
    rec.take();
    assert_eq!(c.end_study_session(session).unwrap(), None);
    assert!(rec.take().is_empty());
}

#[test]
fn a_second_start_ends_the_first_as_replaced() {
    let (c, _) = setup();
    let deck = c.create_deck("French", None).unwrap();
    let first = c.start_study_session(Some(deck)).unwrap();
    let rec = Recorder::attach(&c);
    let second = c.start_study_session(None).unwrap();
    let batches = rec.take();
    assert_eq!(batches.len(), 2);
    assert!(matches!(
        batches[0].as_slice(),
        [Event::StudySessionEnded { session, deck: d, reason: EndReason::Replaced, .. }]
            if *session == first && *d == Some(deck)
    ));
    assert_eq!(
        batches[1],
        vec![Event::StudySessionStarted {
            session: second,
            deck: None
        }]
    );
    assert_eq!(c.end_study_session(first).unwrap(), None);
}

#[test]
fn closing_the_collection_ends_the_session_as_closed() {
    let (c, _) = setup();
    let session = c.start_study_session(None).unwrap();
    let rec = Recorder::attach(&c);
    c.close().unwrap();
    assert!(matches!(
        rec.one().as_slice(),
        [Event::StudySessionEnded { session: s, reason: EndReason::Closed, .. }] if *s == session
    ));
}

#[test]
fn an_answer_names_the_open_session_and_only_while_it_is_open() {
    let (c, _) = setup();
    let (a, b) = (basic_card(&c, "a"), basic_card(&c, "b"));
    let rec = Recorder::attach(&c);
    let session = c.start_study_session(Some(default_deck())).unwrap();
    rec.take();
    c.answer(a, Rating::Good, 1_000).unwrap();
    c.end_study_session(session).unwrap();
    rec.take();
    c.answer(b, Rating::Good, 1_000).unwrap();
    let after = rec.one();
    assert!(matches!(
        after.as_slice(),
        [Event::CardAnswered { session: None, .. }]
    ));
    // And the first named the session.
    let session = c.start_study_session(None).unwrap();
    rec.take();
    c.answer(a, Rating::Good, 1_000).unwrap();
    assert!(matches!(
        rec.one().as_slice(),
        [Event::CardAnswered { session: Some(s), .. }] if *s == session
    ));
}

#[test]
fn a_missing_or_deleted_deck_is_not_found_and_leaves_the_open_session_alone() {
    let (c, _) = setup();
    let open = c.start_study_session(None).unwrap();
    assert_eq!(
        c.start_study_session(Some(id(7))),
        Err(StudyError::NotFound)
    );
    let deck = c.create_deck("Gone", None).unwrap();
    c.delete_deck(deck).unwrap();
    assert_eq!(c.start_study_session(Some(deck)), Err(StudyError::NotFound));
    assert!(c.end_study_session(open).unwrap().is_some());
}
