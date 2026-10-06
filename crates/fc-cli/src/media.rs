//! `fc add-media`, `fc media` and the other media commands: attaching files and finding the ones
//! nothing uses or that are missing, tried from a terminal.

use std::collections::BTreeMap;

use fc_core::id::Id;
use fc_core::media::{HASH_DIGITS, MediaFile};

use super::{Failure, open, plural};

impl From<fc_core::media::MediaError> for Failure {
    fn from(error: fc_core::media::MediaError) -> Self {
        Self::Core(error.to_string())
    }
}

fn describe(file: &MediaFile) -> String {
    format!(
        "{}  {}",
        &file.hash[..HASH_DIGITS],
        plural(file.size as usize, "byte")
    )
}

fn ids(ids: &[Id]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn add(file: &str, path: &str) -> Result<String, Failure> {
    let bytes =
        std::fs::read(path).map_err(|e| Failure::Core(format!("Cannot read \"{path}\": {e}.")))?;
    let collection = open(file)?;
    let added = collection.add_media(path, &bytes)?;
    collection.close()?;
    Ok(if added.new {
        format!(
            "Added {} ({}). Use it in a field as <img src=\"{0}\"> or [sound:{0}].",
            added.name,
            plural(bytes.len(), "byte")
        )
    } else {
        format!(
            "The collection already has these bytes. Use {} (the same file under any of its names).",
            added.name
        )
    })
}

pub fn list(file: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    let files = collection.media_files()?;
    // The names that notes use for each file.
    let mut names: BTreeMap<Id, Vec<String>> = BTreeMap::new();
    for reference in collection.media_references()? {
        if let Some(found) = collection.media_file(&reference.name)? {
            names.entry(found.id).or_default().push(reference.name);
        }
    }
    let mut text = format!("{} in the collection", plural(files.len(), "media file"));
    for found in &files {
        let used = names.get(&found.id);
        let state = match (found.deleted, used) {
            (true, _) => "deleted",
            (false, Some(_)) => "used",
            (false, None) => "unused",
        };
        text.push_str(&format!(
            "\n{}  {state}{}",
            describe(found),
            if found.has_bytes {
                ""
            } else {
                ", bytes not here"
            }
        ));
        if let Some(used) = used {
            text.push_str(&format!("  {}", used.join(", ")));
        }
    }
    collection.close()?;
    Ok(text)
}

pub fn get(file: &str, name: &str, out: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    let bytes = collection.media_bytes(name)?;
    collection.close()?;
    std::fs::write(out, &bytes)
        .map_err(|e| Failure::Core(format!("Cannot write \"{out}\": {e}.")))?;
    Ok(format!("Wrote {} to {out}", plural(bytes.len(), "byte")))
}

pub fn check(file: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    let report = collection.check_media()?;
    collection.close()?;
    let mut text = format!(
        "{} unused, {} missing, {} whose bytes are not here",
        report.unused.len(),
        report.missing.len(),
        report.without_bytes.len()
    );
    for found in &report.unused {
        text.push_str(&format!("\nunused: {}", describe(found)));
    }
    for reference in &report.missing {
        text.push_str(&format!("\nmissing: {}", reference.name));
        if !reference.notes.is_empty() {
            text.push_str(&format!("  notes: {}", ids(&reference.notes)));
        }
        if !reference.note_types.is_empty() {
            text.push_str(&format!("  note types: {}", ids(&reference.note_types)));
        }
    }
    for found in &report.without_bytes {
        text.push_str(&format!("\nno bytes: {}", describe(found)));
    }
    Ok(text)
}

pub fn delete_unused(file: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    let deleted = collection.delete_unused_media()?;
    collection.close()?;
    Ok(format!(
        "Deleted {}",
        plural(deleted.len(), "unused media file")
    ))
}
