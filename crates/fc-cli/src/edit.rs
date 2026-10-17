//! `fc edit-note`, `fc change-notetype`, `fc delete-note`, `fc restore-note`, `fc find-duplicates`, `fc set-tags`,
//! `fc delete-tag`, `fc delete-media` and `fc restore-media`: changing and deleting what is already
//! in a collection, tried from a terminal.

use fc_core::id::Id;
use fc_core::note::NoteChange;
use fc_core::notetype::{Field, NoteType, Template};

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

pub fn edit_note(file: &str, note: &str, args: &[String]) -> Result<String, Failure> {
    let mut values = Vec::new();
    let mut tags: Option<Vec<&str>> = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--tag" {
            let tag = args
                .next()
                .ok_or_else(|| Failure::Usage("--tag needs a tag.".to_owned()))?;
            tags.get_or_insert_with(Vec::new).push(tag);
        } else {
            values.push(arg.clone());
        }
    }
    if values.is_empty() && tags.is_none() {
        return Err(Failure::Usage(
            "edit-note needs at least one Field=value or --tag.".to_owned(),
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
    let given = field_values(&note_type, &values)?;
    let given: Vec<(Id, &str)> = given
        .iter()
        .map(|(id, text)| (*id, text.as_str()))
        .collect();
    let edit = collection.edit_note_with(note, &given, tags.as_deref())?;
    collection.close()?;
    let tags = if edit.old_tags.is_some() {
        " Its tags changed."
    } else {
        ""
    };
    Ok(format!(
        "Changed note {note}. {}{tags}",
        change_text(&edit.change)
    ))
}

pub fn delete_notes(file: &str, notes: &[String]) -> Result<String, Failure> {
    if notes.is_empty() {
        return Err(Failure::Usage("delete-note needs note IDs.".to_owned()));
    }
    let wanted: Vec<Id> = notes.iter().map(|n| note_id(n)).collect::<Result<_, _>>()?;
    let collection = open(file)?;
    let done = collection.delete_notes(&wanted)?;
    collection.close()?;
    Ok(format!(
        "Deleted {} and {}. They are in the trash: restore them with restore-note",
        plural(done.notes.len(), "note"),
        plural(done.cards.len(), "card")
    ))
}

pub fn restore_notes(file: &str, notes: &[String]) -> Result<String, Failure> {
    if notes.is_empty() {
        return Err(Failure::Usage("restore-note needs note IDs.".to_owned()));
    }
    let wanted: Vec<Id> = notes.iter().map(|n| note_id(n)).collect::<Result<_, _>>()?;
    let collection = open(file)?;
    let done = collection.restore_notes(&wanted)?;
    collection.close()?;
    Ok(if let [one] = wanted.as_slice() {
        format!(
            "Restored note {one}. {}",
            change_text(&NoteChange {
                added_cards: done.cards,
                ..Default::default()
            })
        )
    } else {
        format!(
            "Restored {} and {}.",
            plural(done.notes.len(), "note"),
            plural(done.cards.len(), "card")
        )
    })
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

/// `NEW=OLD` arguments for the new and the old part list, by name or ID. `NEW=` means nothing fills
/// it. A part not given is matched by name ignoring case, then by position.
fn part_map<T>(
    kind: &str,
    given: &[String],
    new: &[T],
    old: &[T],
    parts: impl Fn(&T) -> (Id, &str) + Copy,
) -> Result<Vec<(Id, Option<Id>)>, Failure> {
    let find = |list: &[T], wanted: &str, what: &str| -> Result<Id, Failure> {
        list.iter()
            .map(parts)
            .find(|(id, name)| id.to_string() == wanted || name.eq_ignore_ascii_case(wanted))
            .map(|(id, _)| id)
            .ok_or_else(|| Failure::Core(format!("No {what} {kind} called \"{wanted}\".")))
    };
    let mut chosen: Vec<(Id, Option<Id>)> = Vec::new();
    for arg in given {
        let (to, from) = arg
            .split_once('=')
            .ok_or_else(|| Failure::Usage(format!("\"{arg}\" is not New={kind}.")))?;
        let to = find(new, to, "new")?;
        let from = if from.is_empty() {
            None
        } else {
            Some(find(old, from, "old")?)
        };
        chosen.retain(|(t, _)| *t != to);
        chosen.push((to, from));
    }
    // The rest: the old part with the same name, then the one in the same position. An old part
    // is used once.
    let mut used: Vec<Id> = chosen.iter().filter_map(|(_, from)| *from).collect();
    let mut matched: Vec<(Id, Option<Id>)> = Vec::new();
    for item in new {
        let (id, name) = parts(item);
        if chosen.iter().any(|(t, _)| *t == id) {
            continue;
        }
        let pick = old
            .iter()
            .map(parts)
            .find(|(old_id, other)| !used.contains(old_id) && other.eq_ignore_ascii_case(name))
            .map(|(old_id, _)| old_id);
        used.extend(pick);
        matched.push((id, pick));
    }
    for (position, item) in new.iter().enumerate() {
        let (id, _) = parts(item);
        if let Some(entry) = matched
            .iter_mut()
            .find(|(t, from)| *t == id && from.is_none())
        {
            let pick = old
                .get(position)
                .map(|old| parts(old).0)
                .filter(|p| !used.contains(p));
            used.extend(pick);
            entry.1 = pick;
        }
    }
    chosen.extend(matched);
    Ok(chosen)
}

pub fn change_note_type(file: &str, args: &[String]) -> Result<String, Failure> {
    let mut fields = Vec::new();
    let mut cards = Vec::new();
    let mut words = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let list = match arg.as_str() {
            "--field" => Some(&mut fields),
            "--card" => Some(&mut cards),
            _ => None,
        };
        match list {
            Some(list) => list.push(
                args.next()
                    .ok_or_else(|| Failure::Usage(format!("{arg} needs New=Old.")))?
                    .clone(),
            ),
            None => words.push(arg.clone()),
        }
    }
    let [wanted, notes @ ..] = words.as_slice() else {
        return Err(Failure::Usage(
            "change-notetype needs a note type and note IDs.".to_owned(),
        ));
    };
    if notes.is_empty() {
        return Err(Failure::Usage(
            "change-notetype needs a note type and note IDs.".to_owned(),
        ));
    }
    let notes: Vec<Id> = notes.iter().map(|n| note_id(n)).collect::<Result<_, _>>()?;
    let collection = open(file)?;
    let to = find_note_type(&collection, wanted)?;
    let first = collection
        .note(notes[0])?
        .filter(|n| !n.deleted)
        .ok_or_else(|| Failure::Core("No note with that ID.".to_owned()))?;
    let from = collection
        .note_type(first.note_type)?
        .ok_or_else(|| Failure::Core("That note's note type is gone.".to_owned()))?;
    let field_map = part_map("field", &fields, &to.fields, &from.fields, |f: &Field| {
        (f.id, f.name.as_str())
    })?;
    let card_map = part_map(
        "card",
        &cards,
        &to.templates,
        &from.templates,
        |t: &Template| (t.id, t.name.as_str()),
    )?;
    let done = collection.change_note_type(&notes, to.id, &field_map, &card_map)?;
    collection.close()?;
    Ok(format!(
        "Changed {} from {} to {}. Cards made or brought back: {}. Cards removed: {}. Schedules \
         carried: {}.{}",
        plural(done.notes.len(), "note"),
        from.name,
        to.name,
        done.cards_added.len(),
        done.cards_removed.len(),
        done.carried.len(),
        if done.reset.is_empty() {
            String::new()
        } else {
            format!(" Cards reset to new: {}.", done.reset.len())
        }
    ))
}
