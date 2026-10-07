//! `fc fake`: a large made-up collection for performance testing (step 1.14a).
//!
//! The text is invented (Polish-looking words and English-looking glosses). The same seed gives the
//! same notes, decks, tags and answers; due dates can differ a little, because the core's fuzz is
//! seeded from IDs, which are new every run. The review history is made by answering real cards
//! through the real core with a clock that steps one day at a time, so it is what the core would
//! write, but it is plausible and not faithful: a card answered Again is not shown again the same
//! day.

use std::sync::Arc;
use std::time::Instant;

use fc_core::clock::{Host, ManualClock};
use fc_core::collection::Collection;
use fc_core::deck;
use fc_core::id::Id;
use fc_core::notetype::{self, NoteType};
use fc_core::scheduling::Rating;
use fc_core::search::{MAX_LIMIT, Mode, SearchOptions, Sort, SortKey};

use super::{Failure, host, plural};

const DAY_MS: i64 = 86_400_000;
/// Time between two simulated answers.
const ANSWER_GAP_MS: i64 = 5_000;
/// Most answers in one simulated day, so a day's answers fit in the hour before the real "now".
const MAX_PER_DAY: u32 = 600;

#[derive(Debug, Clone)]
pub struct Options {
    pub notes: u32,
    pub decks: u32,
    pub media: Option<u32>,
    pub review_days: u32,
    pub new_per_day: u32,
    pub reviews_per_day: u32,
    pub seed: u64,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self, Failure> {
        let mut options = Self {
            notes: 1000,
            decks: 20,
            media: None,
            review_days: 0,
            new_per_day: 20,
            reviews_per_day: 100,
            seed: 1,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let name = arg.as_str();
            let mut number = |max: u64| -> Result<u64, Failure> {
                let value = args
                    .next()
                    .ok_or_else(|| Failure::Usage(format!("{name} needs a number.")))?;
                value.parse().ok().filter(|n| *n <= max).ok_or_else(|| {
                    Failure::Usage(format!(
                        "\"{value}\" is not a number from 0 to {max} for {name}."
                    ))
                })
            };
            match name {
                "--notes" => options.notes = number(2_000_000)? as u32,
                "--decks" => options.decks = number(100_000)? as u32,
                "--media" => options.media = Some(number(1_000_000)? as u32),
                "--review-days" => options.review_days = number(3650)? as u32,
                "--new-per-day" => options.new_per_day = number(u64::from(MAX_PER_DAY))? as u32,
                "--reviews-per-day" => {
                    options.reviews_per_day = number(u64::from(MAX_PER_DAY))? as u32;
                }
                "--seed" => options.seed = number(u64::MAX)?,
                other => {
                    return Err(Failure::Usage(format!(
                        "\"fake\" does not take \"{other}\"."
                    )));
                }
            }
        }
        if options.new_per_day + options.reviews_per_day > MAX_PER_DAY {
            return Err(Failure::Usage(format!(
                "--new-per-day and --reviews-per-day together may be at most {MAX_PER_DAY}."
            )));
        }
        Ok(options)
    }
}

