//! Temporary methods for the storage spike (step 0.4). Deleted in step 1.1 with `fc_core::spike`.

use fc_core::Core;
use fc_core::spike::{MAX_NOTE_CHARS, SpikeError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ApiError, ErrorKind, Method, OpContext};

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AddNoteInput {
    pub text: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SpikeNotes {
    pub notes: Vec<String>,
}

fn not_open() -> ApiError {
    ApiError::new(ErrorKind::NotFound, "No collection is open.")
}

fn to_api(error: SpikeError) -> ApiError {
    match error {
        SpikeError::InvalidNote => ApiError::invalid_input(format!(
            "A note must be between 1 and {MAX_NOTE_CHARS} characters. Change the text and try again."
        )),
        SpikeError::Storage(_) => ApiError::internal(),
    }
}

fn list(core: &Core) -> Result<SpikeNotes, ApiError> {
    let notes = core
        .with_spike(|store| store.list_notes())
        .ok_or_else(not_open)?
        .map_err(to_api)?;
    Ok(SpikeNotes { notes })
}

/// Adds a note and returns every note.
pub struct SpikeAddNote;

impl Method for SpikeAddNote {
    const NAME: &'static str = "spikeAddNote";
    type Input = AddNoteInput;
    type Output = SpikeNotes;

    fn call(core: &Core, input: AddNoteInput, _: &OpContext) -> Result<SpikeNotes, ApiError> {
        core.with_spike(|store| store.add_note(&input.text))
            .ok_or_else(not_open)?
            .map_err(to_api)?;
        list(core)
    }
}

/// Lists every note.
pub struct SpikeListNotes;

impl Method for SpikeListNotes {
    const NAME: &'static str = "spikeListNotes";
    type Input = ();
    type Output = SpikeNotes;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<SpikeNotes, ApiError> {
        list(core)
    }
}
