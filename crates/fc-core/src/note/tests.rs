use std::sync::Arc;

use rusqlite::types::Value;

use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, MIGRATIONS, Schema};
use crate::notetype::{Kind, builtin};
use crate::sync::{DYNAMIC_TABLES, SYNCED_TABLES, check_schema};

const START: i64 = 1_700_000_000_000;

fn host(clock: &Arc<ManualClock>) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes(*b"installation-one"),
    }
}

fn collection() -> Collection {
    let clock = Arc::new(ManualClock::new(START));
    Collection::create(":memory:", host(&clock)).unwrap()
}

fn field_ids(collection: &Collection, note_type: Id) -> Vec<Id> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    found.fields.iter().map(|f| f.id).collect()
}

fn template_ids(collection: &Collection, note_type: Id) -> Vec<Id> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    found.templates.iter().map(|t| t.id).collect()
}

/// Adds a Basic note.
fn basic(collection: &Collection, front: &str, back: &str) -> Id {
    let fields = field_ids(collection, builtin::basic());
    collection
        .add_note(builtin::basic(), &[(fields[0], front), (fields[1], back)])
        .unwrap()
        .id
}

fn reversed(collection: &Collection, front: &str, back: &str) -> Id {
    let fields = field_ids(collection, builtin::basic_and_reversed());
    collection
        .add_note(
            builtin::basic_and_reversed(),
            &[(fields[0], front), (fields[1], back)],
        )
        .unwrap()
        .id
}

fn cloze(collection: &Collection, text: &str) -> Result<AddedNote, NoteError> {
    let fields = field_ids(collection, builtin::cloze());
    collection.add_note(builtin::cloze(), &[(fields[0], text)])
}

fn card_ids(collection: &Collection, note: Id) -> Vec<Id> {
    collection
        .cards_of_note(note)
        .unwrap()
        .iter()
        .map(|c| c.id)
        .collect()
}

fn ordinals(collection: &Collection, note: Id) -> Vec<u32> {
    collection
        .cards_of_note(note)
        .unwrap()
        .iter()
        .map(|c| c.ordinal)
        .collect()
}

fn count(collection: &Collection, table: &str) -> i64 {
    collection
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

// Which cards a note makes.

#[test]
fn a_basic_note_makes_one_card_with_the_deterministic_id() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let added = collection
        .add_note(builtin::basic(), &[(fields[0], "pies"), (fields[1], "dog")])
        .unwrap();
    let template = template_ids(&collection, builtin::basic())[0];
    assert_eq!(added.cards, [card_id(added.id, template, 0)]);
    assert_eq!(card_ids(&collection, added.id), added.cards);
    assert!(added.duplicates.is_empty());
    let note = collection.note(added.id).unwrap().unwrap();
    assert_eq!(note.note_type, builtin::basic());
    assert_eq!(note.fields[0].name, "Front");
    assert_eq!(note.fields[0].value, "pies");
    assert!(!note.deleted);
}

#[test]
fn card_ids_depend_on_the_note_the_template_and_the_ordinal_only() {
    let (a, b) = (Id::from_bytes([1; 16]), Id::from_bytes([2; 16]));
    let (t, u) = (Id::from_bytes([3; 16]), Id::from_bytes([4; 16]));
    assert_eq!(card_id(a, t, 0), card_id(a, t, 0));
    assert_ne!(card_id(a, t, 0), card_id(b, t, 0));
    assert_ne!(card_id(a, t, 0), card_id(a, u, 0));
    assert_ne!(card_id(a, t, 0), card_id(a, t, 1));
    assert_ne!(card_id(a, t, 1), card_id(a, t, 256));
}

#[test]
fn a_back_that_is_empty_still_makes_the_basic_card_but_an_empty_front_makes_none() {
    let collection = collection();
    let note = basic(&collection, "pies", "");
    assert_eq!(card_ids(&collection, note).len(), 1);
    let fields = field_ids(&collection, builtin::basic());
    let refused = collection.add_note(builtin::basic(), &[(fields[1], "dog")]);
    assert_eq!(refused, Err(NoteError::NoCards { cloze: false }));
    let blank = collection.add_note(builtin::basic(), &[(fields[0], "<br> &nbsp;")]);
    assert_eq!(blank, Err(NoteError::NoCards { cloze: false }));
    assert_eq!(
        count(&collection, "note"),
        1,
        "a refused note writes nothing"
    );
    assert_eq!(count(&collection, "card"), 1);
}