/// xorshift64*: small, fast and the same on every platform.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number from 0 to `n` - 1.
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) % n.max(1) as u64) as usize
    }

    /// True `per_mille` times in a thousand.
    fn chance(&mut self, per_mille: usize) -> bool {
        self.below(1000) < per_mille
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const CONSONANTS: [&str; 18] = [
    "k", "t", "m", "r", "l", "sz", "cz", "n", "p", "w", "dz", "ż", "j", "b", "d", "g", "z", "c",
];
const VOWELS: [&str; 8] = ["a", "e", "i", "o", "u", "y", "ą", "ę"];
const GLOSS_CONSONANTS: [&str; 12] = ["b", "d", "f", "g", "h", "l", "m", "n", "p", "s", "t", "w"];
const GLOSS_VOWELS: [&str; 5] = ["a", "e", "i", "o", "u"];
const TAGS: [&str; 16] = [
    "grammar::verbs",
    "grammar::nouns",
    "grammar::cases::genitive",
    "grammar::cases::dative",
    "vocab::food",
    "vocab::travel",
    "vocab::work",
    "level::a1",
    "level::a2",
    "level::b1",
    "source::book",
    "source::podcast",
    "mined",
    "hard",
    "formal",
    "slang",
];

fn word(rng: &mut Rng, consonants: &[&str], vowels: &[&str]) -> String {
    let mut text = String::new();
    for _ in 0..2 + rng.below(3) {
        text.push_str(rng.pick(consonants));
        text.push_str(rng.pick(vowels));
    }
    text
}

fn polish(rng: &mut Rng) -> String {
    word(rng, &CONSONANTS, &VOWELS)
}

fn gloss(rng: &mut Rng) -> String {
    let words: Vec<String> = (0..1 + rng.below(3))
        .map(|_| word(rng, &GLOSS_CONSONANTS, &GLOSS_VOWELS))
        .collect();
    words.join(" ")
}

/// A sentence with one to three clozes.
fn cloze_text(rng: &mut Rng) -> String {
    let count = 1 + rng.below(3);
    let mut words: Vec<String> = (0..4 + rng.below(5)).map(|_| polish(rng)).collect();
    for number in 1..=count.min(words.len()) {
        let at = rng.below(words.len());
        if !words[at].contains("{{") {
            words[at] = format!("{{{{c{number}::{}}}}}", words[at]);
        }
    }
    words.join(" ")
}

fn media_bytes(rng: &mut Rng, index: u32) -> Vec<u8> {
    // Not a real image: the core stores bytes and never looks inside. The index makes each unique.
    let mut bytes = index.to_le_bytes().to_vec();
    bytes.extend((0..1024 + rng.below(3072)).map(|_| rng.next() as u8));
    bytes
}

struct Progress {
    label: &'static str,
    total: u32,
    next_report: u32,
}

impl Progress {
    fn new(label: &'static str, total: u32) -> Self {
        Self {
            label,
            total,
            next_report: total / 10,
        }
    }

    fn at(&mut self, done: u32) {
        if self.total >= 1000 && done >= self.next_report && done < self.total {
            eprintln!("  {}: {done} of {}", self.label, self.total);
            self.next_report = done + self.total / 10;
        }
    }
}

fn seconds(start: Instant) -> String {
    format!("{:.1} s", start.elapsed().as_secs_f64())
}

fn field_ids(note_type: &NoteType) -> (Id, Id) {
    (note_type.fields[0].id, note_type.fields[1].id)
}

pub fn fake(file: &str, options: &Options) -> Result<String, Failure> {
    if std::path::Path::new(file).exists() {
        return Err(Failure::Core(format!(
            "{file} exists already. Choose a file name that is not used."
        )));
    }
    let real = host::host_for(file, true).map_err(Failure::Core)?;
    let now = real.clock.now();
    let clock = Arc::new(ManualClock::new(
        now.unix_ms - i64::from(options.review_days) * DAY_MS - 3_600_000,
    ));
    clock.set_utc_offset_minutes(now.utc_offset_minutes);
    let collection = Collection::create(
        file,
        Host {
            clock: clock.clone(),
            installation_id: real.installation_id,
        },
    )?;
    let mut rng = Rng::new(options.seed);
    let total = Instant::now();

    // Decks: a few at the top, the rest inside a random earlier deck, at most three levels deep.
    let started = Instant::now();
    let mut decks: Vec<(Id, usize)> = Vec::new();
    let top = (options.decks / 5).max(1);
    for index in 0..options.decks {
        let (parent, depth, name) = if index < top {
            (None, 1, format!("Deck {}", index + 1))
        } else {
            let (parent, depth) = loop {
                let (id, depth) = *rng.pick(&decks);
                if depth < 3 {
                    break (id, depth);
                }
            };
            (Some(parent), depth + 1, format!("Part {}", index + 1))
        };
        let id = collection.create_deck(&name, parent)?;
        decks.push((id, depth));
    }
    let decks_time = seconds(started);

    // Notes, with tags and media. The media is added first so notes can refer to it.
    let started = Instant::now();
    let types = collection.note_types()?;
    let find = |id: Id| {
        types
            .iter()
            .find(|t| t.id == id)
            .expect("a built-in note type")
    };
    let (basic, reversed, cloze) = (
        find(notetype::builtin::basic()),
        find(notetype::builtin::basic_and_reversed()),
        find(notetype::builtin::cloze()),
    );
    let media_wanted = options.media.unwrap_or(options.notes / 50);
    let mut media_names = Vec::new();
    for index in 0..media_wanted {
        let bytes = media_bytes(&mut rng, index);
        media_names.push(
            collection
                .add_media(&format!("picture-{index}.png"), &bytes)?
                .name,
        );
    }
    let mut cards: Vec<Id> = Vec::new();
    let mut tagged = 0u32;
    let mut media_next = 0usize;
    let mut fronts = std::collections::HashSet::new();
    let mut progress = Progress::new("notes", options.notes);
    for index in 0..options.notes {
        let target = if decks.is_empty() {
            deck::default_deck()
        } else {
            rng.pick(&decks).0
        };
        let roll = rng.below(100);
        let (note_type, mut back) = if roll < 60 {
            (basic, gloss(&mut rng))
        } else if roll < 85 {
            (reversed, gloss(&mut rng))
        } else {
            (cloze, gloss(&mut rng))
        };
        // No two notes share a first field, so sorting by it is a fixed order.
        let front = loop {
            let text = if note_type.id == cloze.id {
                cloze_text(&mut rng)
            } else {
                polish(&mut rng)
            };
            if fronts.insert(text.clone()) {
                break text;
            }
        };
        // Every media file ends up on exactly one note, so `fc media-check` finds nothing unused.
        let notes_left = (options.notes - index) as usize;
        if rng.below(notes_left) < media_names.len() - media_next {
            back.push_str(&format!("<br><img src=\"{}\">", media_names[media_next]));
            media_next += 1;
        }
        let (first, second) = field_ids(note_type);
        let added = collection.add_note_to_deck(
            target,
            note_type.id,
            &[(first, &front), (second, &back)],
        )?;
        cards.extend(&added.cards);
        if rng.chance(500) {
            let tags: Vec<&str> = (0..1 + rng.below(3)).map(|_| *rng.pick(&TAGS)).collect();
            collection.add_tags(&[added.id], &tags)?;
            tagged += 1;
        }
        progress.at(index + 1);
    }
    let notes_time = seconds(started);

    let position: std::collections::HashMap<Id, usize> = cards
        .iter()
        .enumerate()
        .map(|(at, card)| (*card, at))
        .collect();
    let mut new_left: Vec<usize> = (0..cards.len()).collect();
    // Review history: each simulated day answers some new cards and some due ones.
    let started = Instant::now();
    let mut answers = 0u64;
    let mut progress = Progress::new("days", options.review_days);
    for day in 0..options.review_days {
        clock.set(now.unix_ms - i64::from(options.review_days - day) * DAY_MS - 3_600_000);
        // New cards come from our own list and due ones from a search put in creation order, so
        // the picks do not depend on card IDs (which differ per run, and break ties in the core's
        // sorts). Past 1,000 due cards in a day the search window can differ between runs.
        let mut todo = Vec::new();
        for _ in 0..(options.new_per_day as usize).min(new_left.len()) {
            let at = rng.below(new_left.len());
            todo.push(cards[new_left.swap_remove(at)]);
        }
        if options.reviews_per_day > 0 {
            let page = collection.search(
                "is:due",
                &SearchOptions {
                    sort: Sort {
                        key: SortKey::SortField,
                        descending: false,
                    },
                    mode: Mode::Cards,
                    offset: 0,
                    limit: MAX_LIMIT,
                    seed: 0,
                },
            )?;
            let mut due: Vec<Id> = page.rows.iter().map(|row| row.card).collect();
            due.sort_by_key(|card| position[card]);
            for taken in 0..(options.reviews_per_day as usize).min(due.len()) {
                let at = taken + rng.below(due.len() - taken);
                due.swap(taken, at);
                todo.push(due[taken]);
            }
        }
        for card in todo {
            let rating = match rng.below(100) {
                0..8 => Rating::Again,
                8..18 => Rating::Hard,
                18..88 => Rating::Good,
                _ => Rating::Easy,
            };
            let duration = 2_000 + rng.below(13_000) as u32;
            collection.answer(card, rating, duration)?;
            clock.advance(ANSWER_GAP_MS);
            answers += 1;
        }
        progress.at(day + 1);
    }
    let reviews_time = seconds(started);

    // Today: some cards suspended, a few buried.
    clock.set(now.unix_ms);
    let mut suspended = Vec::new();
    let mut buried = Vec::new();
    for card in &cards {
        if rng.chance(10) {
            suspended.push(*card);
        } else if rng.chance(5) {
            buried.push(*card);
        }
    }
    collection.suspend_cards(&suspended)?;
    collection.bury_cards(&buried)?;
    collection.close()?;

    let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    Ok(format!(
        "Made {file} ({:.1} MB) in {}\n\
         Seed: {}\n\
         Decks: {} ({decks_time})\n\
         Notes: {} with {} ({} tagged), {} media files ({notes_time})\n\
         Review history: {} over {} ({reviews_time})\n\
         Suspended: {}, buried today: {}",
        size as f64 / 1_048_576.0,
        seconds(total),
        options.seed,
        options.decks,
        options.notes,
        plural(cards.len(), "card"),
        tagged,
        media_names.len(),
        plural(answers as usize, "answer"),
        plural(options.review_days as usize, "day"),
        suspended.len(),
        buried.len(),
    ))
}
