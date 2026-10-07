//! `fc notetype`, `fc field` and `fc template`: changing note types, their fields and their card
//! templates, tried from a terminal.

use fc_core::collection::Collection;
use fc_core::id::Id;
use fc_core::notetype::{Field, Kind, NoteType, Template};

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
