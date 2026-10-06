//! Runs the real `fc` binary: search and saved searches.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-search-{}-{name}.db", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(format!("{}.installation", self.path()));
    }
}

fn fc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fc"))
        .args(args)
        .output()
        .unwrap()
}

fn out(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn collection(file: &TempFile) {
    out(&fc(&["new", file.path()]));
    out(&fc(&["add-deck", file.path(), "Polish"]));
    for (front, deck, tag) in [
        ("kot", "Polish", "animals"),
        ("Łódź", "Polish", "places"),
        ("chat", "Default", "animals"),
    ] {
        out(&fc(&[
            "add-note",
            file.path(),
            "Basic",
            "--deck",
            deck,
            "--tag",
            tag,
            &format!("Front={front}"),
            "Back=x",
        ]));
    }
}

#[test]
fn finds_cards_and_prints_their_deck_and_state() {
    let file = TempFile::new("find");
    collection(&file);
    let text = out(&fc(&["search", file.path(), "deck:Polish tag:animals"]));
    assert!(text.starts_with("Found 1 card, showing 1 to 1:"), "{text}");
    assert!(
        text.contains("New") && text.contains("Polish") && text.contains("kot"),
        "{text}"
    );
    let text = out(&fc(&["search", file.path(), "ŁÓDŹ"]));
    assert!(text.contains("Łódź"), "{text}");
    let text = out(&fc(&["search", file.path(), "nothing at all"]));
    assert_eq!(text.trim(), "Found 0 cards");
}

#[test]
fn sorts_pages_and_lists_notes() {
    let file = TempFile::new("sort");
    collection(&file);
    let text = out(&fc(&[
        "search",
        file.path(),
        "",
        "--sort",
        "field",
        "--limit",
        "2",
    ]));
    assert!(text.starts_with("Found 3 cards, showing 1 to 2:"), "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[1].contains("chat") && lines[2].contains("kot"),
        "{text}"
    );
    let text = out(&fc(&[
        "search",
        file.path(),
        "",
        "--sort",
        "field:desc",
        "--offset",
        "2",
        "--notes",
    ]));
    assert!(text.starts_with("Found 3 notes, showing 3 to 3:"), "{text}");
    assert!(text.contains("chat"), "{text}");
}

#[test]
fn a_query_that_cannot_be_read_says_where() {
    let file = TempFile::new("syntax");
    collection(&file);
    let message = err(&fc(&["search", file.path(), "kot (or"]));
    assert!(message.contains("at character"), "{message}");
    let message = err(&fc(&["search", file.path(), "decks:Polish"]));
    assert!(
        message.contains("Did you mean the filter \"deck:\""),
        "{message}"
    );
    let message = err(&fc(&["search", file.path(), "", "--sort", "size"]));
    assert!(message.contains("not a way to sort"), "{message}");
}

#[test]
fn saves_runs_lists_and_deletes_a_search() {
    let file = TempFile::new("saved");
    collection(&file);
    assert!(out(&fc(&["searches", file.path()])).contains("No saved searches"));
    out(&fc(&[
        "save-search",
        file.path(),
        "Animals",
        "tag:animals",
        "--sort",
        "field",
        "--notes",
    ]));
    let listed = out(&fc(&["searches", file.path()]));
    assert!(
        listed.contains("Animals")
            && listed.contains("[notes, field:asc]")
            && listed.contains("tag:animals"),
        "{listed}"
    );
    let ran = out(&fc(&["run-search", file.path(), "animals"]));
    assert!(ran.starts_with("Found 2 notes, showing 1 to 2:"), "{ran}");
    assert!(
        ran.lines().nth(1).unwrap().contains("chat"),
        "sorted by field: {ran}"
    );
    let message = err(&fc(&["save-search", file.path(), "animals", "kot"]));
    assert!(message.contains("already a saved search"), "{message}");
    out(&fc(&["delete-search", file.path(), "Animals"]));
    let message = err(&fc(&["run-search", file.path(), "Animals"]));
    assert!(message.contains("No saved search called"), "{message}");
}
