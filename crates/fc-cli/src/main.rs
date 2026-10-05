//! Developer CLI. It drives `fc-core` directly, so every step of Phase 1 can be tried from a
//! terminal before any UI exists. Step 1.14 grows it into the full tool.

mod host;

use std::process::ExitCode;

use fc_core::collection::Collection;

const USAGE: &str = "\
Usage:
  fc new <file>    Create an empty collection at <file>
  fc info <file>   Open a collection, print facts about it, and close it
  fc notetypes <file>
                   List the note types with their fields and templates
  fc notes <file>  List the notes with their fields and cards
  fc add-note <file> <note type> <Field=value>...
                   Add a note. Fields left out are empty. Warns about duplicates
  fc render <file> <note ID>
                   Print the front and back HTML of each card of a note, and its media
  fc help          Show this text";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
                }
            }
            collection.close()?;
            Ok(text)
        }
        [command, file, note_type, values @ ..] if command == "add-note" => {
            let host =
                host::host_for(file, std::path::Path::new(file).exists()).map_err(Failure::Core)?;
            let collection = Collection::open(file, host)?;
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
            for value in values {
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
            let added = collection.add_note(found.id, &given)?;
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
        [] => Err(Failure::Usage("No command given.".to_owned())),
        [command, ..]
            if matches!(
                command.as_str(),
                "new" | "info" | "notetypes" | "notes" | "add-note" | "render" | "help"
            ) =>
        {
            Err(Failure::Usage(format!(
                "Wrong number of arguments for \"{command}\"."
            )))
        }
        [command, ..] => Err(Failure::Usage(format!("Unknown command \"{command}\"."))),
    }
}
