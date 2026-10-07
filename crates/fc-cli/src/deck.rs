//! `fc deck`, `fc preset` and `fc move-cards`: changing decks and their option presets, tried from
//! a terminal.

use fc_core::deck::{Preset, PresetChange};
use fc_core::id::Id;

use super::{Failure, find_deck, open};

fn wrong(command: &str) -> Failure {
    Failure::Usage(format!("Wrong arguments for \"{command}\"."))
}

fn card_ids(texts: &[String]) -> Result<Vec<Id>, Failure> {
    texts
        .iter()
        .map(|text| {
            text.parse()
                .map_err(|_| Failure::Usage(format!("\"{text}\" is not a card ID.")))
        })
        .collect()
}

fn on_off(text: &str) -> Result<bool, Failure> {
    match text {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(Failure::Usage(format!("\"{text}\" is not on or off."))),
    }
}

/// Minutes as "1 10", "1,10" or "none" (no steps).
fn steps(text: &str) -> Result<Vec<u32>, Failure> {
    if text == "none" {
        return Ok(Vec::new());
    }
    text.split([' ', ','])
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse().map_err(|_| {
                Failure::Usage(format!(
                    "\"{part}\" is not a number of minutes. Write steps as \"1 10\" or none."
                ))
            })
        })
        .collect()
}

fn number<T: std::str::FromStr>(name: &str, text: &str) -> Result<T, Failure> {
    text.parse()
        .map_err(|_| Failure::Usage(format!("\"{text}\" is not a valid value for {name}.")))
}

