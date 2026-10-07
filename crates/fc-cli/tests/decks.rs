//! Runs the real `fc` binary: decks, presets and moving cards.

mod common;
use common::*;

fn card_ids(notes: &str) -> Vec<String> {
    notes
        .lines()
        .find_map(|l| l.trim().strip_prefix("Card IDs: "))
        .unwrap()
        .split(", ")
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_deck_can_be_renamed_moved_deleted_and_restored() {
    let file = TempFile::collection("decks", "life.fc");
    let f = file.path();
    out(&fc(&["add-deck", f, "Polish"]));
    out(&fc(&["add-deck", f, "Polish::Verbs"]));
    out(&fc(&["add-deck", f, "Spanish"]));
    out(&fc(&[
        "add-note",
        f,
        "Basic",
        "--deck",
        "Polish::Verbs",
        "Front=biec",
        "Back=run",
    ]));

    out(&fc(&["deck", f, "rename", "Polish::Verbs", "Actions"]));
    let moved = out(&fc(&["deck", f, "move", "polish::actions", "Spanish"]));
    assert!(moved.contains("Spanish::Actions"), "{moved}");
    let moved = out(&fc(&["deck", f, "move", "Spanish::Actions", "--top"]));
    assert!(moved.trim_end().ends_with("is now Actions"), "{moved}");

    let deleted = out(&fc(&["deck", f, "delete", "Actions"]));
    assert!(deleted.contains("Restore it with"), "{deleted}");
    let listing = out(&fc(&["decks", f]));
    assert!(
        listing.contains("Deleted decks (can be restored): Actions ("),
        "{listing}"
    );
    // The deck can be restored by name and by ID.
    out(&fc(&["deck", f, "restore", "Actions"]));
    let listing = out(&fc(&["decks", f]));
    assert!(listing.contains("Actions (1 card"), "{listing}");
    assert!(!listing.contains("Deleted decks"), "{listing}");
    let id = id_in(&out(&fc(&["add-deck", f, "Temp"])));
    out(&fc(&["deck", f, "delete", "Temp"]));
    out(&fc(&["deck", f, "restore", &id]));
}

#[test]
fn deck_limits_and_cards_can_be_changed() {
    let file = TempFile::collection("decks", "cards.fc");
    let f = file.path();
    out(&fc(&["add-deck", f, "Polish"]));
    out(&fc(&["add-deck", f, "Polish::Verbs"]));
    assert!(out(&fc(&["deck", f, "limits", "Polish", "on"])).contains("now count"));
    assert!(out(&fc(&["deck", f, "limits", "Polish", "off"])).contains("no longer count"));

    let note = out(&fc(&[
        "add-note",
        f,
        "Basic and reversed",
        "Front=kot",
        "Back=cat",
    ]));
    let cards = card_ids(&out(&fc(&["notes", f])));
    assert_eq!(cards.len(), 2, "{note}");
    let moved = out(&fc(&[
        "move-cards",
        f,
        "polish::verbs",
        &cards[0],
        &cards[1],
    ]));
    assert_eq!(moved.trim_end(), "Moved 2 cards to Polish::Verbs");
    assert!(out(&fc(&["decks", f])).contains("Verbs (2 cards"));
}

#[test]
fn presets_can_be_added_changed_assigned_deleted_and_restored() {
    let file = TempFile::collection("decks", "presets.fc");
    let f = file.path();
    out(&fc(&["add-deck", f, "Polish"]));
    out(&fc(&["preset", f, "add", "Slow"]));
    out(&fc(&["preset", f, "rename", "slow", "Gentle"]));
    out(&fc(&[
        "preset",
        f,
        "set",
        "Gentle",
        "--new-per-day",
        "5",
        "--reviews-per-day",
        "50",
        "--learning-steps",
        "1,5 15",
        "--relearning-steps",
        "none",
        "--retention",
        "0.85",
        "--space-siblings",
        "off",
    ]));
    let listing = out(&fc(&["decks", f]));
    assert!(
        listing.contains(
            "Gentle: 5 new a day, 50 reviews a day, learning steps 1 5 15 minutes, desired \
             retention 0.85, used by 0 decks"
        ),
        "{listing}"
    );

    out(&fc(&["preset", f, "assign", "Polish", "Gentle"]));
    assert!(out(&fc(&["decks", f])).contains("Polish (0 cards, preset Gentle)"));
    out(&fc(&["preset", f, "delete", "Gentle"]));
    let listing = out(&fc(&["decks", f]));
    assert!(listing.contains("preset Default"), "{listing}");
    assert!(
        listing.contains("Deleted presets (can be restored): Gentle ("),
        "{listing}"
    );
    out(&fc(&["preset", f, "restore", "Gentle"]));
    assert!(!out(&fc(&["decks", f])).contains("Deleted presets"));

    // The FSRS parameters: a wrong count is refused, "default" clears them.
    assert!(
        !err(&fc(&[
            "preset",
            f,
            "set",
            "Gentle",
            "--fsrs-parameters",
            "1 2 3"
        ]))
        .is_empty()
    );
    out(&fc(&[
        "preset",
        f,
        "set",
        "Gentle",
        "--fsrs-parameters",
        "default",
    ]));
}

#[test]
fn mistakes_are_refused_with_a_message() {
    let file = TempFile::collection("decks", "errors.fc");
    let f = file.path();
    out(&fc(&["add-deck", f, "Polish"]));
    assert!(err(&fc(&["deck", f, "rename", "Nothing", "X"])).contains("No deck called"));
    assert!(!err(&fc(&["deck", f, "rename", "Polish", "A::B"])).is_empty());
    assert!(!err(&fc(&["deck", f, "delete", "Default"])).is_empty());
    assert!(!err(&fc(&["deck", f, "move", "Polish", "Polish"])).is_empty());
    assert!(err(&fc(&["deck", f, "restore", "Polish"])).contains("No deleted deck"));
    assert!(err(&fc(&["deck", f, "limits", "Polish", "maybe"])).contains("not on or off"));
    assert!(err(&fc(&["preset", f, "assign", "Polish", "Nope"])).contains("No preset called"));
    assert!(!err(&fc(&["preset", f, "delete", "Default"])).is_empty());
    assert!(err(&fc(&["preset", f, "set", "Default"])).contains("needs at least one option"));
    assert!(
        err(&fc(&[
            "preset",
            f,
            "set",
            "Default",
            "--new-per-day",
            "lots"
        ]))
        .contains("not a valid")
    );
    assert!(
        err(&fc(&[
            "preset",
            f,
            "set",
            "Default",
            "--learning-steps",
            "1 x"
        ]))
        .contains("minutes")
    );
    assert!(err(&fc(&["move-cards", f, "Polish"])).contains("needs card IDs"));
    assert!(err(&fc(&["move-cards", f, "Polish", "abc"])).contains("not a card ID"));
}