#[test]
fn a_field_holding_only_an_image_counts_as_filled() {
    let collection = collection();
    let note = basic(&collection, "<img src=\"a.png\">", "");
    assert_eq!(card_ids(&collection, note).len(), 1);
}

#[test]
fn basic_and_reversed_makes_two_cards_in_template_order_and_the_reverse_needs_the_back() {
    let collection = collection();
    let both = reversed(&collection, "pies", "dog");
    let templates = template_ids(&collection, builtin::basic_and_reversed());
    let cards = collection.cards_of_note(both).unwrap();
    assert_eq!(
        cards.iter().map(|c| c.template).collect::<Vec<_>>(),
        templates
    );
    let only_front = reversed(&collection, "kot", "");
    assert_eq!(card_ids(&collection, only_front).len(), 1);
}

#[test]
fn filling_and_emptying_a_field_adds_and_removes_the_card_and_it_returns_with_its_id() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic_and_reversed());
    let note = reversed(&collection, "pies", "");
    assert_eq!(card_ids(&collection, note).len(), 1);

    let change = collection
        .set_note_fields(note, &[(fields[1], "dog")])
        .unwrap();
    assert_eq!(change.added_cards.len(), 1);
    assert!(change.removed_cards.is_empty());
    let both = card_ids(&collection, note);
    assert_eq!(both.len(), 2);
    let reverse = change.added_cards[0];
    assert!(both.contains(&reverse));

    let change = collection
        .set_note_fields(note, &[(fields[1], "")])
        .unwrap();
    assert_eq!(change.removed_cards, [reverse]);
    assert_eq!(card_ids(&collection, note).len(), 1);
    let gone = collection.deleted_cards_of_note(note).unwrap();
    assert_eq!(gone.iter().map(|c| c.id).collect::<Vec<_>>(), [reverse]);

    let change = collection
        .set_note_fields(note, &[(fields[1], "hound")])
        .unwrap();
    assert_eq!(change.added_cards, [reverse], "the same card comes back");
    assert_eq!(card_ids(&collection, note), both);
    assert!(collection.deleted_cards_of_note(note).unwrap().is_empty());
    assert_eq!(count(&collection, "card"), 2, "no second card row was made");
}

#[test]
fn an_optional_reverse_card_follows_the_flag_field() {
    let collection = collection();
    let note_type = collection
        .create_note_type("Optional", Kind::Standard)
        .unwrap();
    let flag = collection.add_field(note_type, "Add reverse").unwrap();
    collection
        .add_template(
            note_type,
            "Reverse",
            "{{#Add reverse}}{{Back}}{{/Add reverse}}",
            "{{FrontSide}}{{Front}}",
        )
        .unwrap();
    let fields = field_ids(&collection, note_type);
    let note = collection
        .add_note(note_type, &[(fields[0], "pies"), (fields[1], "dog")])
        .unwrap()
        .id;
    assert_eq!(card_ids(&collection, note).len(), 1);
    collection.set_note_fields(note, &[(flag, "y")]).unwrap();
    assert_eq!(card_ids(&collection, note).len(), 2);
    collection.set_note_fields(note, &[(flag, "")]).unwrap();
    assert_eq!(card_ids(&collection, note).len(), 1);
}

#[test]
fn a_cloze_note_makes_one_card_per_cloze_number() {
    let collection = collection();
    let added = cloze(
        &collection,
        "{{c1::Warszawa}} is in {{c2::Polska}}, {{c1::yes}}",
    )
    .unwrap();
    assert_eq!(ordinals(&collection, added.id), [1, 2]);
    let template = template_ids(&collection, builtin::cloze())[0];
    assert_eq!(
        added.cards,
        [
            card_id(added.id, template, 1),
            card_id(added.id, template, 2)
        ]
    );
}

