//! The editing and bulk methods through `dispatch`, as every host calls them (steps 3.2a and 3.2d).

use std::sync::Arc;

use fc_core::clock::ManualClock;
use fc_core::collection::Collection;
use fc_core::deck::default_deck;
use fc_core::notetype::builtin;
use fc_core::sync::Selection;
use serde_json::{Value, json};

use super::*;

const T0: i64 = 1_791_363_600_000;

fn host(n: u8, clock: &Arc<ManualClock>) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes([n; 16]),
    }
}

pub(crate) fn core() -> (Core, Arc<ManualClock>) {
    let core = Core::new();
    let clock = Arc::new(ManualClock::new(T0));
    core.open_collection(":memory:", host(1, &clock)).unwrap();
    (core, clock)
}

pub(crate) fn call(core: &Core, method: &str, input: Value) -> Result<Value, ApiError> {
    dispatch(core, method, input, &OpContext::uncancellable()).map(|r| r.output)
}

pub(crate) fn ok(core: &Core, method: &str, input: Value) -> Value {
    call(core, method, input).unwrap_or_else(|e| panic!("{method}: {e:?}"))
}

/// The field IDs of a built-in note type.
pub(crate) fn fields(core: &Core, note_type: Id) -> Vec<String> {
    core.with_collection(|c| {
        c.note_type(note_type)
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id.to_string())
            .collect()
    })
    .unwrap()
}

/// A note with this front (and "back"), as `(note ID, card IDs)`. `reversed` makes two cards.
pub(crate) fn add(
    core: &Core,
    front: &str,
    tags: &[&str],
    reversed: bool,
) -> (String, Vec<String>) {
    let note_type = if reversed {
        builtin::basic_and_reversed()
    } else {
        builtin::basic()
    };
    let f = fields(core, note_type);
    let out = ok(
        core,
        "addNote",
        json!({
            "noteTypeId": note_type.to_string(),
            "deckId": default_deck().to_string(),
            "fields": [{ "fieldId": f[0], "value": front }, { "fieldId": f[1], "value": "back" }],
            "tags": tags,
        }),
    );
    let note = out["noteId"].as_str().unwrap().to_owned();
    let cards = core
        .with_collection(|c| {
            c.cards_of_note(note.parse().unwrap())
                .unwrap()
                .iter()
                .map(|k| k.id.to_string())
                .collect()
        })
        .unwrap();
    (note, cards)
}

