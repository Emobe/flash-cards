//! The search and saved-search methods through `dispatch`, as every host calls them.

use std::sync::Arc;

use fc_core::clock::ManualClock;
use fc_core::notetype::builtin;
use fc_core::scheduling::Rating;
use serde_json::{Value, json};

use super::*;

const T0: i64 = 1_791_363_600_000;
const MINUTE: i64 = 60_000;
const DAY: i64 = 86_400_000;

fn core() -> (Core, Arc<ManualClock>) {
    let core = Core::new();
    let clock = Arc::new(ManualClock::new(T0));
    let host = Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes([1; 16]),
    };
    core.open_collection(":memory:", host).unwrap();
    (core, clock)
}

fn call(core: &Core, method: &str, input: Value) -> Result<Value, ApiError> {
    dispatch(core, method, input, &OpContext::uncancellable()).map(|r| r.output)
}

/// Basic notes with these fronts (back "back of <front>") in the Default deck, a millisecond
/// apart, and one Basic and reversed note `gato`. Returns the Basic cards in the same order.
fn add_notes(core: &Core, clock: &ManualClock, fronts: &[&str]) -> Vec<fc_core::id::Id> {
    core.with_collection(|c| {
        let deck = fc_core::deck::default_deck();
        let f: Vec<_> = c
            .note_type(builtin::basic())
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
        let cards = fronts
            .iter()
            .map(|front| {
                let back = format!("back of {front}");
                let added = c
                    .add_note_to_deck(deck, builtin::basic(), &[(f[0], front), (f[1], &back)])
                    .unwrap();
                clock.advance(1);
                added.cards[0]
            })
            .collect();
        let f: Vec<_> = c
            .note_type(builtin::basic_and_reversed())
            .unwrap()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.id)
            .collect();
        c.add_note_to_deck(
            deck,
            builtin::basic_and_reversed(),
            &[(f[0], "gato"), (f[1], "cat")],
        )
        .unwrap();
        clock.advance(1);
        cards
    })
    .unwrap()
}

fn search(core: &Core, query: &str, sort: Value, mode: &str, offset: u32, limit: u32) -> Value {
    call(
        core,
        "search",
        json!({
            "query": query, "sort": sort, "mode": mode,
            "offset": offset, "limit": limit, "seed": 7,
        }),
    )
    .unwrap()
}

fn titles(results: &Value) -> Vec<String> {
    results["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_search_gives_rows_a_list_can_show() {
    let (core, clock) = core();
    let cards = add_notes(&core, &clock, &["kot", "pies"]);
    let out = search(
        &core,
        "kot",
        json!({"key": "created", "descending": true}),
        "cards",
        0,
        100,
    );
    assert_eq!(out["total"], 1);
    let row = &out["rows"][0];
    assert_eq!(row["cardId"], cards[0].to_string());
    assert_eq!(row["title"], "kot");
    assert_eq!(row["detail"], "back of kot");
    assert_eq!(row["state"], "new");
    assert_eq!(row["due"], Value::Null);
    assert_eq!(row["suspended"], false);
    assert_eq!(row["buried"], false);
    assert_eq!(row["deckId"], fc_core::deck::default_deck().to_string());
    assert_eq!(row["noteTypeId"], builtin::basic().to_string());
    assert!(row["noteId"].is_string());
}

#[test]
fn every_sort_and_both_modes_run() {
    let (core, clock) = core();
    add_notes(&core, &clock, &["b", "a", "c"]);
    for key in [
        "created",
        "due",
        "field",
        "deck",
        "note",
        "difficulty",
        "stability",
        "lapses",
        "reviews",
        "random",
    ] {
        for descending in [false, true] {
            let sort = json!({"key": key, "descending": descending});
            assert_eq!(
                search(&core, "", sort.clone(), "notes", 0, 100)["total"],
                4,
                "{key}"
            );
            assert_eq!(
                search(&core, "", sort, "cards", 0, 100)["total"],
                5,
                "{key}"
            );
        }
    }
    let field = |descending| {
        titles(&search(
            &core,
            "-gato",
            json!({"key": "field", "descending": descending}),
            "notes",
            0,
            100,
        ))
    };
    assert_eq!(field(false), ["a", "b", "c"]);
    assert_eq!(field(true), ["c", "b", "a"]);
    let created = titles(&search(
        &core,
        "-gato",
        json!({"key": "created", "descending": true}),
        "notes",
        0,
        100,
    ));
    assert_eq!(created, ["c", "a", "b"], "newest first");
    let shuffle = || {
        titles(&search(
            &core,
            "",
            json!({"key": "random", "descending": false}),
            "cards",
            0,
            100,
        ))
    };
    assert_eq!(shuffle(), shuffle(), "the same seed, the same order");
    let unknown = call(
        &core,
        "search",
        json!({"query": "", "sort": {"key": "nope", "descending": false}, "mode": "cards",
               "offset": 0, "limit": 10, "seed": 0}),
    )
    .unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::InvalidInput);
}

