//! Developer CLI. It drives `fc-core` directly, so every step of Phase 1 can be tried from a
//! terminal before any UI exists. Step 1.14 grows it into the full tool.

mod backup;
mod bench;
mod fake;
mod host;
mod media;
mod merge;
mod search;
mod stats;
mod study;

use std::process::ExitCode;

use fc_core::collection::Collection;

const USAGE: &str = "\
Usage:
  fc new <file>    Create an empty collection at <file>
  fc info <file>   Open a collection, print facts about it, and close it
  fc notetypes <file>
                   List the note types with their fields and templates
  fc notes <file>  List the notes with their fields and cards
  fc decks <file>  List the decks as a tree with their card counts, and the option presets
  fc add-deck <file> <deck>
                   Add a deck. Use Parent::Child to put it inside an existing deck
  fc add-note <file> <note type> [--deck <deck>] [--tag <tag>]... <Field=value>...
                   Add a note to a deck (the Default deck if none is given). Fields left out are
                   empty. Warns about duplicates
  fc tags <file>   List the tags as a tree with the number of notes
  fc tag <file> <note ID> <tag>...
                   Add tags to a note. Use parent::child to put a tag inside another
  fc untag <file> <note ID> <tag>...
                   Take tags off a note
  fc rename-tag <file> <tag> <new name>
                   Rename a tag on every note, together with the tags inside it
  fc render <file> <note ID>
                   Print the front and back HTML of each card of a note, and its media
  fc add-media <file> <path>
                   Add an image or sound file and print the name to use in a field. The same bytes
                   are stored once
  fc media <file>  List the media files, whether notes use them, and the names they use
  fc media-get <file> <name> <path>
                   Write a media file's bytes to <path>
  fc media-check <file>
                   List media nothing uses, names with no file, and files whose bytes are not here
  fc delete-unused-media <file>
                   Delete the media files nothing uses
  fc answer <file> <card ID> <again|hard|good|easy>
                   Answer a card and print when it is due next
  fc undo <file>   Take back the last answer made on this device
  fc schedule <file> <card ID>
                   Print a card's state, due date, memory and every answer so far
  fc due <file> [--deck <deck>]
                   Show how many new, learning and review cards are left to study today, in each
                   deck or in one deck (with the decks inside it)
  fc next <file> [--deck <deck>]
                   Show the card to study next (the Default deck if none is given) and when each
                   answer would bring it back. Does not answer it
  fc suspend <file> <card ID>...
                   Keep cards out of the queue until they are unsuspended
  fc unsuspend <file> <card ID>...
                   Put suspended cards back in the queue
  fc bury <file> <card ID>...
                   Keep cards out of the queue until tomorrow
  fc unbury <file> (<card ID>... | --deck <deck>)
                   Bring buried cards back today, or every buried card in a deck
  fc history <file> <card ID>
                   Print a card's reviews, oldest first (answers that were undone are left out)
  fc stats <file> [--deck <deck>] [--days <n>]
                   Answers per day, time spent and retention over the last n days (30)
  fc forecast <file> [--deck <deck>] [--days <n>]
                   Review cards due today, overdue and on each of the next n days (30)
  fc optimise <file> [--preset <name>] [--apply]
                   Tune the scheduler from the review history of a preset's cards (the Default
                   preset if none is given). Prints the result, and stores it with --apply
  fc search <file> <query> [--sort <key>[:desc]] [--notes] [--limit <n>] [--offset <n>] [--seed <n>]
                   Find cards (or notes, with --notes). Examples: dog, deck:Polish is:due,
                   front:k*t, rated:7:1, difficulty:>7, due:..0 (quote a query that has spaces).
                   Join with spaces (and), or, - (not) and ( ). Sort keys: created, due, field, deck,
                   note, difficulty, stability, lapses, reviews, random
  fc searches <file>
                   List the saved searches
  fc save-search <file> <name> <query> [--sort <key>[:desc]] [--notes]
                   Save a search under a name
  fc run-search <file> <name> [--limit <n>] [--offset <n>]
                   Run a saved search with its own sort
  fc delete-search <file> <name>
                   Delete a saved search
  fc merge <from> <into>
                   Give every change in the file <from> to the file <into>, and print what the
                   merge did. <from> is not changed. Run it both ways to bring two files together
  fc export <file> <backup> [--deck <deck>] [--no-history]
                   Write a backup file (a zip, see docs/backup-format.md) of the whole collection,
                   or of one deck with its sub-decks, notes, note types and media. Review history
                   is included unless --no-history is given. Never overwrites a file
  fc restore <file> <backup>
                   Make the collection match a whole backup. Values that differ are written again as
                   new changes, what the backup does not have goes to the trash, and reviews made
                   since are kept
  fc import <file> <backup>
                   Add what a backup (a deck, or a whole one) holds, and keep what the collection has
  fc backup-info <backup>
                   Say what a backup file holds, without opening any collection
  fc fake <file> [--notes <n>] [--decks <n>] [--media <n>] [--review-days <n>]
                 [--new-per-day <n>] [--reviews-per-day <n>] [--seed <n>]
                   Make a collection of invented notes for performance testing: nested decks, Basic,
                   Basic and reversed and Cloze notes, tags, media, and a review history made by
                   answering cards day by day up to now. The same seed gives the same notes and answers
                   (due dates can differ a little).
                   Defaults: 1000 notes, 20 decks, notes/50 media files, no history, 20 new and
                   100 due cards a day, seed 1. Never overwrites a file
  fc bench <file>  Time the operations that matter at scale (deck counts, queue, searches, tags,
                   stats, export, merge, adding a template, ...) on a copy of the collection. The
                   collection itself is not changed
  fc help          Show this text

