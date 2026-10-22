//! `getCardInfo` through `dispatch`, as every host calls it (step 3.3).

use fc_core::scheduling::Rating;
use serde_json::json;

use crate::edit_tests::{add, call, change_input, core, ok, study};

fn info(core: &fc_core::Core, card: &str) -> serde_json::Value {
    ok(core, "getCardInfo", json!({ "cardId": card }))
}

#[test]
fn a_new_card_has_no_due_date_no_memory_and_no_history() {
    let (core, _) = core();
    let (note, cards) = add(&core, "<b>front</b> &amp; more", &["polish"], false);
    let out = info(&core, &cards[0]);
    assert_eq!(out["card"]["state"], "new");
    assert!(out["card"]["due"].is_null() && out["card"]["memory"].is_null());
    assert!(out["card"]["daysSinceLastAnswer"].is_null());
    assert_eq!(out["card"]["answers"], 0);
    assert_eq!(out["card"]["suspended"], false);
    assert_eq!(out["card"]["deckName"], "Default");
    assert_eq!(out["card"]["templateName"], "Card 1");
    assert_eq!(out["note"]["noteId"], note);
    assert_eq!(out["note"]["noteTypeName"], "Basic");
    assert_eq!(out["note"]["tags"], json!(["polish"]));
    assert_eq!(out["note"]["cardCount"], 1);
    // Plain text, not the stored HTML.
    assert_eq!(out["note"]["fields"][0]["text"], "front & more");
    assert_eq!(out["history"], json!([]));
}

#[test]
fn an_answered_card_shows_its_schedule_and_history() {
    let (core, clock) = core();
    let (_, cards) = add(&core, "one", &[], false);
    study(&core, &cards[0]);
    clock.advance(3 * 86_400_000);
    core.with_collection(|c| {
        c.answer(cards[0].parse().unwrap(), Rating::Again, 2_000)
            .unwrap()
    })
    .unwrap();
    let out = info(&core, &cards[0]);
    assert_eq!(out["card"]["answers"], 2);
    assert_eq!(out["card"]["daysSinceLastAnswer"], 0);
    assert!(out["card"]["memory"]["stability"].as_f64().unwrap() > 0.0);
    let history = out["history"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["kind"], "answer");
    assert_eq!(history[0]["rating"], "easy");
    assert_eq!(history[0]["stateBefore"], "new");
    assert_eq!(history[1]["rating"], "again");
    assert_eq!(history[1]["durationMs"], 2_000);
    // Easy on a new card makes it a review card, so Again on it is a lapse.
    assert_eq!(history[1]["stateBefore"], "review");
    assert_eq!(out["card"]["lapses"], 1);
    assert_eq!(out["card"]["state"], "relearning");
    assert_eq!(out["card"]["due"]["kind"], "time");
}

#[test]
fn a_suspended_card_says_so() {
    let (core, _) = core();
    let (_, cards) = add(&core, "one", &[], false);
    ok(
        &core,
        "suspendCards",
        json!({ "target": { "cards": [cards[0]] } }),
    );
    assert_eq!(info(&core, &cards[0])["card"]["suspended"], true);
}

#[test]
fn history_follows_a_card_through_a_note_type_change() {
    let (core, _) = core();
    let (note, cards) = add(&core, "one", &[], false);
    study(&core, &cards[0]);
    ok(
        &core,
        "changeNoteType",
        change_input(&core, json!({ "notes": [note] })),
    );
    let new_card = core
        .with_collection(|c| {
            c.cards_of_note(note.parse().unwrap()).unwrap()[0]
                .id
                .to_string()
        })
        .unwrap();
    let out = info(&core, &new_card);
    assert_eq!(out["note"]["noteTypeName"], "Basic and reversed");
    let kinds: Vec<_> = out["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kinds, ["answer", "transfer"]);
    // Counted from the history, not the new card's cache, which starts again at 0.
    assert_eq!(out["card"]["answers"], 1);
}

#[test]
fn a_deleted_or_unknown_card_is_not_found_and_a_bad_id_is_invalid() {
    let (core, _) = core();
    let (note, cards) = add(&core, "one", &[], false);
    ok(
        &core,
        "deleteNotes",
        json!({ "target": { "notes": [note] } }),
    );
    let error = call(&core, "getCardInfo", json!({ "cardId": cards[0] })).unwrap_err();
    assert_eq!(error.kind, crate::ErrorKind::NotFound);
    let error = call(&core, "getCardInfo", json!({ "cardId": "nope" })).unwrap_err();
    assert_eq!(error.kind, crate::ErrorKind::InvalidInput);
}
