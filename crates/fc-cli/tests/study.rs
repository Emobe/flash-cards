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
    assert!(err(&missing).contains("That card or deck no longer exists."));
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

/// A collection with two decks and cards: two in Default, one in Polish and one in Polish::Food.
fn with_decks(file: &TempFile) -> Vec<String> {
    out(&fc(&["new", file.path()]));
    out(&fc(&["add-deck", file.path(), "Polish"]));
    out(&fc(&["add-deck", file.path(), "Polish::Food"]));
    for (deck, front) in [
        ("Default", "a"),
        ("Default", "b"),
        ("Polish", "c"),
        ("Polish::Food", "d"),
    ] {
        out(&fc(&[
            "add-note",
            file.path(),
            "Basic",
            "--deck",
            deck,
            &format!("Front={front}"),
            "Back=x",
        ]));
    }
    out(&fc(&["notes", file.path()]))
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Card IDs: "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn shows_what_is_left_to_study_in_each_deck_and_in_one_deck() {
    let file = TempFile::new("due");
    with_decks(&file);
    let all = out(&fc(&["due", file.path()]));
    assert!(
        all.contains("Default: new 2, learning 0, review 0"),
        "{all}"
    );
    assert!(all.contains("Polish: new 2, learning 0, review 0"), "{all}");
    assert!(all.contains("  Food: new 1, learning 0, review 0"), "{all}");
    let one = out(&fc(&["due", file.path(), "--deck", "polish::food"]));
    assert!(one.contains("Food: new 1"), "{one}");
    assert!(!one.contains("Default"), "{one}");
    assert!(err(&fc(&["due", file.path(), "--deck", "Nope"])).contains("No deck called"));
}

#[test]
fn shows_the_next_card_with_what_each_answer_does_and_changes_nothing() {
    let file = TempFile::new("next");
    with_decks(&file);
    let now = ["--now", "2026-10-06T09:00:00+00:00"];
    let first = out(&fc(&[&["next", file.path()][..], &now].concat()));
    assert!(
        first.contains("Left in Default: new 2, learning 0, review 0"),
        "{first}"
    );
    assert!(first.contains("(New)"), "{first}");
    assert!(
        first.contains("Again: 1 minute, Hard: 1 minute, Good: 10 minutes, Easy: "),
        "{first}"
    );
    // Asking again gives the same card: nothing was answered.
    assert_eq!(
        out(&fc(&[&["next", file.path()][..], &now].concat())),
        first
    );
    let polish = out(&fc(
        &[&["next", file.path(), "--deck", "Polish"][..], &now].concat()
    ));
    assert!(polish.contains("in Polish"), "{polish}");
}

#[test]
fn answering_changes_what_next_shows_and_a_deck_with_nothing_left_is_done() {
    let file = TempFile::new("done");
    let cards = with_decks(&file);
    let at = |time: &str, command: &[&str]| out(&fc(&[command, &["--now", time]].concat()));
    // The Default deck holds cards[0] and cards[1] (the listing order is not the creation order, so
    // go by the card the queue offers).
    for _ in 0..2 {
        let next = at("2026-10-06T09:00:00+00:00", &["next", file.path()]);
        let card = next
            .lines()
            .find_map(|l| l.strip_prefix("Next: card "))
            .and_then(|l| l.split(' ').next())
            .unwrap()
            .to_owned();
        assert!(cards.contains(&card), "{next}");
        out(&fc(&[
            "answer",
            file.path(),
            &card,
            "again",
            "--now",
            "2026-10-06T09:00:00+00:00",
        ]));
    }
    // Both are learning, a minute away: shown early, within twenty minutes.
    let next = at("2026-10-06T09:00:00+00:00", &["next", file.path()]);
    assert!(next.contains("(Learning)"), "{next}");
    // Tomorrow they are due again.
    let tomorrow = at(
        "2026-10-07T09:00:00+00:00",
        &["due", file.path(), "--deck", "Default"],
    );
    assert!(
        tomorrow.contains("Default: new 0, learning 2, review 0"),
        "{tomorrow}"
    );
    // A deck with nothing in it is done.
    out(&fc(&["add-deck", file.path(), "Empty"]));
    let empty = out(&fc(&["next", file.path(), "--deck", "Empty"]));
    assert!(empty.contains("Done for today."), "{empty}");
}

#[test]
fn suspends_and_buries_cards_and_brings_them_back() {
    let file = TempFile::new("hide");
    let cards = with_decks(&file);
    let now = ["--now", "2026-10-06T09:00:00+00:00"];
    let run = |args: &[&str]| out(&fc(&[args, &now].concat()));
    let suspended = run(&["suspend", file.path(), &cards[0], &cards[1]]);
    assert!(suspended.contains("Suspended 2 cards."), "{suspended}");
    assert!(run(&["due", file.path()]).contains("Default: new 0"));
    assert!(run(&["unsuspend", file.path(), &cards[0]]).contains("Unsuspended 1 card."));
    assert!(run(&["due", file.path()]).contains("Default: new 1"));

    assert!(
        run(&["bury", file.path(), &cards[2], &cards[3]])
            .contains("Buried 2 cards until tomorrow.")
    );
    assert!(run(&["due", file.path(), "--deck", "Polish"]).contains("Polish: new 0"));
    // Tomorrow they are back without anything being written.
    let tomorrow = out(&fc(&[
        "due",
        file.path(),
        "--deck",
        "Polish",
        "--now",
        "2026-10-07T09:00:00+00:00",
    ]));
    assert!(tomorrow.contains("Polish: new 2"), "{tomorrow}");
    // Unburying today, one card or a whole deck.
    run(&["unbury", file.path(), &cards[2]]);
    assert!(run(&["due", file.path(), "--deck", "Polish"]).contains("Polish: new 1"));
    let by_deck = run(&["unbury", file.path(), "--deck", "Polish"]);
    assert!(
        by_deck.contains("Unburied the cards in Polish."),
        "{by_deck}"
    );
    assert!(run(&["due", file.path(), "--deck", "Polish"]).contains("Polish: new 2"));
}

#[test]
fn explains_wrong_arguments_for_the_queue_commands() {
    let file = TempFile::new("queue-errors");
    let cards = with_decks(&file);
    assert!(err(&fc(&["suspend", file.path()])).contains("needs card IDs"));
    assert!(err(&fc(&["suspend", file.path(), "nonsense"])).contains("is not a card ID"));
    assert!(
        err(&fc(&["suspend", file.path(), "--deck", "Polish"]))
            .contains("takes card IDs, not --deck")
    );
    assert!(err(&fc(&["unbury", file.path()])).contains("card IDs or --deck"));
    assert!(
        err(&fc(&["unbury", file.path(), &cards[0], "--deck", "Polish"]))
            .contains("card IDs or --deck")
    );
    assert!(err(&fc(&["due", file.path(), "extra"])).contains("does not take"));
    assert!(err(&fc(&["next", file.path(), "--deck"])).contains("--deck needs a deck"));
    let missing = "00000000-0000-7000-8000-000000000000";
    assert!(err(&fc(&["bury", file.path(), missing])).contains("no longer exists"));
}