Options for any command:
  --now <time>     Pretend it is this time (RFC 3339, e.g. 2026-10-06T09:00:00+02:00). The offset
                   in the time is used unless --utc-offset is given. Step through days with it
  --utc-offset <minutes>
                   Pretend the device's time zone is this many minutes ahead of UTC";

/// Takes `--now` and `--utc-offset` (anywhere on the line) out of the arguments.
fn take_clock_options(args: Vec<String>) -> Result<Vec<String>, Failure> {
    let mut rest = Vec::new();
    let mut overrides = host::Overrides::default();
    let mut offset_from_time = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let wants = |name: &str| Failure::Usage(format!("{name} needs a value."));
        match arg.as_str() {
            "--now" => {
                let value = args.next().ok_or_else(|| wants("--now"))?;
                let time = chrono::DateTime::parse_from_rfc3339(&value).map_err(|_| {
                    Failure::Usage(format!(
                        "\"{value}\" is not a time. Use RFC 3339, for example \
                         2026-10-06T09:00:00+02:00."
                    ))
                })?;
                overrides.unix_ms = Some(time.timestamp_millis());
                offset_from_time = Some(time.offset().local_minus_utc() / 60);
            }
            "--utc-offset" => {
                let value = args.next().ok_or_else(|| wants("--utc-offset"))?;
                let minutes: i32 = value
                    .parse()
                    .ok()
                    .filter(|m| (-1440..=1440).contains(m))
                    .ok_or_else(|| {
                        Failure::Usage(format!(
                            "\"{value}\" is not a UTC offset in minutes, for example 120 or -300."
                        ))
                    })?;
                overrides.utc_offset_minutes = Some(minutes);
            }
            _ => rest.push(arg),
        }
    }
    if overrides.utc_offset_minutes.is_none() {
        overrides.utc_offset_minutes = offset_from_time;
    }
    host::set_overrides(overrides);
    Ok(rest)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match take_clock_options(args) {
        Ok(args) => args,
        Err(Failure::Usage(message)) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
        Err(Failure::Core(message)) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    match run(&args) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(message)) => {
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Core(message)) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

enum Failure {
    /// The command line was wrong.
    Usage(String),
    /// The core refused. The message already says what to do.
    Core(String),
}

