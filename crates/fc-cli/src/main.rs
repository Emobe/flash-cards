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
        [] => Err(Failure::Usage("No command given.".to_owned())),
        [command, ..] if matches!(command.as_str(), "new" | "info" | "notetypes" | "help") => Err(
            Failure::Usage(format!("Wrong number of arguments for \"{command}\".")),
        ),
        [command, ..] => Err(Failure::Usage(format!("Unknown command \"{command}\"."))),
    }
}
