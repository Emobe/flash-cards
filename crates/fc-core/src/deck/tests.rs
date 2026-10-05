use std::sync::Arc;

use rusqlite::types::Value;

use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, MIGRATIONS, Schema};
use crate::note::NoteError;
use crate::notetype::builtin;
use crate::sync::{SYNCED_TABLES, check_schema};

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

fn fields(collection: &Collection, note_type: Id) -> Vec<Id> {
    let found = collection.note_type(note_type).unwrap().unwrap();
    found.fields.iter().map(|f| f.id).collect()
}

/// Adds a Basic note to a deck and returns the note and its card.
fn basic_in(collection: &Collection, deck: Id, front: &str) -> (Id, Id) {
    let f = fields(collection, builtin::basic());
    let added = collection
        .add_note_to_deck(deck, builtin::basic(), &[(f[0], front), (f[1], "back")])
        .unwrap();
    (added.id, added.cards[0])
}

/// A Basic and reversed note with both cards in `deck`.
fn reversed_in(collection: &Collection, deck: Id, front: &str) -> (Id, Vec<Id>) {
    let f = fields(collection, builtin::basic_and_reversed());
    let added = collection
        .add_note_to_deck(
            deck,
            builtin::basic_and_reversed(),
            &[(f[0], front), (f[1], "back")],
        )
        .unwrap();
    (added.id, added.cards)
}

fn paths(collection: &Collection) -> Vec<String> {
    collection
        .decks()
        .unwrap()
        .into_iter()
        .map(|d| d.path)
        .collect()
}

fn live_cards(collection: &Collection, note: Id) -> Vec<Id> {
    collection
        .cards_of_note(note)
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect()
}

fn deck_of(collection: &Collection, note: Id) -> Vec<Id> {
    collection
        .cards_of_note(note)
        .unwrap()
        .into_iter()
        .map(|c| c.deck)
        .collect()
}

fn remote_device() -> Id {
    Id::from_bytes(*b"remote-device-01")
}

/// What a merge will do in step 1.11: write a register with the clock it had on another device.
/// `at` is a time in ms, so a later `at` is a later write.
fn remote_set(c: &Collection, entity: &str, id: Id, field: &str, value: Value, at: i64) {
    c.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    c.conn
        .execute(
            &format!("UPDATE {entity} SET {field} = ?1 WHERE id = ?2"),
            rusqlite::params![value, id],
        )
        .unwrap();
    c.conn.execute("DELETE FROM write_guard", []).unwrap();
    c.conn
        .execute(
            "INSERT OR REPLACE INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            rusqlite::params![entity, id, field, at << 16, remote_device()],
        )
        .unwrap();
}

fn parent_value(parent: Option<Id>) -> Value {
    Value::Blob(parent.map_or_else(Vec::new, |p| p.as_bytes().to_vec()))
}

/// A deck made on another device.
fn remote_deck(c: &Collection, id: Id, name: &str, parent: Option<Id>, at: i64) {
    c.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    c.conn
        .execute(
            "INSERT INTO deck (id, name, parent, options_preset, deleted) VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![id, name, parent_value(parent), default_preset()],
        )
        .unwrap();
    c.conn.execute("DELETE FROM write_guard", []).unwrap();
    for field in DECK.registers {
        c.conn
            .execute(
                "INSERT OR REPLACE INTO register_clock (entity_type, entity_id, field, hlc, device, pushed)
                 VALUES ('deck', ?1, ?2, ?3, ?4, 1)",
                rusqlite::params![id, field, at << 16, remote_device()],
            )
            .unwrap();
    }
}

fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}

// The Default deck and preset.

#[test]
fn a_new_collection_has_the_default_deck_and_preset() {
    let c = collection();
    let decks = c.decks().unwrap();
    assert_eq!(decks.len(), 1);
    assert_eq!(decks[0].id, default_deck());
    assert_eq!(decks[0].name, "Default");
    assert_eq!(decks[0].parent, None);
    assert_eq!(decks[0].preset, default_preset());
    let presets = c.presets().unwrap();
    assert_eq!(presets.len(), 1);
    let preset = &presets[0];
    assert_eq!(preset.id, default_preset());
    assert_eq!(preset.name, "Default");
    assert_eq!(preset.new_per_day, 20);
    assert_eq!(preset.reviews_per_day, 200);
    assert_eq!(preset.learning_steps, vec![1, 10]);
    assert_eq!(preset.desired_retention, 0.9);
    assert_eq!(preset.decks, 1);
    assert_eq!(c.deck_preset(default_deck()).unwrap(), *preset);
}

#[test]
fn two_collections_made_separately_share_the_default_deck_and_preset() {
    assert_eq!(default_deck(), collection().decks().unwrap()[0].id);
    assert_eq!(default_preset(), collection().presets().unwrap()[0].id);
    assert_ne!(default_deck(), default_preset());
}

#[test]
fn the_default_deck_and_preset_have_the_lowest_clock_and_nothing_to_push() {
    let c = collection();
    let clock = c
        .register_clock("deck", default_deck(), "name")
        .unwrap()
        .unwrap();
    assert_eq!(clock.hlc.to_stored(), 0);
    assert!(clock.pushed);
    let clock = c
        .register_clock("options_preset", default_preset(), "new_per_day")
        .unwrap()
        .unwrap();
    assert_eq!(clock.hlc.to_stored(), 0);
}