#[test]
fn editing_cloze_numbers_adds_removes_and_restores_cards() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::cloze());
    let added = cloze(&collection, "{{c1::a}} {{c2::b}}").unwrap();
    let (one, two) = (added.cards[0], added.cards[1]);

    let change = collection
        .set_note_fields(added.id, &[(fields[0], "{{c1::a}} b {{c3::c}}")])
        .unwrap();
    assert_eq!(change.removed_cards, [two]);
    assert_eq!(change.added_cards.len(), 1);
    assert_eq!(ordinals(&collection, added.id), [1, 3]);

    let change = collection
        .set_note_fields(added.id, &[(fields[0], "{{c1::a}} {{c2::b}} {{c3::c}}")])
        .unwrap();
    assert_eq!(change.added_cards, [two], "card 2 comes back as itself");
    assert_eq!(ordinals(&collection, added.id), [1, 2, 3]);
    assert!(card_ids(&collection, added.id).contains(&one));
}

#[test]
fn cloze_markers_outside_the_cloze_field_make_no_cards_and_a_note_without_any_is_refused() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::cloze());
    let refused = collection.add_note(
        builtin::cloze(),
        &[(fields[0], "plain text"), (fields[1], "{{c1::extra}}")],
    );
    assert_eq!(refused, Err(NoteError::NoCards { cloze: true }));
    assert!(refused.unwrap_err().to_string().contains("{{c1::"));
    let added = cloze(&collection, "{{c1::a}}").unwrap();
    // Editing away the last cloze is allowed: a merge could do the same. The card goes to the trash.
    let change = collection
        .set_note_fields(added.id, &[(fields[0], "no cloze now")])
        .unwrap();
    assert_eq!(change.removed_cards.len(), 1);
    assert!(card_ids(&collection, added.id).is_empty());
}

#[test]
fn a_cloze_number_over_the_limit_is_plain_text() {
    let collection = collection();
    let added = cloze(&collection, "{{c1::a}} {{c501::b}} {{c500::c}}").unwrap();
    assert_eq!(ordinals(&collection, added.id), [1, 500]);
}

// Editing.

fn clock_of(collection: &Collection, note: Id, field: Id) -> crate::sync::RegisterClock {
    collection
        .register_clock("note", note, &field.to_string())
        .unwrap()
        .unwrap()
}

#[test]
fn only_a_value_that_changes_is_written() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let note = basic(&collection, "pies", "dog");
    let (front, back) = (
        clock_of(&collection, note, fields[0]),
        clock_of(&collection, note, fields[1]),
    );

    collection
        .set_note_fields(note, &[(fields[0], "pies"), (fields[1], "dog")])
        .unwrap();
    assert_eq!(clock_of(&collection, note, fields[0]), front);
    assert_eq!(clock_of(&collection, note, fields[1]), back);

    collection
        .set_note_fields(note, &[(fields[0], "pies"), (fields[1], "hound")])
        .unwrap();
    assert_eq!(
        clock_of(&collection, note, fields[0]),
        front,
        "the other field is left alone"
    );
    assert!(clock_of(&collection, note, fields[1]).hlc > back.hlc);
    assert!(!clock_of(&collection, note, fields[1]).pushed);
}

#[test]
fn an_empty_value_that_was_never_set_is_not_stored() {
    let collection = collection();
    let note = basic(&collection, "pies", "");
    assert_eq!(count(&collection, "note_field_value"), 1);
    let back = field_ids(&collection, builtin::basic())[1];
    collection.set_note_fields(note, &[(back, "")]).unwrap();
    assert_eq!(count(&collection, "note_field_value"), 1);
}

#[test]
fn a_value_for_a_field_the_note_type_does_not_have_is_refused_and_nothing_is_written() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let note = basic(&collection, "pies", "dog");
    let before = count(&collection, "note_field_value");
    let stranger = Id::from_bytes([9; 16]);
    assert_eq!(
        collection.set_note_fields(note, &[(fields[0], "kot"), (stranger, "x")]),
        Err(NoteError::UnknownField)
    );
    assert_eq!(
        collection.add_note(builtin::basic(), &[(stranger, "x")]),
        Err(NoteError::UnknownField)
    );
    assert_eq!(count(&collection, "note_field_value"), before);
    assert_eq!(
        collection.note(note).unwrap().unwrap().fields[0].value,
        "pies"
    );

    collection
        .remove_field(builtin::basic(), fields[1])
        .unwrap();
    assert_eq!(
        collection.set_note_fields(note, &[(fields[1], "x")]),
        Err(NoteError::UnknownField),
        "a removed field cannot be edited"
    );
}

