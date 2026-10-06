//! Runs the real `fc` binary: create, open and close a collection, and the failure messages.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-{}-{name}.db", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn installation(&self) -> PathBuf {
        PathBuf::from(format!("{}.installation", self.path()))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.installation());
    }
}

fn fc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fc"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn creates_then_opens_and_closes_a_collection() {
    let file = TempFile::new("roundtrip");
    let created = fc(&["new", file.path()]);
    assert!(created.status.success(), "{}", stderr(&created));
    assert!(file.0.exists());

    let info = fc(&["info", file.path()]);
    assert!(info.status.success(), "{}", stderr(&info));
    let text = stdout(&info);
    assert!(text.contains("Storage version: 8 (this build understands up to 8)"));
    assert!(text.contains("Device ID: "));
    assert!(text.contains("Created by core: 0.0.0"));
}

#[test]
fn refuses_to_overwrite_and_to_open_what_is_missing() {
    let file = TempFile::new("refuse");
    assert!(fc(&["new", file.path()]).status.success());

    let again = fc(&["new", file.path()]);
    assert_eq!(again.status.code(), Some(1));
    assert!(stderr(&again).contains("already exists"));

    let missing = TempFile::new("missing");
    let info = fc(&["info", missing.path()]);
    assert_eq!(info.status.code(), Some(1));
    assert!(stderr(&info).contains("No collection exists"));
    assert!(!missing.0.exists());
    assert!(!missing.installation().exists());
}

fn device_id(output: &Output) -> String {
    stdout(output)
        .lines()
        .find_map(|line| line.strip_prefix("Device ID: "))
        .unwrap()
        .to_owned()
}

#[test]
fn the_device_id_is_kept_and_a_copied_collection_gets_its_own() {
    let original = TempFile::new("device-original");
    assert!(fc(&["new", original.path()]).status.success());
    let id = device_id(&fc(&["info", original.path()]));
    assert_eq!(id.len(), 36);
    assert_eq!(device_id(&fc(&["info", original.path()])), id);

    // Only the .db is copied, as when a collection is sent to another machine.
    let copy = TempFile::new("device-copy");
    std::fs::copy(&original.0, &copy.0).unwrap();
    assert_ne!(device_id(&fc(&["info", copy.path()])), id);
    // The original is not affected by the copy.
    assert_eq!(device_id(&fc(&["info", original.path()])), id);

    // Copying the installation file too is not a different installation: same device ID. This
    // is why a restore must regenerate it explicitly (step 1.13).
    let twin = TempFile::new("device-twin");
    std::fs::copy(&original.0, &twin.0).unwrap();
    std::fs::copy(original.installation(), twin.installation()).unwrap();
    assert_eq!(device_id(&fc(&["info", twin.path()])), id);
}

#[test]
fn a_collection_from_a_newer_version_fails_with_an_update_message() {
    let file = TempFile::new("newer");
    assert!(fc(&["new", file.path()]).status.success());
    // There is no CLI command to fake this, so set the version the way a newer app would have.
    let bytes = std::fs::read(&file.0).unwrap();
    let mut patched = bytes.clone();
    // `user_version` is the big-endian u32 at byte offset 60 of the SQLite header.
    patched[60..64].copy_from_slice(&99u32.to_be_bytes());
    std::fs::write(&file.0, &patched).unwrap();

    let info = fc(&["info", file.path()]);
    assert_eq!(info.status.code(), Some(1));
    assert!(stderr(&info).contains("newer version of the app"));
    assert_eq!(std::fs::read(&file.0).unwrap(), patched);
}

#[test]
fn bad_usage_exits_with_2_and_prints_the_usage() {
    for args in [&[][..], &["nope"], &["new"], &["info", "a", "b"]] {
        let output = fc(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).contains("Usage:"), "{args:?}");
    }
    assert!(fc(&["help"]).status.success());
}

