//! `fc answer`, `fc undo` and `fc schedule`: answering cards by hand, to try the schedule before
//! any study screen exists.

use chrono::{DateTime, FixedOffset, NaiveDate, SecondsFormat, TimeDelta};
use fc_core::collection::Collection;
use fc_core::id::Id;
use fc_core::scheduling::{CardState, Due, Rating, study_day};
use fc_core::study::{CardEvent, EventKind};
use fc_core::study::{Counts, Next};

use super::{Failure, host, plural};

fn card_id(text: &str) -> Result<Id, Failure> {
    text.parse().map_err(|_| {
        Failure::Usage(format!(
            "\"{text}\" is not a card ID. `fc notes` lists them."
        ))
    })
}

fn rating(text: &str) -> Result<Rating, Failure> {
    match text.to_lowercase().as_str() {
        "again" => Ok(Rating::Again),
        "hard" => Ok(Rating::Hard),
        "good" => Ok(Rating::Good),
        "easy" => Ok(Rating::Easy),
        _ => Err(Failure::Usage(format!(
            "\"{text}\" is not an answer. Use again, hard, good or easy."
        ))),
    }
}

fn rating_name(rating: Rating) -> &'static str {
    match rating {
        Rating::Again => "Again",
        Rating::Hard => "Hard",
        Rating::Good => "Good",
        Rating::Easy => "Easy",
    }
}

fn state_name(state: CardState) -> &'static str {
    match state {
        CardState::New => "New",
        CardState::Learning => "Learning",
        CardState::Review => "Review",
        CardState::Relearning => "Relearning",
    }
}

/// A moment as local time at `offset` minutes ahead of UTC.
fn time(unix_ms: i64, offset: i32) -> String {
    let utc = DateTime::from_timestamp_millis(unix_ms);
    let zone = FixedOffset::east_opt(offset.saturating_mul(60));
    match (utc, zone) {
        (Some(utc), Some(zone)) => utc
            .with_timezone(&zone)
            .to_rfc3339_opts(SecondsFormat::Secs, false),
        _ => format!("{unix_ms} ms"),
    }
}

/// A study day as a calendar date.
fn date(day: i64) -> String {
    NaiveDate::from_ymd_opt(1970, 1, 1)
        .and_then(|epoch| epoch.checked_add_signed(TimeDelta::days(day)))
        .map_or_else(|| "?".to_owned(), |date| date.to_string())
}

/// A length of time, as the person would say it.
fn span(minutes: i64) -> String {
    match minutes {
        m if m < 120 => plural(m as usize, "minute"),
        m if m < 2 * 24 * 60 => plural((m / 60) as usize, "hour"),
        m => plural((m / (24 * 60)) as usize, "day"),
    }
}

/// How long until a moment, as the person would say it.
fn in_minutes(minutes: i64) -> String {
    if minutes < 0 {
        format!("{} ago", span(-minutes))
    } else {
        format!("in {}", span(minutes))
    }
}

fn in_days(days: i64) -> String {
    match days {
        0 => "today".to_owned(),
        d if d < 0 => format!("{} ago", plural((-d) as usize, "day")),
        d => format!("in {}", plural(d as usize, "day")),
    }
}

struct Open {
    collection: Collection,
    now: fc_core::clock::Reading,
}

fn open(file: &str) -> Result<Open, Failure> {
    let host = host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
    let now = host.clock.now();
    Ok(Open {
        collection: Collection::open(file, host)?,
        now,
    })
}

/// When a card is due, from the due day or time it has.
fn due(open: &Open, due_day: Option<i64>, due_ms: Option<i64>) -> Result<String, Failure> {
    let offset = open.now.utc_offset_minutes;
    Ok(match (due_ms, due_day) {
        (Some(ms), _) => format!(
            "{} ({})",
            time(ms, offset),
            in_minutes((ms - open.now.unix_ms).div_euclid(60_000))
        ),
        (None, Some(day)) => {
            let today = study_day(open.now.unix_ms, offset, open.collection.day_start_hour()?);
            format!(
                "on study day {day} ({}), {}",
                date(day),
                in_days(day - today)
            )
        }
        (None, None) => "not scheduled (a new card)".to_owned(),
    })
}

