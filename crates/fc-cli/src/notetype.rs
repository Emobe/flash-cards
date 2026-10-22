//! `fc notetype`, `fc field` and `fc template`: changing note types, their fields and their card
//! templates, tried from a terminal.

use fc_core::collection::Collection;
use fc_core::id::Id;
use fc_core::notetype::{Draft, DraftField, DraftTemplate, Field, Kind, NoteType, Template};
use serde_json::{Value, json};

use super::{Failure, open};

/// A value on the command line: the text itself, or `@path` to read it from a file (a template or
/// a stylesheet is awkward to type).
pub fn text_arg(value: &str) -> Result<String, Failure> {
    match value.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| Failure::Core(format!("Cannot read \"{path}\": {e}."))),
        None => Ok(value.to_owned()),
    }
}

/// Finds one of `items` by ID or by name (ignoring case). `what` is for the message.
fn pick<'a, T>(
    items: &'a [T],
    wanted: &str,
    what: &str,
    parts: impl Fn(&T) -> (Id, &str),
) -> Result<&'a T, Failure> {
    let lower = wanted.to_lowercase();
    let by_id: Option<Id> = wanted.parse().ok();
    match items
        .iter()
        .find(|item| Some(parts(item).0) == by_id || parts(item).1.to_lowercase() == lower)
    {
        Some(found) => Ok(found),
        None => {
            let names: Vec<&str> = items.iter().map(|item| parts(item).1).collect();
            Err(Failure::Core(format!(
                "No {what} called \"{wanted}\". {}",
                if names.is_empty() {
                    "There are none.".to_owned()
                } else {
                    format!("The {what}s are: {}.", names.join(", "))
                }
            )))
        }
    }
}

/// A live note type, by name or ID.
pub fn find_note_type(collection: &Collection, wanted: &str) -> Result<NoteType, Failure> {
    let types = collection.note_types()?;
    Ok(pick(&types, wanted, "note type", |t| (t.id, &t.name))?.clone())
}

fn field_of<'a>(note_type: &'a NoteType, wanted: &str) -> Result<&'a Field, Failure> {
    pick(&note_type.fields, wanted, "field", |f| (f.id, &f.name))
}

fn template_of<'a>(note_type: &'a NoteType, wanted: &str) -> Result<&'a Template, Failure> {
    pick(&note_type.templates, wanted, "template", |t| {
        (t.id, &t.name)
    })
}

/// A 1-based position on the command line as the 0-based index the core wants.
fn position(text: &str) -> Result<usize, Failure> {
    text.parse::<usize>()
        .ok()
        .filter(|n| *n >= 1)
        .map(|n| n - 1)
        .ok_or_else(|| Failure::Usage(format!("\"{text}\" is not a position (1 is the first).")))
}

fn wrong(command: &str) -> Failure {
    Failure::Usage(format!("Wrong arguments for \"{command}\"."))
}

/// A draft as JSON, the shape `notetype save` reads.
fn draft_json(draft: &Draft) -> Value {
    json!({
        "name": draft.name,
        "css": draft.css,
        "sortField": draft.sort_field,
        "fields": draft.fields.iter().map(|f| json!({
            "id": f.id.map(|id| id.to_string()), "name": f.name,
        })).collect::<Vec<_>>(),
        "templates": draft.templates.iter().map(|t| json!({
            "id": t.id.map(|id| id.to_string()), "name": t.name, "front": t.front, "back": t.back,
        })).collect::<Vec<_>>(),
    })
}

fn bad_draft(what: &str) -> Failure {
    Failure::Usage(format!("The draft file is not valid: {what}."))
}

fn draft_text(value: &Value, key: &str) -> Result<String, Failure> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| bad_draft(&format!("\"{key}\" must be text")))
}

fn draft_id(value: &Value) -> Result<Option<Id>, Failure> {
    match value.get("id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => text
            .parse()
            .map(Some)
            .map_err(|_| bad_draft("an \"id\" is not an ID")),
        Some(_) => Err(bad_draft("an \"id\" must be text or null")),
    }
}

/// Reads a draft from JSON text: `{"name", "css", "sortField": n or null, "fields": [{"id": ID or
/// null, "name"}], "templates": [{"id", "name", "front", "back"}]}`. `notetype draft` prints one.
fn parse_draft(text: &str) -> Result<Draft, Failure> {
    let value: Value = serde_json::from_str(text).map_err(|e| bad_draft(&e.to_string()))?;
    let list = |key: &str| -> Result<Vec<Value>, Failure> {
        value
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| bad_draft(&format!("\"{key}\" must be a list")))
    };
    Ok(Draft {
        name: draft_text(&value, "name")?,
        css: draft_text(&value, "css")?,
        sort_field: match value.get("sortField") {
            None | Some(Value::Null) => None,
            Some(n) => Some(
                n.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| bad_draft("\"sortField\" must be a number or null"))?,
            ),
        },
        fields: list("fields")?
            .iter()
            .map(|f| {
                Ok(DraftField {
                    id: draft_id(f)?,
                    name: draft_text(f, "name")?,
                })
            })
            .collect::<Result<_, Failure>>()?,
        templates: list("templates")?
            .iter()
            .map(|t| {
                Ok(DraftTemplate {
                    id: draft_id(t)?,
                    name: draft_text(t, "name")?,
                    front: draft_text(t, "front")?,
                    back: draft_text(t, "back")?,
                })
            })
            .collect::<Result<_, Failure>>()?,
    })
}

