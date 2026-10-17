//! Step 3.2a: `edit_note_with`, `delete_notes`, `restore_notes`, the changed sets the bulk
//! operations answer, and the reads that turn notes into cards and back.

use super::*;
use crate::collection::Collection;
use crate::deck::default_deck;
use crate::events::Event;
use crate::events::tests::Recorder;
use crate::notetype::builtin;
use crate::search::Mode;
use crate::sync::Selection;
use crate::sync::merge_tests::{digest, pair};

fn fields(c: &Collection, note_type: Id) -> Vec<Id> {
    let found = c.note_type(note_type).unwrap().unwrap();
    found.fields.iter().map(|f| f.id).collect()
}

fn add(c: &Collection, front: &str, tags: &[&str]) -> AddedNote {
    let f = fields(c, builtin::basic());
    c.add_note_with(&NewNote {
        deck: default_deck(),
        note_type: builtin::basic(),
        values: &[(f[0], front), (f[1], "back")],
        tags,
    })
    .unwrap()
}

fn add_reversed(c: &Collection, front: &str) -> AddedNote {
    let f = fields(c, builtin::basic_and_reversed());
    c.add_note(
        builtin::basic_and_reversed(),
        &[(f[0], front), (f[1], "back")],
    )
    .unwrap()
}

fn values(c: &Collection, note: Id) -> Vec<String> {
    let found = c.note(note).unwrap().unwrap();
    found.fields.into_iter().map(|f| f.value).collect()
}

fn missing() -> Id {
    Id::from_bytes([0xee; 16])
}

// ---- edit_note_with -------------------------------------------------------------------------------

#[test]
fn an_edit_writes_values_and_tags_and_answers_what_they_were() {
    let (a, _) = pair();
    let added = add(&a.c, "kot", &["animals", "old"]);
    let f = fields(&a.c, builtin::basic());
    let rec = Recorder::attach(&a.c);
    let edit =
        a.c.edit_note_with(
            added.id,
            &[(f[0], "pies"), (f[1], "back")],
            Some(&["animals", "new"]),
        )
        .unwrap();
    assert_eq!(values(&a.c, added.id), vec!["pies", "back"]);
    assert_eq!(a.c.note_tags(added.id).unwrap(), vec!["animals", "new"]);
    assert_eq!(edit.old_values, vec![(f[0], "kot".to_owned())]);
    assert_eq!(
        edit.old_tags,
        Some(vec!["animals".to_owned(), "old".to_owned()])
    );
    assert_eq!(
        rec.one(),
        vec![Event::NoteEdited {
            note: added.id,
            fields: vec![f[0]],
            cards_added: vec![],
            cards_removed: vec![],
        }]
    );
}

#[test]
fn writing_the_old_values_back_undoes_an_edit() {
    let (a, _) = pair();
    let added = add(&a.c, "kot", &["animals"]);
    let f = fields(&a.c, builtin::basic());
    let edit =
        a.c.edit_note_with(added.id, &[(f[0], "pies"), (f[1], "tył")], Some(&["x"]))
            .unwrap();
    let old: Vec<(Id, &str)> = edit
        .old_values
        .iter()
        .map(|(f, v)| (*f, v.as_str()))
        .collect();
    let tags: Vec<&str> = edit
        .old_tags
        .as_ref()
        .unwrap()
        .iter()
        .map(String::as_str)
        .collect();
    a.c.edit_note_with(added.id, &old, Some(&tags)).unwrap();
    assert_eq!(values(&a.c, added.id), vec!["kot", "back"]);
    assert_eq!(a.c.note_tags(added.id).unwrap(), vec!["animals"]);
}

#[test]
fn a_tags_only_edit_is_one_note_edited_with_no_fields() {
    let (a, _) = pair();
    let added = add(&a.c, "kot", &[]);
    let rec = Recorder::attach(&a.c);
    let edit = a.c.edit_note_with(added.id, &[], Some(&["pl"])).unwrap();
    assert!(edit.old_values.is_empty());
    assert_eq!(edit.old_tags, Some(vec![]));
    assert_eq!(
        rec.one(),
        vec![Event::NoteEdited {
            note: added.id,
            fields: vec![],
            cards_added: vec![],
            cards_removed: vec![],
        }]
    );
}

#[test]
fn an_edit_that_changes_nothing_writes_nothing() {
    let (a, _) = pair();
    let added = add(&a.c, "kot", &["animals"]);
    let f = fields(&a.c, builtin::basic());
    let before = digest(&a.c);
    let rec = Recorder::attach(&a.c);
    let edit =
        a.c.edit_note_with(
            added.id,
            &[(f[0], "kot"), (f[1], "back")],
            Some(&["ANIMALS"]),
        )
        .unwrap();
    assert_eq!(edit, NoteEdit::default());
    assert_eq!(digest(&a.c), before);
    assert!(rec.take().is_empty());
}

