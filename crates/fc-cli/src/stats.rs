//! `fc history`, `fc stats`, `fc forecast` and `fc optimise`: the numbers a stats screen will show,
//! and tuning the scheduler from the review history.

use fc_core::deck::PresetChange;
use fc_core::scheduling::Due;
use fc_core::stats::{DayRange, MIN_TRAINING_ITEMS};

use super::study::{card_id, date, open, preview_text, rating_name, state_name, time};
use super::{Failure, find_deck, plural};

/// The options these commands take, taken out of the arguments they were given.
#[derive(Default)]
pub struct Options {
    pub deck: Option<String>,
    pub days: Option<u32>,
    pub preset: Option<String>,
    pub apply: bool,
}

impl Options {
    /// Takes `--deck`, `--days`, `--preset` and `--apply` out of `args`. What is left over is
    /// returned.
    pub fn take(args: &[String]) -> Result<(Self, Vec<String>), Failure> {
        let mut options = Self::default();
        let mut rest = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .cloned()
                    .ok_or_else(|| Failure::Usage(format!("{name} needs a value.")))
            };
            match arg.as_str() {
                "--deck" => options.deck = Some(value("--deck")?),
                "--preset" => options.preset = Some(value("--preset")?),
                "--days" => {
                    let text = value("--days")?;
                    options.days =
                        Some(text.parse().ok().filter(|d| *d >= 1).ok_or_else(|| {
                            Failure::Usage(format!(
                                "\"{text}\" is not a number of days, such as 30."
                            ))
                        })?);
                }
                "--apply" => options.apply = true,
                _ => rest.push(arg.clone()),
            }
        }
        Ok((options, rest))
    }
}

pub fn history(file: &str, card: &str) -> Result<String, Failure> {
    let card = card_id(card)?;
    let open = open(file)?;
    let history = open.collection.card_history(card)?;
    let mut text = format!("Card {card}: {}", plural(history.len(), "review"));
    for e in &history {
        let since = e.days_since_previous.map_or(String::new(), |d| {
            format!(", {} after the last", plural(d as usize, "day"))
        });
        let result = match e.due {
            Some(Due::Days(d)) => format!(", back in {}", plural(d as usize, "day")),
            Some(Due::Minutes(_)) => format!(", back in {}", preview_text(e.due.unwrap())),
            None => String::new(),
        };
        let memory = e.memory.map_or(String::new(), |m| {
            format!(
                ", stability {:.2} days, difficulty {:.2}",
                m.stability, m.difficulty
            )
        });
        text.push_str(&format!(
            "\n  {} (day {}) {}: {} -> {}{since}{result}{memory}",
            time(e.time_ms, e.utc_offset),
            e.day,
            rating_name(e.rating),
            e.state_before.map_or("?", state_name),
            e.state.map_or("?", state_name),
        ));
    }
    open.collection.close()?;
    Ok(text)
}

fn seconds(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, s % 3600 / 60)
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

pub fn stats(file: &str, options: &Options) -> Result<String, Failure> {
    let open = open(file)?;
    let c = &open.collection;
    let deck = options
        .deck
        .as_deref()
        .map(|p| find_deck(c, p))
        .transpose()?;
    let range = DayRange::last(c.study_today()?, options.days.unwrap_or(30));
    let counts = c.daily_counts(range, deck.as_ref().map(|d| d.id))?;
    let retention = c.retention(range, deck.as_ref().map(|d| d.id))?;
    let mut text = format!(
        "Answers from {} to {}{}:\n\nday         new learn relearn review again time",
        date(range.from),
        date(range.to),
        deck.as_ref()
            .map_or(String::new(), |d| format!(" in {}", d.path)),
    );
    let mut total = 0;
    let mut time_ms = 0;
    for d in &counts {
        total += d.answers();
        time_ms += d.time_ms;
        if d.answers() > 0 {
            text.push_str(&format!(
                "\n{}  {:>3} {:>5} {:>7} {:>6} {:>5} {}",
                date(d.day),
                d.new,
                d.learning,
                d.relearning,
                d.review,
                d.again,
                seconds(d.time_ms),
            ));
        }
    }
    text.push_str(&format!(
        "\n\n{} in {}, {}.\nRetention: {}",
        plural(total as usize, "answer"),
        plural(counts.len(), "day"),
        seconds(time_ms),
        match retention.rate() {
            Some(rate) => format!(
                "{:.1}% ({} of {} answers on review cards were not Again)",
                rate * 100.0,
                retention.passed,
                retention.reviews
            ),
            None => "no answers on review cards in this range".to_owned(),
        }
    ));
    open.collection.close()?;
    Ok(text)
}

pub fn forecast(file: &str, options: &Options) -> Result<String, Failure> {
    let open = open(file)?;
    let c = &open.collection;
    let deck = options
        .deck
        .as_deref()
        .map(|p| find_deck(c, p))
        .transpose()?;
    let f = c.due_forecast(options.days.unwrap_or(30), deck.as_ref().map(|d| d.id))?;
    let mut text = format!(
        "Due from {}{}:\n\noverdue: {}\nlearning today: {}",
        date(f.today),
        deck.as_ref()
            .map_or(String::new(), |d| format!(" in {}", d.path)),
        f.overdue,
        f.learning_today,
    );
    let mut sum = f.overdue;
    for d in &f.days {
        sum += d.reviews;
        if d.reviews > 0 || d.day == f.today {
            text.push_str(&format!(
                "\n{} ({}): {}",
                date(d.day),
                if d.day == f.today {
                    "today".to_owned()
                } else {
                    format!("day +{}", d.day - f.today)
                },
                d.reviews
            ));
        }
    }
    text.push_str(&format!(
        "\n\n{} due in all (limits are not applied).",
        plural(sum as usize, "review")
    ));
    open.collection.close()?;
    Ok(text)
}

pub fn optimise(file: &str, options: &Options) -> Result<String, Failure> {
    let open = open(file)?;
    let c = &open.collection;
    let presets = c.presets()?;
    let wanted = options.preset.as_deref().unwrap_or("Default");
    let preset = presets
        .iter()
        .find(|p| p.name.to_lowercase() == wanted.to_lowercase())
        .ok_or_else(|| {
            Failure::Core(format!(
                "There is no options preset called \"{wanted}\". `fc decks` lists them."
            ))
        })?;
    let data = c.optimisation_data(preset.id)?;
    let mut text = format!(
        "Options preset {}: {} from {}, {} to learn from (at least {MIN_TRAINING_ITEMS} are needed).",
        preset.name,
        plural(data.reviews as usize, "review"),
        plural(data.cards as usize, "card"),
        data.items,
    );
    text.push_str(if preset.fsrs_parameters.is_empty() {
        "\nThe preset uses the default parameters."
    } else {
        "\nThe preset uses tuned parameters."
    });
    let tuned = data.optimise()?;
    text.push_str(&format!(
        "\nParameters: {}",
        tuned
            .parameters
            .iter()
            .map(|p| format!("{p:.4}"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    if options.apply {
        c.set_preset_options(
            preset.id,
            &PresetChange {
                fsrs_parameters: Some(tuned.parameters),
                ..Default::default()
            },
        )
        .map_err(|e| Failure::Core(e.to_string()))?;
        text.push_str(&format!(
            "\nStored in {}. Cards answered from now on use them. Cards already studied keep \
             the memory state they have until their next answer.",
            preset.name
        ));
    } else {
        text.push_str("\nNothing was changed. Add --apply to store them in the preset.");
    }
    open.collection.close()?;
    Ok(text)
}
