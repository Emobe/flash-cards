//! The note type methods through `dispatch` (step 3.4a).

use fc_core::notetype::builtin;
use serde_json::{Value, json};

use super::*;
use crate::edit_tests::{add, call, core, fields, ok};

fn basic() -> String {
    builtin::basic().to_string()
}

fn details(core: &Core, id: &str) -> Value {
    ok(core, "getNoteTypeDetails", json!({ "noteTypeId": id }))
}

fn undo(core: &Core, token: &Value) -> Value {
    ok(core, "undoChange", json!({ "undoId": token["id"] }))
}

fn live_cards(core: &Core, note: &str) -> usize {
    core.with_collection(|c| c.cards_of_note(note.parse().unwrap()).unwrap().len())
        .unwrap()
}

#[test]
fn details_give_a_draft_and_the_counts() {
    let (core, _) = core();
    add(&core, "one", &[], false);
    add(&core, "two", &[], false);
    let d = details(&core, &basic());
    assert_eq!(d["noteCount"], 2);
    assert_eq!(d["draft"]["name"], "Basic");
    assert_eq!(d["draft"]["fields"].as_array().unwrap().len(), 2);
    assert_eq!(d["draft"]["templates"][0]["name"], "Card 1");
    assert_eq!(d["draft"]["sortField"], 0);
    assert_eq!(d["removedFields"], json!([]));
    let list = ok(&core, "getNoteTypes", Value::Null);
    let found = list["noteTypes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == basic())
        .unwrap();
    assert_eq!(found["noteCount"], 2);
    let missing = call(
        &core,
        "getNoteTypeDetails",
        json!({ "noteTypeId": Id::from_bytes([7; 16]).to_string() }),
    );
    assert_eq!(missing.unwrap_err().kind, ErrorKind::NotFound);
}

#[test]
fn check_then_save_then_undo() {
    let (core, _) = core();
    let (note, _) = add(&core, "one", &[], false);
    let mut draft = details(&core, &basic())["draft"].clone();
    draft["name"] = json!("Polish words");
    draft["templates"][0]["front"] = json!("no fields here");
    let input = json!({ "noteTypeId": basic(), "draft": draft });

    let plan = ok(&core, "checkNoteType", input.clone());
    assert_eq!(
        plan,
        json!({ "cardsAdded": 0, "cardsRemoved": 1, "notesAffected": 1 })
    );
    assert_eq!(live_cards(&core, &note), 1, "a check writes nothing");

    let saved = ok(&core, "saveNoteType", input);
    assert_eq!(
        saved["message"],
        "Saved Polish words. 1 card moved to the trash."
    );
    assert_eq!(saved["cardsRemoved"], 1);
    assert_eq!(live_cards(&core, &note), 0);
    assert_eq!(details(&core, &basic())["draft"]["name"], "Polish words");

    let back = undo(&core, &saved["undo"]);
    assert_eq!(back["message"], "Put back the earlier version of Basic.");
    assert_eq!(live_cards(&core, &note), 1);
    assert_eq!(details(&core, &basic())["draft"]["name"], "Basic");
}

#[test]
fn saving_with_nothing_changed_keeps_no_undo() {
    let (core, _) = core();
    let draft = details(&core, &basic())["draft"].clone();
    let saved = ok(
        &core,
        "saveNoteType",
        json!({ "noteTypeId": basic(), "draft": draft }),
    );
    assert_eq!(saved["undo"], Value::Null);
    assert_eq!(saved["message"], "No changes to save in Basic.");
}

#[test]
fn a_mistake_is_refused_with_the_reason_and_writes_nothing() {
    let (core, _) = core();
    let mut draft = details(&core, &basic())["draft"].clone();
    draft["templates"][0]["front"] = json!("{{#Front}}");
    let input = json!({ "noteTypeId": basic(), "draft": draft });
    for method in ["checkNoteType", "saveNoteType"] {
        let error = call(&core, method, input.clone()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidInput);
        assert!(error.message.contains("never closed"), "{}", error.message);
    }
    assert_eq!(
        details(&core, &basic())["draft"]["templates"][0]["front"],
        "{{Front}}"
    );
    let mut draft = details(&core, &basic())["draft"].clone();
    draft["fields"][0]["id"] = json!("not an ID");
    let error = call(
        &core,
        "saveNoteType",
        json!({ "noteTypeId": basic(), "draft": draft }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidInput);
}

#[test]
fn removed_items_are_listed_and_a_draft_restores_them() {
    let (core, _) = core();
    let id = ok(
        &core,
        "createNoteType",
        json!({ "name": "Words", "startFrom": basic(), "kind": null }),
    )["noteTypeId"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut draft = details(&core, &id)["draft"].clone();
    let gone = draft["fields"].as_array_mut().unwrap().remove(1);
    ok(
        &core,
        "saveNoteType",
        json!({ "noteTypeId": id, "draft": draft }),
    );
    let d = details(&core, &id);
    assert_eq!(d["removedFields"][0]["name"], gone["name"]);
    let mut draft = d["draft"].clone();
    draft["fields"]
        .as_array_mut()
        .unwrap()
        .push(d["removedFields"][0].clone());
    ok(
        &core,
        "saveNoteType",
        json!({ "noteTypeId": id, "draft": draft }),
    );
    let d = details(&core, &id);
    assert_eq!(d["draft"]["fields"].as_array().unwrap().len(), 2);
    assert_eq!(d["removedFields"], json!([]));
}

#[test]
fn rename_field_in_templates_rewrites_a_draft_only() {
    let (core, _) = core();
    let out = ok(
        &core,
        "renameFieldInTemplates",
        json!({
            "templates": [{ "id": null, "name": "T", "front": "{{Front}} {{hint:Front}}", "back": "{{#Front}}x{{/Front}}" }],
            "from": "Front", "to": "Word",
        }),
    );
    assert_eq!(out["templates"][0]["front"], "{{Word}} {{hint:Word}}");
    assert_eq!(out["templates"][0]["back"], "{{#Word}}x{{/Word}}");
    assert_eq!(
        details(&core, &basic())["draft"]["templates"][0]["front"],
        "{{Front}}"
    );
}

#[test]
fn preview_notes_and_cards_with_real_notes() {
    let (core, _) = core();
    let (note, _) = add(&core, "<b>kot</b>", &[], false);
    let list = ok(
        &core,
        "previewNotes",
        json!({ "noteTypeId": basic(), "limit": null }),
    );
    assert_eq!(list["total"], 1);
    assert_eq!(list["notes"][0]["noteId"], note);
    assert_eq!(list["notes"][0]["title"], "kot");

    let mut draft = details(&core, &basic())["draft"].clone();
    draft["templates"][0]["front"] = json!("[{{Front}}] {{Nope}}");
    let input = |note: Value, draft: &Value| json!({ "noteTypeId": basic(), "draft": draft, "templateIndex": 0, "noteId": note, "ordinal": 0 });
    let real = ok(&core, "previewCard", input(json!(note), &draft));
    assert!(real["front"].as_str().unwrap().contains("[<b>kot</b>]"));
    assert_eq!(real["unknownFields"], json!(["Nope"]));
    assert_eq!(real["makesCard"], true);
    let none = ok(&core, "previewCard", input(Value::Null, &draft));
    assert!(none["front"].as_str().unwrap().contains("[(Front)]"));

    draft["templates"][0]["front"] = json!("{{/Front}}");
    let bad = ok(&core, "previewCard", input(json!(note), &draft));
    assert_eq!(bad["front"], Value::Null);
    assert_eq!(bad["problems"][0]["side"], "front");
    assert_eq!(bad["problems"][0]["mistakes"][0]["line"], 1);
    assert_eq!(bad["problems"][0]["mistakes"][0]["column"], 1);
    assert!(
        !bad["problems"][0]["mistakes"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("line")
    );
}

#[test]
fn create_from_a_copy_or_a_shape_and_undo() {
    let (core, _) = core();
    let copy = ok(
        &core,
        "createNoteType",
        json!({ "name": "Mine", "startFrom": basic(), "kind": null }),
    );
    let id = copy["noteTypeId"].as_str().unwrap();
    assert_ne!(id, basic());
    assert_eq!(
        details(&core, id)["draft"]["fields"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(copy["undo"]["message"], "Added Mine.");
    let cloze = ok(
        &core,
        "createNoteType",
        json!({ "name": "Gaps", "startFrom": null, "kind": "cloze" }),
    );
    assert_eq!(
        details(&core, cloze["noteTypeId"].as_str().unwrap())["kind"],
        "cloze"
    );
    // Only the latest change can be undone.
    let gone = call(&core, "undoChange", json!({ "undoId": copy["undo"]["id"] })).unwrap_err();
    assert_eq!(gone.message, "That can no longer be undone.");
    assert_eq!(undo(&core, &cloze["undo"])["message"], "Removed Gaps.");
    let names: Vec<String> = ok(&core, "getNoteTypes", Value::Null)["noteTypes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"Mine".to_owned()) && !names.contains(&"Gaps".to_owned()));
    let empty = call(
        &core,
        "createNoteType",
        json!({ "name": " ", "startFrom": null, "kind": null }),
    );
    assert_eq!(empty.unwrap_err().kind, ErrorKind::InvalidInput);
}

#[test]
fn delete_takes_the_notes_along_and_undo_brings_them_back() {
    let (core, _) = core();
    let id = ok(
        &core,
        "createNoteType",
        json!({ "name": "Temp", "startFrom": basic(), "kind": null }),
    )["noteTypeId"]
        .as_str()
        .unwrap()
        .to_owned();
    let f = fields(&core, id.parse().unwrap());
    let note = ok(
        &core,
        "addNote",
        json!({
            "noteTypeId": id, "deckId": fc_core::deck::default_deck().to_string(),
            "fields": [{ "fieldId": f[0], "value": "q" }, { "fieldId": f[1], "value": "a" }], "tags": [],
        }),
    )["noteId"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = ok(&core, "deleteNoteType", json!({ "noteTypeId": id }));
    assert_eq!(out["undo"]["message"], "Deleted Temp and 1 note.");
    assert_eq!(
        call(&core, "getNoteTypeDetails", json!({ "noteTypeId": id }))
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
    assert_eq!(undo(&core, &out["undo"])["message"], "Restored Temp.");
    assert_eq!(live_cards(&core, &note), 1);
    assert_eq!(details(&core, &id)["noteCount"], 1);
}

#[test]
fn a_later_change_makes_a_note_type_undo_too_old() {
    let (core, _) = core();
    let mut draft = details(&core, &basic())["draft"].clone();
    draft["name"] = json!("Renamed");
    let saved = ok(
        &core,
        "saveNoteType",
        json!({ "noteTypeId": basic(), "draft": draft }),
    );
    ok(
        &core,
        "createNoteType",
        json!({ "name": "Other", "startFrom": null, "kind": null }),
    );
    let error = call(
        &core,
        "undoChange",
        json!({ "undoId": saved["undo"]["id"] }),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(error.message, "That can no longer be undone.");
}
