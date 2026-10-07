//! Runs the real `fc` binary: editing and deleting notes, tags, media and saved searches, and the
//! settings commands.

mod common;
use common::*;

#[test]
fn a_note_can_be_edited_deleted_and_restored() {
    let file = TempFile::collection("edit", "notes.fc");
    let f = file.path();
    let first = note_id(&out(&fc(&[
        "add-note",
        f,
        "Basic",
        "Front=kot",
        "Back=cat",
    ])));
    let second = note_id(&out(&fc(&[
        "add-note",
        f,
        "Basic",
        "Front=pies",
        "Back=dog",
    ])));

    let edited = out(&fc(&["edit-note", f, &first, "Back=kitten"]));
    assert!(edited.contains("Changed note"), "{edited}");
    let notes = out(&fc(&["notes", f]));
    assert!(
        notes.contains("Back: kitten") && notes.contains("Front: kot"),
        "{notes}"
    );

    // Editing the first field into another note's value warns.
    let edited = out(&fc(&["edit-note", f, &first, "Front=pies"]));
    assert!(edited.contains("Warning: 1 duplicate"), "{edited}");

    let found = out(&fc(&["find-duplicates", f, "Basic", "PIES"]));
    assert!(
        found.starts_with("2 notes with that first field"),
        "{found}"
    );
    assert!(out(&fc(&["find-duplicates", f, "Basic", "nothing"])).starts_with("No Basic note"));

    let deleted = out(&fc(&["delete-note", f, &first, &second]));
    assert!(deleted.starts_with("Deleted 2 notes"), "{deleted}");
    let notes = out(&fc(&["notes", f]));
    let (live, trash) = notes.split_once("Deleted (can be restored)").unwrap();
    assert!(!live.contains(&first) && trash.contains(&first) && trash.contains(&second));
    let restored = out(&fc(&["restore-note", f, &first]));
    assert!(
        restored.contains("Cards added or brought back: 1"),
        "{restored}"
    );
    let notes = out(&fc(&["notes", f]));
    assert!(
        notes
            .split("Deleted (can be restored)")
            .next()
            .unwrap()
            .contains(&first)
    );
}

#[test]
fn editing_refuses_what_it_cannot_do() {
    let file = TempFile::collection("edit", "errors.fc");
    let f = file.path();
    let note = note_id(&out(&fc(&["add-note", f, "Basic", "Front=kot"])));
    assert!(err(&fc(&["edit-note", f, &note, "Nope=x"])).contains("has no field"));
    assert!(err(&fc(&["edit-note", f, &note, "Front"])).contains("not Field=value"));
    assert!(err(&fc(&["edit-note", f, &note])).contains("at least one"));
    assert!(err(&fc(&["edit-note", f, "abc", "Front=x"])).contains("not a note ID"));
    assert!(err(&fc(&["delete-note", f])).contains("needs note IDs"));
    // A deleted note cannot be edited.
    out(&fc(&["delete-note", f, &note]));
    assert!(err(&fc(&["edit-note", f, &note, "Front=x"])).contains("No note with that ID"));
}

#[test]
fn tags_can_be_set_and_deleted() {
    let file = TempFile::collection("edit", "tags.fc");
    let f = file.path();
    let a = note_id(&out(&fc(&[
        "add-note", f, "Basic", "--tag", "old", "Front=a",
    ])));
    let b = note_id(&out(&fc(&[
        "add-note",
        f,
        "Basic",
        "--tag",
        "old::inner",
        "Front=b",
    ])));

    let set = out(&fc(&["set-tags", f, &a, "verbs", "ch::sz"]));
    assert!(
        set.contains("Tags now: ch::sz verbs") || set.contains("verbs"),
        "{set}"
    );
    assert!(!set.contains("old"), "{set}");
    assert_eq!(out(&fc(&["set-tags", f, &a])).trim_end(), "Tags now: none");

    out(&fc(&["tag", f, &a, "old"]));
    let tagged = out(&fc(&["tagged", f, "old"]));
    assert!(
        tagged.starts_with("2 notes with old") && tagged.contains(&a),
        "{tagged}"
    );
    let tagged = out(&fc(&["tagged", f, "old", "--no-children"]));
    assert!(
        tagged.starts_with("1 note with old") && !tagged.contains(&b),
        "{tagged}"
    );
    assert!(err(&fc(&["tagged", f, "old", "--bogus"])).contains("--no-children"));
    let deleted = out(&fc(&["delete-tag", f, "old"]));
    assert!(deleted.contains("from 2 notes"), "{deleted}");
    let tags = out(&fc(&["tags", f]));
    assert!(tags.contains("(none)"), "{tags}");
    let notes = out(&fc(&["notes", f]));
    assert!(!notes.contains("Tags: old"), "{notes}");
    assert!(notes.contains(&b), "{notes}");
    assert!(!err(&fc(&["set-tags", f, &a, "two words"])).is_empty());
}