#[test]
fn the_default_deck_and_preset_cannot_be_deleted_but_can_be_renamed() {
    let c = collection();
    assert_eq!(c.delete_deck(default_deck()), Err(DeckError::Default));
    assert_eq!(c.delete_preset(default_preset()), Err(DeckError::Default));
    c.rename_deck(default_deck(), "Inbox").unwrap();
    c.rename_preset(default_preset(), "Standard").unwrap();
    assert_eq!(c.decks().unwrap()[0].name, "Inbox");
    assert_eq!(c.presets().unwrap()[0].name, "Standard");
    assert!(c.deleted_decks().unwrap().is_empty());
}

#[test]
fn a_card_with_no_deck_stored_is_in_the_default_deck() {
    let c = collection();
    let (note, card) = basic_in(&c, default_deck(), "a");
    // A card from before decks: the register holds nothing.
    remote_set(&c, "card", card, "deck", Value::Blob(Vec::new()), 5);
    assert_eq!(deck_of(&c, note), vec![default_deck()]);
    assert_eq!(c.decks().unwrap()[0].cards, 1);
}

// Creating, renaming and moving.

#[test]
fn decks_nest_and_are_listed_in_tree_order_by_name() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let verbs = c.create_deck("verbs", Some(polish)).unwrap();
    let nouns = c.create_deck("Nouns", Some(polish)).unwrap();
    let past = c.create_deck("Past", Some(verbs)).unwrap();
    c.create_deck("Archive", None).unwrap();

    assert_eq!(
        paths(&c),
        [
            "Archive",
            "Default",
            "Polish",
            "Polish::Nouns",
            "Polish::verbs",
            "Polish::verbs::Past",
        ]
    );
    let decks = c.decks().unwrap();
    let depth = |id: Id| decks.iter().find(|d| d.id == id).unwrap().depth;
    assert_eq!(
        [depth(polish), depth(nouns), depth(verbs), depth(past)],
        [0, 1, 1, 2]
    );
    assert_eq!(
        decks.iter().find(|d| d.id == past).unwrap().parent,
        Some(verbs)
    );
}

#[test]
fn a_new_deck_uses_the_default_preset() {
    let c = collection();
    let deck = c.create_deck("Polish", None).unwrap();
    assert_eq!(c.deck_preset(deck).unwrap().id, default_preset());
}

#[test]
fn deck_names_are_checked_with_a_message_that_says_what_to_do() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    assert_eq!(c.create_deck("  ", None), Err(DeckError::EmptyName));
    assert_eq!(
        c.create_deck("a::b", None),
        Err(DeckError::NameHasSeparator)
    );
    assert_eq!(
        c.create_deck("polish", None),
        Err(DeckError::NameTaken("polish".to_owned()))
    );
    // The same name under another parent is fine.
    c.create_deck("Polish", Some(polish)).unwrap();
    assert_eq!(
        c.create_deck("x", Some(id(9))),
        Err(DeckError::NotFound),
        "a parent that does not exist"
    );
    let other = c.create_deck("Other", None).unwrap();
    assert_eq!(
        c.rename_deck(other, "POLISH"),
        Err(DeckError::NameTaken("POLISH".to_owned()))
    );
    assert_eq!(
        c.rename_deck(other, "a::b"),
        Err(DeckError::NameHasSeparator)
    );
    assert!(
        DeckError::NameHasSeparator
            .to_string()
            .contains("To put a deck inside another, move it.")
    );
    assert!(DeckError::MoveIntoItself.to_string().contains("Choose"));
    assert_eq!(c.rename_deck(id(9), "x"), Err(DeckError::NotFound));
}

#[test]
fn names_are_trimmed_and_a_deleted_parent_is_refused() {
    let c = collection();
    let polish = c.create_deck("  Polish ", None).unwrap();
    assert_eq!(c.deck(polish).unwrap().unwrap().name, "Polish");
    c.delete_deck(polish).unwrap();
    assert_eq!(c.create_deck("x", Some(polish)), Err(DeckError::NotFound));
}

#[test]
fn renaming_to_the_same_name_writes_nothing() {
    let c = collection();
    let deck = c.create_deck("Polish", None).unwrap();
    let before = c.register_clock("deck", deck, "name").unwrap();
    c.rename_deck(deck, "Polish").unwrap();
    assert_eq!(c.register_clock("deck", deck, "name").unwrap(), before);
    c.rename_deck(deck, "Polski").unwrap();
    assert_ne!(c.register_clock("deck", deck, "name").unwrap(), before);
    assert_eq!(paths(&c), ["Default", "Polski"]);
}

#[test]
fn moving_a_deck_writes_only_that_deck_and_takes_its_sub_decks_along() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", None).unwrap();
    let child = c.create_deck("Child", Some(a)).unwrap();
    let child_clock = c.register_clock("deck", child, "parent").unwrap();

    c.move_deck(a, Some(b)).unwrap();
    assert_eq!(paths(&c), ["B", "B::A", "B::A::Child", "Default"]);
    assert_eq!(
        c.register_clock("deck", child, "parent").unwrap(),
        child_clock,
        "the sub-deck was not written"
    );
    c.move_deck(a, None).unwrap();
    assert_eq!(paths(&c), ["A", "A::Child", "B", "Default"]);
}