#[test]
fn a_deleted_or_missing_note_or_note_type_is_not_found() {
    let collection = collection();
    let note = basic(&collection, "pies", "dog");
    let front = field_ids(&collection, builtin::basic())[0];
    collection.delete_note(note).unwrap();
    assert_eq!(
        collection.set_note_fields(note, &[(front, "x")]),
        Err(NoteError::NotFound)
    );
    assert_eq!(
        collection.set_note_fields(Id::from_bytes([7; 16]), &[]),
        Err(NoteError::NotFound)
    );
    collection.delete_note_type(builtin::basic()).unwrap();
    assert_eq!(
        collection.add_note(builtin::basic(), &[(front, "x")]),
        Err(NoteError::NotFound)
    );
}

// Duplicates.

#[test]
fn a_duplicate_first_field_is_a_warning_not_a_block() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let first = basic(&collection, "pies", "dog");
    let second = collection
        .add_note(
            builtin::basic(),
            &[(fields[0], "  <b>Pies</b>&nbsp;"), (fields[1], "hound")],
        )
        .unwrap();
    assert_eq!(second.duplicates, [first]);
    assert_eq!(
        card_ids(&collection, second.id).len(),
        1,
        "the note was added"
    );
    assert_eq!(collection.notes(builtin::basic()).unwrap().len(), 2);
}

#[test]
fn duplicates_are_by_note_type_and_first_field_only() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    basic(&collection, "pies", "dog");
    // The same front in another note type, and the same back in this one, are not duplicates.
    let reversed_fields = field_ids(&collection, builtin::basic_and_reversed());
    let other = collection
        .add_note(
            builtin::basic_and_reversed(),
            &[(reversed_fields[0], "pies"), (reversed_fields[1], "dog")],
        )
        .unwrap();
    assert!(other.duplicates.is_empty());
    let same_back = collection
        .add_note(builtin::basic(), &[(fields[0], "kot"), (fields[1], "dog")])
        .unwrap();
    assert!(same_back.duplicates.is_empty());
    assert!(
        collection
            .find_duplicates(builtin::basic(), "")
            .unwrap()
            .is_empty()
    );
    assert!(
        collection
            .find_duplicates(builtin::basic(), "<br>")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        collection
            .find_duplicates(builtin::basic(), "PIES")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn deleted_notes_are_not_duplicates_and_editing_reports_them_only_for_the_first_field() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic());
    let old = basic(&collection, "pies", "dog");
    let note = basic(&collection, "kot", "cat");

    let to_pies = collection
        .set_note_fields(note, &[(fields[0], "pies")])
        .unwrap();
    assert_eq!(to_pies.duplicates, [old]);
    let back_only = collection
        .set_note_fields(note, &[(fields[1], "feline")])
        .unwrap();
    assert!(
        back_only.duplicates.is_empty(),
        "the first field did not change"
    );

    collection.delete_note(old).unwrap();
    let again = collection
        .set_note_fields(note, &[(fields[0], "pies2")])
        .unwrap();
    assert!(again.duplicates.is_empty());
    assert!(
        collection
            .find_duplicates(builtin::basic(), "pies")
            .unwrap()
            .is_empty()
    );
}

// Deleting and restoring.

#[test]
fn deleting_a_note_deletes_its_cards_and_restoring_brings_the_same_cards_back() {
    let collection = collection();
    let note = reversed(&collection, "pies", "dog");
    let cards = card_ids(&collection, note);
    collection.delete_note(note).unwrap();
    assert!(card_ids(&collection, note).is_empty());
    assert_eq!(collection.deleted_notes().unwrap().len(), 1);
    assert!(
        collection
            .notes(builtin::basic_and_reversed())
            .unwrap()
            .is_empty()
    );
    collection.delete_note(note).unwrap();

    let change = collection.restore_note(note).unwrap();
    assert_eq!(change.added_cards.len(), 2);
    assert_eq!(card_ids(&collection, note), cards);
    assert_eq!(count(&collection, "card"), 2);
    assert_eq!(
        collection.restore_note(note).unwrap(),
        NoteChange::default()
    );
}

#[test]
fn a_card_removed_before_the_note_was_deleted_stays_removed_on_restore() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic_and_reversed());
    let note = reversed(&collection, "pies", "dog");
    collection
        .set_note_fields(note, &[(fields[1], "")])
        .unwrap();
    collection.delete_note(note).unwrap();
    collection.restore_note(note).unwrap();
    assert_eq!(card_ids(&collection, note).len(), 1);
}