#[test]
fn media_can_be_deleted_and_restored() {
    let file = TempFile::collection("edit", "media.fc");
    let f = file.path();
    let image = TempFile::new("edit", "dot.png");
    std::fs::write(image.path(), b"not really a png").unwrap();
    let added = out(&fc(&["add-media", f, image.path()]));
    let name = added
        .strip_prefix("Added ")
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .to_owned();

    let deleted = out(&fc(&["delete-media", f, &name]));
    assert!(deleted.contains("restore-media"), "{deleted}");
    assert!(out(&fc(&["media", f])).contains("deleted"));
    out(&fc(&["restore-media", f, &name]));
    assert!(!out(&fc(&["media", f])).contains("deleted"));

    // A file a note uses cannot be deleted.
    out(&fc(&[
        "add-note",
        f,
        "Basic",
        &format!("Front=<img src=\"{name}\">"),
    ]));
    let refused = err(&fc(&["delete-media", f, &name]));
    assert!(refused.contains("still used by a note"), "{refused}");
    assert!(!out(&fc(&["media", f])).contains("deleted"));
    assert!(!err(&fc(&["delete-media", f, "missing.png"])).is_empty());
}

#[test]
fn a_saved_search_can_be_updated() {
    let file = TempFile::collection("edit", "search.fc");
    let f = file.path();
    out(&fc(&["save-search", f, "Cats", "cat"]));
    let updated = out(&fc(&[
        "update-search",
        f,
        "cats",
        "--name",
        "Kittens",
        "--query",
        "kitten",
        "--sort",
        "created:desc",
        "--notes",
    ]));
    assert!(
        updated.contains("Updated \"Kittens\": kitten (notes, sorted by created)"),
        "{updated}"
    );
    let listing = out(&fc(&["searches", f]));
    assert!(
        listing.contains("Kittens") && !listing.contains("Cats"),
        "{listing}"
    );
    // Only the query changes; the rest stays.
    let updated = out(&fc(&["update-search", f, "Kittens", "--cards"]));
    assert!(updated.contains("(cards, sorted by created)"), "{updated}");

    assert!(err(&fc(&["update-search", f, "Kittens"])).contains("needs --name"));
    assert!(err(&fc(&["update-search", f, "Nope", "--name", "X"])).contains("No saved search"));
    assert!(!err(&fc(&["update-search", f, "Kittens", "--query", "(("])).is_empty());
    assert!(!err(&fc(&["update-search", f, "Kittens", "--sort", "sideways"])).is_empty());
}

#[test]
fn settings_can_be_read_and_changed() {
    let file = TempFile::collection("edit", "settings.fc");
    let f = file.path();
    assert!(out(&fc(&["day-start-hour", f])).contains("starts at 0:00"));
    assert!(out(&fc(&["day-start-hour", f, "4"])).contains("now starts at 4:00"));
    assert!(out(&fc(&["day-start-hour", f])).contains("starts at 4:00"));
    assert!(!err(&fc(&["day-start-hour", f, "24"])).is_empty());
    assert!(err(&fc(&["day-start-hour", f, "late"])).contains("not a valid value"));

    let shown = out(&fc(&["backup-settings", f]));
    assert!(shown.contains("every 24 hours, keeping 5"), "{shown}");
    let changed = out(&fc(&[
        "backup-settings",
        f,
        "--interval",
        "0",
        "--keep",
        "3",
    ]));
    assert!(changed.contains("off, keeping 3"), "{changed}");
    // One option changes only that one.
    let changed = out(&fc(&["backup-settings", f, "--interval", "12"]));
    assert!(changed.contains("every 12 hours, keeping 3"), "{changed}");
    assert!(!err(&fc(&["backup-settings", f, "--keep", "0"])).is_empty());
    assert!(err(&fc(&["backup-settings", f, "--bogus", "1"])).contains("Unknown option"));

    let before = out(&fc(&["device-id", f]));
    assert_eq!(out(&fc(&["device-id", f])), before);
    let after = out(&fc(&["device-id", f, "--regenerate"]));
    assert!(after.starts_with("New device ID: "), "{after}");
    assert_ne!(
        after.trim_end().strip_prefix("New device ID: "),
        before.trim_end().strip_prefix("Device ID: ")
    );

    // The schedule is rebuilt from the answers, so it comes out the same.
    out(&fc(&["add-note", f, "Basic", "Front=a", "Back=b"]));
    let card = out(&fc(&["notes", f]))
        .lines()
        .find_map(|l| l.trim().strip_prefix("Card IDs: ").map(str::to_owned))
        .unwrap();
    out(&fc(&["answer", f, &card, "good"]));
    let schedule = out(&fc(&["schedule", f, &card]));
    assert!(out(&fc(&["rebuild-schedule", f])).starts_with("Rebuilt"));
    assert_eq!(out(&fc(&["schedule", f, &card])), schedule);
}