#[test]
fn a_deck_cannot_be_moved_inside_itself_or_its_sub_decks() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", Some(a)).unwrap();
    let cc = c.create_deck("C", Some(b)).unwrap();
    assert_eq!(c.move_deck(a, Some(a)), Err(DeckError::MoveIntoItself));
    assert_eq!(c.move_deck(a, Some(cc)), Err(DeckError::MoveIntoItself));
    assert_eq!(paths(&c), ["A", "A::B", "A::B::C", "Default"]);
}

#[test]
fn a_move_that_would_clash_with_a_sibling_name_is_refused() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let inner = c.create_deck("Same", Some(a)).unwrap();
    let outer = c.create_deck("Same", None).unwrap();
    assert_eq!(
        c.move_deck(outer, Some(a)),
        Err(DeckError::NameTaken("Same".to_owned()))
    );
    assert_eq!(
        c.move_deck(inner, None),
        Err(DeckError::NameTaken("Same".to_owned()))
    );
    // Moving to where it already is does nothing, and is not a clash with itself.
    c.move_deck(inner, Some(a)).unwrap();
}

// Deleting and restoring.

#[test]
fn deleting_a_deck_deletes_its_sub_decks_and_their_cards_and_keeps_the_history() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let verbs = c.create_deck("Verbs", Some(polish)).unwrap();
    let (n1, c1) = basic_in(&c, polish, "one");
    let (n2, c2) = basic_in(&c, verbs, "two");
    let (n3, c3) = basic_in(&c, default_deck(), "three");

    c.delete_deck(polish).unwrap();

    assert_eq!(paths(&c), ["Default"]);
    let deleted: Vec<String> = c
        .deleted_decks()
        .unwrap()
        .into_iter()
        .map(|d| d.path)
        .collect();
    assert_eq!(deleted, ["Polish", "Polish::Verbs"]);
    assert!(c.note(n1).unwrap().unwrap().deleted);
    assert!(c.note(n2).unwrap().unwrap().deleted);
    assert!(!c.note(n3).unwrap().unwrap().deleted);
    assert_eq!(live_cards(&c, n3), vec![c3]);
    // The cards are in the trash, not gone.
    let stored: i64 = c
        .conn
        .query_row(
            "SELECT count(*) FROM card WHERE id IN (?1, ?2)",
            [c1, c2],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, 2);
    assert_eq!(c.deleted_cards_of_note(n1).unwrap()[0].id, c1);
}

#[test]
fn deleting_a_deck_twice_does_nothing_and_a_missing_deck_is_not_found() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    c.delete_deck(a).unwrap();
    let clock = c.register_clock("deck", a, "deleted").unwrap();
    c.delete_deck(a).unwrap();
    assert_eq!(c.register_clock("deck", a, "deleted").unwrap(), clock);
    assert_eq!(c.delete_deck(id(9)), Err(DeckError::NotFound));
}

#[test]
fn a_note_with_a_card_in_another_deck_stays_when_one_deck_is_deleted() {
    let c = collection();
    let b = c.create_deck("B", None).unwrap();
    let (note, cards) = reversed_in(&c, default_deck(), "front");
    c.move_cards(&cards[1..], b).unwrap();
    assert_eq!(deck_of(&c, note), vec![default_deck(), b]);

    c.delete_deck(b).unwrap();

    assert!(!c.note(note).unwrap().unwrap().deleted);
    assert_eq!(live_cards(&c, note), vec![cards[0]]);
    assert_eq!(c.deleted_cards_of_note(note).unwrap()[0].id, cards[1]);
}

#[test]
fn editing_a_note_does_not_bring_back_a_card_from_a_deleted_deck() {
    let c = collection();
    let b = c.create_deck("B", None).unwrap();
    let (note, cards) = reversed_in(&c, default_deck(), "front");
    c.move_cards(&cards[1..], b).unwrap();
    c.delete_deck(b).unwrap();

    let f = fields(&c, builtin::basic_and_reversed());
    let change = c.set_note_fields(note, &[(f[0], "front!")]).unwrap();

    assert!(change.added_cards.is_empty());
    assert_eq!(live_cards(&c, note), vec![cards[0]]);
}

#[test]
fn changing_the_note_type_does_not_bring_back_a_card_from_a_deleted_deck() {
    let c = collection();
    let b = c.create_deck("B", None).unwrap();
    let (note, cards) = reversed_in(&c, default_deck(), "front");
    c.move_cards(&cards[1..], b).unwrap();
    c.delete_deck(b).unwrap();

    let f = fields(&c, builtin::basic_and_reversed());
    c.add_field(builtin::basic_and_reversed(), "Extra").unwrap();
    c.rename_field(builtin::basic_and_reversed(), f[1], "Answer")
        .unwrap();

    assert_eq!(live_cards(&c, note), vec![cards[0]]);
}

#[test]
fn restoring_a_deck_brings_back_what_was_deleted_with_it() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let verbs = c.create_deck("Verbs", Some(polish)).unwrap();
    let (n1, c1) = basic_in(&c, polish, "one");
    let (n2, c2) = basic_in(&c, verbs, "two");
    let (alone, _) = basic_in(&c, polish, "alone");
    c.delete_note(alone).unwrap();
    c.delete_deck(polish).unwrap();

    c.restore_deck(polish).unwrap();

    assert_eq!(paths(&c), ["Default", "Polish", "Polish::Verbs"]);
    assert_eq!(live_cards(&c, n1), vec![c1], "same card, same ID");
    assert_eq!(live_cards(&c, n2), vec![c2]);
    assert!(
        c.note(alone).unwrap().unwrap().deleted,
        "a note deleted on its own before stays in the trash"
    );
    assert_eq!(c.decks().unwrap()[1].cards, 1);
}