#[test]
fn deleting_a_note_type_deletes_its_notes_and_cards_and_restoring_it_brings_back_only_those() {
    let collection = collection();
    let kept = basic(&collection, "kot", "cat");
    let trashed = basic(&collection, "pies", "dog");
    let other_type = reversed(&collection, "dom", "house");
    collection.delete_note(trashed).unwrap();

    collection.delete_note_type(builtin::basic()).unwrap();
    assert!(card_ids(&collection, kept).is_empty());
    assert!(collection.notes(builtin::basic()).unwrap().is_empty());
    assert_eq!(collection.deleted_notes().unwrap().len(), 2);
    assert!(
        collection
            .deleted_note_types()
            .unwrap()
            .iter()
            .any(|t| t.id == builtin::basic())
    );
    assert_eq!(
        card_ids(&collection, other_type).len(),
        2,
        "other types are untouched"
    );
    for card in collection.deleted_cards_of_note(kept).unwrap() {
        assert!(card.deleted);
    }

    collection.restore_note_type(builtin::basic()).unwrap();
    assert_eq!(card_ids(&collection, kept).len(), 1);
    assert!(
        collection.note(trashed).unwrap().unwrap().deleted,
        "deleted on its own, before"
    );
    assert!(card_ids(&collection, trashed).is_empty());
    assert_eq!(collection.notes(builtin::basic()).unwrap().len(), 1);
}

#[test]
fn a_live_note_keeps_a_deleted_note_type_alive_and_deleting_again_removes_it() {
    let collection = collection();
    let note = basic(&collection, "kot", "cat");
    collection.delete_note_type(builtin::basic()).unwrap();
    assert!(
        collection
            .note_types()
            .unwrap()
            .iter()
            .all(|t| t.id != builtin::basic())
    );

    // A note arrives that was added on another device while this one deleted the type.
    let stray = Id::from_bytes([8; 16]);
    collection
        .write(|w| {
            w.insert(
                "note",
                stray,
                vec![
                    (
                        "note_type",
                        Value::Blob(builtin::basic().as_bytes().to_vec()),
                    ),
                    ("deleted", Value::Integer(0)),
                ],
            )
        })
        .unwrap();
    let alive = collection.note_type(builtin::basic()).unwrap().unwrap();
    assert!(!alive.deleted, "a live note keeps its note type alive");
    assert!(
        collection
            .note_types()
            .unwrap()
            .iter()
            .any(|t| t.id == builtin::basic())
    );
    assert!(
        collection
            .deleted_note_types()
            .unwrap()
            .iter()
            .all(|t| t.id != builtin::basic())
    );
    // Nothing was written to bring it back: the register still says deleted.
    let stored: i64 = collection
        .conn
        .query_row(
            "SELECT deleted FROM note_type WHERE id = ?1",
            [builtin::basic()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, 1);
    assert!(collection.note(note).unwrap().unwrap().deleted);

    // It can be used and edited while alive, and deleting it again takes the stray note with it.
    collection
        .rename_note_type(builtin::basic(), "Kept")
        .unwrap();
    collection.delete_note_type(builtin::basic()).unwrap();
    assert!(collection.note(stray).unwrap().unwrap().deleted);
    assert!(
        collection
            .note_types()
            .unwrap()
            .iter()
            .all(|t| t.id != builtin::basic())
    );
}

// Changes to the note type.

#[test]
fn adding_a_template_makes_cards_for_the_notes_that_exist() {
    let collection = collection();
    let with_back = basic(&collection, "pies", "dog");
    let without_back = basic(&collection, "kot", "");
    collection
        .add_template(builtin::basic(), "Reverse", "{{Back}}", "{{Front}}")
        .unwrap();
    assert_eq!(card_ids(&collection, with_back).len(), 2);
    assert_eq!(card_ids(&collection, without_back).len(), 1);
}

#[test]
fn removing_a_template_deletes_its_cards_and_restoring_it_brings_them_back() {
    let collection = collection();
    let note = reversed(&collection, "pies", "dog");
    let cards = card_ids(&collection, note);
    let second = template_ids(&collection, builtin::basic_and_reversed())[1];
    collection
        .remove_template(builtin::basic_and_reversed(), second)
        .unwrap();
    assert_eq!(card_ids(&collection, note), cards[..1]);
    collection
        .restore_template(builtin::basic_and_reversed(), second)
        .unwrap();
    assert_eq!(card_ids(&collection, note), cards);
}

#[test]
fn changing_template_text_changes_which_notes_have_the_card() {
    let collection = collection();
    let note = basic(&collection, "pies", "");
    let template = template_ids(&collection, builtin::basic())[0];
    collection
        .set_template_text(builtin::basic(), template, "{{Back}}", "{{Front}}")
        .unwrap();
    assert!(
        card_ids(&collection, note).is_empty(),
        "the front now needs Back"
    );
    collection
        .set_template_text(builtin::basic(), template, "{{Front}}", "{{Back}}")
        .unwrap();
    assert_eq!(card_ids(&collection, note).len(), 1);
}

#[test]
fn removing_a_field_removes_the_cards_that_needed_it_and_restoring_it_brings_them_back() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic_and_reversed());
    let note = reversed(&collection, "pies", "dog");
    let cards = card_ids(&collection, note);
    collection
        .remove_field(builtin::basic_and_reversed(), fields[1])
        .unwrap();
    assert_eq!(
        card_ids(&collection, note),
        cards[..1],
        "the reverse needs the hidden Back"
    );
    collection
        .restore_field(builtin::basic_and_reversed(), fields[1])
        .unwrap();
    assert_eq!(card_ids(&collection, note), cards);
}