#[test]
fn lists_the_built_in_note_types() {
    let file = TempFile::new("notetypes");
    assert!(fc(&["new", file.path()]).status.success());
    let listed = fc(&["notetypes", file.path()]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let text = stdout(&listed);
    assert!(text.contains("Basic (standard)"));
    assert!(text.contains("Fields: Front, Back"));
    assert!(text.contains("Basic and reversed (standard)"));
    assert!(text.contains("Templates: Card 1, Card 2"));
    assert!(text.contains("Cloze (cloze)"));
    assert!(text.contains("Fields: Text, Extra"));
    assert!(!text.contains("Deleted"));
}

#[test]
fn adds_notes_and_lists_them_with_their_cards() {
    let file = TempFile::new("notes");
    assert!(fc(&["new", file.path()]).status.success());

    let added = fc(&[
        "add-note",
        file.path(),
        "Basic and reversed",
        "Front=pies",
        "Back=dog",
    ]);
    assert!(added.status.success(), "{}", stderr(&added));
    assert!(stdout(&added).contains("2 cards"), "{}", stdout(&added));

    let cloze = fc(&[
        "add-note",
        file.path(),
        "Cloze",
        "Text={{c1::a}} and {{c2::b}}",
    ]);
    assert!(cloze.status.success(), "{}", stderr(&cloze));
    assert!(stdout(&cloze).contains("2 cards"));

    let plain = fc(&["add-note", file.path(), "Basic", "Front=pies", "Back=dog"]);
    assert!(plain.status.success(), "{}", stderr(&plain));
    assert!(!stdout(&plain).contains("duplicate"));
    let again = fc(&["add-note", file.path(), "basic", "Front=Pies", "Back=hound"]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(stdout(&again).contains("duplicate"), "{}", stdout(&again));

    let listed = fc(&["notes", file.path()]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let text = stdout(&listed);
    assert!(text.contains("Front: pies"));
    assert!(text.contains("Text: {{c1::a}} and {{c2::b}}"));
    assert!(text.contains("Cards: Card 1, Card 2"));
    assert!(text.contains("Cards: Cloze 1, Cloze 2"));
}

#[test]
fn add_note_explains_what_went_wrong() {
    let file = TempFile::new("notes-errors");
    assert!(fc(&["new", file.path()]).status.success());

    let unknown = fc(&["add-note", file.path(), "Nope", "Front=x"]);
    assert!(!unknown.status.success());
    assert!(stderr(&unknown).contains("No note type called \"Nope\""));
    assert!(stderr(&unknown).contains("Basic"));

    let bad_field = fc(&["add-note", file.path(), "Basic", "Question=x"]);
    assert!(!bad_field.status.success());
    assert!(stderr(&bad_field).contains("has no field \"Question\""));

    let no_cards = fc(&["add-note", file.path(), "Basic", "Back=x"]);
    assert!(!no_cards.status.success());
    assert!(stderr(&no_cards).contains("would make no cards"));
}

#[test]
fn renders_the_cards_of_a_note() {
    let file = TempFile::new("render");
    assert!(fc(&["new", file.path()]).status.success());
    let added = fc(&[
        "add-note",
        file.path(),
        "Cloze",
        "Text={{c1::pies}} is a dog",
        "Extra=<img src=\"pies.png\">",
    ]);
    assert!(added.status.success(), "{}", stderr(&added));
    let output = stdout(&added);
    let id = output.split_whitespace().nth(2).unwrap();

    let rendered = fc(&["render", file.path(), id]);
    assert!(rendered.status.success(), "{}", stderr(&rendered));
    let text = stdout(&rendered);
    assert!(text.contains("== Cloze 1 =="), "{text}");
    assert!(
        text.contains("<span class=\"cloze\">[...]</span> is a dog"),
        "{text}"
    );
    assert!(
        text.contains("<span class=\"cloze\">pies</span> is a dog"),
        "{text}"
    );
    assert!(text.contains("-- media: pies.png --"), "{text}");

    let missing = fc(&[
        "render",
        file.path(),
        "00000000-0000-0000-0000-000000000000",
    ]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("No note"));
    let bad = fc(&["render", file.path(), "x"]);
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn adds_decks_and_notes_to_them_and_lists_the_tree() {
    let file = TempFile::new("decks");
    assert!(fc(&["new", file.path()]).status.success());

    let polish = fc(&["add-deck", file.path(), "Polish"]);
    assert!(polish.status.success(), "{}", stderr(&polish));
    let verbs = fc(&["add-deck", file.path(), "polish::Verbs"]);
    assert!(verbs.status.success(), "{}", stderr(&verbs));
    let added = fc(&[
        "add-note",
        file.path(),
        "Basic and reversed",
        "--deck",
        "Polish::Verbs",
        "Front=pies",
        "Back=dog",
    ]);
    assert!(added.status.success(), "{}", stderr(&added));
    assert!(
        fc(&["add-note", file.path(), "Basic", "Front=a", "Back=b"])
            .status
            .success()
    );

    let listed = fc(&["decks", file.path()]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let text = stdout(&listed);
    assert!(
        text.contains("\nDefault (1 card, preset Default)"),
        "{text}"
    );
    assert!(
        text.contains("\nPolish (0 cards, preset Default)"),
        "{text}"
    );
    assert!(
        text.contains("\n  Verbs (2 cards, preset Default)"),
        "{text}"
    );
    assert!(
        text.contains(
            "Default: 20 new a day, 200 reviews a day, learning steps 1 10 minutes, desired retention 0.90, used by 3 decks"
        ),
        "{text}"
    );
}

#[test]
fn deck_commands_explain_what_went_wrong() {
    let file = TempFile::new("deck-errors");
    assert!(fc(&["new", file.path()]).status.success());

    let missing_parent = fc(&["add-deck", file.path(), "Nope::Child"]);
    assert!(!missing_parent.status.success());
    assert!(stderr(&missing_parent).contains("No deck called \"Nope\". The decks are: Default."));

    assert!(fc(&["add-deck", file.path(), "Polish"]).status.success());
    let again = fc(&["add-deck", file.path(), "polish"]);
    assert!(!again.status.success());
    assert!(stderr(&again).contains("There is already one called \"polish\" here."));

    let wrong_deck = fc(&[
        "add-note",
        file.path(),
        "Basic",
        "--deck",
        "Nope",
        "Front=a",
    ]);
    assert!(!wrong_deck.status.success());
    assert!(stderr(&wrong_deck).contains("No deck called \"Nope\""));

    let no_value = fc(&["add-note", file.path(), "Basic", "Front=a", "--deck"]);
    assert_eq!(no_value.status.code(), Some(2));
    assert!(stderr(&no_value).contains("--deck needs a deck."));
}

/// The ID in a line such as "Added note <id> with 1 card".
fn added_note_id(output: &Output) -> String {
    stdout(output)
        .split_whitespace()
        .nth(2)
        .expect("a note ID")
        .to_owned()
}

#[test]
fn tags_notes_lists_the_tree_and_renames_a_parent() {
    let file = TempFile::new("tags");
    assert!(fc(&["new", file.path()]).status.success());

    let first = fc(&[
        "add-note",
        file.path(),
        "Basic",
        "--tag",
        "lang::polish",
        "--tag",
        "verbs",
        "Front=pies",
        "Back=dog",
    ]);
    assert!(first.status.success(), "{}", stderr(&first));
    let first = added_note_id(&first);
    let second = fc(&["add-note", file.path(), "Basic", "Front=kot", "Back=cat"]);
    let second = added_note_id(&second);

    let tagged = fc(&["tag", file.path(), &second, "lang::german", "LANG::Polish"]);
    assert!(tagged.status.success(), "{}", stderr(&tagged));
    assert!(
        stdout(&tagged).contains("Added 2 tags to the note. Tags now: lang::german lang::polish"),
        "{}",
        stdout(&tagged)
    );

    let listed = stdout(&fc(&["tags", file.path()]));
    assert!(
        listed.contains("\nlang (0 notes, 2 with what is inside)"),
        "{listed}"
    );
    assert!(listed.contains("\n  german (1 note)"), "{listed}");
    assert!(listed.contains("\n  polish (2 notes)"), "{listed}");
    assert!(listed.contains("\nverbs (1 note)"), "{listed}");

    let renamed = fc(&["rename-tag", file.path(), "lang", "speech"]);
    assert!(renamed.status.success(), "{}", stderr(&renamed));
    assert!(
        stdout(&renamed).contains("on 2 notes"),
        "{}",
        stdout(&renamed)
    );
    let notes = stdout(&fc(&["notes", file.path()]));
    assert!(notes.contains("Tags: speech::polish verbs"), "{notes}");
    assert!(
        notes.contains("Tags: speech::german speech::polish"),
        "{notes}"
    );

    let untagged = fc(&["untag", file.path(), &first, "verbs"]);
    assert!(untagged.status.success(), "{}", stderr(&untagged));
    assert!(
        stdout(&untagged).contains("Removed 1 tag from the note. Tags now: speech::polish"),
        "{}",
        stdout(&untagged)
    );
}

#[test]
fn tag_commands_explain_what_went_wrong() {
    let file = TempFile::new("tag-errors");
    assert!(fc(&["new", file.path()]).status.success());
    let added = fc(&["add-note", file.path(), "Basic", "Front=a", "Back=b"]);
    let note = added_note_id(&added);

    let spaced = fc(&["tag", file.path(), &note, "two words"]);
    assert!(!spaced.status.success());
    assert!(
        stderr(&spaced).contains("has a space in it"),
        "{}",
        stderr(&spaced)
    );

    // A bad tag on add-note is refused before the note is made.
    let bad = fc(&[
        "add-note",
        file.path(),
        "Basic",
        "--tag",
        "a::",
        "Front=never",
    ]);
    assert!(!bad.status.success());
    assert!(stderr(&bad).contains("has an empty part"));
    assert!(!stdout(&fc(&["notes", file.path()])).contains("never"));

    let missing = fc(&["rename-tag", file.path(), "nope", "x"]);
    assert!(!missing.status.success());
    assert!(stderr(&missing).contains("No note has the tag \"nope\""));

    let not_an_id = fc(&["tag", file.path(), "123", "x"]);
    assert_eq!(not_an_id.status.code(), Some(2));
    assert!(stderr(&not_an_id).contains("is not a note ID"));

    let no_tag = fc(&["tag", file.path(), &note]);
    assert_eq!(no_tag.status.code(), Some(2));
    assert!(stderr(&no_tag).contains("needs a tag"));

    let empty = fc(&["tags", file.path()]);
    assert!(stdout(&empty).contains("(none)"));
}