#[test]
fn restoring_a_deck_that_is_not_deleted_does_nothing_and_a_name_clash_is_refused() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    c.restore_deck(a).unwrap();
    c.delete_deck(a).unwrap();
    c.create_deck("A", None).unwrap();
    assert_eq!(c.restore_deck(a), Err(DeckError::NameTaken("A".to_owned())));
}

#[test]
fn deleting_a_deck_that_holds_the_default_deck_moves_the_default_deck_out() {
    let c = collection();
    let outer = c.create_deck("Outer", None).unwrap();
    let below = c.create_deck("Below", Some(default_deck())).unwrap();
    c.move_deck(default_deck(), Some(outer)).unwrap();
    let (kept, _) = basic_in(&c, default_deck(), "kept");
    let (also_kept, _) = basic_in(&c, below, "below");
    let (lost, _) = basic_in(&c, outer, "lost");

    c.delete_deck(outer).unwrap();

    assert_eq!(paths(&c), ["Default", "Default::Below"]);
    assert!(!c.note(kept).unwrap().unwrap().deleted);
    assert!(!c.note(also_kept).unwrap().unwrap().deleted);
    assert!(c.note(lost).unwrap().unwrap().deleted);
}

// Cards in decks.

#[test]
fn add_note_goes_to_the_default_deck_and_add_note_to_deck_to_that_deck() {
    let c = collection();
    let f = fields(&c, builtin::basic());
    let plain = c.add_note(builtin::basic(), &[(f[0], "a")]).unwrap();
    assert_eq!(deck_of(&c, plain.id), vec![default_deck()]);
    let deck = c.create_deck("Polish", None).unwrap();
    let (note, _) = basic_in(&c, deck, "b");
    assert_eq!(deck_of(&c, note), vec![deck]);
    assert_eq!(c.decks().unwrap()[1].cards, 1);

    c.delete_deck(deck).unwrap();
    assert_eq!(
        c.add_note_to_deck(deck, builtin::basic(), &[(f[0], "c")]),
        Err(NoteError::NotFound)
    );
    assert_eq!(
        c.add_note_to_deck(id(9), builtin::basic(), &[(f[0], "c")]),
        Err(NoteError::NotFound)
    );
}

#[test]
fn a_card_made_by_an_edit_goes_to_the_deck_of_the_notes_other_cards() {
    let c = collection();
    let deck = c.create_deck("Polish", None).unwrap();
    let f = fields(&c, builtin::basic_and_reversed());
    // No Back, so only card 1 exists.
    let added = c
        .add_note_to_deck(deck, builtin::basic_and_reversed(), &[(f[0], "front")])
        .unwrap();
    assert_eq!(added.cards.len(), 1);
    c.set_note_fields(added.id, &[(f[1], "back")]).unwrap();
    assert_eq!(deck_of(&c, added.id), vec![deck, deck]);

    // Moved to another deck first, the new card follows the card it has.
    let other = c.create_deck("Other", None).unwrap();
    let (note, card) = {
        let added = c
            .add_note_to_deck(deck, builtin::basic_and_reversed(), &[(f[0], "x")])
            .unwrap();
        (added.id, added.cards[0])
    };
    c.move_cards(&[card], other).unwrap();
    c.set_note_fields(note, &[(f[1], "y")]).unwrap();
    assert_eq!(deck_of(&c, note), vec![other, other]);
}

#[test]
fn moving_cards_changes_their_deck_and_writes_only_what_changes() {
    let c = collection();
    let deck = c.create_deck("Polish", None).unwrap();
    let (_, in_default) = basic_in(&c, default_deck(), "a");
    let (_, already) = basic_in(&c, deck, "b");
    let before = c.register_clock("card", already, "deck").unwrap();

    c.move_cards(&[in_default, already], deck).unwrap();

    assert_eq!(c.decks().unwrap()[1].cards, 2);
    assert_eq!(c.register_clock("card", already, "deck").unwrap(), before);
    c.move_cards(&[in_default], default_deck()).unwrap();
    assert_eq!(c.decks().unwrap()[0].cards, 1);
}

#[test]
fn moving_cards_needs_a_live_deck_and_live_cards_and_changes_nothing_on_failure() {
    let c = collection();
    let deck = c.create_deck("Polish", None).unwrap();
    let (note, card) = basic_in(&c, default_deck(), "a");
    let (gone, gone_card) = basic_in(&c, default_deck(), "b");
    c.delete_note(gone).unwrap();

    assert_eq!(
        c.move_cards(&[card, gone_card], deck),
        Err(DeckError::NotFound)
    );
    assert_eq!(deck_of(&c, note), vec![default_deck()], "nothing moved");
    assert_eq!(c.move_cards(&[card], id(9)), Err(DeckError::NotFound));
    c.delete_deck(deck).unwrap();
    assert_eq!(c.move_cards(&[card], deck), Err(DeckError::NotFound));
}

// What a merge can leave behind (ADR 0006, sections 5 and 6).

#[test]
fn a_cycle_is_broken_by_ignoring_the_newest_move() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", None).unwrap();
    // Device one moves A under B, device two moves B under A, a little later.
    remote_set(&c, "deck", a, "parent", parent_value(Some(b)), 10);
    remote_set(&c, "deck", b, "parent", parent_value(Some(a)), 20);
    // B's move is the newest, so it is ignored: B at the top, A inside it.
    assert_eq!(paths(&c), ["B", "B::A", "Default"]);
}

