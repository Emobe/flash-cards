//! What a merge emits (ADR 0009, part 2), and a session's summary across devices.

use super::merge_tests::{add, digest, edit_front, pair, sync};
use super::*;
use crate::events::Event;
use crate::events::tests::Recorder;
use crate::scheduling::Rating;
use crate::study::answer_tests::{DAY, MINUTE, good};

#[test]
fn a_merge_emits_one_summary_with_the_report_counts_and_no_note_events() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    good(&a.c, card);
    let rec = Recorder::attach(&b.c);
    let report = b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
    let batch = rec.one();
    let count = |n: usize| u32::try_from(n).unwrap();
    assert_eq!(
        batch,
        vec![Event::MergeApplied {
            registers_applied: count(report.registers_applied),
            rows_added: count(report.rows_added),
            unknown_kept: count(report.unknown_registers + report.unknown_rows),
            notes_reconciled: count(report.notes_reconciled),
            cards_rebuilt: count(report.cards_rebuilt),
            rejected: count(report.rejected.len()),
        }]
    );
    assert!(report.registers_applied > 0 && report.rows_added > 0);
}

#[test]
fn merging_the_same_batch_again_emits_nothing() {
    let (a, b) = pair();
    add(&a.c, "x", "y");
    let batch = a.c.changes(Selection::All).unwrap();
    b.c.merge(&batch).unwrap();
    let rec = Recorder::attach(&b.c);
    b.c.merge(&batch).unwrap();
    assert!(rec.take().is_empty());
}

#[test]
fn a_failed_merge_emits_nothing_and_changes_nothing() {
    let (a, b) = pair();
    add(&b.c, "theirs", "b");
    let before = digest(&a.c);
    let rec = Recorder::attach(&a.c);
    a.c.conn
        .execute_batch("ALTER TABLE deck RENAME TO deck_gone")
        .unwrap();
    assert!(a.c.merge(&b.c.changes(Selection::All).unwrap()).is_err());
    a.c.conn
        .execute_batch("ALTER TABLE deck_gone RENAME TO deck")
        .unwrap();
    assert!(rec.take().is_empty());
    assert_eq!(digest(&a.c), before);
}

#[test]
fn a_merge_that_changes_a_note_emits_no_note_event() {
    let (a, b) = pair();
    let (note, _) = add(&a.c, "x", "y");
    sync(&a, &b);
    a.clock.advance(DAY);
    edit_front(&a.c, note, "edited");
    let rec = Recorder::attach(&b.c);
    b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
    assert!(matches!(rec.one().as_slice(), [Event::MergeApplied { .. }]));
}

#[test]
fn a_summary_counts_this_devices_answers_only() {
    let (a, b) = pair();
    let (_, card) = add(&a.c, "x", "y");
    sync(&a, &b);
    let session = a.c.start_study_session(None).unwrap();
    a.clock.advance(MINUTE);
    b.clock.advance(MINUTE);
    b.c.answer(card, Rating::Good, 1_000).unwrap();
    sync(&b, &a);
    let summary = a.c.end_study_session(session).unwrap().unwrap();
    assert_eq!(summary.answered, 0);
}
