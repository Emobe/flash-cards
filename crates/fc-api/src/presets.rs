//! Option presets (step 2.5b): reading them, changing them and choosing a deck's preset. Thin
//! wrappers over `fc_core::deck`.

use fc_core::Core;
use fc_core::deck::{DeckError, Preset, PresetChange};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::collection::not_open;
use crate::study::{internal, parse_id};
use crate::{ApiError, ErrorKind, Method, OpContext};

fn preset_error(what: &str, error: DeckError) -> ApiError {
    match error {
        DeckError::NotFound => ApiError::new(
            ErrorKind::NotFound,
            "That deck or options preset was deleted. Go back to the deck list and try again.",
        ),
        DeckError::EmptyName
        | DeckError::NameTaken(_)
        | DeckError::Default
        | DeckError::Retention
        | DeckError::Limit
        | DeckError::Steps => ApiError::invalid_input(error.to_string()),
        other => internal(what, other),
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PresetSummary {
    /// A UUID string.
    pub id: String,
    pub name: String,
    /// The Default preset, which cannot be deleted.
    pub is_default: bool,
    pub new_per_day: u32,
    pub reviews_per_day: u32,
    /// Minutes between the steps, in order.
    pub learning_steps: Vec<u32>,
    pub relearning_steps: Vec<u32>,
    /// A fraction from 0.70 to 0.99.
    pub desired_retention: f64,
    /// Hold a card back until tomorrow when a sibling was answered today.
    pub space_siblings: bool,
    /// Decks that use it.
    pub deck_count: u32,
}

fn summary(preset: Preset) -> PresetSummary {
    PresetSummary {
        id: preset.id.to_string(),
        is_default: preset.id == fc_core::deck::default_preset(),
        name: preset.name,
        new_per_day: preset.new_per_day,
        reviews_per_day: preset.reviews_per_day,
        learning_steps: preset.learning_steps,
        relearning_steps: preset.relearning_steps,
        desired_retention: preset.desired_retention,
        space_siblings: preset.space_siblings,
        deck_count: u32::try_from(preset.decks).unwrap_or(u32::MAX),
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PresetList {
    /// Every preset that is not deleted, by name.
    pub presets: Vec<PresetSummary>,
}

pub struct GetPresets;

impl Method for GetPresets {
    const NAME: &'static str = "getPresets";
    type Input = ();
    type Output = PresetList;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<PresetList, ApiError> {
        let presets = core
            .with_collection(|c| c.presets())
            .ok_or_else(|| not_open(core))?
            .map_err(|error| preset_error("read the options presets", error))?;
        Ok(PresetList {
            presets: presets.into_iter().map(summary).collect(),
        })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AddPresetInput {
    pub name: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NewPreset {
    /// A UUID string.
    pub id: String,
}

/// A new preset with the starting options.
pub struct CreatePreset;

impl Method for CreatePreset {
    const NAME: &'static str = "createPreset";
    type Input = AddPresetInput;
    type Output = NewPreset;

    fn call(core: &Core, input: AddPresetInput, _: &OpContext) -> Result<NewPreset, ApiError> {
        let id = core
            .with_collection(|c| c.create_preset(&input.name))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| preset_error("create the options preset", error))?;
        Ok(NewPreset { id: id.to_string() })
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RenamePresetInput {
    pub preset_id: String,
    pub name: String,
}

pub struct RenamePreset;

impl Method for RenamePreset {
    const NAME: &'static str = "renamePreset";
    type Input = RenamePresetInput;
    type Output = ();

    fn call(core: &Core, input: RenamePresetInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.preset_id)?;
        core.with_collection(|c| c.rename_preset(id, &input.name))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| preset_error("rename the options preset", error))
    }
}

/// Every field is optional: one left out stays as it is.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SetPresetOptionsInput {
    pub preset_id: String,
    pub new_per_day: Option<u32>,
    pub reviews_per_day: Option<u32>,
    pub learning_steps: Option<Vec<u32>>,
    pub relearning_steps: Option<Vec<u32>>,
    pub desired_retention: Option<f64>,
    pub space_siblings: Option<bool>,
}

/// Changes the options of a preset. A bad value changes nothing.
pub struct SetPresetOptions;

impl Method for SetPresetOptions {
    const NAME: &'static str = "setPresetOptions";
    type Input = SetPresetOptionsInput;
    type Output = ();

    fn call(core: &Core, input: SetPresetOptionsInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.preset_id)?;
        let change = PresetChange {
            new_per_day: input.new_per_day,
            reviews_per_day: input.reviews_per_day,
            learning_steps: input.learning_steps,
            relearning_steps: input.relearning_steps,
            desired_retention: input.desired_retention,
            space_siblings: input.space_siblings,
            fsrs_parameters: None,
        };
        core.with_collection(|c| c.set_preset_options(id, &change))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| preset_error("change the options preset", error))
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PresetIdInput {
    pub preset_id: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeletedPreset {
    /// Decks that went back to the Default preset.
    pub decks: u32,
}

/// Deletes a preset. Its decks go back to the Default preset.
pub struct DeletePreset;

impl Method for DeletePreset {
    const NAME: &'static str = "deletePreset";
    type Input = PresetIdInput;
    type Output = DeletedPreset;

    fn call(core: &Core, input: PresetIdInput, _: &OpContext) -> Result<DeletedPreset, ApiError> {
        let id = parse_id(&input.preset_id)?;
        let decks = core
            .with_collection(|c| {
                let used = c.preset(id)?.filter(|p| !p.deleted).map_or(0, |p| p.decks);
                c.delete_preset(id)?;
                Ok::<_, DeckError>(used)
            })
            .ok_or_else(|| not_open(core))?
            .map_err(|error| preset_error("delete the options preset", error))?;
        Ok(DeletedPreset {
            decks: u32::try_from(decks).unwrap_or(u32::MAX),
        })
    }
}

/// Brings a deleted preset back. Its decks stay on the Default preset.
pub struct RestorePreset;

impl Method for RestorePreset {
    const NAME: &'static str = "restorePreset";
    type Input = PresetIdInput;
    type Output = ();

    fn call(core: &Core, input: PresetIdInput, _: &OpContext) -> Result<(), ApiError> {
        let id = parse_id(&input.preset_id)?;
        core.with_collection(|c| c.restore_preset(id))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| preset_error("restore the options preset", error))
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SetDeckPresetInput {
    pub deck_id: String,
    pub preset_id: String,
}

/// Makes a deck use a preset.
pub struct SetDeckPreset;

impl Method for SetDeckPreset {
    const NAME: &'static str = "setDeckPreset";
    type Input = SetDeckPresetInput;
    type Output = ();

    fn call(core: &Core, input: SetDeckPresetInput, _: &OpContext) -> Result<(), ApiError> {
        let deck = parse_id(&input.deck_id)?;
        let preset = parse_id(&input.preset_id)?;
        core.with_collection(|c| c.set_deck_preset(deck, preset))
            .ok_or_else(|| not_open(core))?
            .map_err(|error| preset_error("choose the options preset", error))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fc_core::clock::ManualClock;
    use fc_core::id::Id;
    use serde_json::{Value, json};

    use crate::{Host, dispatch};

    use super::*;

    fn core() -> Core {
        let core = Core::new();
        let host = Host {
            clock: Arc::new(ManualClock::new(1_791_363_600_000)),
            installation_id: Id::from_bytes([1; 16]),
        };
        core.open_collection(":memory:", host).unwrap();
        core
    }

    fn call(core: &Core, method: &str, input: Value) -> Result<Value, ApiError> {
        dispatch(core, method, input, &OpContext::uncancellable()).map(|o| o.output)
    }

    fn presets(core: &Core) -> Vec<Value> {
        call(core, "getPresets", Value::Null).unwrap()["presets"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn make(core: &Core, name: &str) -> String {
        call(core, "createPreset", json!({ "name": name })).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn deck(core: &Core, name: &str) -> String {
        call(
            core,
            "createDeck",
            json!({ "name": name, "parentId": null }),
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn a_new_collection_has_the_default_preset_with_the_starting_options() {
        let core = core();
        let list = presets(&core);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["isDefault"], true);
        assert_eq!(list[0]["newPerDay"], 20);
        assert_eq!(list[0]["reviewsPerDay"], 200);
        assert_eq!(list[0]["learningSteps"], json!([1, 10]));
        assert_eq!(list[0]["relearningSteps"], json!([10]));
        assert_eq!(list[0]["desiredRetention"], 0.9);
        assert_eq!(list[0]["spaceSiblings"], true);
        assert_eq!(list[0]["deckCount"], 1);
    }

    #[test]
    fn a_preset_is_created_renamed_and_its_options_changed() {
        let core = core();
        let id = make(&core, "Hard words");
        call(
            &core,
            "renamePreset",
            json!({ "presetId": id, "name": "Hard" }),
        )
        .unwrap();
        call(
            &core,
            "setPresetOptions",
            json!({
                "presetId": id,
                "newPerDay": 5,
                "reviewsPerDay": 80,
                "learningSteps": [2, 20, 60],
                "relearningSteps": [],
                "desiredRetention": 0.95,
                "spaceSiblings": false,
            }),
        )
        .unwrap();
        let hard = presets(&core)
            .into_iter()
            .find(|p| p["id"] == id.as_str())
            .unwrap();
        assert_eq!(hard["name"], "Hard");
        assert_eq!(hard["newPerDay"], 5);
        assert_eq!(hard["reviewsPerDay"], 80);
        assert_eq!(hard["learningSteps"], json!([2, 20, 60]));
        assert_eq!(hard["relearningSteps"], json!([]));
        assert_eq!(hard["desiredRetention"], 0.95);
        assert_eq!(hard["spaceSiblings"], false);
    }

    #[test]
    fn a_field_left_out_stays_as_it_is() {
        let core = core();
        let id = make(&core, "A");
        call(
            &core,
            "setPresetOptions",
            json!({ "presetId": id, "newPerDay": 7 }),
        )
        .unwrap();
        let a = presets(&core)
            .into_iter()
            .find(|p| p["id"] == id.as_str())
            .unwrap();
        assert_eq!(a["newPerDay"], 7);
        assert_eq!(a["reviewsPerDay"], 200);
        assert_eq!(a["learningSteps"], json!([1, 10]));
    }

    #[test]
    fn a_bad_value_is_refused_with_a_sentence_and_changes_nothing() {
        let core = core();
        let id = make(&core, "A");
        for change in [
            json!({ "newPerDay": 10_000 }),
            json!({ "reviewsPerDay": 10_000 }),
            json!({ "desiredRetention": 0.5 }),
            json!({ "desiredRetention": 1.0 }),
            json!({ "learningSteps": [0] }),
            json!({ "learningSteps": [1, 1441] }),
            json!({ "relearningSteps": [1, 2, 3, 4, 5, 6, 7, 8, 9] }),
            // A good value next to a bad one is not written either.
            json!({ "newPerDay": 3, "desiredRetention": 0.2 }),
        ] {
            let mut input = change.clone();
            input["presetId"] = json!(id);
            let err = call(&core, "setPresetOptions", input).unwrap_err();
            assert_eq!(err.kind, ErrorKind::InvalidInput, "{change}");
            assert!(!err.message.is_empty());
        }
        let a = presets(&core)
            .into_iter()
            .find(|p| p["id"] == id.as_str())
            .unwrap();
        assert_eq!(a["newPerDay"], 20);
        assert_eq!(a["desiredRetention"], 0.9);
    }

    #[test]
    fn names_must_be_unique_and_not_empty() {
        let core = core();
        make(&core, "Hard");
        for name in ["", "  ", "hard", "DEFAULT"] {
            let err = call(&core, "createPreset", json!({ "name": name })).unwrap_err();
            assert_eq!(err.kind, ErrorKind::InvalidInput, "{name:?}");
        }
        assert_eq!(presets(&core).len(), 2);
    }

    #[test]
    fn a_deck_is_given_a_preset_and_deleting_it_sends_the_deck_back_to_the_default() {
        let core = core();
        let polish = deck(&core, "Polish");
        let czech = deck(&core, "Czech");
        let hard = make(&core, "Hard");
        call(
            &core,
            "setDeckPreset",
            json!({ "deckId": polish, "presetId": hard }),
        )
        .unwrap();
        call(
            &core,
            "setDeckPreset",
            json!({ "deckId": czech, "presetId": hard }),
        )
        .unwrap();
        let count = |core: &Core, id: &str| {
            presets(core)
                .into_iter()
                .find(|p| p["id"] == id)
                .map(|p| p["deckCount"].clone())
        };
        assert_eq!(count(&core, &hard), Some(json!(2)));

        let gone = call(&core, "deletePreset", json!({ "presetId": hard })).unwrap();
        assert_eq!(gone, json!({ "decks": 2 }));
        assert_eq!(count(&core, &hard), None);
        let default = presets(&core)[0]["id"].as_str().unwrap().to_owned();
        assert_eq!(count(&core, &default), Some(json!(3)));

        // Deleting again counts nothing. Restore brings the preset back, with no decks.
        let again = call(&core, "deletePreset", json!({ "presetId": hard })).unwrap();
        assert_eq!(again, json!({ "decks": 0 }));
        call(&core, "restorePreset", json!({ "presetId": hard })).unwrap();
        assert_eq!(count(&core, &hard), Some(json!(0)));
    }

    #[test]
    fn the_default_preset_cannot_be_deleted_and_a_taken_name_blocks_restore() {
        let core = core();
        let default = presets(&core)[0]["id"].as_str().unwrap().to_owned();
        let err = call(&core, "deletePreset", json!({ "presetId": default })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("Default"));

        let hard = make(&core, "Hard");
        call(&core, "deletePreset", json!({ "presetId": hard })).unwrap();
        make(&core, "Hard");
        let err = call(&core, "restorePreset", json!({ "presetId": hard })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("Hard"));
    }

    #[test]
    fn a_missing_preset_or_deck_and_a_bad_id_are_refused() {
        let core = core();
        let gone = Id::from_bytes([9; 16]).to_string();
        let polish = deck(&core, "Polish");
        for (method, input) in [
            ("renamePreset", json!({ "presetId": gone, "name": "x" })),
            (
                "setPresetOptions",
                json!({ "presetId": gone, "newPerDay": 1 }),
            ),
            ("deletePreset", json!({ "presetId": gone })),
            ("restorePreset", json!({ "presetId": gone })),
            (
                "setDeckPreset",
                json!({ "deckId": polish, "presetId": gone }),
            ),
            ("setDeckPreset", json!({ "deckId": gone, "presetId": gone })),
        ] {
            assert_eq!(
                call(&core, method, input).unwrap_err().kind,
                ErrorKind::NotFound,
                "{method}"
            );
        }
        let err = call(&core, "deletePreset", json!({ "presetId": "nope" })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn without_a_collection_it_says_so() {
        let err = dispatch(
            &Core::new(),
            "getPresets",
            Value::Null,
            &OpContext::uncancellable(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
    }
}
