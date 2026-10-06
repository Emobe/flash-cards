//! Runs the real `fc` binary: answering cards by hand, with a pretend clock.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-study-{}-{name}.db", std::process::id())))
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

/// A collection with one Basic note, and the ID of its card.
fn with_a_card(file: &TempFile) -> String {
    out(&fc(&["new", file.path()]));
    out(&fc(&[
        "add-note",
        file.path(),
        "Basic",
        "Front=hello",
        "Back=world",
    ]));
    let notes = out(&fc(&["notes", file.path()]));
    notes
        .lines()
        .find_map(|line| line.trim().strip_prefix("Card IDs: "))
        .unwrap()
        .to_owned()
}

#[test]
fn answers_a_card_through_its_steps_to_a_review_and_shows_the_schedule() {
    let file = TempFile::new("steps");
    let card = with_a_card(&file);
    let first = out(&fc(&[
        "answer",
        file.path(),
        &card,
        "good",
        "--now",
        "2026-10-06T09:00:00+00:00",
    ]));
    assert!(
        first.contains("Answered Good. Now Learning (step 2)"),
        "{first}"
    );
    assert!(
        first.contains("due 2026-10-06T09:10:00+00:00 (in 10 minutes)"),
        "{first}"
    );

    let second = out(&fc(&[
        "answer",
        file.path(),
        &card,
        "Good",
        "--now",
        "2026-10-06T09:10:00+00:00",
    ]));
    assert!(second.contains("Now Review"), "{second}");
    // 2026-10-06 is study day 20,732, and the first review is two days later.
    assert!(
        second.contains("on study day 20734 (2026-10-08), in 2 days"),
        "{second}"
    );

    let schedule = out(&fc(&[
        "schedule",
        file.path(),
        &card,
        "--now",
        "2026-10-07T09:00:00+00:00",
    ]));
    assert!(schedule.contains("State: Review"), "{schedule}");
    assert!(
        schedule.contains("Due: on study day 20734 (2026-10-08), in 1 day"),
        "{schedule}"
    );
    assert!(schedule.contains("Answers: 2, lapses: 0"), "{schedule}");
    assert!(schedule.contains("Events: 2"), "{schedule}");
    assert!(
        schedule.contains("(day 20732) Good: New -> Learning"),
        "{schedule}"
    );
    assert!(
        schedule.contains("(day 20732) Good: Learning -> Review"),
        "{schedule}"
    );
}

#[test]
fn a_utc_offset_moves_the_study_day() {
    let file = TempFile::new("offset");
    let card = with_a_card(&file);
    // 23:30 UTC is already the next day at UTC+2.
    out(&fc(&[
        "answer",
        file.path(),
        &card,
        "good",
        "--now",
        "2026-10-06T23:30:00+00:00",
        "--utc-offset",
        "120",
    ]));
    let schedule = out(&fc(&["schedule", file.path(), &card]));
    assert!(schedule.contains("(day 20733) Good"), "{schedule}");
    // The offset in the time given is used when there is no --utc-offset.
    let other = TempFile::new("offset-in-time");
    let card = with_a_card(&other);
    out(&fc(&[
        "answer",
        other.path(),
        &card,
        "good",
        "--now",
        "2026-10-07T01:30:00+02:00",
    ]));
    let schedule = out(&fc(&["schedule", other.path(), &card]));
    assert!(
        schedule.contains("2026-10-07T01:30:00+02:00 (day 20733) Good"),
        "{schedule}"
    );
}

#[test]
fn undoes_answers_one_at_a_time() {
    let file = TempFile::new("undo");
    let card = with_a_card(&file);
    assert!(out(&fc(&["undo", file.path()])).contains("There is nothing to undo."));
    for (time, answer) in [
        ("2026-10-06T09:00:00+00:00", "good"),
        ("2026-10-06T09:10:00+00:00", "easy"),
    ] {
        out(&fc(&["answer", file.path(), &card, answer, "--now", time]));
    }
    let undone = out(&fc(&[
        "undo",
        file.path(),
        "--now",
        "2026-10-06T09:11:00+00:00",
    ]));
    assert!(undone.contains("Undid Easy"), "{undone}");
    assert!(undone.contains("It is now Learning"), "{undone}");
    let undone = out(&fc(&["undo", file.path()]));
    assert!(undone.contains("Undid Good"), "{undone}");
    assert!(undone.contains("It is now New"), "{undone}");
    assert!(out(&fc(&["undo", file.path()])).contains("There is nothing to undo."));
    let schedule = out(&fc(&["schedule", file.path(), &card]));
    assert!(schedule.contains("State: New"), "{schedule}");
    assert!(schedule.contains("Events: 4"), "{schedule}");
    assert!(schedule.contains("undo of"), "{schedule}");
}

#[test]
fn explains_what_went_wrong() {
    let file = TempFile::new("errors");
    let card = with_a_card(&file);
    let wrong = fc(&["answer", file.path(), &card, "ok"]);
    assert_eq!(wrong.status.code(), Some(2));
    assert!(err(&wrong).contains("\"ok\" is not an answer. Use again, hard, good or easy."));
    let wrong = fc(&["answer", file.path(), "123", "good"]);
    assert_eq!(wrong.status.code(), Some(2));
    assert!(err(&wrong).contains("\"123\" is not a card ID"));
    let missing = fc(&[
        "answer",
        file.path(),
        "6d13136c-b899-571c-abde-7d442cf14223",
        "good",
    ]);
    assert!(err(&missing).contains("That card no longer exists."));
    let wrong = fc(&["info", file.path(), "--now", "yesterday"]);
    assert_eq!(wrong.status.code(), Some(2));
    assert!(err(&wrong).contains("\"yesterday\" is not a time"));
    let wrong = fc(&["info", file.path(), "--utc-offset", "9999"]);
    assert_eq!(wrong.status.code(), Some(2));
    assert!(err(&wrong).contains("not a UTC offset"));
    let wrong = fc(&["info", file.path(), "--now"]);
    assert!(err(&wrong).contains("--now needs a value."));
    let wrong = fc(&["answer", file.path()]);
    assert!(err(&wrong).contains("Wrong number of arguments for \"answer\"."));
}
