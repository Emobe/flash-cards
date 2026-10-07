//! `fc edit-note`, `fc delete-note`, `fc restore-note`, `fc find-duplicates`, `fc set-tags`,
//! `fc delete-tag`, `fc delete-media` and `fc restore-media`: changing and deleting what is already
//! in a collection, tried from a terminal.

use fc_core::id::Id;
use fc_core::note::NoteChange;
use fc_core::notetype::NoteType;

use super::notetype::find_note_type;
use super::{Failure, note_id, open, plural};

/// `Field=value` arguments as field IDs with their text. A field is given by its name.
pub fn field_values(note_type: &NoteType, values: &[String]) -> Result<Vec<(Id, String)>, Failure> {
    let mut given = Vec::new();
    for value in values {
        let (name, text) = value
            .split_once('=')
            .ok_or_else(|| Failure::Usage(format!("\"{value}\" is not Field=value.")))?;
        let field = note_type
            .fields
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| {
                let names: Vec<&str> = note_type.fields.iter().map(|f| f.name.as_str()).collect();
                Failure::Core(format!(
                    "{} has no field \"{name}\". Its fields are: {}.",
                    note_type.name,
                    names.join(", ")
                ))
            })?;
        given.push((field.id, text.to_owned()));
    }
    Ok(given)
}

fn ids(ids: &[Id]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a change did to the cards of a note, and any duplicate it made.
fn change_text(change: &NoteChange) -> String {
    let mut text = format!(
        "Cards added or brought back: {}. Cards removed: {}.",
        change.added_cards.len(),
        change.removed_cards.len()
    );
    if !change.duplicates.is_empty() {
        text.push_str(&format!(
            "\nWarning: {} duplicate of the first field already exists: {}",
            change.duplicates.len(),
            ids(&change.duplicates)
        ));
    }
    text
}

pub fn edit_note(file: &str, note: &str, values: &[String]) -> Result<String, Failure> {
    if values.is_empty() {
        return Err(Failure::Usage(
            "edit-note needs at least one Field=value.".to_owned(),
        ));
    }
    let note = note_id(note)?;
    let collection = open(file)?;
    let Some(found) = collection.note(note)?.filter(|n| !n.deleted) else {
        return Err(Failure::Core("No note with that ID.".to_owned()));
    };
    let note_type = collection
        .note_type(found.note_type)?
        .ok_or_else(|| Failure::Core("That note's note type is gone.".to_owned()))?;
    let given = field_values(&note_type, values)?;
    let given: Vec<(Id, &str)> = given
        .iter()
        .map(|(id, text)| (*id, text.as_str()))
        .collect();
    let change = collection.set_note_fields(note, &given)?;
    collection.close()?;
    Ok(format!("Changed note {note}. {}", change_text(&change)))
}

pub fn delete_notes(file: &str, notes: &[String]) -> Result<String, Failure> {
    if notes.is_empty() {
        return Err(Failure::Usage("delete-note needs note IDs.".to_owned()));
    }
    let wanted: Vec<Id> = notes.iter().map(|n| note_id(n)).collect::<Result<_, _>>()?;
    let collection = open(file)?;
    for note in &wanted {
        collection.delete_note(*note)?;
    }
    collection.close()?;
    Ok(format!(
        "Deleted {}. They are in the trash: restore one with restore-note",
        plural(wanted.len(), "note")
    ))
}

pub fn restore_note(file: &str, note: &str) -> Result<String, Failure> {
    let note = note_id(note)?;
    let collection = open(file)?;
    let change = collection.restore_note(note)?;
    collection.close()?;
    Ok(format!("Restored note {note}. {}", change_text(&change)))
}

pub fn find_duplicates(file: &str, note_type: &str, text: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    let found = find_note_type(&collection, note_type)?;
    let duplicates = collection.find_duplicates(found.id, text)?;
    collection.close()?;
    Ok(if duplicates.is_empty() {
        format!("No {} note has \"{text}\" as its first field.", found.name)
    } else {
        format!(
            "{} with that first field: {}",
            plural(duplicates.len(), "note"),
            ids(&duplicates)
        )
    })
}

pub fn set_tags(file: &str, note: &str, tags: &[String]) -> Result<String, Failure> {
    let note = note_id(note)?;
    let collection = open(file)?;
    let names: Vec<&str> = tags.iter().map(String::as_str).collect();
    collection.set_note_tags(note, &names)?;
    let now = collection.note_tags(note)?;
    collection.close()?;
    Ok(format!(
        "Tags now: {}",
        if now.is_empty() {
            "none".to_owned()
        } else {
            now.join(" ")
        }
    ))
}

pub fn delete_tag(file: &str, tag: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    let changed = collection.delete_tag(tag)?;
    collection.close()?;
    Ok(format!(
        "Deleted {tag} (and the tags inside it) from {}",
        plural(changed, "note")
    ))
}

pub fn delete_media(file: &str, name: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    collection.delete_media(name)?;
    collection.close()?;
    Ok(format!(
        "Deleted {name}. The bytes are kept: bring it back with restore-media"
    ))
}

pub fn restore_media(file: &str, name: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    collection.restore_media(name)?;
    collection.close()?;
    Ok(format!("Restored {name}"))
}
