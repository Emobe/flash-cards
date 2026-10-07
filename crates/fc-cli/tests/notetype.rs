//! Runs the real `fc` binary: note types, fields and templates.

mod common;
use common::*;

#[test]
fn a_note_type_goes_through_its_whole_life() {
    let file = TempFile::collection("notetype", "life.fc");
    let f = file.path();

    let added = out(&fc(&["notetype", f, "add", "Vocab"]));
    assert!(added.starts_with("Added note type Vocab"), "{added}");
    out(&fc(&["notetype", f, "rename", "vocab", "Words"]));
    let listing = out(&fc(&["notetypes", f]));
    assert!(listing.contains("Words (standard)"), "{listing}");
    assert!(!listing.contains("Vocab"), "{listing}");

    // CSS from the line and from a file.
    out(&fc(&[
        "notetype",
        f,
        "css",
        "Words",
        ".card { color: red }",
    ]));
    let css = TempFile::new("notetype", "style.css");
    std::fs::write(css.path(), ".card { color: blue }").unwrap();
    out(&fc(&[
        "notetype",
        f,
        "css",
        "Words",
        &format!("@{}", css.path()),
    ]));
    drop(css);

    // A note on it, then the type is deleted and restored with its note.
    let note = note_id(&out(&fc(&[
        "add-note",
        f,
        "Words",
        "Front=kot",
        "Back=cat",
    ])));
    out(&fc(&["notetype", f, "sort-field", "Words", "Back"]));
    out(&fc(&["notetype", f, "delete", "Words"]));
    // The note is not in the live list any more, only in the one that can be restored.
    let notes = out(&fc(&["notes", f]));
    let (live, trash) = notes.split_once("Deleted (can be restored)").unwrap();
    assert!(!live.contains(&note) && trash.contains(&note), "{notes}");
    let listing = out(&fc(&["notetypes", f]));
    assert!(listing.contains("Deleted (can be restored)"), "{listing}");
    out(&fc(&["notetype", f, "restore", "Words"]));
    assert!(!out(&fc(&["notes", f])).contains("Deleted (can be restored)"));
    assert!(out(&fc(&["notes", f])).contains(&note));
}

#[test]
fn fields_can_be_added_renamed_moved_removed_and_restored() {
    let file = TempFile::collection("notetype", "fields.fc");
    let f = file.path();
    out(&fc(&["notetype", f, "add", "Words"]));

    let added = out(&fc(&["field", f, "Words", "add", "Example"]));
    assert!(added.starts_with("Added field Example to Words"), "{added}");
    out(&fc(&["field", f, "Words", "rename", "Example", "Sentence"]));
    out(&fc(&["field", f, "Words", "move", "Sentence", "1"]));
    let listing = out(&fc(&["notetypes", f]));
    assert!(
        listing.contains("Fields: Sentence, Front, Back"),
        "{listing}"
    );

    out(&fc(&["field", f, "Words", "remove", "Sentence"]));
    let listing = out(&fc(&["notetypes", f]));
    assert!(listing.contains("Removed fields: Sentence ("), "{listing}");
    out(&fc(&["field", f, "Words", "restore", "Sentence"]));
    let listing = out(&fc(&["notetypes", f]));
    assert!(
        listing.contains("Fields: Sentence, Front, Back"),
        "{listing}"
    );
    assert!(!listing.contains("Removed fields"), "{listing}");
}

#[test]
fn renaming_a_field_rewrites_the_templates_that_use_it() {
    let file = TempFile::collection("notetype", "rewrite.fc");
    let f = file.path();
    let note = note_id(&out(&fc(&[
        "add-note",
        f,
        "Basic",
        "Front=kot",
        "Back=cat",
    ])));
    out(&fc(&["field", f, "Basic", "rename", "Front", "Polish"]));
    let rendered = out(&fc(&["render", f, &note]));
    assert!(rendered.contains("kot"), "{rendered}");
}

#[test]
fn templates_can_be_added_changed_moved_removed_and_restored() {
    let file = TempFile::collection("notetype", "templates.fc");
    let f = file.path();
    let note = note_id(&out(&fc(&[
        "add-note",
        f,
        "Basic",
        "Front=kot",
        "Back=cat",
    ])));
    assert!(out(&fc(&["notes", f])).contains("Cards: Card 1\n"));

    // A new template makes a card for the existing note.
    let added = out(&fc(&[
        "template",
        f,
        "Basic",
        "add",
        "Reverse",
        "{{Back}}",
        "{{Front}}",
    ]));
    assert!(added.starts_with("Added template Reverse"), "{added}");
    assert!(out(&fc(&["notes", f])).contains("Cards: Card 1, Reverse"));

    // Text from a file, one side at a time.
    let back = TempFile::new("notetype", "back.html");
    std::fs::write(back.path(), "{{FrontSide}}<hr>[{{Back}}]").unwrap();
    out(&fc(&[
        "template",
        f,
        "Basic",
        "set",
        "Card 1",
        "--back",
        &format!("@{}", back.path()),
    ]));
    let rendered = out(&fc(&["render", f, &note]));
    assert!(rendered.contains("[cat]"), "{rendered}");

    out(&fc(&["template", f, "Basic", "move", "Reverse", "1"]));
    assert!(out(&fc(&["notes", f])).contains("Cards: Reverse, Card 1"));

    out(&fc(&["template", f, "Basic", "remove", "Reverse"]));
    assert!(out(&fc(&["notes", f])).contains("Cards: Card 1\n"));
    assert!(out(&fc(&["notetypes", f])).contains("Removed templates: Reverse ("));
    out(&fc(&["template", f, "Basic", "restore", "Reverse"]));
    assert!(
        out(&fc(&["notes", f])).contains("Cards: Card 1, Reverse")
            || out(&fc(&["notes", f])).contains("Cards: Reverse, Card 1")
    );
}

#[test]
fn mistakes_are_refused_with_a_message() {
    let file = TempFile::collection("notetype", "errors.fc");
    let f = file.path();
    assert!(err(&fc(&["notetype", f, "rename", "Nothing", "X"])).contains("No note type called"));
    assert!(err(&fc(&["field", f, "Basic", "rename", "Nope", "X"])).contains("No field called"));
    // A template with a syntax error is refused and nothing changes.
    let bad = err(&fc(&[
        "template", f, "Basic", "set", "Card 1", "--front", "{{Front",
    ]));
    assert!(!bad.is_empty());
    assert!(out(&fc(&["notetypes", f])).contains("Basic (standard)"));
    // The last template and a cloze note type's only template cannot go.
    assert!(!err(&fc(&["template", f, "Basic", "remove", "Card 1"])).is_empty());
    assert!(
        !err(&fc(&[
            "template",
            f,
            "Cloze",
            "add",
            "Two",
            "{{Text}}",
            "{{Extra}}"
        ]))
        .is_empty()
    );
    // Usage mistakes.
    assert!(err(&fc(&["notetype", f, "frobnicate"])).contains("Wrong arguments"));
    assert!(err(&fc(&["field", f, "Basic", "move", "Front", "0"])).contains("not a position"));
    assert!(err(&fc(&["template", f, "Basic", "set", "Card 1"])).contains("needs --front"));
    assert!(err(&fc(&["notetype", f, "css", "Basic", "@/no/such/file"])).contains("Cannot read"));
}
