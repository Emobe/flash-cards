//! The wire form of every core event (`docs/events.md`) and the forwarding of core events.

use std::sync::{Arc, Mutex};

use fc_core::clock::{Host, ManualClock};
use fc_core::notetype::builtin;
use fc_core::scheduling::Rating;
use serde_json::{Value, json};

use super::*;

fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}

/// The UUID string of `id(n)`, which is what the wire carries.
fn s(n: u8) -> String {
    id(n).to_string()
}

fn wire(event: &Event) -> Value {
    serde_json::to_value(CoreEvent::from(event)).unwrap()
}

#[test]
fn every_event_has_its_documented_json_shape() {
    let summary = fc_core::study::SessionSummary {
        answered: 3,
        again: 1,
        studied_ms: 9_000,
        elapsed_ms: 60_000,
    };
    let cases: Vec<(Event, Value)> = vec![
        (
            Event::NoteAdded {
                note: id(1),
                note_type: id(2),
                deck: id(3),
                cards: vec![id(4), id(5)],
            },
            json!({ "kind": "noteAdded", "note": s(1), "noteType": s(2), "deck": s(3),
                    "cards": [s(4), s(5)] }),
        ),
        (
            Event::NoteEdited {
                note: id(1),
                fields: vec![id(2)],
                cards_added: vec![id(3)],
                cards_removed: vec![],
            },
            json!({ "kind": "noteEdited", "note": s(1), "fields": [s(2)],
                    "cardsAdded": [s(3)], "cardsRemoved": [] }),
        ),
        (
            Event::NoteDeleted { note: id(1) },
            json!({ "kind": "noteDeleted", "note": s(1) }),
        ),
        (
            Event::NoteRestored {
                note: id(1),
                cards: vec![id(2)],
            },
            json!({ "kind": "noteRestored", "note": s(1), "cards": [s(2)] }),
        ),
        (
            Event::CardAnswered {
                card: id(1),
                deck: id(2),
                event: id(3),
                rating: Rating::Hard,
                session: Some(id(4)),
            },
            json!({ "kind": "cardAnswered", "card": s(1), "deck": s(2), "event": s(3),
                    "rating": "hard", "session": s(4) }),
        ),
        (
            Event::CardAnswered {
                card: id(1),
                deck: id(2),
                event: id(3),
                rating: Rating::Easy,
                session: None,
            },
            json!({ "kind": "cardAnswered", "card": s(1), "deck": s(2), "event": s(3),
                    "rating": "easy", "session": null }),
        ),
        (
            Event::AnswerUndone {
                card: id(1),
                event: id(2),
            },
            json!({ "kind": "answerUndone", "card": s(1), "event": s(2) }),
        ),
        (
            Event::StudySessionStarted {
                session: id(1),
                deck: None,
            },
            json!({ "kind": "studySessionStarted", "session": s(1), "deck": null }),
        ),
        (
            Event::StudySessionEnded {
                session: id(1),
                deck: Some(id(2)),
                reason: EndReason::Replaced,
                summary,
            },
            json!({ "kind": "studySessionEnded", "session": s(1), "deck": s(2),
                    "reason": "replaced",
                    "summary": { "answered": 3, "again": 1, "studiedMs": 9000,
                                 "elapsedMs": 60000 } }),
        ),
        (
            Event::MergeApplied {
                registers_applied: 1,
                rows_added: 2,
                unknown_kept: 3,
                notes_reconciled: 4,
                cards_rebuilt: 5,
                rejected: 6,
            },
            json!({ "kind": "mergeApplied", "registersApplied": 1, "rowsAdded": 2,
                    "unknownKept": 3, "notesReconciled": 4, "cardsRebuilt": 5,
                    "rejected": 6 }),
        ),
        (Event::SyncCompleted, json!({ "kind": "syncCompleted" })),
    ];
    for (event, expected) in cases {
        assert_eq!(wire(&event), expected, "{event:?}");
    }
}

#[test]
fn the_rating_and_end_reason_unions_cover_every_core_value() {
    for (rating, name) in [
        (Rating::Again, "again"),
        (Rating::Hard, "hard"),
        (Rating::Good, "good"),
        (Rating::Easy, "easy"),
    ] {
        let event = Event::CardAnswered {
            card: id(1),
            deck: id(1),
            event: id(1),
            rating,
            session: None,
        };
        assert_eq!(wire(&event)["rating"], name);
    }
    for (reason, name) in [
        (EndReason::Ended, "ended"),
        (EndReason::Replaced, "replaced"),
        (EndReason::Closed, "closed"),
    ] {
        let event = Event::StudySessionEnded {
            session: id(1),
            deck: None,
            reason,
            summary: fc_core::study::SessionSummary::default(),
        };
        assert_eq!(wire(&event)["reason"], name);
    }
}

#[derive(Default)]
struct Recording(Mutex<Vec<Notice>>);

impl EventSink for Recording {
    fn send(&self, notice: Notice) {
        self.0.lock().unwrap().push(notice);
    }
}

#[test]
fn forward_events_sends_a_collections_events_as_notices_in_order() {
    let core = Core::new();
    let sink = Arc::new(Recording::default());
    forward_events(&core, sink.clone());
    let host = Host {
        clock: Arc::new(ManualClock::new(1_700_000_000_000)),
        installation_id: id(8),
    };
    core.open_collection(":memory:", host).unwrap();
    let note = core
        .with_collection(|c| {
            let field = c.note_type(builtin::basic()).unwrap().unwrap().fields[0].id;
            let added = c.add_note(builtin::basic(), &[(field, "front")]).unwrap();
            c.start_study_session(None).unwrap();
            c.answer(added.cards[0], Rating::Good, 1_000).unwrap();
            added.id
        })
        .unwrap();
    let kinds: Vec<String> = sink
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|notice| match serde_json::to_value(notice).unwrap() {
            Value::Object(o) => {
                assert_eq!(o["type"], "event");
                o["event"]["kind"].as_str().unwrap().to_owned()
            }
            other => panic!("{other}"),
        })
        .collect();
    assert_eq!(kinds, ["noteAdded", "studySessionStarted", "cardAnswered"]);
    let first = sink.0.lock().unwrap()[0].clone();
    assert!(matches!(
        first,
        Notice::Event { event: CoreEvent::NoteAdded { note: n, .. } } if n == note.to_string()
    ));
}

#[test]
fn no_event_notice_carries_field_text() {
    let core = Core::new();
    let sink = Arc::new(Recording::default());
    forward_events(&core, sink.clone());
    let host = Host {
        clock: Arc::new(ManualClock::new(1_700_000_000_000)),
        installation_id: id(8),
    };
    core.open_collection(":memory:", host).unwrap();
    core.with_collection(|c| {
        let field = c.note_type(builtin::basic()).unwrap().unwrap().fields[0].id;
        c.add_note(builtin::basic(), &[(field, "marker-text-ZZ")])
            .unwrap();
    })
    .unwrap();
    let json = serde_json::to_string(&*sink.0.lock().unwrap()).unwrap();
    assert!(!json.contains("marker-text-ZZ"));
}
