//! Runs the real `fc` binary: review history, stats, the forecast and optimising, with a pretend
//! clock.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-stats-{}-{name}.db", std::process::id())))
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

/// A collection with `n` Basic notes, and the card IDs.
fn with_cards(file: &TempFile, n: usize) -> Vec<String> {
    out(&fc(&["new", file.path()]));
    for i in 0..n {
        out(&fc(&[
            "add-note",
            file.path(),
            "Basic",
            &format!("Front=word {i}"),
            "Back=x",
        ]));
    }
    out(&fc(&["notes", file.path()]))
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Card IDs: "))
        .map(|ids| ids.split_whitespace().next().unwrap().to_owned())
        .collect()
}

fn answer(file: &TempFile, card: &str, rating: &str, now: &str) {
    out(&fc(&["answer", file.path(), card, rating, "--now", now]));
}

#[test]
fn shows_a_cards_history_and_leaves_out_an_undone_answer() {
    let file = TempFile::new("history");
    let card = with_cards(&file, 1).remove(0);
    answer(&file, &card, "good", "2026-10-06T09:00:00+00:00");
    answer(&file, &card, "good", "2026-10-06T09:10:00+00:00");
    answer(&file, &card, "again", "2026-10-08T09:00:00+00:00");
    out(&fc(&[
        "undo",
        file.path(),
        "--now",
        "2026-10-08T09:01:00+00:00",
    ]));
    let history = out(&fc(&["history", file.path(), &card]));
    assert!(history.contains("2 reviews"), "{history}");
    assert!(
        history.contains("(day 20732) Good: New -> Learning"),
        "{history}"
    );
    assert!(
        history.contains("Good: Learning -> Review, 0 days after the last, back in 2 days"),
        "{history}"
    );
    assert!(!history.contains("Again"), "{history}");
    let error = err(&fc(&["history", file.path(), "not-an-id"]));
    assert!(error.contains("is not a card ID"), "{error}");
}

#[test]
fn stats_and_the_forecast_read_what_was_answered() {
    let file = TempFile::new("stats");
    let cards = with_cards(&file, 2);
    answer(&file, &cards[0], "good", "2026-10-06T09:00:00+00:00");
    answer(&file, &cards[0], "good", "2026-10-06T09:10:00+00:00");
    answer(&file, &cards[1], "again", "2026-10-06T09:20:00+00:00");
    // On day 2 the first card is a review card, and the answer is Again.
    answer(&file, &cards[0], "again", "2026-10-08T09:00:00+00:00");
    let now = ["--now", "2026-10-09T09:00:00+00:00"];
    let stats = out(&fc(
        &[&["stats", file.path(), "--days", "5"][..], &now].concat()
    ));
    assert!(
        stats.contains("Answers from 2026-10-05 to 2026-10-09"),
        "{stats}"
    );
    assert!(stats.contains("4 answers in 5 days"), "{stats}");
    assert!(
        stats.contains("Retention: 0.0% (0 of 1 answers on review cards were not Again)"),
        "{stats}"
    );
    let decks = out(&fc(&[
        &["stats", file.path(), "--deck", "Default", "--days", "5"][..],
        &now,
    ]
    .concat()));
    assert!(decks.contains("in Default"), "{decks}");
    let none = out(&fc(&[
        &["stats", file.path(), "--days", "2"][..],
        &["--now", "2026-12-09T09:00:00+00:00"],
    ]
    .concat()));
    assert!(none.contains("0 answers"), "{none}");
    assert!(none.contains("no answers on review cards"), "{none}");

    // Card 0 lapsed on 2026-10-08 and is in a relearning step; card 1 is in a learning step.
    let forecast = out(&fc(&[&["forecast", file.path()][..], &now].concat()));
    assert!(forecast.contains("learning today: 2"), "{forecast}");
    assert!(forecast.contains("0 reviews due in all"), "{forecast}");
    // Card 0's first review was set for 2026-10-08, so after the lapse nothing is due on a day.
    let error = err(&fc(&["forecast", file.path(), "--days", "0"]));
    assert!(error.contains("not a number of days"), "{error}");
    let error = err(&fc(&["stats", file.path(), "--deck", "Nope"]));
    assert!(!error.is_empty());
}

#[test]
fn optimising_needs_history_and_stores_the_result_only_when_asked() {
    let file = TempFile::new("optimise");
    let cards = with_cards(&file, 12);
    let few = err(&fc(&["optimise", file.path()]));
    assert!(few.contains("not enough review history"), "{few}");
    // Twelve cards, each answered on 7 days: 72 reviews that come on a later day than the one
    // before them.
    for day in 1..=7 {
        for (i, card) in cards.iter().enumerate() {
            let rating = if (i + day) % 7 == 0 { "again" } else { "good" };
            answer(
                &file,
                card,
                rating,
                &format!("2026-10-{:02}T09:00:00+00:00", day + 10),
            );
        }
    }
    let tuned = out(&fc(&["optimise", file.path()]));
    assert!(
        tuned.contains("84 reviews from 12 cards, 72 to learn from"),
        "{tuned}"
    );
    assert!(tuned.contains("Nothing was changed"), "{tuned}");
    assert!(tuned.contains("uses the default parameters"), "{tuned}");
    let applied = out(&fc(&["optimise", file.path(), "--apply"]));
    assert!(applied.contains("Stored in Default"), "{applied}");
    // The same history gives the same parameters, and the preset kept them.
    let again = out(&fc(&["optimise", file.path(), "--preset", "default"]));
    let parameters = |text: &str| {
        text.lines()
            .find(|l| l.starts_with("Parameters: "))
            .unwrap()
            .to_owned()
    };
    assert_eq!(parameters(&tuned), parameters(&applied));
    assert_eq!(parameters(&tuned), parameters(&again));
    assert!(again.contains("uses tuned parameters"), "{again}");
    let error = err(&fc(&["optimise", file.path(), "--preset", "Nope"]));
    assert!(error.contains("no options preset called"), "{error}");
}
