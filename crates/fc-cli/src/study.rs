//! `fc answer`, `fc undo` and `fc schedule`: answering cards by hand, to try the schedule before
//! any study screen exists.

use chrono::{DateTime, FixedOffset, NaiveDate, SecondsFormat, TimeDelta};
use fc_core::collection::Collection;
use fc_core::id::Id;
use fc_core::scheduling::{CardState, Rating, study_day};
use fc_core::study::{CardEvent, EventKind};

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

/// How long until a moment, as the person would say it.
fn in_minutes(minutes: i64) -> String {
    let span = |m: i64| match m {
        m if m < 120 => plural(m as usize, "minute"),
        m if m < 2 * 24 * 60 => plural((m / 60) as usize, "hour"),
        m => plural((m / (24 * 60)) as usize, "day"),
    };
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