#[test]
fn renaming_a_field_rewrites_the_templates_so_no_card_disappears() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic_and_reversed());
    let note = reversed(&collection, "pies", "dog");
    let cards = card_ids(&collection, note);
    let note_type = builtin::basic_and_reversed();
    let second = template_ids(&collection, note_type)[1];
    collection.remove_template(note_type, second).unwrap();

    collection
        .rename_field(note_type, fields[1], "Answer")
        .unwrap();

    let found = collection.note_type(note_type).unwrap().unwrap();
    assert_eq!(found.templates.len(), 1);
    assert_eq!(
        found.templates[0].back,
        "{{FrontSide}}\n\n<hr id=answer>\n\n{{Answer}}"
    );
    assert_eq!(card_ids(&collection, note), cards[..1]);
    // The removed template was rewritten too, so restoring it still works.
    collection.restore_template(note_type, second).unwrap();
    let found = collection.note_type(note_type).unwrap().unwrap();
    assert_eq!(found.templates[1].front, "{{Answer}}");
    assert_eq!(card_ids(&collection, note), cards);
}

#[test]
fn renaming_a_field_rewrites_filters_and_sections_and_leaves_other_text_alone() {
    let collection = collection();
    let note_type = collection
        .create_note_type("Custom", Kind::Standard)
        .unwrap();
    let fields = field_ids(&collection, note_type);
    let template = template_ids(&collection, note_type)[0];
    collection
        .set_template_text(
            note_type,
            template,
            "{{#Front}}{{hint:Front}} Front{{/Front}}",
            "{{FrontSide}} {{ Back }}",
        )
        .unwrap();
    collection
        .rename_field(note_type, fields[0], "Word")
        .unwrap();
    let found = collection.note_type(note_type).unwrap().unwrap();
    assert_eq!(
        found.templates[0].front,
        "{{#Word}}{{hint:Word}} Front{{/Word}}"
    );
    assert_eq!(found.templates[0].back, "{{FrontSide}} {{ Back }}");
}

// How the tables are kept.

#[test]
fn every_register_of_a_note_and_a_card_has_a_clock() {
    let collection = collection();
    let fields = field_ids(&collection, builtin::basic_and_reversed());
    let note = reversed(&collection, "pies", "dog");
    collection
        .set_note_fields(note, &[(fields[1], "")])
        .unwrap();
    collection
        .set_note_fields(note, &[(fields[1], "hound")])
        .unwrap();
    collection.delete_note(note).unwrap();
    collection.restore_note(note).unwrap();
    cloze(&collection, "{{c1::a}}").unwrap();

    for table in [NOTE, CARD] {
        let ids: Vec<Id> = collection
            .conn
            .prepare(&format!("SELECT id FROM {}", table.table))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(!ids.is_empty());
        for id in ids {
            for register in table.registers {
                assert!(
                    collection
                        .register_clock(table.entity, id, register)
                        .unwrap()
                        .is_some(),
                    "{} {id} has no clock for {register}",
                    table.entity
                );
            }
        }
    }
    let values: Vec<(Id, Id)> = collection
        .conn
        .prepare("SELECT note, field FROM note_field_value")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!values.is_empty());
    for (note, field) in values {
        assert!(
            collection
                .register_clock("note", note, &field.to_string())
                .unwrap()
                .is_some(),
            "value {note}/{field} has no clock"
        );
    }
}