pub fn deck(file: &str, args: &[String]) -> Result<String, Failure> {
    let collection = open(file)?;
    let text = match args {
        [verb, path, name] if verb == "rename" => {
            let found = find_deck(&collection, path)?;
            collection.rename_deck(found.id, name)?;
            format!("Renamed {} to {}", found.path, name.trim())
        }
        [verb, path, parent] if verb == "move" => {
            let found = find_deck(&collection, path)?;
            let parent = if parent == "--top" {
                None
            } else {
                Some(find_deck(&collection, parent)?.id)
            };
            collection.move_deck(found.id, parent)?;
            let moved = collection
                .deck(found.id)?
                .map_or(found.path, |deck| deck.path);
            format!("Moved the deck. It is now {moved}")
        }
        [verb, path] if verb == "delete" => {
            let found = find_deck(&collection, path)?;
            collection.delete_deck(found.id)?;
            format!(
                "Deleted {} with its sub-decks and cards. Restore it with: deck <file> restore {}",
                found.path, found.id
            )
        }
        [verb, wanted] if verb == "restore" => {
            let deleted = collection.deleted_decks()?;
            let by_id: Option<Id> = wanted.parse().ok();
            let lower = wanted.to_lowercase();
            let Some(found) = deleted
                .iter()
                .find(|d| Some(d.id) == by_id || d.path.to_lowercase() == lower)
            else {
                return Err(Failure::Core(format!(
                    "No deleted deck \"{wanted}\". Deleted decks: {}.",
                    if deleted.is_empty() {
                        "none".to_owned()
                    } else {
                        deleted
                            .iter()
                            .map(|d| format!("{} ({})", d.path, d.id))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                )));
            };
            collection.restore_deck(found.id)?;
            format!("Restored {}", found.path)
        }
        [verb, path, setting] if verb == "limits" => {
            let found = find_deck(&collection, path)?;
            let on = on_off(setting)?;
            collection.set_deck_limits_include_subdecks(found.id, on)?;
            format!(
                "The limits of {} {} the cards in its sub-decks",
                found.path,
                if on { "now count" } else { "no longer count" }
            )
        }
        _ => return Err(wrong("deck")),
    };
    collection.close()?;
    Ok(text)
}

pub fn move_cards(file: &str, deck: &str, cards: &[String]) -> Result<String, Failure> {
    if cards.is_empty() {
        return Err(Failure::Usage("move-cards needs card IDs.".to_owned()));
    }
    let collection = open(file)?;
    let target = find_deck(&collection, deck)?;
    collection.move_cards(&card_ids(cards)?, target.id)?;
    collection.close()?;
    Ok(format!(
        "Moved {} to {}",
        super::plural(cards.len(), "card"),
        target.path
    ))
}

/// A live preset by ID or name (ignoring case).
fn find_preset(presets: &[Preset], wanted: &str) -> Result<Preset, Failure> {
    let by_id: Option<Id> = wanted.parse().ok();
    let lower = wanted.to_lowercase();
    match presets
        .iter()
        .find(|p| Some(p.id) == by_id || p.name.to_lowercase() == lower)
    {
        Some(found) => Ok(found.clone()),
        None => {
            let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
            Err(Failure::Core(format!(
                "No preset called \"{wanted}\". The presets are: {}.",
                names.join(", ")
            )))
        }
    }
}

/// What `preset set` was asked to change.
fn change_from(options: &[String]) -> Result<PresetChange, Failure> {
    let mut change = PresetChange::default();
    let mut options = options.iter();
    while let Some(option) = options.next() {
        let value = options
            .next()
            .ok_or_else(|| Failure::Usage(format!("{option} needs a value.")))?;
        match option.as_str() {
            "--new-per-day" => change.new_per_day = Some(number(option, value)?),
            "--reviews-per-day" => change.reviews_per_day = Some(number(option, value)?),
            "--learning-steps" => change.learning_steps = Some(steps(value)?),
            "--relearning-steps" => change.relearning_steps = Some(steps(value)?),
            "--retention" => change.desired_retention = Some(number(option, value)?),
            "--space-siblings" => change.space_siblings = Some(on_off(value)?),
            "--fsrs-parameters" => {
                change.fsrs_parameters = Some(if value == "default" {
                    Vec::new()
                } else {
                    value
                        .split([' ', ','])
                        .filter(|part| !part.is_empty())
                        .map(|part| number("--fsrs-parameters", part))
                        .collect::<Result<_, _>>()?
                });
            }
            other => return Err(Failure::Usage(format!("Unknown option \"{other}\"."))),
        }
    }
    if change == PresetChange::default() {
        return Err(Failure::Usage(
            "preset set needs at least one option: --new-per-day, --reviews-per-day, \
             --learning-steps, --relearning-steps, --retention, --space-siblings, \
             --fsrs-parameters."
                .to_owned(),
        ));
    }
    Ok(change)
}

pub fn preset(file: &str, args: &[String]) -> Result<String, Failure> {
    let collection = open(file)?;
    let text = match args {
        [verb, name] if verb == "add" => {
            let id = collection.create_preset(name)?;
            format!("Added preset {} ({id})", name.trim())
        }
        [verb, preset, name] if verb == "rename" => {
            let found = find_preset(&collection.presets()?, preset)?;
            collection.rename_preset(found.id, name)?;
            format!("Renamed {} to {}", found.name, name.trim())
        }
        [verb, preset, options @ ..] if verb == "set" => {
            let found = find_preset(&collection.presets()?, preset)?;
            collection.set_preset_options(found.id, &change_from(options)?)?;
            format!("Changed the options of {}", found.name)
        }
        [verb, preset] if verb == "delete" => {
            let found = find_preset(&collection.presets()?, preset)?;
            collection.delete_preset(found.id)?;
            format!(
                "Deleted {}. Its decks use the Default preset. Restore it with: preset <file> \
                 restore {}",
                found.name, found.id
            )
        }
        [verb, preset] if verb == "restore" => {
            let deleted = collection.deleted_presets()?;
            let found = find_preset(&deleted, preset)?;
            collection.restore_preset(found.id)?;
            format!("Restored {}", found.name)
        }
        [verb, deck, preset] if verb == "assign" => {
            let found = find_deck(&collection, deck)?;
            let chosen = find_preset(&collection.presets()?, preset)?;
            collection.set_deck_preset(found.id, chosen.id)?;
            format!("{} now uses the preset {}", found.path, chosen.name)
        }
        _ => return Err(wrong("preset")),
    };
    collection.close()?;
    Ok(text)
}
