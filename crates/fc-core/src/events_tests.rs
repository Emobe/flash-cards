//! Core events (ADR 0009): what each operation emits, and the listener rules.

use std::sync::{Arc, Mutex};

use super::*;
use crate::clock::ManualClock;
use crate::collection::Collection;
use crate::deck::default_deck;
use crate::notetype::builtin;
use crate::study::answer_tests::{DAY0, HOUR, basic_card, host, setup};

/// Keeps every batch it hears.
#[derive(Default)]
pub(crate) struct Recorder(Mutex<Vec<Vec<Event>>>);

impl Recorder {
    pub(crate) fn attach(c: &Collection) -> Arc<Self> {
        let rec = Arc::new(Self::default());
        c.listen(rec.clone());
        rec
    }

    /// The batches heard since the last call.
    pub(crate) fn take(&self) -> Vec<Vec<Event>> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    /// The only batch heard since the last call.
    pub(crate) fn one(&self) -> Vec<Event> {
        let mut batches = self.take();
        assert_eq!(batches.len(), 1, "expected one batch, got {batches:?}");
        batches.remove(0)
    }
}

impl Listener for Recorder {
    fn on_events(&self, events: &[Event]) {
        assert!(
            !events.is_empty(),
            "a listener is never given an empty batch"
        );
        self.0.lock().unwrap().push(events.to_vec());
    }
}

fn basic_fields(c: &Collection) -> (Id, Id) {
    let fields = c.note_type(builtin::basic()).unwrap().unwrap().fields;
    (fields[0].id, fields[1].id)
}

// ---- Each operation emits its event ----

#[test]
fn adding_a_note_emits_note_added_with_its_cards() {
    let (c, _) = setup();
    let rec = Recorder::attach(&c);
    let (front, back) = basic_fields(&c);
    let added = c
        .add_note(builtin::basic(), &[(front, "a"), (back, "b")])
        .unwrap();
    assert_eq!(
        rec.one(),
        vec![Event::NoteAdded {
            note: added.id,
            note_type: builtin::basic(),
            deck: default_deck(),
            cards: added.cards,
        }]
    );
}