#[test]
fn a_cycle_breaks_at_the_other_place_when_the_other_move_is_newer() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", None).unwrap();
    remote_set(&c, "deck", a, "parent", parent_value(Some(b)), 30);
    remote_set(&c, "deck", b, "parent", parent_value(Some(a)), 20);
    // A's move is the newest now, so A is at the top and B is inside it.
    assert_eq!(paths(&c), ["A", "A::B", "Default"]);
}

#[test]
fn a_longer_cycle_and_a_deck_that_is_its_own_parent_show_every_deck_once() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", None).unwrap();
    let cc = c.create_deck("C", None).unwrap();
    let own = c.create_deck("Own", None).unwrap();
    remote_set(&c, "deck", a, "parent", parent_value(Some(b)), 10);
    remote_set(&c, "deck", b, "parent", parent_value(Some(cc)), 11);
    remote_set(&c, "deck", cc, "parent", parent_value(Some(a)), 12);
    remote_set(&c, "deck", own, "parent", parent_value(Some(own)), 13);
    // C's move is the newest, so C is at the top: C, then B inside C, then A inside B.
    assert_eq!(paths(&c), ["C", "C::B", "C::B::A", "Default", "Own"]);
}

#[test]
fn a_parent_that_has_not_arrived_shows_the_deck_at_the_top_level() {
    let c = collection();
    remote_deck(&c, id(1), "Orphan", Some(id(99)), 10);
    assert_eq!(paths(&c), ["Default", "Orphan"]);
    // When the parent arrives, the deck is inside it.
    remote_deck(&c, id(99), "Parent", None, 11);
    assert_eq!(paths(&c), ["Default", "Parent", "Parent::Orphan"]);
}

#[test]
fn decks_with_the_same_name_are_shown_with_numbers_in_id_order() {
    let c = collection();
    remote_deck(&c, id(3), "Polish", None, 10);
    remote_deck(&c, id(1), "Polish", None, 11);
    remote_deck(&c, id(2), "polish", None, 12);
    remote_deck(&c, id(4), "Verbs", Some(id(3)), 13);
    let decks = c.decks().unwrap();
    let shown: Vec<(&str, &str)> = decks
        .iter()
        .map(|d| (d.name.as_str(), d.display_name.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Default", "Default"),
            ("Polish", "Polish"),
            ("polish", "polish (2)"),
            ("Polish", "Polish (3)"),
            ("Verbs", "Verbs"),
        ]
    );
    assert_eq!(decks[4].path, "Polish (3)::Verbs");
    let unique: std::collections::HashSet<_> = decks.iter().map(|d| &d.path).collect();
    assert_eq!(unique.len(), decks.len(), "every path is different");
}

#[test]
fn a_deleted_deck_that_holds_a_live_card_is_alive_again() {
    let c = collection();
    let deck = c.create_deck("Polish", None).unwrap();
    // Another device deletes the deck while a card is moved into it.
    let (note, card) = basic_in(&c, default_deck(), "a");
    remote_set(&c, "deck", deck, "deleted", Value::Integer(1), 10);
    assert_eq!(paths(&c), ["Default"]);
    remote_set(
        &c,
        "card",
        card,
        "deck",
        Value::Blob(deck.as_bytes().to_vec()),
        11,
    );
    assert_eq!(paths(&c), ["Default", "Polish"]);
    assert!(c.deleted_decks().unwrap().is_empty());
    assert_eq!(deck_of(&c, note), vec![deck]);
    // It can be edited, and deleting it again tombstones the card that kept it alive.
    c.rename_deck(deck, "Polski").unwrap();
    c.delete_deck(deck).unwrap();
    assert_eq!(paths(&c), ["Default"]);
    assert!(c.note(note).unwrap().unwrap().deleted);
}

#[test]
fn a_deleted_deck_with_a_live_sub_deck_keeps_its_parents_alive_too() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", Some(a)).unwrap();
    let cc = c.create_deck("C", Some(b)).unwrap();
    remote_set(&c, "deck", a, "deleted", Value::Integer(1), 10);
    remote_set(&c, "deck", b, "deleted", Value::Integer(1), 11);
    assert_eq!(paths(&c), ["A", "A::B", "A::B::C", "Default"]);
    c.delete_deck(cc).unwrap();
    assert_eq!(paths(&c), ["Default"]);
    assert_eq!(c.deleted_decks().unwrap().len(), 3);
}

#[test]
fn restoring_a_deck_inside_a_deleted_deck_shows_the_parent_again() {
    let c = collection();
    let a = c.create_deck("A", None).unwrap();
    let b = c.create_deck("B", Some(a)).unwrap();
    c.delete_deck(a).unwrap();
    c.restore_deck(b).unwrap();
    assert_eq!(paths(&c), ["A", "A::B", "Default"]);
}