#[test]
fn without_tags_an_edit_leaves_the_tags_alone() {
    let (a, _) = pair();
    let added = add(&a.c, "kot", &["animals"]);
    let f = fields(&a.c, builtin::basic());
    let edit =
        a.c.edit_note_with(added.id, &[(f[0], "pies")], None)
            .unwrap();
    assert_eq!(edit.old_tags, None);
    assert_eq!(a.c.note_tags(added.id).unwrap(), vec!["animals"]);
}

#[test]
fn a_bad_tag_refuses_the_whole_edit() {
    let (a, _) = pair();
    let added = add(&a.c, "kot", &["animals"]);
    let f = fields(&a.c, builtin::basic());
    let before = digest(&a.c);
    let rec = Recorder::attach(&a.c);
    let result =
        a.c.edit_note_with(added.id, &[(f[0], "pies")], Some(&["two words"]));
    assert!(matches!(result, Err(NoteError::Tag(_))));
    assert_eq!(digest(&a.c), before);
    assert!(rec.take().is_empty());
}

#[test]
fn an_edit_of_a_deleted_or_missing_note_is_refused() {
    let (a, _) = pair();
    let added = add(&a.c, "kot", &[]);
    a.c.delete_note(added.id).unwrap();
    assert_eq!(
        a.c.edit_note_with(added.id, &[], Some(&["x"])),
        Err(NoteError::NotFound)
    );
    assert_eq!(
        a.c.edit_note_with(missing(), &[], None),
        Err(NoteError::NotFound)
    );
}

#[test]
fn an_edit_that_fills_a_template_makes_its_card() {
    let (a, _) = pair();
    let f = fields(&a.c, builtin::basic_and_reversed());
    let added =
        a.c.add_note(builtin::basic_and_reversed(), &[(f[0], "kot")])
            .unwrap();
    assert_eq!(added.cards.len(), 1);
    let edit =
        a.c.edit_note_with(added.id, &[(f[1], "cat")], Some(&["x"]))
            .unwrap();
    assert_eq!(edit.change.added_cards.len(), 1);
    assert_eq!(edit.old_values, vec![(f[1], String::new())]);
}

#[test]
fn a_replay_of_an_edit_gives_the_same_collection() {
    let (a, b) = pair();
    let added = add(&a.c, "kot", &["animals"]);
    let f = fields(&a.c, builtin::basic());
    a.c.edit_note_with(added.id, &[(f[0], "pies")], Some(&["pl"]))
        .unwrap();
    b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
    assert_eq!(digest(&a.c), digest(&b.c));
}

// ---- delete_notes and restore_notes ---------------------------------------------------------------

#[test]
fn deleting_notes_is_one_write_with_an_event_per_note() {
    let (a, _) = pair();
    let one = add(&a.c, "one", &[]);
    let two = add_reversed(&a.c, "two");
    let rec = Recorder::attach(&a.c);
    let done = a.c.delete_notes(&[one.id, two.id, one.id]).unwrap();
    assert_eq!(done.notes, vec![one.id, two.id]);
    let mut cards = one.cards.clone();
    cards.extend(two.cards.iter().copied());
    let mut got = done.cards.clone();
    got.sort();
    cards.sort();
    assert_eq!(got, cards);
    assert_eq!(
        rec.one(),
        vec![
            Event::NoteDeleted { note: one.id },
            Event::NoteDeleted { note: two.id }
        ]
    );
    assert!(a.c.note(one.id).unwrap().unwrap().deleted);
    assert!(a.c.cards_of_note(two.id).unwrap().is_empty());
}

#[test]
fn a_deleted_note_is_skipped_and_a_missing_one_refuses_the_whole_delete() {
    let (a, _) = pair();
    let one = add(&a.c, "one", &[]);
    let two = add(&a.c, "two", &[]);
    a.c.delete_note(one.id).unwrap();
    let before = digest(&a.c);
    assert_eq!(
        a.c.delete_notes(&[two.id, missing()]),
        Err(NoteError::NotFound)
    );
    assert_eq!(digest(&a.c), before);
    let done = a.c.delete_notes(&[one.id, two.id]).unwrap();
    assert_eq!(done.notes, vec![two.id]);
    assert_eq!(a.c.delete_notes(&[one.id]).unwrap(), NotesChange::default());
}

#[test]
fn restoring_notes_brings_the_same_cards_back_in_one_write() {
    let (a, _) = pair();
    let one = add(&a.c, "one", &[]);
    let two = add_reversed(&a.c, "two");
    let live = add(&a.c, "three", &[]);
    a.c.delete_notes(&[one.id, two.id]).unwrap();
    let rec = Recorder::attach(&a.c);
    let done = a.c.restore_notes(&[one.id, two.id, live.id]).unwrap();
    assert_eq!(done.notes, vec![one.id, two.id]);
    assert_eq!(done.cards.len(), 3);
    let events = rec.one();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], Event::NoteRestored { note, .. } if note == one.id));
    let back: Vec<Id> =
        a.c.cards_of_note(two.id)
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect();
    assert_eq!(back, two.cards);
    let before = digest(&a.c);
    assert_eq!(a.c.restore_notes(&[missing()]), Err(NoteError::NotFound));
    assert_eq!(digest(&a.c), before);
}