/// "3 cards added, 1 card removed, 2 notes affected".
fn consequences(outcome: &fc_core::notetype::SaveOutcome) -> String {
    format!(
        "{} added, {} sent to the trash, {} affected",
        plural(outcome.cards_added, "card", "cards"),
        plural(outcome.cards_removed, "card", "cards"),
        plural(outcome.notes_affected, "note", "notes"),
    )
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

pub fn notetype(file: &str, args: &[String]) -> Result<String, Failure> {
    let collection = open(file)?;
    let text = match args {
        [verb, name, rest @ ..] if verb == "add" => {
            let kind = match rest {
                [] => Kind::Standard,
                [option] if option == "--cloze" => Kind::Cloze,
                _ => return Err(wrong("notetype add")),
            };
            let id = collection.create_note_type(name, kind)?;
            format!("Added note type {} ({id})", name.trim())
        }
        [verb, note_type, name] if verb == "rename" => {
            let found = find_note_type(&collection, note_type)?;
            collection.rename_note_type(found.id, name)?;
            format!("Renamed {} to {}", found.name, name.trim())
        }
        [verb, note_type, css] if verb == "css" => {
            let found = find_note_type(&collection, note_type)?;
            collection.set_note_type_css(found.id, &text_arg(css)?)?;
            format!("Changed the CSS of {}", found.name)
        }
        [verb, note_type, field] if verb == "sort-field" => {
            let found = find_note_type(&collection, note_type)?;
            let chosen = field_of(&found, field)?;
            collection.set_sort_field(found.id, chosen.id)?;
            format!("{} now sorts by {}", found.name, chosen.name)
        }
        [verb, note_type, name] if verb == "copy" => {
            let found = find_note_type(&collection, note_type)?;
            let id = collection.copy_note_type(found.id, name)?;
            format!("Copied {} to {} ({id})", found.name, name.trim())
        }
        [verb, note_type] if verb == "draft" => {
            let found = find_note_type(&collection, note_type)?;
            serde_json::to_string_pretty(&draft_json(&Draft::of(&found)))
                .map_err(|e| Failure::Core(e.to_string()))?
        }
        [verb, note_type, draft] if verb == "check" || verb == "save" => {
            let found = find_note_type(&collection, note_type)?;
            let draft = parse_draft(&text_arg(draft)?)?;
            collection.check_note_type(found.id, &draft)?;
            if verb == "check" {
                format!(
                    "Saving would change: {}",
                    consequences(&collection.note_type_consequences(found.id, &draft)?)
                )
            } else {
                let done = collection.save_note_type(found.id, &draft)?;
                if done.undo.is_empty() {
                    format!("No changes to save in {}", found.name)
                } else {
                    format!("Saved {}: {}", draft.name.trim(), consequences(&done))
                }
            }
        }
        [verb, note_type, options @ ..] if verb == "preview" => {
            let found = find_note_type(&collection, note_type)?;
            let (mut template, mut note, mut ordinal) = (0, None, 1);
            let mut draft = Draft::of(&found);
            let mut options = options.iter();
            while let Some(option) = options.next() {
                let value = options
                    .next()
                    .ok_or_else(|| Failure::Usage(format!("{option} needs a value.")))?;
                match option.as_str() {
                    "--template" => template = position(value)?,
                    "--note" => {
                        note = Some(value.parse::<Id>().map_err(|_| {
                            Failure::Usage(format!("\"{value}\" is not a note ID."))
                        })?);
                    }
                    "--cloze" => {
                        ordinal = value.parse().map_err(|_| {
                            Failure::Usage(format!("\"{value}\" is not a cloze number."))
                        })?;
                    }
                    "--draft" => draft = parse_draft(&text_arg(value)?)?,
                    other => return Err(Failure::Usage(format!("Unknown option \"{other}\"."))),
                }
            }
            let shown = collection.preview_card(found.id, &draft, template, note, ordinal)?;
            let mut text = String::new();
            for problem in &shown.problems {
                text.push_str(&format!("{problem}\n"));
            }
            if !shown.unknown_fields.is_empty() {
                text.push_str(&format!(
                    "Reads fields the note type does not have: {}\n",
                    shown.unknown_fields.join(", ")
                ));
            }
            if !shown.makes_card {
                text.push_str("This note makes no card with this template.\n");
            }
            if let Some(card) = shown.rendered {
                text.push_str(&format!(
                    "-- front --\n{}\n-- back --\n{}\n-- media: {} --",
                    card.front,
                    card.back,
                    if card.media.is_empty() {
                        "none".to_owned()
                    } else {
                        card.media.join(", ")
                    }
                ));
            }
            text.trim_end().to_owned()
        }
        [verb, note_type] if verb == "delete" => {
            let found = find_note_type(&collection, note_type)?;
            collection.delete_note_type(found.id)?;
            format!(
                "Deleted {} with its notes and cards. Restore it with: notetype <file> restore {}",
                found.name, found.id
            )
        }
        [verb, note_type] if verb == "restore" => {
            let deleted = collection.deleted_note_types()?;
            let found = pick(&deleted, note_type, "deleted note type", |t| {
                (t.id, &t.name)
            })?;
            collection.restore_note_type(found.id)?;
            format!("Restored {}", found.name)
        }
        _ => return Err(wrong("notetype")),
    };
    collection.close()?;
    Ok(text)
}

pub fn field(file: &str, args: &[String]) -> Result<String, Failure> {
    let [note_type, verb, rest @ ..] = args else {
        return Err(wrong("field"));
    };
    let collection = open(file)?;
    let found = find_note_type(&collection, note_type)?;
    let text = match (verb.as_str(), rest) {
        ("add", [name]) => {
            let id = collection.add_field(found.id, name)?;
            format!("Added field {} to {} ({id})", name.trim(), found.name)
        }
        ("rename", [field, name]) => {
            let old = field_of(&found, field)?;
            collection.rename_field(found.id, old.id, name)?;
            format!(
                "Renamed {} to {} (templates that use it are rewritten)",
                old.name,
                name.trim()
            )
        }
        ("move", [field, at]) => {
            let chosen = field_of(&found, field)?;
            collection.move_field(found.id, chosen.id, position(at)?)?;
            format!("Moved {} to position {at}", chosen.name)
        }
        ("remove", [field]) => {
            let chosen = field_of(&found, field)?;
            collection.remove_field(found.id, chosen.id)?;
            format!(
                "Removed {}. Its values are kept, and come back with: field <file> {} restore {}",
                chosen.name, found.name, chosen.id
            )
        }
        ("restore", [field]) => {
            let removed = collection.removed_fields(found.id)?;
            let chosen = pick(&removed, field, "removed field", |f| (f.id, &f.name))?;
            collection.restore_field(found.id, chosen.id)?;
            format!("Restored {}", chosen.name)
        }
        _ => return Err(wrong("field")),
    };
    collection.close()?;
    Ok(text)
}

pub fn template(file: &str, args: &[String]) -> Result<String, Failure> {
    let [note_type, verb, rest @ ..] = args else {
        return Err(wrong("template"));
    };
    let collection = open(file)?;
    let found = find_note_type(&collection, note_type)?;
    let text = match (verb.as_str(), rest) {
        ("add", [name, front, back]) => {
            let id =
                collection.add_template(found.id, name, &text_arg(front)?, &text_arg(back)?)?;
            format!("Added template {} to {} ({id})", name.trim(), found.name)
        }
        ("rename", [template, name]) => {
            let old = template_of(&found, template)?;
            collection.rename_template(found.id, old.id, name)?;
            format!("Renamed {} to {}", old.name, name.trim())
        }
        ("set", [template, options @ ..]) => {
            let chosen = template_of(&found, template)?;
            let (mut front, mut back) = (chosen.front.clone(), chosen.back.clone());
            let mut options = options.iter();
            let mut changed = 0;
            while let Some(option) = options.next() {
                let value = options
                    .next()
                    .ok_or_else(|| Failure::Usage(format!("{option} needs a value.")))?;
                match option.as_str() {
                    "--front" => front = text_arg(value)?,
                    "--back" => back = text_arg(value)?,
                    other => return Err(Failure::Usage(format!("Unknown option \"{other}\"."))),
                }
                changed += 1;
            }
            if changed == 0 {
                return Err(Failure::Usage(
                    "template set needs --front <text> or --back <text> (or @path).".to_owned(),
                ));
            }
            collection.set_template_text(found.id, chosen.id, &front, &back)?;
            format!("Changed {}", chosen.name)
        }
        ("move", [template, at]) => {
            let chosen = template_of(&found, template)?;
            collection.move_template(found.id, chosen.id, position(at)?)?;
            format!("Moved {} to position {at}", chosen.name)
        }
        ("remove", [template]) => {
            let chosen = template_of(&found, template)?;
            collection.remove_template(found.id, chosen.id)?;
            format!(
                "Removed {} and its cards. They come back with: template <file> {} restore {}",
                chosen.name, found.name, chosen.id
            )
        }
        ("restore", [template]) => {
            let removed = collection.removed_templates(found.id)?;
            let chosen = pick(&removed, template, "removed template", |t| (t.id, &t.name))?;
            collection.restore_template(found.id, chosen.id)?;
            format!("Restored {}", chosen.name)
        }
        _ => return Err(wrong("template")),
    };
    collection.close()?;
    Ok(text)
}