#[test]
fn pages_follow_one_order_and_the_total_is_the_whole_search() {
    let (core, clock) = core();
    let fronts: Vec<String> = (0..25).map(|i| format!("n{i:02}")).collect();
    let fronts: Vec<&str> = fronts.iter().map(String::as_str).collect();
    add_notes(&core, &clock, &fronts);
    let sort = json!({"key": "field", "descending": false});
    let first = search(&core, "n*", sort.clone(), "notes", 0, 10);
    let third = search(&core, "n*", sort.clone(), "notes", 20, 10);
    assert_eq!(first["total"], 25);
    assert_eq!(titles(&first)[0], "n00");
    assert_eq!(titles(&third), ["n20", "n21", "n22", "n23", "n24"]);
    let count = search(&core, "n*", sort, "notes", 0, 0);
    assert_eq!(count["total"], 25);
    assert!(titles(&count).is_empty(), "limit 0 only counts");
}

#[test]
fn a_query_that_does_not_read_is_invalid_input_with_the_core_sentence() {
    let (core, clock) = core();
    add_notes(&core, &clock, &["kot"]);
    let input = |query: &str| {
        json!({"query": query, "sort": {"key": "created", "descending": true}, "mode": "notes",
               "offset": 0, "limit": 10, "seed": 0})
    };
    let err = call(&core, "search", input("kot (")).unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    assert!(err.message.contains("never closed"), "{}", err.message);
    assert!(err.message.contains("at character 5"), "{}", err.message);
    let err = call(&core, "search", input("frnot:x")).unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    assert!(err.message.contains("frnot"), "{}", err.message);
}

#[test]
fn check_query_gives_the_problem_and_where_it_is() {
    let (core, _) = core();
    let check = |query: &str| call(&core, "checkQuery", json!({ "query": query })).unwrap();
    assert_eq!(
        check("deck:=\"Polish::Verbs\" kot"),
        json!({ "problem": null })
    );
    let out = check("kot (pies");
    assert_eq!(out["problem"]["position"], 4);
    let message = out["problem"]["message"].as_str().unwrap();
    assert!(message.contains("never closed"), "{message}");
    assert!(
        !message.contains("at character"),
        "the position is data: {message}"
    );
    assert_eq!(
        check("łódź \"a")["problem"]["position"],
        5,
        "counted in characters"
    );
    assert_eq!(
        check("frnot:x"),
        json!({ "problem": null }),
        "fields are known only when run"
    );
}

#[test]
fn due_is_days_from_today_or_minutes_from_now() {
    let (core, clock) = core();
    let cards = add_notes(&core, &clock, &["learning", "review"]);
    // One learning step of an hour, so Again on a new card is due in an hour.
    let presets = call(&core, "getPresets", Value::Null).unwrap();
    let preset = presets["presets"][0]["id"].clone();
    call(
        &core,
        "setPresetOptions",
        json!({ "presetId": preset, "learningSteps": [60] }),
    )
    .unwrap();
    core.with_collection(|c| {
        c.answer(cards[0], Rating::Again, 1_000).unwrap();
        c.answer(cards[1], Rating::Easy, 1_000).unwrap();
    })
    .unwrap();
    let due = |front: &str| {
        let out = search(
            &core,
            &format!("front:{front}"),
            json!({"key": "created", "descending": true}),
            "cards",
            0,
            10,
        );
        out["rows"][0]["due"].clone()
    };
    assert_eq!(due("learning"), json!({ "kind": "time", "minutes": 60 }));
    clock.advance(30 * MINUTE + 1);
    assert_eq!(
        due("learning"),
        json!({ "kind": "time", "minutes": 30 }),
        "rounded up"
    );
    clock.advance(40 * MINUTE);
    assert_eq!(due("learning"), json!({ "kind": "time", "minutes": -10 }));

    // Put the clock 3 days before the review card's day, then 2 days after it.
    let due_day = core
        .with_collection(|c| {
            let page = c.search("front:review", &Default::default()).unwrap();
            page.rows[0].due_day.unwrap()
        })
        .unwrap();
    clock.set((due_day - 3) * DAY + 12 * 3_600_000);
    assert_eq!(due("review"), json!({ "kind": "day", "days": 3 }));
    clock.set((due_day + 2) * DAY + 12 * 3_600_000);
    assert_eq!(due("review"), json!({ "kind": "day", "days": -2 }));
    let out = search(
        &core,
        "front:review",
        json!({"key": "created", "descending": true}),
        "cards",
        0,
        10,
    );
    assert_eq!(out["rows"][0]["state"], "review");
}