#[test]
fn adding_a_note_to_a_deck_names_that_deck() {
    let (c, _) = setup();
    let deck = c.create_deck("French", None).unwrap();
    let rec = Recorder::attach(&c);
    let (front, _) = basic_fields(&c);
    let added = c
        .add_note_to_deck(deck, builtin::basic(), &[(front, "a")])
        .unwrap();
    match rec.one().as_slice() {
        [Event::NoteAdded { note, deck: d, .. }] => {
            assert_eq!((*note, *d), (added.id, deck));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn editing_a_note_emits_the_fields_that_changed() {
    let (c, _) = setup();
    let (front, back) = basic_fields(&c);
    let note = c
        .add_note(builtin::basic(), &[(front, "a"), (back, "b")])
        .unwrap()
        .id;
    let rec = Recorder::attach(&c);
    c.set_note_fields(note, &[(front, "a"), (back, "changed")])
        .unwrap();
    assert_eq!(
        rec.one(),
        vec![Event::NoteEdited {
            note,
            fields: vec![back],
            cards_added: vec![],
            cards_removed: vec![],
        }]
    );
}

#[test]
fn deleting_and_restoring_a_note_emit_their_events() {
    let (c, _) = setup();
    let (front, back) = basic_fields(&c);
    let added = c
        .add_note(builtin::basic(), &[(front, "a"), (back, "b")])
        .unwrap();
    let rec = Recorder::attach(&c);
    c.delete_note(added.id).unwrap();
    assert_eq!(rec.one(), vec![Event::NoteDeleted { note: added.id }]);
    c.restore_note(added.id).unwrap();
    assert_eq!(
        rec.one(),
        vec![Event::NoteRestored {
            note: added.id,
            cards: added.cards,
        }]
    );
}

#[test]
fn answering_and_undoing_emit_with_the_review_event() {
    let (c, _) = setup();
    let card = basic_card(&c, "a");
    let rec = Recorder::attach(&c);
    let answered = c.answer(card, Rating::Hard, 2_000).unwrap();
    assert_eq!(
        rec.one(),
        vec![Event::CardAnswered {
            card,
            deck: default_deck(),
            event: answered.event,
            rating: Rating::Hard,
            session: None,
        }]
    );
    c.undo_answer().unwrap().unwrap();
    assert_eq!(
        rec.one(),
        vec![Event::AnswerUndone {
            card,
            event: answered.event,
        }]
    );
}

#[test]
fn sync_completed_has_no_fields_and_reaches_listeners() {
    let (c, _) = setup();
    let rec = Recorder::attach(&c);
    c.emit_now(Event::SyncCompleted);
    assert_eq!(rec.one(), vec![Event::SyncCompleted]);
}

// ---- When nothing is emitted ----

#[test]
fn no_ops_emit_nothing() {
    let (c, _) = setup();
    let (front, back) = basic_fields(&c);
    let note = c
        .add_note(builtin::basic(), &[(front, "a"), (back, "b")])
        .unwrap()
        .id;
    c.delete_note(note).unwrap();
    let live_note = c
        .add_note(builtin::basic(), &[(front, "live"), (back, "back")])
        .unwrap()
        .id;
    let rec = Recorder::attach(&c);

    c.delete_note(note).unwrap(); // already deleted
    c.restore_note(live_note).unwrap(); // already live
    c.set_note_fields(live_note, &[(front, "live"), (back, "back")])
        .unwrap(); // same values
    assert_eq!(c.end_study_session(Id::from_bytes([9; 16])).unwrap(), None);
    assert!(rec.take().is_empty());
}

#[test]
fn a_failed_write_emits_nothing() {
    let (c, _) = setup();
    let (front, _) = basic_fields(&c);
    let deck = c.create_deck("Gone", None).unwrap();
    let added = c.add_note(builtin::basic(), &[(front, "a")]).unwrap();
    let (card, note) = (added.cards[0], added.id);
    c.delete_deck(deck).unwrap();
    c.delete_note(note).unwrap();
    let rec = Recorder::attach(&c);

    assert!(
        c.add_note_to_deck(deck, builtin::basic(), &[(front, "x")])
            .is_err()
    );
    assert!(c.answer(card, Rating::Good, 1_000).is_err());
    assert!(c.start_study_session(Some(deck)).is_err());
    assert!(rec.take().is_empty());
}

#[test]
fn no_event_carries_field_text() {
    let (c, _) = setup();
    let rec = Recorder::attach(&c);
    let (front, back) = basic_fields(&c);
    let marker = "ZZ-secret-marker-ZZ";
    let note = c
        .add_note(builtin::basic(), &[(front, marker), (back, marker)])
        .unwrap();
    c.set_note_fields(note.id, &[(front, "ZZ-secret-marker-ZZ!")])
        .unwrap();
    c.answer(note.cards[0], Rating::Good, 1_000).unwrap();
    c.undo_answer().unwrap();
    c.delete_note(note.id).unwrap();
    c.restore_note(note.id).unwrap();
    let all = rec.take();
    assert!(all.len() >= 6);
    assert!(!format!("{all:?}").contains("secret-marker"));
}

// ---- Listeners ----

struct Panicker;

impl Listener for Panicker {
    fn on_events(&self, _: &[Event]) {
        panic!("a listener that panics");
    }
}

#[test]
fn a_panicking_listener_is_caught_and_the_next_listener_still_hears() {
    let (c, _) = setup();
    c.listen(Arc::new(Panicker));
    let rec = Recorder::attach(&c);
    let (front, _) = basic_fields(&c);
    let first = c.add_note(builtin::basic(), &[(front, "a")]).unwrap();
    assert_eq!(rec.one().len(), 1);
    // The next operation works, and the panicking listener is still registered and still called.
    let second = c.add_note(builtin::basic(), &[(front, "b")]).unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(rec.one().len(), 1);
}

#[test]
fn a_listener_added_to_a_core_before_opening_hears_the_collection() {
    let core = crate::Core::new();
    let rec = Arc::new(Recorder::default());
    core.listen(rec.clone());
    let clock = Arc::new(ManualClock::new(DAY0 + 9 * HOUR));
    core.open_collection(":memory:", host(&clock)).unwrap();
    core.with_collection(|c| {
        let (front, _) = basic_fields(c);
        c.add_note(builtin::basic(), &[(front, "a")]).unwrap();
    })
    .unwrap();
    assert!(matches!(rec.one().as_slice(), [Event::NoteAdded { .. }]));
}

#[test]
fn a_listener_added_to_a_core_after_opening_hears_it_too() {
    let core = crate::Core::new();
    let clock = Arc::new(ManualClock::new(DAY0 + 9 * HOUR));
    core.open_collection(":memory:", host(&clock)).unwrap();
    let rec = Arc::new(Recorder::default());
    core.listen(rec.clone());
    core.with_collection(|c| {
        let (front, _) = basic_fields(c);
        c.add_note(builtin::basic(), &[(front, "a")]).unwrap();
    })
    .unwrap();
    assert_eq!(rec.one().len(), 1);
}

#[test]
fn replacing_the_core_collection_ends_its_session_as_closed() {
    let core = crate::Core::new();
    let rec = Arc::new(Recorder::default());
    core.listen(rec.clone());
    let clock = Arc::new(ManualClock::new(DAY0 + 9 * HOUR));
    core.open_collection(":memory:", host(&clock)).unwrap();
    let session = core
        .with_collection(|c| c.start_study_session(None).unwrap())
        .unwrap();
    rec.take();
    core.open_collection(":memory:", host(&clock)).unwrap();
    match rec.one().as_slice() {
        [
            Event::StudySessionEnded {
                session: s, reason, ..
            },
        ] => {
            assert_eq!((*s, *reason), (session, EndReason::Closed));
        }
        other => panic!("{other:?}"),
    }
    core.with_collection(|c| c.start_study_session(None).unwrap());
    rec.take();
    core.close_collection().unwrap();
    assert!(matches!(
        rec.one().as_slice(),
        [Event::StudySessionEnded {
            reason: EndReason::Closed,
            ..
        }]
    ));
}

#[test]
fn a_batch_is_in_the_order_it_was_emitted() {
    let rec = Recorder::default();
    let listeners = Listeners::default();
    listeners.add(Arc::new(Recorder::default()));
    let own = Arc::new(rec);
    listeners.add(own.clone());
    let a = Id::from_bytes([1; 16]);
    let b = Id::from_bytes([2; 16]);
    listeners.deliver(&[
        Event::NoteDeleted { note: a },
        Event::NoteDeleted { note: b },
    ]);
    listeners.deliver(&[]);
    assert_eq!(
        own.one(),
        vec![
            Event::NoteDeleted { note: a },
            Event::NoteDeleted { note: b }
        ]
    );
}

#[test]
fn counts_saturate() {
    assert_eq!(count(7), 7);
    assert_eq!(count(usize::MAX), u32::MAX);
}

#[test]
fn collection_stays_send() {
    fn send<T: Send>() {}
    send::<Collection>();
}
