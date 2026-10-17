//! A transferred card as the rest of the study code sees it (ADR 0014, decision 6): an older build
//! reading the kind, and the counts of answers.

use super::answer_tests::{DAY, basic_card, events, setup};
use super::fold::{ParameterSets, fold};
use super::schedule::read_cached;
use super::*;
use crate::notetype::builtin;
use crate::scheduling::Rating;
use crate::stats::DayRange;

/// The note of `old` changed to Basic and reversed with Card 1 kept. Answers the new Card 1.
fn transferred(c: &crate::collection::Collection, old: Id) -> Id {
    let note: Id = c
        .conn
        .query_row("SELECT note FROM card WHERE id = ?1", [old], |r| r.get(0))
        .unwrap();
    let f = |t: Id| -> (Vec<Id>, Vec<Id>) {
        let found = c.note_type(t).unwrap().unwrap();
        (
            found.fields.iter().map(|f| f.id).collect(),
            found.templates.iter().map(|t| t.id).collect(),
        )
    };
    let (of, ot) = f(builtin::basic());
    let (nf, nt) = f(builtin::basic_and_reversed());
    let done = c
        .change_note_type(
            &[note],
            builtin::basic_and_reversed(),
            &[(nf[0], Some(of[0])), (nf[1], Some(of[1]))],
            &[(nt[0], Some(ot[0]))],
        )
        .unwrap();
    done.carried[0].0
}

#[test]
fn a_kind_the_fold_does_not_know_with_the_same_columns_gives_the_same_transferred_card() {
    let (c, clock) = setup();
    let old = basic_card(&c, "a");
    c.answer(old, Rating::Easy, 2_000).unwrap();
    let card = transferred(&c, old);
    clock.advance(DAY);
    c.answer(card, Rating::Good, 2_000).unwrap();
    let sets = ParameterSets::load(&c.conn).unwrap();
    let known = events(&c, card);
    let unknown: Vec<CardEvent> = known
        .iter()
        .cloned()
        .map(|mut e| {
            if e.kind == EventKind::Transfer {
                e.kind = EventKind::Unknown("transfer-from-later".to_owned());
            }
            e
        })
        .collect();
    assert_eq!(fold(&unknown, &sets), fold(&known, &sets));
    assert_eq!(fold(&known, &sets), read_cached(&c.conn, card).unwrap());
}

#[test]
fn daily_counts_and_retention_ignore_a_transfer() {
    let (c, _) = setup();
    let range = DayRange {
        from: 19_600,
        to: 19_700,
    };
    let old = basic_card(&c, "a");
    c.answer(old, Rating::Easy, 2_000).unwrap();
    let counts = c.daily_counts(range, None).unwrap();
    let retention = c.retention(range, None).unwrap();
    let card = transferred(&c, old);
    assert!(c.card_schedule(card).is_ok());
    assert_eq!(c.daily_counts(range, None).unwrap(), counts);
    assert_eq!(c.retention(range, None).unwrap(), retention);
}