#[test]
fn reading_decks_never_fails_whatever_merged_in() {
    // A small deterministic fuzz: decks with random parents (including themselves, cycles and
    // missing decks) and random clocks, inserted in two different orders, always give every deck
    // once, a finite path, and the same answer.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for _ in 0..200 {
        let count = 1 + next(6) as u8;
        let specs: Vec<(u8, Option<u8>, i64)> = (1..=count)
            .map(|n| {
                let parent = match next(8) {
                    0 => None,
                    p if p <= u64::from(count) => Some(p as u8),
                    _ => Some(200),
                };
                (n, parent, 10 + next(5) as i64)
            })
            .collect();
        let build = |order: Vec<&(u8, Option<u8>, i64)>| {
            let c = collection();
            for (n, parent, at) in order {
                remote_deck(&c, id(*n), &format!("D{}", n % 2), parent.map(id), *at);
            }
            c.decks().unwrap()
        };
        let forward = build(specs.iter().collect());
        let backward = build(specs.iter().rev().collect());
        assert_eq!(forward.len(), usize::from(count) + 1);
        assert_eq!(forward, backward, "{specs:?}");
        let unique: std::collections::HashSet<_> = forward.iter().map(|d| &d.path).collect();
        assert_eq!(unique.len(), forward.len(), "{specs:?}");
    }
}

// Presets.

#[test]
fn presets_are_created_renamed_and_listed_by_name() {
    let c = collection();
    let hard = c.create_preset("Hard").unwrap();
    c.create_preset("Easy").unwrap();
    let names: Vec<String> = c.presets().unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["Default", "Easy", "Hard"]);
    let made = c.preset(hard).unwrap().unwrap();
    assert_eq!(
        (
            made.new_per_day,
            made.reviews_per_day,
            made.desired_retention
        ),
        (20, 200, 0.9)
    );
    assert_eq!(made.learning_steps, vec![1, 10]);
    assert_eq!(made.decks, 0);
    c.rename_preset(hard, "Harder").unwrap();
    assert_eq!(c.preset(hard).unwrap().unwrap().name, "Harder");
    assert_eq!(c.create_preset(" "), Err(DeckError::EmptyName));
    assert_eq!(
        c.create_preset("easy"),
        Err(DeckError::NameTaken("easy".to_owned()))
    );
    assert_eq!(
        c.rename_preset(hard, "EASY"),
        Err(DeckError::NameTaken("EASY".to_owned()))
    );
    assert_eq!(c.rename_preset(id(9), "x"), Err(DeckError::NotFound));
}

#[test]
fn preset_options_are_changed_one_register_at_a_time() {
    let c = collection();
    let preset = c.create_preset("Mine").unwrap();
    let steps_clock = c
        .register_clock("options_preset", preset, "learning_steps")
        .unwrap();
    c.set_preset_options(
        preset,
        &PresetChange {
            new_per_day: Some(5),
            reviews_per_day: Some(0),
            desired_retention: Some(0.85),
            ..PresetChange::default()
        },
    )
    .unwrap();
    let got = c.preset(preset).unwrap().unwrap();
    assert_eq!(got.new_per_day, 5);
    assert_eq!(got.reviews_per_day, 0);
    assert_eq!(got.desired_retention, 0.85);
    assert_eq!(got.learning_steps, vec![1, 10]);
    assert_eq!(
        c.register_clock("options_preset", preset, "learning_steps")
            .unwrap(),
        steps_clock,
        "a register that was not changed was not written"
    );

    c.set_preset_options(
        preset,
        &PresetChange {
            learning_steps: Some(vec![1, 5, 30, 1440]),
            ..PresetChange::default()
        },
    )
    .unwrap();
    assert_eq!(
        c.preset(preset).unwrap().unwrap().learning_steps,
        vec![1, 5, 30, 1440]
    );
    c.set_preset_options(
        preset,
        &PresetChange {
            learning_steps: Some(vec![]),
            ..PresetChange::default()
        },
    )
    .unwrap();
    assert!(c.preset(preset).unwrap().unwrap().learning_steps.is_empty());
}

#[test]
fn writing_the_value_a_preset_option_already_has_writes_nothing() {
    let c = collection();
    let preset = c.create_preset("Mine").unwrap();
    let before = c
        .register_clock("options_preset", preset, "new_per_day")
        .unwrap();
    c.set_preset_options(
        preset,
        &PresetChange {
            new_per_day: Some(20),
            ..PresetChange::default()
        },
    )
    .unwrap();
    assert_eq!(
        c.register_clock("options_preset", preset, "new_per_day")
            .unwrap(),
        before
    );
}