fn deck(core: &Core, name: &str) -> String {
    ok(
        core,
        "createDeck",
        json!({ "name": name, "parentId": null }),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn note(core: &Core, id: &str) -> Value {
    ok(core, "getNote", json!({ "noteId": id }))
}

fn undo(core: &Core, token: &Value) -> Result<Value, ApiError> {
    call(core, "undoChange", json!({ "undoId": token["id"] }))
}

fn gone(result: Result<Value, ApiError>) {
    let error = result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(error.message, "That can no longer be undone.");
}

fn decks_of(core: &Core, cards: &[String]) -> Vec<String> {
    core.with_collection(|c| {
        cards
            .iter()
            .map(|card| {
                let id: Id = card.parse().unwrap();
                let note = c.notes_of_cards(&[id]).unwrap()[0];
                c.cards_of_note(note)
                    .unwrap()
                    .into_iter()
                    .find(|k| k.id == id)
                    .unwrap()
                    .deck
                    .to_string()
            })
            .collect()
    })
    .unwrap()
}

fn suspended(core: &Core, query: &str) -> usize {
    ok(
        core,
        "searchIds",
        json!({ "query": format!("is:suspended {query}"), "mode": "cards" }),
    )["ids"]
        .as_array()
        .unwrap()
        .len()
}

// ---- getNote and updateNote -----------------------------------------------------------------------

#[test]
fn get_note_answers_the_fields_tags_and_decks() {
    let (core, _) = core();
    let (id, _) = add(&core, "kot", &["pl", "animals"], true);
    let f = fields(&core, builtin::basic_and_reversed());
    let got = note(&core, &id);
    assert_eq!(got["noteTypeId"], builtin::basic_and_reversed().to_string());
    assert_eq!(
        got["fields"],
        json!([{ "fieldId": f[0], "value": "kot" }, { "fieldId": f[1], "value": "back" }])
    );
    assert_eq!(got["tags"], json!(["animals", "pl"]));
    assert_eq!(got["deckIds"], json!([default_deck().to_string()]));
    assert_eq!(got["cardCount"], 2);

    ok(&core, "deleteNotes", json!({ "target": { "notes": [id] } }));
    let error = call(&core, "getNote", json!({ "noteId": id })).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    let error = call(&core, "getNote", json!({ "noteId": "nope" })).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidInput);
}

#[test]
fn update_note_saves_fields_tags_and_deck_and_one_undo_puts_all_back() {
    let (core, _) = core();
    let (id, cards) = add(&core, "kot", &["pl"], true);
    let verbs = deck(&core, "Verbs");
    let f = fields(&core, builtin::basic_and_reversed());
    let out = ok(
        &core,
        "updateNote",
        json!({
            "noteId": id,
            "fields": [{ "fieldId": f[0], "value": "pies" }, { "fieldId": f[1], "value": "back" }],
            "tags": ["animals"],
            "deckId": verbs,
        }),
    );
    assert_eq!(out["undo"]["message"], "Saved.");
    assert_eq!(out["cardsAdded"], json!([]));
    let got = note(&core, &id);
    assert_eq!(got["fields"][0]["value"], "pies");
    assert_eq!(got["tags"], json!(["animals"]));
    assert_eq!(got["deckIds"], json!([verbs]));

    let message = undo(&core, &out["undo"]).unwrap()["message"].clone();
    assert_eq!(message, "The note is back as it was.");
    let got = note(&core, &id);
    assert_eq!(got["fields"][0]["value"], "kot");
    assert_eq!(got["tags"], json!(["pl"]));
    assert_eq!(decks_of(&core, &cards), vec![default_deck().to_string(); 2]);
    // Once only.
    gone(undo(&core, &out["undo"]));
}

#[test]
fn an_update_that_changes_nothing_has_no_undo_and_keeps_the_last_one() {
    let (core, _) = core();
    let (id, cards) = add(&core, "kot", &["pl"], false);
    let moved = ok(
        &core,
        "suspendCards",
        json!({ "target": { "cards": cards } }),
    );
    let f = fields(&core, builtin::basic());
    let out = ok(
        &core,
        "updateNote",
        json!({ "noteId": id, "fields": [{ "fieldId": f[0], "value": "kot" }], "tags": ["PL"] }),
    );
    assert_eq!(out["undo"], Value::Null);
    undo(&core, &moved["undo"]).unwrap();
}

#[test]
fn update_note_refuses_a_deleted_deck_before_writing_anything() {
    let (core, _) = core();
    let (id, _) = add(&core, "kot", &[], false);
    let verbs = deck(&core, "Verbs");
    ok(&core, "deleteDeck", json!({ "deckId": verbs }));
    let f = fields(&core, builtin::basic());
    let error = call(
        &core,
        "updateNote",
        json!({
            "noteId": id,
            "fields": [{ "fieldId": f[0], "value": "pies" }],
            "tags": [],
            "deckId": verbs,
        }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(note(&core, &id)["fields"][0]["value"], "kot");
}

#[test]
fn update_note_answers_the_cards_it_made_and_removed_and_refuses_a_bad_tag() {
    let (core, _) = core();
    let f = fields(&core, builtin::basic_and_reversed());
    let (id, _) = add(&core, "kot", &[], true);
    let out = ok(
        &core,
        "updateNote",
        json!({ "noteId": id, "fields": [{ "fieldId": f[1], "value": "" }], "tags": [] }),
    );
    assert_eq!(out["cardsRemoved"].as_array().unwrap().len(), 1);
    let error = call(
        &core,
        "updateNote",
        json!({ "noteId": id, "fields": [], "tags": ["two words"] }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidInput);
}

// ---- searchIds ------------------------------------------------------------------------------------

#[test]
fn search_ids_answers_every_note_or_card() {
    let (core, _) = core();
    let (one, _) = add(&core, "one", &["pl"], true);
    add(&core, "two", &[], false);
    let notes = ok(
        &core,
        "searchIds",
        json!({ "query": "tag:pl", "mode": "notes" }),
    );
    assert_eq!(notes["ids"], json!([one]));
    let cards = ok(&core, "searchIds", json!({ "query": "", "mode": "cards" }));
    assert_eq!(cards["ids"].as_array().unwrap().len(), 3);
    let error = call(&core, "searchIds", json!({ "query": "(", "mode": "cards" })).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidInput);
}

// ---- The bulk actions and their undo --------------------------------------------------------------

#[test]
fn a_card_action_on_notes_reaches_every_card_and_undo_moves_them_back() {
    let (core, _) = core();
    let (one, one_cards) = add(&core, "one", &[], true);
    let (_, two_cards) = add(&core, "two", &[], false);
    let polish = deck(&core, "Polish");
    let verbs = ok(
        &core,
        "createDeck",
        json!({ "name": "Verbs", "parentId": polish }),
    )["id"]
        .clone();
    let out = ok(
        &core,
        "moveCards",
        json!({ "target": { "notes": [one] }, "deckId": verbs }),
    );
    assert_eq!(out["count"], 2);
    assert_eq!(out["undo"]["message"], "Moved 2 cards to Polish::Verbs.");
    assert_eq!(
        decks_of(&core, &one_cards),
        vec![verbs.as_str().unwrap(); 2]
    );
    assert_eq!(
        decks_of(&core, &two_cards),
        vec![default_deck().to_string()]
    );

    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Moved 2 cards back."
    );
    assert_eq!(
        decks_of(&core, &one_cards),
        vec![default_deck().to_string(); 2]
    );
}

#[test]
fn a_move_from_two_decks_goes_back_to_each_and_says_what_could_not() {
    let (core, _) = core();
    let (_, a) = add(&core, "one", &[], false);
    let (_, b) = add(&core, "two", &[], false);
    let first = deck(&core, "First");
    let target = deck(&core, "Target");
    ok(
        &core,
        "moveCards",
        json!({ "target": { "cards": [a[0]] }, "deckId": first }),
    );
    let out = ok(
        &core,
        "moveCards",
        json!({ "target": { "cards": [a[0], b[0]] }, "deckId": target }),
    );
    assert_eq!(out["count"], 2);
    ok(&core, "deleteDeck", json!({ "deckId": first }));
    // Deleting First deleted nothing that moved: the cards are in Target now.
    let message = undo(&core, &out["undo"]).unwrap()["message"].clone();
    assert_eq!(
        message,
        "Moved 1 card back. 1 card could not be put back, because it or its deck was deleted."
    );
    assert_eq!(decks_of(&core, &b), vec![default_deck().to_string()]);
}

#[test]
fn tags_on_cards_reach_their_notes_and_undo_takes_off_only_what_was_added() {
    let (core, _) = core();
    let (one, one_cards) = add(&core, "one", &["pl"], true);
    let (two, _) = add(&core, "two", &[], false);
    let out = ok(
        &core,
        "addTags",
        json!({ "target": { "cards": one_cards }, "tags": ["PL", "verbs"] }),
    );
    assert_eq!(out["count"], 1);
    assert_eq!(out["undo"]["message"], "Added verbs to 1 note.");
    let out = ok(
        &core,
        "addTags",
        json!({ "target": { "notes": [one, two] }, "tags": ["pl", "x"] }),
    );
    assert_eq!(out["count"], 2);
    assert_eq!(out["undo"]["message"], "Added 2 tags to 2 notes.");
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Took the tags back off 2 notes."
    );
    assert_eq!(note(&core, &one)["tags"], json!(["pl", "verbs"]));
    assert_eq!(note(&core, &two)["tags"], json!([]));

    let out = ok(
        &core,
        "removeTags",
        json!({ "target": { "notes": [one, two] }, "tags": ["pl"] }),
    );
    assert_eq!(out["undo"]["message"], "Removed pl from 1 note.");
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Put the tags back on 1 note."
    );
    assert_eq!(note(&core, &one)["tags"], json!(["pl", "verbs"]));
    let nothing = ok(
        &core,
        "removeTags",
        json!({ "target": { "notes": [two] }, "tags": ["pl"] }),
    );
    assert_eq!(nothing, json!({ "count": 0, "undo": null }));
}