impl From<fc_core::note::NoteError> for Failure {
    fn from(error: fc_core::note::NoteError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<fc_core::notetype::NoteTypeError> for Failure {
    fn from(error: fc_core::notetype::NoteTypeError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<fc_core::template::RenderError> for Failure {
    fn from(error: fc_core::template::RenderError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<fc_core::deck::DeckError> for Failure {
    fn from(error: fc_core::deck::DeckError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<fc_core::study::StudyError> for Failure {
    fn from(error: fc_core::study::StudyError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<fc_core::search::SearchError> for Failure {
    fn from(error: fc_core::search::SearchError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<fc_core::tag::TagError> for Failure {
    fn from(error: fc_core::tag::TagError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<fc_core::collection::CollectionError> for Failure {
    fn from(error: fc_core::collection::CollectionError) -> Self {
        Self::Core(error.to_string())
    }
}

fn describe(note_type: &fc_core::notetype::NoteType) -> String {
    let kind = match note_type.kind {
        fc_core::notetype::Kind::Standard => "standard",
        fc_core::notetype::Kind::Cloze => "cloze",
    };
    let fields: Vec<&str> = note_type.fields.iter().map(|f| f.name.as_str()).collect();
    let templates: Vec<&str> = note_type
        .templates
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    format!(
        "\n\n{} ({kind})\n  ID: {}\n  Fields: {}\n  Templates: {}",
        note_type.name,
        note_type.id,
        fields.join(", "),
        templates.join(", ")
    )
}

/// The name of a card: its template, with the cloze number for a cloze card.
fn card_name(note_type: &fc_core::notetype::NoteType, card: &fc_core::note::Card) -> String {
    let template = note_type
        .templates
        .iter()
        .find(|t| t.id == card.template)
        .map_or("?", |t| t.name.as_str());
    if card.ordinal == 0 {
        template.to_owned()
    } else {
        format!("{template} {}", card.ordinal)
    }
}

/// The live deck at a path such as `Polish::Verbs`, ignoring case.
fn find_deck(collection: &Collection, path: &str) -> Result<fc_core::deck::Deck, Failure> {
    let wanted = path.to_lowercase();
    let decks = collection.decks()?;
    match decks.iter().find(|d| d.path.to_lowercase() == wanted) {
        Some(deck) => Ok(deck.clone()),
        None => {
            let paths: Vec<&str> = decks.iter().map(|d| d.path.as_str()).collect();
            Err(Failure::Core(format!(
                "No deck called \"{path}\". The decks are: {}.",
                paths.join(", ")
            )))
        }
    }
}

fn open(file: &str) -> Result<Collection, Failure> {
    let host = host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
    Ok(Collection::open(file, host)?)
}

fn note_id(text: &str) -> Result<fc_core::id::Id, Failure> {
    text.parse()
        .map_err(|_| Failure::Usage(format!("\"{text}\" is not a note ID.")))
}

/// Takes `--deck <deck>` out of the arguments, and gives back the rest.
fn take_deck_option(args: &[String]) -> Result<(Option<String>, Vec<String>), Failure> {
    let mut rest = args.to_vec();
    let deck = match rest.iter().position(|a| a == "--deck") {
        Some(at) => {
            let path = rest
                .get(at + 1)
                .ok_or_else(|| Failure::Usage("--deck needs a deck.".to_owned()))?
                .clone();
            rest.drain(at..=at + 1);
            Some(path)
        }
        None => None,
    };
    Ok((deck, rest))
}

fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

fn run(args: &[String]) -> Result<String, Failure> {
    match args {
        [command] if command == "help" => Ok(USAGE.to_owned()),
        [command, file] if command == "new" => {
            Collection::create(file, host::host_for(file, true).map_err(Failure::Core)?)?
                .close()?;
            Ok(format!("Created a collection at {file}"))
        }
        [command, file] if command == "info" => {
            let persist = std::path::Path::new(file).exists();
            let host = host::host_for(file, persist).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
            let info = collection.info()?;
            collection.close()?;
            let mut text = format!(
                "Collection: {file}\nStorage version: {} (this build understands up to {})\nCreated by core: {}\nDevice ID: {}",
                info.schema_version, info.supported_schema_version, info.created_by, info.device_id
            );
            if !info.unsupported_features.is_empty() {
                text.push_str(&format!(
                    "\nNeeds features this build lacks (sync stays paused): {}",
                    info.unsupported_features.join(", ")
                ));
            }
            Ok(text)
        }
        [command, file] if command == "notetypes" => {
            let persist = std::path::Path::new(file).exists();
            let host = host::host_for(file, persist).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
            let live = collection
                .note_types()
                .map_err(|error| Failure::Core(error.to_string()))?;
            let deleted = collection
                .deleted_note_types()
                .map_err(|error| Failure::Core(error.to_string()))?;
            collection.close()?;
            let mut text = format!("Note types in {file}:");
            for note_type in live {
                text.push_str(&describe(&note_type));
            }
            if !deleted.is_empty() {
                text.push_str("\n\nDeleted (can be restored):");
                for note_type in deleted {
                    text.push_str(&describe(&note_type));
                }
            }
            Ok(text)
        }
        [command, file] if command == "notes" => {
            let host =
                host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
            let mut text = format!("Notes in {file}:");
            for note_type in collection.note_types()? {
                for note in collection.notes(note_type.id)? {
                    text.push_str(&format!("\n\n{} {}", note_type.name, note.id));
                    for field in &note.fields {
                        text.push_str(&format!("\n  {}: {}", field.name, field.value));
                    }
                    let names: Vec<String> = collection
                        .cards_of_note(note.id)?
                        .iter()
                        .map(|card| card_name(&note_type, card))
                        .collect();
                    text.push_str(&format!("\n  Cards: {}", names.join(", ")));
                    let ids: Vec<String> = collection
                        .cards_of_note(note.id)?
                        .iter()
                        .map(|card| card.id.to_string())
                        .collect();
                    text.push_str(&format!("\n  Card IDs: {}", ids.join(", ")));
                    let tags = collection.note_tags(note.id)?;
                    if !tags.is_empty() {
                        text.push_str(&format!("\n  Tags: {}", tags.join(" ")));
                    }
                }
            }
            collection.close()?;
            Ok(text)
        }
        [command, file] if command == "decks" => {
            let host =
                host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
            let presets = collection.presets()?;
            let mut text = format!("Decks in {file}:\n");
            for deck in collection.decks()? {
                let preset = presets
                    .iter()
                    .find(|p| p.id == deck.preset)
                    .map_or("?", |p| p.name.as_str());
                text.push_str(&format!(
                    "\n{}{} ({}, preset {preset})",
                    "  ".repeat(deck.depth),
                    deck.display_name,
                    plural(deck.cards, "card"),
                ));
            }
            text.push_str("\n\nOption presets:");
            for preset in &presets {
                let steps: Vec<String> = preset.learning_steps.iter().map(u32::to_string).collect();
                text.push_str(&format!(
                    "\n{}: {} new a day, {} reviews a day, learning steps {} minutes, desired \
                     retention {:.2}, used by {}",
                    preset.name,
                    preset.new_per_day,
                    preset.reviews_per_day,
                    if steps.is_empty() {
                        "none".to_owned()
                    } else {
                        steps.join(" ")
                    },
                    preset.desired_retention,
                    plural(preset.decks, "deck"),
                ));
            }
            collection.close()?;
            Ok(text)
        }
        [command, file] if command == "tags" => {
            let collection = open(file)?;
            let mut text = format!("Tags in {file}:\n");
            let tags = collection.tags()?;
            if tags.is_empty() {
                text.push_str("\n(none)");
            }
            for tag in tags {
                let own = tag
                    .name
                    .rsplit(fc_core::tag::SEPARATOR)
                    .next()
                    .unwrap_or("");
                text.push_str(&format!(
                    "\n{}{own} ({}{})",
                    "  ".repeat(tag.depth),
                    plural(tag.notes, "note"),
                    if tag.total > tag.notes {
                        format!(", {} with what is inside", tag.total)
                    } else {
                        String::new()
                    }
                ));
            }
            collection.close()?;
            Ok(text)
        }
        [command, file, note, tags @ ..] if command == "tag" || command == "untag" => {
            if tags.is_empty() {
                return Err(Failure::Usage(format!("\"{command}\" needs a tag.")));
            }
            let note = note_id(note)?;
            let collection = open(file)?;
            let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
            let text = if command == "tag" {
                let added = collection.add_tags(&[note], &tags)?;
                format!("Added {} to the note", plural(added, "tag"))
            } else {
                let removed = collection.remove_tags(&[note], &tags)?;
                format!("Removed {} from the note", plural(removed, "tag"))
            };
            let now = collection.note_tags(note)?;
            collection.close()?;
            Ok(format!(
                "{text}. Tags now: {}",
                if now.is_empty() {
                    "none".to_owned()
                } else {
                    now.join(" ")
                }
            ))
        }
        [command, file, from, to] if command == "rename-tag" => {
            let collection = open(file)?;
            let changed = collection.rename_tag(from, to)?;
            collection.close()?;
            Ok(format!(
                "Renamed {from} to {to} on {}",
                plural(changed, "note")
            ))
        }
        [command, file, path] if command == "add-deck" => {
            let host =
                host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
            let (parent, name) = match path.rsplit_once(fc_core::deck::SEPARATOR) {
                Some((parent, name)) => (Some(find_deck(&collection, parent)?.id), name),
                None => (None, path.as_str()),
            };
            let id = collection.create_deck(name, parent)?;
            collection.close()?;
            Ok(format!("Added deck {path} ({id})"))
        }
        [command, file, note_type, values @ ..] if command == "add-note" => {
            let host =
                host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
            let mut values = values.to_vec();
            let mut tags = Vec::new();
            while let Some(at) = values.iter().position(|v| v == "--tag") {
                let tag = values
                    .get(at + 1)
                    .ok_or_else(|| Failure::Usage("--tag needs a tag.".to_owned()))?
                    .clone();
                values.drain(at..=at + 1);
                tags.push(tag);
            }
            let tag_names: Vec<&str> = tags.iter().map(String::as_str).collect();
            // Check the tags before anything is written.
            for tag in &tag_names {
                fc_core::tag::check(tag)?;
            }
            let deck = match values.iter().position(|v| v == "--deck") {
                Some(at) => {
                    let path = values
                        .get(at + 1)
                        .ok_or_else(|| Failure::Usage("--deck needs a deck.".to_owned()))?
                        .clone();
                    values.drain(at..=at + 1);
                    find_deck(&collection, &path)?.id
                }
                None => fc_core::deck::default_deck(),
            };
            let wanted = note_type.to_lowercase();
            let types = collection.note_types()?;
            let Some(found) = types.iter().find(|t| t.name.to_lowercase() == wanted) else {
                let names: Vec<&str> = types.iter().map(|t| t.name.as_str()).collect();
                return Err(Failure::Core(format!(
                    "No note type called \"{note_type}\". The note types are: {}.",
                    names.join(", ")
                )));
            };
            let mut given = Vec::new();
            for value in &values {
                let (name, text) = value
                    .split_once('=')
                    .ok_or_else(|| Failure::Usage(format!("\"{value}\" is not Field=value.")))?;
                let field = found
                    .fields
                    .iter()
                    .find(|f| f.name == name)
                    .ok_or_else(|| {
                        let names: Vec<&str> =
                            found.fields.iter().map(|f| f.name.as_str()).collect();
                        Failure::Core(format!(
                            "{} has no field \"{name}\". Its fields are: {}.",
                            found.name,
                            names.join(", ")
                        ))
                    })?;
                given.push((field.id, text));
            }
            let added = collection.add_note_to_deck(deck, found.id, &given)?;
            collection.add_tags(&[added.id], &tag_names)?;
            collection.close()?;
            let mut text = format!(
                "Added note {} with {} card{}",
                added.id,
                added.cards.len(),
                if added.cards.len() == 1 { "" } else { "s" }
            );
            if !added.duplicates.is_empty() {
                text.push_str(&format!(
                    "\nWarning: {} duplicate of the first field already exists: {}",
                    added.duplicates.len(),
                    added
                        .duplicates
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Ok(text)
        }
        [command, file, note] if command == "render" => {
            let note_id: fc_core::id::Id = note
                .parse()
                .map_err(|_| Failure::Usage(format!("\"{note}\" is not a note ID.")))?;
            let host =
                host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
            let Some(found) = collection.note(note_id)?.filter(|n| !n.deleted) else {
                return Err(Failure::Core("No note with that ID.".to_owned()));
            };
            let note_type = collection
                .note_type(found.note_type)?
                .ok_or_else(|| Failure::Core("That note's note type is gone.".to_owned()))?;
            let mut text = String::new();
            for card in collection.cards_of_note(note_id)? {
                let rendered = collection.render_card(card.id)?;
                text.push_str(&format!(
                    "== {} ==\n-- front --\n{}\n-- back --\n{}\n-- media: {} --\n",
                    card_name(&note_type, &card),
                    rendered.front,
                    rendered.back,
                    if rendered.media.is_empty() {
                        "none".to_owned()
                    } else {
                        rendered.media.join(", ")
                    }
                ));
            }
            collection.close()?;
            Ok(text.trim_end().to_owned())
        }
        [command, from, into] if command == "merge" => merge::merge(from, into),
        [command, file] if command == "bench" => bench::bench(file),
        [command, file, rest @ ..] if command == "fake" => {
            fake::fake(file, &fake::Options::parse(rest)?)
        }
        [command, file, backup, rest @ ..] if command == "export" => {
            let mut deck = None;
            let mut history = true;
            let mut rest = rest.iter();
            while let Some(option) = rest.next() {
                match option.as_str() {
                    "--no-history" => history = false,
                    "--deck" => {
                        deck =
                            Some(rest.next().ok_or_else(|| {
                                Failure::Usage("--deck needs a deck.".to_owned())
                            })?);
                    }
                    other => {
                        return Err(Failure::Usage(format!("Unknown option \"{other}\".")));
                    }
                }
            }
            backup::export(file, backup, deck.map(String::as_str), history)
        }
        [command, file, backup] if command == "restore" => backup::restore(file, backup),
        [command, file, backup] if command == "import" => backup::import(file, backup),
        [command, backup] if command == "backup-info" => backup::info(backup),
        [command, file, path] if command == "add-media" => media::add(file, path),
        [command, file] if command == "media" => media::list(file),
        [command, file, name, out] if command == "media-get" => media::get(file, name, out),
        [command, file] if command == "media-check" => media::check(file),
        [command, file] if command == "delete-unused-media" => media::delete_unused(file),
        [command, file, card, answer] if command == "answer" => study::answer(file, card, answer),
        [command, file] if command == "undo" => study::undo(file),
        [command, file, card] if command == "history" => stats::history(file, card),
        [command, file, rest @ ..]
            if matches!(command.as_str(), "stats" | "forecast" | "optimise") =>
        {
            let (options, extra) = stats::Options::take(rest)?;
            if let Some(extra) = extra.first() {
                return Err(Failure::Usage(format!(
                    "\"{command}\" does not take \"{extra}\"."
                )));
            }
            match command.as_str() {
                "stats" => stats::stats(file, &options),
                "forecast" => stats::forecast(file, &options),
                _ => stats::optimise(file, &options),
            }
        }
        [command, file, rest @ ..]
            if matches!(
                command.as_str(),
                "search" | "searches" | "save-search" | "run-search" | "delete-search"
            ) =>
        {
            let (options, words) = search::Options::take(rest)?;
            match (command.as_str(), words.as_slice()) {
                ("search", [query]) => search::search(file, query, &options),
                ("searches", []) => search::list(file),
                ("save-search", [name, query]) => search::save(file, name, query, &options),
                ("run-search", [name]) => search::run_saved(file, name, &options),
                ("delete-search", [name]) => search::delete(file, name),
                _ => Err(Failure::Usage(format!(
                    "Wrong number of arguments for \"{command}\"."
                ))),
            }
        }
        [command, file, card] if command == "schedule" => study::schedule(file, card),
        [command, file, rest @ ..] if command == "due" || command == "next" => {
            let (deck, extra) = take_deck_option(rest)?;
            if !extra.is_empty() {
                return Err(Failure::Usage(format!(
                    "\"{}\" does not take \"{}\".",
                    command, extra[0]
                )));
            }
            if command == "due" {
                study::due_counts(file, deck.as_deref())
            } else {
                study::next(file, deck.as_deref())
            }
        }
        [command, file, rest @ ..]
            if matches!(
                command.as_str(),
                "suspend" | "unsuspend" | "bury" | "unbury"
            ) =>
        {
            let (deck, cards) = take_deck_option(rest)?;
            let what = match command.as_str() {
                "suspend" => study::Hide::Suspend,
                "unsuspend" => study::Hide::Unsuspend,
                "bury" => study::Hide::Bury,
                _ => study::Hide::Unbury,
            };
            let unbury = matches!(what, study::Hide::Unbury);
            if deck.is_some() && !unbury {
                return Err(Failure::Usage(format!(
                    "\"{command}\" takes card IDs, not --deck."
                )));
            }
            if deck.is_some() == !cards.is_empty() {
                return Err(Failure::Usage(format!(
                    "\"{command}\" needs {}.",
                    if unbury {
                        "card IDs or --deck <deck>, one of them"
                    } else {
                        "card IDs"
                    }
                )));
            }
            study::hide(file, what, &cards, deck.as_deref())
        }
        [] => Err(Failure::Usage("No command given.".to_owned())),
        [command, ..]
            if matches!(
                command.as_str(),
                "new"
                    | "info"
                    | "notetypes"
                    | "notes"
                    | "decks"
                    | "add-deck"
                    | "add-note"
                    | "tags"
                    | "tag"
                    | "untag"
                    | "rename-tag"
                    | "render"
                    | "answer"
                    | "undo"
                    | "schedule"
                    | "due"
                    | "next"
                    | "suspend"
                    | "unsuspend"
                    | "bury"
                    | "unbury"
                    | "history"
                    | "stats"
                    | "forecast"
                    | "optimise"
                    | "search"
                    | "searches"
                    | "save-search"
                    | "run-search"
                    | "delete-search"
                    | "merge"
                    | "export"
                    | "restore"
                    | "import"
                    | "backup-info"
                    | "help"
            ) =>
        {
            Err(Failure::Usage(format!(
                "Wrong number of arguments for \"{command}\"."
            )))
        }
        [command, ..] => Err(Failure::Usage(format!("Unknown command \"{command}\"."))),
    }
}