#[test]
fn bad_option_values_are_refused_and_nothing_is_changed() {
    let c = collection();
    let preset = c.create_preset("Mine").unwrap();
    let bad = |change: PresetChange| c.set_preset_options(preset, &change);
    let good_and = |mut change: PresetChange| {
        change.new_per_day = Some(7);
        change
    };
    assert_eq!(
        bad(good_and(PresetChange {
            desired_retention: Some(0.69),
            ..PresetChange::default()
        })),
        Err(DeckError::Retention)
    );
    for retention in [1.0, 0.0, f64::NAN, -1.0] {
        assert_eq!(
            bad(PresetChange {
                desired_retention: Some(retention),
                ..PresetChange::default()
            }),
            Err(DeckError::Retention)
        );
    }
    for retention in [0.7, 0.99] {
        bad(PresetChange {
            desired_retention: Some(retention),
            ..PresetChange::default()
        })
        .unwrap();
    }
    assert_eq!(
        bad(PresetChange {
            new_per_day: Some(10_000),
            ..PresetChange::default()
        }),
        Err(DeckError::Limit)
    );
    assert_eq!(
        bad(PresetChange {
            reviews_per_day: Some(10_000),
            ..PresetChange::default()
        }),
        Err(DeckError::Limit)
    );
    bad(PresetChange {
        new_per_day: Some(9999),
        ..PresetChange::default()
    })
    .unwrap();
    for steps in [vec![0], vec![1441], vec![1; 9]] {
        assert_eq!(
            bad(good_and(PresetChange {
                learning_steps: Some(steps),
                ..PresetChange::default()
            })),
            Err(DeckError::Steps)
        );
    }
    assert_eq!(
        c.preset(preset).unwrap().unwrap().new_per_day,
        9999,
        "the failures changed nothing"
    );
    assert_eq!(
        c.set_preset_options(id(9), &PresetChange::default()),
        Err(DeckError::NotFound)
    );
    for error in [DeckError::Retention, DeckError::Limit, DeckError::Steps] {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn several_decks_can_share_a_preset_and_a_change_shows_in_all_of_them() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let spanish = c.create_deck("Spanish", None).unwrap();
    let preset = c.create_preset("Languages").unwrap();
    c.set_deck_preset(polish, preset).unwrap();
    c.set_deck_preset(spanish, preset).unwrap();
    assert_eq!(c.preset(preset).unwrap().unwrap().decks, 2);
    assert_eq!(c.preset(default_preset()).unwrap().unwrap().decks, 1);

    c.set_preset_options(
        preset,
        &PresetChange {
            new_per_day: Some(3),
            ..PresetChange::default()
        },
    )
    .unwrap();

    assert_eq!(c.deck_preset(polish).unwrap().new_per_day, 3);
    assert_eq!(c.deck_preset(spanish).unwrap().new_per_day, 3);
    assert_eq!(c.deck_preset(default_deck()).unwrap().new_per_day, 20);
    // Giving one deck another preset leaves the other where it was.
    c.set_deck_preset(spanish, default_preset()).unwrap();
    assert_eq!(c.deck_preset(spanish).unwrap().new_per_day, 20);
    assert_eq!(c.deck_preset(polish).unwrap().new_per_day, 3);
    assert_eq!(c.set_deck_preset(polish, id(9)), Err(DeckError::NotFound));
    assert_eq!(c.set_deck_preset(id(9), preset), Err(DeckError::NotFound));
}

#[test]
fn deleting_a_preset_sends_its_decks_to_the_default_preset() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let preset = c.create_preset("Mine").unwrap();
    c.set_deck_preset(polish, preset).unwrap();
    c.delete_preset(preset).unwrap();

    assert_eq!(c.deck_preset(polish).unwrap().id, default_preset());
    assert_eq!(c.presets().unwrap().len(), 1);
    assert_eq!(c.deleted_presets().unwrap()[0].name, "Mine");
    // Again does nothing, and a missing preset is not found.
    c.delete_preset(preset).unwrap();
    assert_eq!(c.delete_preset(id(9)), Err(DeckError::NotFound));
    // The deck is not given a deleted preset.
    assert_eq!(c.set_deck_preset(polish, preset), Err(DeckError::NotFound));
}

#[test]
fn restoring_a_preset_brings_it_back_without_its_decks() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let preset = c.create_preset("Mine").unwrap();
    c.set_deck_preset(polish, preset).unwrap();
    c.delete_preset(preset).unwrap();

    c.restore_preset(preset).unwrap();

    assert_eq!(c.presets().unwrap().len(), 2);
    assert_eq!(c.deck_preset(polish).unwrap().id, default_preset());
    c.restore_preset(preset).unwrap();
    c.delete_preset(preset).unwrap();
    c.create_preset("mine").unwrap();
    assert_eq!(
        c.restore_preset(preset),
        Err(DeckError::NameTaken("Mine".to_owned()))
    );
}

#[test]
fn a_deleted_preset_that_a_live_deck_uses_is_alive_again() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let preset = c.create_preset("Mine").unwrap();
    // Another device deletes the preset while the deck is given it.
    remote_set(
        &c,
        "options_preset",
        preset,
        "deleted",
        Value::Integer(1),
        10,
    );
    assert_eq!(c.presets().unwrap().len(), 1);
    remote_set(
        &c,
        "deck",
        polish,
        "options_preset",
        Value::Blob(preset.as_bytes().to_vec()),
        11,
    );
    assert_eq!(c.presets().unwrap().len(), 2);
    assert_eq!(c.deck_preset(polish).unwrap().id, preset);
    // Deleting it again sends the deck to the Default preset.
    c.delete_preset(preset).unwrap();
    assert_eq!(c.deck_preset(polish).unwrap().id, default_preset());
}

#[test]
fn a_deck_whose_preset_is_missing_uses_the_default_preset() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    remote_set(
        &c,
        "deck",
        polish,
        "options_preset",
        Value::Blob(id(77).as_bytes().to_vec()),
        10,
    );
    assert_eq!(c.deck_preset(polish).unwrap().id, default_preset());
    assert_eq!(c.decks().unwrap()[1].preset, default_preset());
    remote_set(
        &c,
        "deck",
        polish,
        "options_preset",
        Value::Blob(Vec::new()),
        11,
    );
    assert_eq!(c.deck_preset(polish).unwrap().id, default_preset());
}