#[test]
fn a_replay_of_a_bulk_delete_and_restore_gives_the_same_collection() {
    let (a, b) = pair();
    let one = add(&a.c, "one", &[]);
    let two = add_reversed(&a.c, "two");
    a.c.delete_notes(&[one.id, two.id]).unwrap();
    a.c.restore_notes(&[two.id]).unwrap();
    b.c.merge(&a.c.changes(Selection::All).unwrap()).unwrap();
    assert_eq!(digest(&a.c), digest(&b.c));
}

// ---- What the bulk operations answer --------------------------------------------------------------

#[test]
fn moving_cards_answers_each_moved_card_with_its_old_deck() {
    let (a, _) = pair();
    let one = add(&a.c, "one", &[]);
    let two = add_reversed(&a.c, "two");
    let verbs = a.c.create_deck("Verbs", None).unwrap();
    a.c.move_cards(&[two.cards[0]], verbs).unwrap();
    let mut cards = vec![one.cards[0], two.cards[0], one.cards[0]];
    cards.extend(two.cards.iter().copied());
    let moved = a.c.move_cards(&cards, verbs).unwrap();
    assert_eq!(
        moved,
        vec![
            (one.cards[0], default_deck()),
            (two.cards[1], default_deck())
        ]
    );
    let back = a.c.move_cards(&[two.cards[0]], default_deck()).unwrap();
    assert_eq!(back, vec![(two.cards[0], verbs)]);
}

#[test]
fn tagging_answers_the_pairs_that_changed() {
    let (a, _) = pair();
    let one = add(&a.c, "one", &["pl"]);
    let two = add(&a.c, "two", &[]);
    let mut added = a.c.add_tags(&[one.id, two.id], &["PL", "verbs"]).unwrap();
    added.sort();
    let mut wanted = vec![
        (one.id, "verbs".to_owned()),
        (two.id, "pl".to_owned()),
        (two.id, "verbs".to_owned()),
    ];
    wanted.sort();
    assert_eq!(added, wanted);
    let removed = a.c.remove_tags(&[one.id], &["verbs", "nothing"]).unwrap();
    assert_eq!(removed, vec![(one.id, "verbs".to_owned())]);
}

#[test]
fn suspending_answers_the_cards_that_changed() {
    let (a, _) = pair();
    let two = add_reversed(&a.c, "two");
    assert_eq!(
        a.c.suspend_cards(&[two.cards[0]]).unwrap(),
        vec![two.cards[0]]
    );
    assert_eq!(a.c.suspend_cards(&two.cards).unwrap(), vec![two.cards[1]]);
    assert_eq!(
        a.c.unsuspend_cards(&[two.cards[1], two.cards[1]]).unwrap(),
        vec![two.cards[1]]
    );
}

// ---- Notes to cards and back, and every match of a search -----------------------------------------

#[test]
fn cards_of_notes_and_notes_of_cards() {
    let (a, _) = pair();
    let one = add(&a.c, "one", &[]);
    let two = add_reversed(&a.c, "two");
    let mut sorted = two.cards.clone();
    sorted.sort();
    let mut wanted = one.cards.clone();
    wanted.extend(sorted);
    assert_eq!(
        a.c.cards_of_notes(&[one.id, two.id, one.id]).unwrap(),
        wanted
    );
    assert_eq!(
        a.c.notes_of_cards(&[two.cards[1], one.cards[0], two.cards[0]])
            .unwrap(),
        vec![two.id, one.id]
    );
    a.c.delete_note(one.id).unwrap();
    assert_eq!(a.c.cards_of_notes(&[one.id]), Err(NoteError::NotFound));
    assert_eq!(a.c.cards_of_notes(&[missing()]), Err(NoteError::NotFound));
    assert_eq!(
        a.c.notes_of_cards(&[one.cards[0]]),
        Err(NoteError::NotFound)
    );
}

#[test]
fn search_ids_answers_every_match_in_either_mode() {
    let (a, _) = pair();
    let one = add(&a.c, "one", &["pl"]);
    let two = add_reversed(&a.c, "two");
    add(&a.c, "three", &[]);
    let mut notes = vec![one.id, two.id];
    notes.sort();
    assert_eq!(a.c.search_ids("tag:pl or two", Mode::Notes).unwrap(), notes);
    let mut cards = one.cards.clone();
    cards.extend(two.cards.iter().copied());
    cards.sort();
    assert_eq!(a.c.search_ids("tag:pl or two", Mode::Cards).unwrap(), cards);
    assert!(
        a.c.search_ids("nothing-matches", Mode::Cards)
            .unwrap()
            .is_empty()
    );
    assert!(a.c.search_ids("(", Mode::Cards).is_err());
}