pub fn answer(file: &str, card: &str, answer: &str) -> Result<String, Failure> {
    let (card, rating) = (card_id(card)?, rating(answer)?);
    let open = open(file)?;
    let answered = open.collection.answer(card, rating, 0)?;
    let schedule = open.collection.card_schedule(card)?;
    let text = format!(
        "Answered {}. Now {}{}, due {}.",
        rating_name(rating),
        state_name(answered.state),
        match answered.state {
            CardState::Learning | CardState::Relearning => format!(" (step {})", schedule.step + 1),
            _ => String::new(),
        },
        due(&open, answered.due_day, answered.due_ms)?
    );
    open.collection.close()?;
    Ok(text)
}

pub fn undo(file: &str) -> Result<String, Failure> {
    let open = open(file)?;
    let text = match open.collection.undo_answer()? {
        None => "There is nothing to undo.".to_owned(),
        Some(undone) => format!(
            "Undid {} on card {}. It is now {}{}.",
            rating_name(undone.rating),
            undone.card,
            state_name(undone.schedule.state),
            if undone.schedule.state == CardState::New {
                String::new()
            } else {
                format!(
                    ", due {}",
                    due(&open, undone.schedule.due_day, undone.schedule.due_ms)?
                )
            }
        ),
    };
    open.collection.close()?;
    Ok(text)
}

fn describe_event(event: &CardEvent) -> String {
    let when = time(event.time_ms, event.utc_offset);
    match &event.kind {
        EventKind::Review => {
            let rating = event.rating.map_or("?", rating_name);
            let before = event.state_before.map_or("?", state_name);
            let after = event.state.map_or("?", state_name);
            let due = match (event.due_ms, event.due_day) {
                (Some(ms), _) => format!(", due {}", time(ms, event.utc_offset)),
                (None, Some(day)) => format!(", due day {day} ({})", date(day)),
                (None, None) => String::new(),
            };
            format!(
                "{when} (day {}) {rating}: {before} -> {after}{due}",
                event.day
            )
        }
        EventKind::Void => format!(
            "{when} (day {}) undo of {}",
            event.day,
            event.target.map_or("?".to_owned(), |t| t.to_string())
        ),
        EventKind::Unknown(kind) => format!("{when} (day {}) {kind} (unknown kind)", event.day),
    }
}

pub fn schedule(file: &str, card: &str) -> Result<String, Failure> {
    let card = card_id(card)?;
    let open = open(file)?;
    let schedule = open.collection.card_schedule(card)?;
    let events = open.collection.card_events(card)?;
    let mut text = format!("Card {card}\nState: {}", state_name(schedule.state));
    if schedule.state != CardState::New {
        if matches!(schedule.state, CardState::Learning | CardState::Relearning) {
            text.push_str(&format!(" (step {})", schedule.step + 1));
        }
        text.push_str(&format!(
            "\nDue: {}",
            due(&open, schedule.due_day, schedule.due_ms)?
        ));
        if let Some(memory) = schedule.memory {
            text.push_str(&format!(
                "\nMemory: stability {:.2} days, difficulty {:.2}",
                memory.stability, memory.difficulty
            ));
        }
        if let Some(day) = schedule.last_day {
            text.push_str(&format!("\nLast answered: study day {day} ({})", date(day)));
        }
        text.push_str(&format!(
            "\nAnswers: {}, lapses: {}",
            schedule.answers, schedule.lapses
        ));
    }
    text.push_str(&format!("\nEvents: {}", events.len()));
    for event in &events {
        text.push_str(&format!("\n  {}", describe_event(event)));
    }
    open.collection.close()?;
    Ok(text)
}

fn counts_text(counts: Counts) -> String {
    format!(
        "new {}, learning {}, review {}",
        counts.new, counts.learning, counts.review
    )
}

/// What an answer would do, for the answer buttons.
fn preview_text(due: Due) -> String {
    match due {
        Due::Minutes(minutes) => span(i64::from(minutes)),
        Due::Days(days) => plural(days as usize, "day"),
    }
}

