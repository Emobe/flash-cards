//! Developer CLI. It drives `fc-core` directly, so every step of Phase 1 can be tried from a
//! terminal before any UI exists. Step 1.14 grows it into the full tool.

mod host;

use std::process::ExitCode;

use fc_core::collection::Collection;

const USAGE: &str = "\
Usage:
  fc new <file>    Create an empty collection at <file>
  fc info <file>   Open a collection, print facts about it, and close it
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
        [] => Err(Failure::Usage("No command given.".to_owned())),
        [command, ..] if matches!(command.as_str(), "new" | "info" | "help") => Err(
            Failure::Usage(format!("Wrong number of arguments for \"{command}\".")),
        ),
        [command, ..] => Err(Failure::Usage(format!("Unknown command \"{command}\"."))),
    }
}
