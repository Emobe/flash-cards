//! Runs the real `fc` binary: the fake collection and the bench.

use std::path::PathBuf;
use std::process::{Command, Output};

const NOW: &str = "2026-10-06T09:00:00+00:00";

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-fake-{}-{name}", std::process::id())))
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

/// The lines of `text` that start with `prefix`, without the file name (which differs per run).
fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting with {prefix:?} in:\n{text}"))
}

fn make(file: &TempFile, extra: &[&str]) -> String {
    let mut args = vec!["--now", NOW, "fake", file.path()];
    args.extend(extra);
    out(&fc(&args))
}

const SMALL: [&str; 10] = [
    "--notes",
    "120",
    "--decks",
    "8",
    "--review-days",
    "5",
    "--media",
    "4",
    "--seed",
    "7",
];

#[test]
fn makes_what_was_asked_for() {
    let file = TempFile::new("asked");
    let text = make(&file, &SMALL);
    assert!(line(&text, "Decks:").starts_with("Decks: 8 "), "{text}");
    assert!(
        line(&text, "Notes:").starts_with("Notes: 120 with "),
        "{text}"
    );
    assert!(line(&text, "Notes:").contains("4 media files"), "{text}");
    assert!(
        line(&text, "Review history:").contains("over 5 days"),
        "{text}"
    );

    let decks = out(&fc(&["decks", file.path()]));
    assert!(decks.contains("Deck 1"), "{decks}");
    assert!(decks.contains("Part"), "sub-decks are made: {decks}");
    // Every media file is used by a note.
    let media = out(&fc(&["media-check", file.path()]));
    assert!(media.starts_with("0 unused, 0 missing"), "{media}");
    // The history is real: the core counts the answers.
    let stats = out(&fc(&["--now", NOW, "stats", file.path(), "--days", "5"]));
    assert!(stats.contains(" answers in 5 days"), "{stats}");
    // Cards are left to study.
    let due = out(&fc(&["--now", NOW, "due", file.path()]));
    assert!(due.contains("new "), "{due}");
}

#[test]
fn the_same_seed_gives_the_same_notes_and_answers() {
    let (first, second, other) = (
        TempFile::new("same-1"),
        TempFile::new("same-2"),
        TempFile::new("same-3"),
    );
    make(&first, &SMALL);
    make(&second, &SMALL);
    make(
        &other,
        &[
            "--notes",
            "120",
            "--decks",
            "8",
            "--review-days",
            "5",
            "--seed",
            "8",
        ],
    );
    // IDs are random, and the scheduler's fuzz is seeded from them, so due dates can differ. The
    // notes, tags and the answers made each day do not.
    let view = |file: &TempFile| {
        [
            out(&fc(&["tags", file.path()])).replace(file.path(), ""),
            out(&fc(&["--now", NOW, "stats", file.path(), "--days", "5"])),
        ]
        .join("\n")
    };
    assert_eq!(view(&first), view(&second));
    assert_ne!(view(&first), view(&other));
}

#[test]
fn never_replaces_a_file() {
    let file = TempFile::new("exists");
    make(&file, &["--notes", "5", "--decks", "1"]);
    let before = std::fs::read(file.path()).unwrap();
    let message = err(&fc(&["fake", file.path()]));
    assert!(message.contains("exists already"), "{message}");
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
}

#[test]
fn bad_options_are_usage_errors() {
    let file = TempFile::new("bad");
    for args in [
        vec!["--notes"],
        vec!["--notes", "many"],
        vec!["--colour", "red"],
        vec!["--new-per-day", "400", "--reviews-per-day", "400"],
    ] {
        let mut line = vec!["fake", file.path()];
        line.extend(&args);
        let output = fc(&line);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    assert!(!std::path::Path::new(file.path()).exists());
}

#[test]
fn a_collection_with_no_decks_or_history_is_fine() {
    let file = TempFile::new("plain");
    let text = make(&file, &["--notes", "30", "--decks", "0"]);
    assert!(
        line(&text, "Review history:").contains("0 answers"),
        "{text}"
    );
    let search = out(&fc(&["search", file.path(), "", "--limit", "1"]));
    assert!(search.contains("found") || search.contains("Found") || !search.is_empty());
}

#[test]
fn the_fake_collection_merges_and_exports() {
    let file = TempFile::new("merge-from");
    let into = TempFile::new("merge-into");
    let backup = TempFile::new("merge-backup");
    make(&file, &SMALL);
    out(&fc(&["new", into.path()]));
    let merged = out(&fc(&["merge", file.path(), into.path()]));
    assert!(!merged.contains("could not be applied"), "{merged}");
    out(&fc(&["export", file.path(), backup.path()]));
    let info = out(&fc(&["backup-info", backup.path()]));
    assert!(info.contains("Media files: 4"), "{info}");
}

#[test]
fn bench_times_a_copy_and_leaves_the_collection_alone() {
    let file = TempFile::new("bench.db");
    make(&file, &SMALL);
    let before = std::fs::read(file.path()).unwrap();
    let text = out(&fc(&["--now", NOW, "bench", file.path()]));
    for name in [
        "deck counts",
        "next card",
        "search is:due",
        "tags()",
        "export, with history",
        "merge into a new collection",
        "rebuild_schedule",
        "add a template to Basic",
    ] {
        assert!(text.contains(name), "no {name:?} in:\n{text}");
    }
    assert!(!text.contains("failed:"), "{text}");
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name().to_string_lossy().contains(&format!(
                "fc-cli-fake-{}-bench.db.bench",
                std::process::id()
            ))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn bench_needs_a_collection() {
    let file = TempFile::new("missing");
    let message = err(&fc(&["bench", file.path()]));
    assert!(message.contains("There is no collection"), "{message}");
}