/// `fc due`: what is left to study in each deck, or in one deck.
pub fn due_counts(file: &str, deck: Option<&str>) -> Result<String, Failure> {
    let open = open(file)?;
    let collection = &open.collection;
    let only = deck
        .map(|path| super::find_deck(collection, path))
        .transpose()?;
    let counts = collection.deck_counts()?;
    let mut text = String::from("Left to study today:\n");
    for listed in collection.decks()? {
        if only.as_ref().is_some_and(|d| d.id != listed.id) {
            continue;
        }
        let found = counts
            .iter()
            .find(|c| c.deck == listed.id)
            .map_or_else(Counts::default, |c| c.counts);
        text.push_str(&format!(
            "\n{}{}: {}",
            "  ".repeat(if only.is_some() { 0 } else { listed.depth }),
            listed.display_name,
            counts_text(found)
        ));
    }
    open.collection.close()?;
    Ok(text)
}

/// `fc next`: the card to study next in a deck, and what each answer would do. It does not answer.
pub fn next(file: &str, deck: Option<&str>) -> Result<String, Failure> {
    let open = open(file)?;
    let collection = &open.collection;
    let deck = match deck {
        Some(path) => super::find_deck(collection, path)?,
        None => super::find_deck(collection, "Default")?,
    };
    let text = match collection.next_card(deck.id)? {
        Next::Card {
            card,
            deck: card_deck,
            state,
            previews,
            counts,
        } => {
            let at = collection
                .decks()?
                .into_iter()
                .find(|d| d.id == card_deck)
                .map_or_else(|| "?".to_owned(), |d| d.path);
            format!(
                "Left in {}: {}\nNext: card {card} in {at} ({})\nAgain: {}, Hard: {}, Good: {}, \
                 Easy: {}",
                deck.path,
                counts_text(counts),
                state_name(state),
                preview_text(previews[0]),
                preview_text(previews[1]),
                preview_text(previews[2]),
                preview_text(previews[3]),
            )
        }
        Next::Waiting { until_ms, counts } => format!(
            "Left in {}: {}\nNothing is due now. The next learning card is due {} ({}).",
            deck.path,
            counts_text(counts),
            time(until_ms, open.now.utc_offset_minutes),
            in_minutes((until_ms - open.now.unix_ms).div_euclid(60_000)),
        ),
        Next::Done { counts } => format!(
            "Left in {}: {}\nDone for today.",
            deck.path,
            counts_text(counts)
        ),
    };
    open.collection.close()?;
    Ok(text)
}

pub enum Hide {
    Suspend,
    Unsuspend,
    Bury,
    Unbury,
}

/// `fc suspend`, `fc unsuspend`, `fc bury` and `fc unbury` for cards, and `fc unbury --deck`.
pub fn hide(
    file: &str,
    what: Hide,
    cards: &[String],
    deck: Option<&str>,
) -> Result<String, Failure> {
    let ids = cards
        .iter()
        .map(|card| card_id(card))
        .collect::<Result<Vec<_>, _>>()?;
    let open = open(file)?;
    let collection = &open.collection;
    let text = match (what, deck) {
        (Hide::Unbury, Some(path)) => {
            let deck = super::find_deck(collection, path)?;
            collection.unbury_deck(deck.id)?;
            format!("Unburied the cards in {}.", deck.path)
        }
        (Hide::Suspend, _) => {
            collection.suspend_cards(&ids)?;
            format!("Suspended {}.", plural(ids.len(), "card"))
        }
        (Hide::Unsuspend, _) => {
            collection.unsuspend_cards(&ids)?;
            format!("Unsuspended {}.", plural(ids.len(), "card"))
        }
        (Hide::Bury, _) => {
            collection.bury_cards(&ids)?;
            format!("Buried {} until tomorrow.", plural(ids.len(), "card"))
        }
        (Hide::Unbury, None) => {
            collection.unbury_cards(&ids)?;
            format!("Unburied {}.", plural(ids.len(), "card"))
        }
    };
    open.collection.close()?;
    Ok(text)
}
