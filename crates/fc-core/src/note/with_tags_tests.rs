//! `add_note_with`: a note, its cards and its tags in one write (step 2.4a).

use super::*;
use crate::collection::Collection;
use crate::deck::default_deck;
use crate::events::Event;
use crate::events::tests::Recorder;
use crate::notetype::builtin;
use crate::sync::Selection;
use crate::sync::merge_tests::{digest, pair};

fn basic_fields(c: &Collection) -> Vec<Id> {
    let found = c.note_type(builtin::basic()).unwrap().unwrap();
    found.fields.iter().map(|f| f.id).collect()
}

fn add_tagged(c: &Collection, front: &str, tags: &[&str]) -> Result<AddedNote, NoteError> {
    let f = basic_fields(c);
    c.add_note_with(&NewNote {
        deck: default_deck(),
        note_type: builtin::basic(),
        values: &[(f[0], front), (f[1], "back")],
        tags,
    })
}

fn count(c: &Collection, table: &str) -> i64 {
    c.conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn the_tags_are_written_with_the_note() {
    let (a, _) = pair();
    let added = add_tagged(&a.c, "kot", &["animals", "pl::basic"]).unwrap();
    assert_eq!(
        a.c.note_tags(added.id).unwrap(),
        vec!["animals", "pl::basic"]
    );
    assert_eq!(added.cards.len(), 1);
}

#[test]
fn no_tags_is_the_same_as_add_note_to_deck() {
    let (a, _) = pair();
    let added = add_tagged(&a.c, "kot", &[]).unwrap();
    assert!(a.c.note_tags(added.id).unwrap().is_empty());
}

#[test]
fn a_tag_is_spelled_the_way_the_collection_already_spells_it_and_once() {
    let (a, _) = pair();
    add_tagged(&a.c, "one", &["polish"]).unwrap();
    let added = add_tagged(&a.c, "two", &["Polish", "POLISH", "new"]).unwrap();
    assert_eq!(a.c.note_tags(added.id).unwrap(), vec!["new", "polish"]);
}

#[test]
fn a_bad_tag_refuses_the_whole_note_and_writes_nothing() {
    let (a, _) = pair();
    let before = digest(&a.c);
    let (notes, cards, tags) = (
        count(&a.c, "note"),
        count(&a.c, "card"),
        count(&a.c, "note_tag"),
    );
    let rec = Recorder::attach(&a.c);
    for bad in ["", "two words", "a::", "::b"] {
        let result = add_tagged(&a.c, "kot", &["fine", bad]);
        assert!(matches!(result, Err(NoteError::Tag(_))), "{bad:?}");
    }
    assert_eq!(count(&a.c, "note"), notes);
    assert_eq!(count(&a.c, "card"), cards);
    assert_eq!(count(&a.c, "note_tag"), tags);
    assert_eq!(digest(&a.c), before);
    assert!(rec.take().is_empty());
}

#[test]
fn a_note_that_makes_no_cards_writes_no_tags() {
    let (a, _) = pair();
    let result = add_tagged(&a.c, "", &["animals"]);
    assert!(matches!(result, Err(NoteError::NoCards { cloze: false })));
    assert_eq!(count(&a.c, "note_tag"), 0);
}

#[test]
fn the_note_added_event_fires_once() {
    let (a, _) = pair();
    let rec = Recorder::attach(&a.c);
    let added = add_tagged(&a.c, "kot", &["animals", "pl"]).unwrap();
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
fn a_replay_of_the_change_log_has_the_tags() {
    let (a, b) = pair();
    let added = add_tagged(&a.c, "kot", &["animals", "pl::basic"]).unwrap();
    b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
    assert_eq!(
        b.c.note_tags(added.id).unwrap(),
        vec!["animals", "pl::basic"]
    );
    assert_eq!(digest(&a.c), digest(&b.c));
}