#[test]
fn raw_sql_cannot_write_the_note_and_card_tables() {
    let collection = collection();
    let note = basic(&collection, "pies", "dog");
    for sql in [
        "INSERT INTO note (id) VALUES (x'01')",
        "UPDATE note SET deleted = 1",
        "DELETE FROM note",
        "INSERT INTO note_field_value (note, field, value) VALUES (x'01', x'02', 'x')",
        "UPDATE note_field_value SET value = 'x'",
        "DELETE FROM note_field_value",
        "INSERT INTO card (id) VALUES (x'01')",
        "UPDATE card SET deleted = 1",
        "DELETE FROM card",
    ] {
        assert!(collection.conn.execute(sql, []).is_err(), "{sql}");
    }
    assert_eq!(
        collection.note(note).unwrap().unwrap().fields[0].value,
        "pies"
    );
}

#[test]
fn the_real_schema_is_valid_with_the_note_tables_declared() {
    let collection = collection();
    check_schema(&collection.conn, SYNCED_TABLES).unwrap();
    assert!(SYNCED_TABLES.iter().any(|t| t.table == NOTE.table));
    assert!(SYNCED_TABLES.iter().any(|t| t.table == CARD.table));
    assert!(DYNAMIC_TABLES.iter().any(|t| t.table == NOTE_VALUE.table));
    let exists: i64 = collection
        .conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'note_field_value'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(exists, 1);
}

#[test]
fn a_note_value_register_is_known_but_other_names_for_notes_are_not() {
    let collection = collection();
    let field = field_ids(&collection, builtin::basic())[0];
    assert!(collection.knows_register("note", &field.to_string()));
    assert!(collection.knows_register("note", "deleted"));
    assert!(!collection.knows_register("note", "colour"));
    assert!(!collection.knows_register("card", &field.to_string()));
}

#[test]
fn two_collections_make_the_same_cards_for_the_same_note() {
    let (a, b) = (collection(), collection());
    let note = Id::from_bytes([6; 16]);
    let field = field_ids(&a, builtin::basic_and_reversed());
    assert_eq!(field, field_ids(&b, builtin::basic_and_reversed()));
    for side in [&a, &b] {
        side.write(|w| {
            w.insert(
                "note",
                note,
                vec![
                    (
                        "note_type",
                        Value::Blob(builtin::basic_and_reversed().as_bytes().to_vec()),
                    ),
                    ("deleted", Value::Integer(0)),
                ],
            )?;
            w.set_value("note", note, field[0], "pies")?;
            w.set_value("note", note, field[1], "dog")
        })
        .unwrap();
        side.set_note_fields(note, &[]).unwrap();
    }
    assert_eq!(card_ids(&a, note).len(), 2);
    assert_eq!(card_ids(&a, note), card_ids(&b, note));
}

#[test]
fn a_version_3_collection_upgrades_to_notes_and_cards() {
    const V3: Schema = Schema {
        migrations: &[MIGRATIONS[0], MIGRATIONS[1], MIGRATIONS[2]],
        tables: &[
            SYNCED_TABLES[0],
            SYNCED_TABLES[1],
            SYNCED_TABLES[2],
            SYNCED_TABLES[3],
        ],
    };
    let path = std::env::temp_dir().join(format!("fc-note-v3-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = Arc::new(ManualClock::new(START));
    let old = Collection::create_with(location, V3, host(&clock)).unwrap();
    old.create_note_type("Mine", Kind::Standard).unwrap();
    old.close().unwrap();

    let upgraded = Collection::open(location, host(&clock)).unwrap();
    let info = upgraded.info().unwrap();
    assert_eq!(info.schema_version, 10);
    assert_eq!(
        upgraded.note_types().unwrap().len(),
        4,
        "the built-ins and Mine are kept"
    );
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    let note = basic(&upgraded, "pies", "dog");
    assert_eq!(card_ids(&upgraded, note).len(), 1);
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&path);
}