#[test]
fn saved_searches_are_made_listed_changed_and_deleted() {
    let (core, _) = core();
    let sort = json!({"key": "field", "descending": false});
    let id = call(
        &core,
        "saveSearch",
        json!({"name": "Verbs to fix", "query": "tag:=verbs", "sort": sort, "mode": "notes"}),
    )
    .unwrap()["id"]
        .clone();
    call(
        &core,
        "saveSearch",
        json!({"name": "all", "query": "", "sort": sort, "mode": "cards"}),
    )
    .unwrap();
    let list = call(&core, "getSavedSearches", Value::Null).unwrap();
    assert_eq!(list["searches"][0]["name"], "all", "by name");
    assert_eq!(
        list["searches"][1],
        json!({"id": id, "name": "Verbs to fix", "query": "tag:=verbs", "sort": sort,
               "mode": "notes"})
    );

    call(
        &core,
        "updateSavedSearch",
        json!({"id": id, "query": "tag:=nouns"}),
    )
    .unwrap();
    let list = call(&core, "getSavedSearches", Value::Null).unwrap();
    assert_eq!(list["searches"][1]["query"], "tag:=nouns");
    assert_eq!(
        list["searches"][1]["name"], "Verbs to fix",
        "the rest stays"
    );
    assert_eq!(list["searches"][1]["mode"], "notes");

    let taken = call(&core, "updateSavedSearch", json!({"id": id, "name": "ALL"})).unwrap_err();
    assert_eq!(taken.kind, ErrorKind::InvalidInput);
    assert!(taken.message.contains("already"), "{}", taken.message);
    let empty = call(
        &core,
        "saveSearch",
        json!({"name": "  ", "query": "", "sort": sort, "mode": "cards"}),
    )
    .unwrap_err();
    assert_eq!(empty.kind, ErrorKind::InvalidInput);
    let unreadable = call(
        &core,
        "saveSearch",
        json!({"name": "bad", "query": "(", "sort": sort, "mode": "cards"}),
    )
    .unwrap_err();
    assert_eq!(unreadable.kind, ErrorKind::InvalidInput);

    call(&core, "deleteSavedSearch", json!({"id": id})).unwrap();
    let list = call(&core, "getSavedSearches", Value::Null).unwrap();
    assert_eq!(list["searches"].as_array().unwrap().len(), 1);
    let gone = call(&core, "updateSavedSearch", json!({"id": id, "name": "x"})).unwrap_err();
    assert_eq!(gone.kind, ErrorKind::NotFound);
    let gone = call(&core, "deleteSavedSearch", json!({"id": id})).unwrap_err();
    assert_eq!(gone.kind, ErrorKind::NotFound);
    let bad_id = call(&core, "deleteSavedSearch", json!({"id": "nope"})).unwrap_err();
    assert_eq!(bad_id.kind, ErrorKind::InvalidInput);
}

#[test]
fn with_no_collection_open_every_method_says_so() {
    let core = Core::new();
    let sort = json!({"key": "created", "descending": true});
    for (method, input) in [
        (
            "search",
            json!({"query": "", "sort": sort, "mode": "notes", "offset": 0, "limit": 10,
                   "seed": 0}),
        ),
        ("getSavedSearches", Value::Null),
        (
            "saveSearch",
            json!({"name": "a", "query": "", "sort": sort, "mode": "notes"}),
        ),
        (
            "updateSavedSearch",
            json!({"id": fc_core::deck::default_deck().to_string()}),
        ),
        (
            "deleteSavedSearch",
            json!({"id": fc_core::deck::default_deck().to_string()}),
        ),
    ] {
        let err = call(&core, method, input).unwrap_err();
        assert_eq!(err, crate::collection::not_open(&core), "{method}");
    }
    assert_eq!(
        call(&core, "checkQuery", json!({"query": "a"})).unwrap(),
        json!({ "problem": null }),
        "checking needs no collection"
    );
}
