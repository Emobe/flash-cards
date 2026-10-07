//! Runs the real `fc` binary: export, restore and import (step 1.13a).

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-backup-{}-{name}", std::process::id())))
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

fn add(file: &TempFile, front: &str, extra: &[&str]) {
    let front = format!("Front={front}");
    let mut args = vec!["add-note", file.path(), "Basic"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&[&front, "Back=b"]);
    out(&fc(&args));
}

#[test]
fn a_backup_restores_after_the_collection_changed_and_never_overwrites_a_file() {
    let (db, backup) = (TempFile::new("a.db"), TempFile::new("a.fcbackup"));
    out(&fc(&["new", db.path()]));
    add(&db, "before", &[]);

    let text = out(&fc(&["export", db.path(), backup.path()]));
    assert!(text.contains("Holds: the whole collection"), "{text}");
    assert!(text.contains("Review history: included"), "{text}");
    let info = out(&fc(&["backup-info", backup.path()]));
    assert!(info.contains("Registers: "), "{info}");
    assert!(info.contains("Format version: 1"), "{info}");

    let again = err(&fc(&["export", db.path(), backup.path()]));
    assert!(again.contains("exists already"), "{again}");

    add(&db, "after", &[]);
    assert!(out(&fc(&["notes", db.path()])).contains("after"));
    let report = out(&fc(&["restore", db.path(), backup.path()]));
    assert!(report.starts_with("Restored "), "{report}");
    assert!(report.contains("Removed (to the trash"), "{report}");
    let notes = out(&fc(&["notes", db.path()]));
    assert!(notes.contains("before"), "{notes}");
    // It is in the trash, which `notes` lists after the live notes.
    let live = notes.split("Deleted (can be restored)").next().unwrap();
    assert!(!live.contains("after"), "{notes}");

    // Restoring it again finds nothing to do.
    let report = out(&fc(&["restore", db.path(), backup.path()]));
    assert!(report.contains("Registers written: 0"), "{report}");
}

#[test]
fn a_deck_is_exported_with_its_notes_and_imported_into_another_collection() {
    let (from, to, backup) = (
        TempFile::new("from.db"),
        TempFile::new("to.db"),
        TempFile::new("deck.fcbackup"),
    );
    out(&fc(&["new", from.path()]));
    out(&fc(&["add-deck", from.path(), "Polish"]));
    add(&from, "kot", &["--deck", "Polish"]);
    add(&from, "elsewhere", &[]);
    out(&fc(&["new", to.path()]));

    let text = out(&fc(&[
        "export",
        from.path(),
        backup.path(),
        "--deck",
        "Polish",
        "--no-history",
    ]));
    assert!(text.contains("the deck \"Polish\""), "{text}");
    assert!(text.contains("Review history: not included"), "{text}");

    // A deck file is not a backup of a whole collection.
    let refused = err(&fc(&["restore", to.path(), backup.path()]));
    assert!(refused.contains("holds one deck"), "{refused}");

    let report = out(&fc(&["import", to.path(), backup.path()]));
    assert!(report.starts_with("Imported "), "{report}");
    let notes = out(&fc(&["notes", to.path()]));
    assert!(notes.contains("kot"), "{notes}");
    assert!(!notes.contains("elsewhere"), "{notes}");
    assert!(out(&fc(&["decks", to.path()])).contains("Polish"));
}

#[test]
fn a_file_that_is_not_a_backup_is_refused_in_plain_words() {
    let (db, junk) = (TempFile::new("c.db"), TempFile::new("junk"));
    out(&fc(&["new", db.path()]));
    std::fs::write(junk.path(), b"not a backup").unwrap();
    let text = err(&fc(&["restore", db.path(), junk.path()]));
    assert!(text.contains("not a flash card backup"), "{text}");
    let text = err(&fc(&["backup-info", junk.path()]));
    assert!(text.contains("not a flash card backup"), "{text}");
    let text = err(&fc(&["restore", db.path(), "/no/such/file.fcbackup"]));
    assert!(text.contains("Could not open"), "{text}");
}