#[test]
fn suspend_and_its_undo_change_only_the_cards_that_changed() {
    let (core, _) = core();
    let (one, cards) = add(&core, "one", &[], true);
    ok(
        &core,
        "suspendCards",
        json!({ "target": { "cards": [cards[0]] } }),
    );
    let out = ok(
        &core,
        "suspendCards",
        json!({ "target": { "notes": [one] } }),
    );
    assert_eq!(out["count"], 1);
    assert_eq!(out["undo"]["message"], "Suspended 1 card.");
    assert_eq!(suspended(&core, ""), 2);
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Unsuspended 1 card again."
    );
    assert_eq!(suspended(&core, ""), 1);
    let out = ok(
        &core,
        "unsuspendCards",
        json!({ "target": { "notes": [one] } }),
    );
    assert_eq!(out["undo"]["message"], "Unsuspended 1 card.");
    undo(&core, &out["undo"]).unwrap();
    assert_eq!(suspended(&core, ""), 1);
}

#[test]
fn delete_notes_counts_and_its_undo_restores_them() {
    let (core, _) = core();
    let (one, one_cards) = add(&core, "one", &[], true);
    let (two, _) = add(&core, "two", &[], false);
    let out = ok(
        &core,
        "deleteNotes",
        json!({ "target": { "cards": [one_cards[1]] } }),
    );
    assert_eq!(out["notes"], 1);
    assert_eq!(out["cards"], 2);
    assert_eq!(out["undo"]["message"], "Deleted 1 note and its 2 cards.");
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Restored 1 note and 2 cards."
    );
    let out = ok(
        &core,
        "deleteNotes",
        json!({ "target": { "notes": [one, two] } }),
    );
    assert_eq!(out["undo"]["message"], "Deleted 2 notes and their 3 cards.");
    let error = call(&core, "getNote", json!({ "noteId": two })).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    // A card of a deleted note is not a target any more.
    let error = call(
        &core,
        "suspendCards",
        json!({ "target": { "cards": [one_cards[0]] } }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
}

#[test]
fn describe_target_counts_notes_cards_suspended_and_what_a_delete_takes() {
    let (core, _) = core();
    let (one, one_cards) = add(&core, "one", &[], true);
    let (two, two_cards) = add(&core, "two", &[], false);
    let describe = |target: Value| ok(&core, "describeTarget", json!({ "target": target }));
    assert_eq!(
        describe(json!({ "notes": [one, two, one] })),
        json!({ "notes": 2, "cards": 3, "suspended": 0, "noteCards": 3, "newCards": 3, "noteTypeId": null })
    );
    ok(
        &core,
        "suspendCards",
        json!({ "target": { "cards": [one_cards[0], two_cards[0]] } }),
    );
    // One card of a two-card note: a delete still takes both.
    assert_eq!(
        describe(json!({ "cards": [one_cards[0], two_cards[0]] })),
        json!({ "notes": 2, "cards": 2, "suspended": 2, "noteCards": 3, "newCards": 2, "noteTypeId": null })
    );
    assert_eq!(
        describe(json!({ "notes": [one] })),
        json!({
            "notes": 1, "cards": 2, "suspended": 1, "noteCards": 2, "newCards": 2,
            "noteTypeId": builtin::basic_and_reversed().to_string(),
        })
    );
    assert_eq!(describe(json!({ "notes": [] }))["cards"], 0);
    ok(
        &core,
        "deleteNotes",
        json!({ "target": { "notes": [two] } }),
    );
    let error = call(
        &core,
        "describeTarget",
        json!({ "target": { "cards": [two_cards[0]] } }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
}

// ---- setDueDate and resetCards --------------------------------------------------------------------

/// Answers a card Easy, so it is a review card.
pub(crate) fn study(core: &Core, card: &str) {
    core.with_collection(|c| {
        c.answer(
            card.parse().unwrap(),
            fc_core::scheduling::Rating::Easy,
            1_000,
        )
        .unwrap()
    })
    .unwrap();
}

pub(crate) fn schedule(core: &Core, card: &str) -> fc_core::study::CardSchedule {
    core.with_collection(|c| c.card_schedule(card.parse().unwrap()).unwrap())
        .unwrap()
}

#[test]
fn set_due_date_on_notes_skips_new_cards_and_its_undo_puts_the_schedule_back() {
    let (core, _) = core();
    let (one, one_cards) = add(&core, "one", &[], true);
    let (two, two_cards) = add(&core, "two", &[], false);
    study(&core, &one_cards[0]);
    study(&core, &two_cards[0]);
    let before = schedule(&core, &one_cards[0]);
    assert_eq!(
        ok(
            &core,
            "describeTarget",
            json!({ "target": { "notes": [one, two] } })
        )["newCards"],
        1
    );
    let out = ok(
        &core,
        "setDueDate",
        json!({ "target": { "notes": [one, two] }, "fromDays": 3, "toDays": 3 }),
    );
    assert_eq!(out["count"], 2);
    assert_eq!(out["skippedNew"], 1);
    assert_eq!(out["undo"]["message"], "Made 2 cards due in 3 days.");
    let today = core.with_collection(|c| c.study_today().unwrap()).unwrap();
    assert_eq!(schedule(&core, &one_cards[0]).due_day, Some(today + 3));
    assert_eq!(schedule(&core, &two_cards[0]).due_day, Some(today + 3));
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Put back the schedule of 2 cards."
    );
    assert_eq!(schedule(&core, &one_cards[0]), before);
    gone(undo(&core, &out["undo"]));

    let messages: Vec<Value> = [(0, 0), (1, 1), (2, 9)]
        .into_iter()
        .map(|(from, to)| {
            ok(
                &core,
                "setDueDate",
                json!({ "target": { "cards": [one_cards[0]] }, "fromDays": from, "toDays": to }),
            )["undo"]["message"]
                .clone()
        })
        .collect();
    assert_eq!(
        messages,
        [
            json!("Made 1 card due today."),
            json!("Made 1 card due tomorrow."),
            json!("Made 1 card due in 2 to 9 days.")
        ]
    );
}

#[test]
fn set_due_date_on_new_cards_only_changes_nothing_and_a_bad_range_is_refused() {
    let (core, _) = core();
    let (_, cards) = add(&core, "one", &[], false);
    let out = ok(
        &core,
        "setDueDate",
        json!({ "target": { "cards": cards }, "fromDays": 1, "toDays": 1 }),
    );
    assert_eq!(out, json!({ "count": 0, "skippedNew": 1, "undo": null }));
    let error = call(
        &core,
        "setDueDate",
        json!({ "target": { "cards": cards }, "fromDays": 5, "toDays": 2 }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidInput);
    assert!(
        error.message.starts_with("Choose a number of days"),
        "{error:?}"
    );
}

#[test]
fn reset_cards_and_its_undo_says_what_was_studied_since() {
    let (core, clock) = core();
    let (_, one) = add(&core, "one", &[], false);
    let (_, two) = add(&core, "two", &[], false);
    let (_, fresh) = add(&core, "fresh", &[], false);
    study(&core, &one[0]);
    study(&core, &two[0]);
    let before = schedule(&core, &two[0]);
    let out = ok(
        &core,
        "resetCards",
        json!({ "target": { "cards": [one[0], two[0], fresh[0]] } }),
    );
    assert_eq!(out["count"], 2);
    assert_eq!(out["undo"]["message"], "Reset 2 cards to new.");
    assert_eq!(
        schedule(&core, &one[0]).state,
        fc_core::scheduling::CardState::New
    );
    // One of them is studied again before the undo.
    clock.advance(60_000);
    study(&core, &one[0]);
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Put back the schedule of 1 card. 1 card could not be put back, because it was studied since."
    );
    assert_eq!(schedule(&core, &two[0]), before);
    // Nothing to reset: no undo, and the kept change stays.
    let none = ok(
        &core,
        "resetCards",
        json!({ "target": { "cards": [fresh[0]] } }),
    );
    assert_eq!(none, json!({ "count": 0, "undo": null }));
}

// ---- changeNoteType -------------------------------------------------------------------------------

pub(crate) fn templates(core: &Core, note_type: Id) -> Vec<String> {
    core.with_collection(|c| {
        c.note_type(note_type)
            .unwrap()
            .unwrap()
            .templates
            .iter()
            .map(|t| t.id.to_string())
            .collect()
    })
    .unwrap()
}

/// The input to change Basic notes to Basic and reversed, Card 1 keeping its schedule.
pub(crate) fn change_input(core: &Core, target: Value) -> Value {
    let (of, nf) = (
        fields(core, builtin::basic()),
        fields(core, builtin::basic_and_reversed()),
    );
    let (ot, nt) = (
        templates(core, builtin::basic()),
        templates(core, builtin::basic_and_reversed()),
    );
    json!({
        "target": target,
        "noteTypeId": builtin::basic_and_reversed().to_string(),
        "fields": [{ "fieldId": nf[0], "from": of[0] }, { "fieldId": nf[1], "from": of[1] }],
        "templates": [{ "templateId": nt[0], "from": ot[0] }, { "templateId": nt[1], "from": null }],
    })
}

fn type_of(core: &Core, id: &str) -> String {
    note(core, id)["noteTypeId"].as_str().unwrap().to_owned()
}

#[test]
fn change_note_type_keeps_the_schedule_and_its_undo_changes_it_back() {
    let (core, clock) = core();
    let (one, one_cards) = add(&core, "one", &[], false);
    let (two, _) = add(&core, "two", &[], false);
    study(&core, &one_cards[0]);
    let before = schedule(&core, &one_cards[0]);

    let out = ok(
        &core,
        "changeNoteType",
        change_input(&core, json!({ "notes": [one, two] })),
    );
    assert_eq!(
        (out["notes"].as_u64(), out["carried"].as_u64()),
        (Some(2), Some(1))
    );
    assert_eq!(
        out["undo"]["message"],
        "Changed 2 notes to Basic and reversed."
    );
    assert_eq!(
        type_of(&core, &one),
        builtin::basic_and_reversed().to_string()
    );
    assert_eq!(note(&core, &one)["cardCount"], 2);
    let (card_1, new_state) = core
        .with_collection(|c| {
            let cards = c.cards_of_note(one.parse().unwrap()).unwrap();
            let s = c.card_schedule(cards[0].id).unwrap();
            (cards[0].id, s)
        })
        .unwrap();
    assert_eq!(new_state.memory, before.memory);
    assert_eq!(new_state.due_day, before.due_day);

    // Studied meanwhile, then undone: the old card is back with that study.
    clock.advance(86_400_000);
    study(&core, &card_1.to_string());
    let studied = schedule(&core, &card_1.to_string());
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Changed 2 notes back to Basic."
    );
    assert_eq!(type_of(&core, &one), builtin::basic().to_string());
    assert_eq!(note(&core, &one)["fields"][0]["value"], "one");
    let back = schedule(&core, &one_cards[0]);
    assert_eq!(back.due_day, studied.due_day);
    assert_eq!(back.memory, studied.memory);
    // It goes once.
    gone(undo(&core, &out["undo"]));
}

#[test]
fn change_note_type_on_cards_changes_their_notes_and_refuses_mixed_notes() {
    let (core, _) = core();
    let (_, basic_cards) = add(&core, "one", &[], false);
    let (reversed, _) = add(&core, "two", &[], true);
    let input = change_input(&core, json!({ "cards": [basic_cards[0]] }));
    let out = ok(&core, "changeNoteType", input);
    assert_eq!(out["notes"], 1);

    let (a, _) = add(&core, "three", &[], false);
    let mixed = call(
        &core,
        "changeNoteType",
        change_input(&core, json!({ "notes": [a, reversed] })),
    )
    .unwrap_err();
    assert_eq!(mixed.kind, ErrorKind::InvalidInput);
    assert_eq!(mixed.message, "Select notes of one note type to change it.");
    assert_eq!(type_of(&core, &a), builtin::basic().to_string());
}

#[test]
fn change_note_type_refuses_what_leaves_a_note_with_no_cards_or_names_a_card_twice() {
    let (core, _) = core();
    let (note_id, _) = add(&core, "one", &[], false);
    let nf = fields(&core, builtin::basic_and_reversed());
    let (ot, nt) = (
        templates(&core, builtin::basic()),
        templates(&core, builtin::basic_and_reversed()),
    );
    let empty = call(
        &core,
        "changeNoteType",
        json!({
            "target": { "notes": [note_id] },
            "noteTypeId": builtin::basic_and_reversed().to_string(),
            "fields": [{ "fieldId": nf[0], "from": null }, { "fieldId": nf[1], "from": null }],
            "templates": [],
        }),
    )
    .unwrap_err();
    assert_eq!(empty.kind, ErrorKind::InvalidInput);
    assert!(
        empty
            .message
            .starts_with("1 note would be left with no cards.")
    );
    let twice = call(
        &core,
        "changeNoteType",
        json!({
            "target": { "notes": [note_id] },
            "noteTypeId": builtin::basic_and_reversed().to_string(),
            "fields": [],
            "templates": [
                { "templateId": nt[0], "from": ot[0] },
                { "templateId": nt[1], "from": ot[0] },
            ],
        }),
    )
    .unwrap_err();
    assert_eq!(twice.kind, ErrorKind::InvalidInput);
    assert_eq!(type_of(&core, &note_id), builtin::basic().to_string());
}

#[test]
fn a_note_that_changed_type_again_is_left_alone_by_the_undo() {
    let (core, _) = core();
    let (one, _) = add(&core, "one", &[], false);
    let (two, _) = add(&core, "two", &[], false);
    let out = ok(
        &core,
        "changeNoteType",
        change_input(&core, json!({ "notes": [one, two] })),
    );
    // Another change to one of them, outside the kept undo, then the undo of the first.
    let other = core
        .with_collection(|c| {
            let other = c
                .create_note_type("Basic 2", fc_core::notetype::Kind::Standard)
                .unwrap();
            let from = c.note_type(builtin::basic_and_reversed()).unwrap().unwrap();
            let to = c.note_type(other).unwrap().unwrap();
            c.change_note_type(
                &[one.parse().unwrap()],
                other,
                &[
                    (to.fields[0].id, Some(from.fields[0].id)),
                    (to.fields[1].id, Some(from.fields[1].id)),
                ],
                &[],
            )
            .unwrap();
            other
        })
        .unwrap();
    assert_eq!(
        undo(&core, &out["undo"]).unwrap()["message"],
        "Changed 1 note back to Basic. 1 note could not be changed back, because it was deleted or changed again."
    );
    assert_eq!(type_of(&core, &two), builtin::basic().to_string());
    assert_eq!(type_of(&core, &one), other.to_string());
}

#[test]
fn get_note_types_lists_the_templates() {
    let (core, _) = core();
    let out = ok(&core, "getNoteTypes", Value::Null);
    let reversed = out["noteTypes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == builtin::basic_and_reversed().to_string())
        .unwrap();
    let names: Vec<&str> = reversed["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Card 1", "Card 2"]);
}

#[test]
fn a_second_change_makes_the_first_token_too_old() {
    let (core, _) = core();
    let (one, _) = add(&core, "one", &[], false);
    let first = ok(
        &core,
        "addTags",
        json!({ "target": { "notes": [one] }, "tags": ["a"] }),
    );
    let second = ok(
        &core,
        "addTags",
        json!({ "target": { "notes": [one] }, "tags": ["b"] }),
    );
    gone(undo(&core, &first["undo"]));
    undo(&core, &second["undo"]).unwrap();
    assert_eq!(note(&core, &one)["tags"], json!(["a"]));
    gone(call(&core, "undoChange", json!({ "undoId": "not an ID" })));
}

#[test]
fn a_new_collection_drops_the_kept_change() {
    let (core, clock) = core();
    let (one, _) = add(&core, "one", &[], false);
    let out = ok(
        &core,
        "addTags",
        json!({ "target": { "notes": [one] }, "tags": ["a"] }),
    );
    core.open_collection(":memory:", host(1, &clock)).unwrap();
    gone(undo(&core, &out["undo"]));
}

#[test]
fn undo_after_a_merge_still_writes_the_old_value() {
    let (core, clock) = core();
    let (id, _) = add(&core, "kot", &[], false);
    let f = fields(&core, builtin::basic());
    let out = ok(
        &core,
        "updateNote",
        json!({ "noteId": id, "fields": [{ "fieldId": f[0], "value": "pies" }], "tags": [] }),
    );
    // Another device gets the edit, edits the same field later, and syncs back.
    let other_clock = Arc::new(ManualClock::new(T0 + 60_000));
    let other = Collection::create(":memory:", host(2, &other_clock)).unwrap();
    core.with_collection(|c| other.merge(&c.changes(Selection::All).unwrap()).unwrap())
        .unwrap();
    other
        .set_note_fields(id.parse().unwrap(), &[(f[0].parse().unwrap(), "ryba")])
        .unwrap();
    core.with_collection(|c| c.merge(&other.changes(Selection::All).unwrap()).unwrap())
        .unwrap();
    assert_eq!(note(&core, &id)["fields"][0]["value"], "ryba");
    clock.advance(120_000);
    undo(&core, &out["undo"]).unwrap();
    assert_eq!(note(&core, &id)["fields"][0]["value"], "kot");
}

#[test]
fn every_method_needs_an_open_collection() {
    let core = Core::new();
    let target = json!({ "notes": [] });
    for (method, input) in [
        (
            "getNote",
            json!({ "noteId": Id::from_bytes([1; 16]).to_string() }),
        ),
        (
            "updateNote",
            json!({ "noteId": Id::from_bytes([1; 16]).to_string(), "fields": [], "tags": [] }),
        ),
        ("searchIds", json!({ "query": "", "mode": "notes" })),
        (
            "moveCards",
            json!({ "target": target, "deckId": default_deck().to_string() }),
        ),
        ("addTags", json!({ "target": target, "tags": ["a"] })),
        ("removeTags", json!({ "target": target, "tags": ["a"] })),
        ("suspendCards", json!({ "target": target })),
        ("unsuspendCards", json!({ "target": target })),
        ("deleteNotes", json!({ "target": target })),
        (
            "setDueDate",
            json!({ "target": target, "fromDays": 0, "toDays": 0 }),
        ),
        ("resetCards", json!({ "target": target })),
        (
            "changeNoteType",
            json!({
                "target": target,
                "noteTypeId": builtin::basic().to_string(),
                "fields": [],
                "templates": [],
            }),
        ),
        ("describeTarget", json!({ "target": target })),
    ] {
        let error = call(&core, method, input).unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound, "{method}");
    }
}