#[test]
fn values_from_another_app_are_read_in_range() {
    let c = collection();
    let preset = c.create_preset("Mine").unwrap();
    remote_set(
        &c,
        "options_preset",
        preset,
        "desired_retention",
        Value::Real(5.0),
        10,
    );
    remote_set(
        &c,
        "options_preset",
        preset,
        "new_per_day",
        Value::Integer(-3),
        10,
    );
    remote_set(
        &c,
        "options_preset",
        preset,
        "reviews_per_day",
        Value::Integer(1_000_000),
        10,
    );
    remote_set(
        &c,
        "options_preset",
        preset,
        "learning_steps",
        Value::Text("0 5 x 99999 15  30".to_owned()),
        10,
    );
    let got = c.preset(preset).unwrap().unwrap();
    assert_eq!(got.desired_retention, 0.99);
    assert_eq!(got.new_per_day, 0);
    assert_eq!(got.reviews_per_day, 9999);
    assert_eq!(got.learning_steps, vec![5, 15, 30]);
    remote_set(
        &c,
        "options_preset",
        preset,
        "desired_retention",
        Value::Real(0.1),
        11,
    );
    assert_eq!(c.preset(preset).unwrap().unwrap().desired_retention, 0.7);
}

// Sync discipline and migration.

#[test]
fn every_register_of_every_deck_and_preset_has_a_clock() {
    let c = collection();
    let polish = c.create_deck("Polish", None).unwrap();
    let preset = c.create_preset("Mine").unwrap();
    c.set_deck_preset(polish, preset).unwrap();
    c.delete_preset(preset).unwrap();
    let (_, card) = basic_in(&c, polish, "a");
    for (table, ids) in [
        (DECK, vec![default_deck(), polish]),
        (PRESET, vec![default_preset(), preset]),
        (crate::note::CARD, vec![card]),
    ] {
        for id in ids {
            for register in table.registers {
                assert!(
                    c.register_clock(table.entity, id, register)
                        .unwrap()
                        .is_some(),
                    "{} {id} has no clock for {register}",
                    table.entity
                );
            }
        }
    }
}

#[test]
fn the_database_refuses_raw_writes_to_the_new_tables() {
    let c = collection();
    basic_in(&c, default_deck(), "a");
    for sql in [
        "INSERT INTO deck (id) VALUES (x'00000000000000000000000000000001')",
        "UPDATE deck SET name = 'x'",
        "DELETE FROM deck",
        "INSERT INTO options_preset (id) VALUES (x'00000000000000000000000000000001')",
        "UPDATE options_preset SET new_per_day = 1",
        "DELETE FROM options_preset",
        "UPDATE card SET deck = x''",
    ] {
        let error = c.conn.execute(sql, []).unwrap_err().to_string();
        assert!(
            error.contains("write path") || error.contains("hard-deleted"),
            "{sql}: {error}"
        );
    }
}

#[test]
fn the_schema_check_passes_with_the_new_tables() {
    let c = collection();
    check_schema(&c.conn, SYNCED_TABLES).unwrap();
}

#[test]
fn a_version_4_collection_upgrades_and_its_cards_are_in_the_default_deck() {
    const V4: Schema = Schema {
        migrations: &[MIGRATIONS[0], MIGRATIONS[1], MIGRATIONS[2], MIGRATIONS[3]],
        tables: &[
            SYNCED_TABLES[0],
            SYNCED_TABLES[1],
            SYNCED_TABLES[2],
            SYNCED_TABLES[3],
        ],
    };
    let path = std::env::temp_dir().join(format!("fc-deck-v4-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_str().unwrap();
    let clock = Arc::new(ManualClock::new(START));
    let old = Collection::create_with(location, V4, host(&clock)).unwrap();
    // A note and a card as step 1.3 wrote them, with no deck column.
    let note = Id::from_bytes([7; 16]);
    let template = old.note_type(builtin::basic()).unwrap().unwrap().templates[0].id;
    let card = crate::note::card_id(note, template, 0);
    old.conn
        .execute("INSERT INTO write_guard (id) VALUES (1)", [])
        .unwrap();
    old.conn
        .execute(
            "INSERT INTO note (id, note_type, deleted) VALUES (?1, ?2, 0)",
            rusqlite::params![note, builtin::basic()],
        )
        .unwrap();
    old.conn
        .execute(
            "INSERT INTO card (id, note, template, ordinal, deleted) VALUES (?1, ?2, ?3, 0, 0)",
            rusqlite::params![card, note, template],
        )
        .unwrap();
    old.conn.execute("DELETE FROM write_guard", []).unwrap();
    old.close().unwrap();

    let upgraded = Collection::open(location, host(&clock)).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 6);
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    assert_eq!(live_cards(&upgraded, note), vec![card]);
    assert_eq!(deck_of(&upgraded, note), vec![default_deck()]);
    assert_eq!(upgraded.decks().unwrap()[0].cards, 1);
    assert_eq!(upgraded.presets().unwrap()[0].id, default_preset());
    assert_eq!(
        upgraded.note_types().unwrap().len(),
        3,
        "the built-ins are kept"
    );
    // The old card can be moved, and then it has a clock for its deck.
    let deck = upgraded.create_deck("Polish", None).unwrap();
    assert!(
        upgraded
            .register_clock("card", card, "deck")
            .unwrap()
            .is_none()
    );
    upgraded.move_cards(&[card], deck).unwrap();
    assert_eq!(deck_of(&upgraded, note), vec![deck]);
    assert!(
        upgraded
            .register_clock("card", card, "deck")
            .unwrap()
            .is_some()
    );
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&path);
}
