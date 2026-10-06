//! The text of a backup: `changes.jsonl` and `manifest.json` (documented in
//! `docs/backup-format.md`). Plain JSON, so any tool can read it.

use std::io::{BufRead, Write};

use rusqlite::types::Value;
use serde_json::{Map, Number, Value as Json, json};

use super::{BackupError, FORMAT_VERSION, Manifest, ManifestScope};
use crate::id::Id;
use crate::sync::{Changes, Clock, Hlc, RegisterChange, RowChange};

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// A SQLite value as JSON. NULL, integers, reals and text are JSON as they are. A blob is
/// `{"blob": "<hex>"}`, and a real that JSON cannot hold is `{"real": "inf"}` or `"-inf"`.
fn encode(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Integer(n) => Json::from(*n),
        Value::Real(x) => match Number::from_f64(*x) {
            Some(number) => Json::Number(number),
            None => json!({ "real": if *x > 0.0 { "inf" } else { "-inf" } }),
        },
        Value::Text(text) => Json::String(text.clone()),
        Value::Blob(bytes) => json!({ "blob": hex(bytes) }),
    }
}

fn decode(json: &Json) -> Result<Value, String> {
    match json {
        Json::Null => Ok(Value::Null),
        Json::Number(n) => n
            .as_i64()
            .map(Value::Integer)
            .or_else(|| {
                if n.is_u64() {
                    None
                } else {
                    n.as_f64().map(Value::Real)
                }
            })
            .ok_or_else(|| format!("the number {n} does not fit")),
        Json::String(text) => Ok(Value::Text(text.clone())),
        Json::Object(object) if object.len() == 1 => match object.iter().next() {
            Some((key, Json::String(text))) if key == "blob" => unhex(text)
                .map(Value::Blob)
                .ok_or_else(|| "a blob that is not hexadecimal".to_owned()),
            Some((key, Json::String(text))) if key == "real" => match text.as_str() {
                "inf" => Ok(Value::Real(f64::INFINITY)),
                "-inf" => Ok(Value::Real(f64::NEG_INFINITY)),
                _ => Err(format!("the real number \"{text}\"")),
            },
            _ => Err("a value that is not a blob or a real".to_owned()),
        },
        _ => Err("a value that is not a SQLite value".to_owned()),
    }
}

pub(super) fn write_changes(out: &mut impl Write, changes: &Changes) -> Result<(), BackupError> {
    for change in &changes.registers {
        let line = json!({
            "kind": "register",
            "entity": change.entity,
            "id": change.entity_id.to_string(),
            "field": change.field,
            "value": encode(&change.value),
            "hlc": change.clock.hlc.to_stored(),
            "device": change.clock.device.to_string(),
        });
        serde_json::to_writer(&mut *out, &line).map_err(|e| BackupError::Io(e.to_string()))?;
        out.write_all(b"\n")?;
    }
    for change in &changes.rows {
        let columns: Vec<Json> = change
            .columns
            .iter()
            .map(|(name, value)| json!([name, encode(value)]))
            .collect();
        let line = json!({
            "kind": "row",
            "entity": change.entity,
            "id": change.id.to_string(),
            "columns": columns,
        });
        serde_json::to_writer(&mut *out, &line).map_err(|e| BackupError::Io(e.to_string()))?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn damaged(line: usize, what: impl std::fmt::Display) -> BackupError {
    BackupError::Damaged(format!("changes.jsonl, line {line}: {what}"))
}

fn text<'a>(object: &'a Map<String, Json>, key: &str, line: usize) -> Result<&'a str, BackupError> {
    object
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| damaged(line, format!("`{key}` is missing or is not text")))
}

fn id(object: &Map<String, Json>, key: &str, line: usize) -> Result<Id, BackupError> {
    text(object, key, line)?
        .parse()
        .map_err(|_| damaged(line, format!("`{key}` is not an ID")))
}

pub(super) fn read_changes(input: impl BufRead) -> Result<Changes, BackupError> {
    let mut changes = Changes::default();
    for (index, line) in input.lines().enumerate() {
        let number = index + 1;
        let line = line.map_err(|e| BackupError::Damaged(format!("changes.jsonl: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let json: Json = serde_json::from_str(&line).map_err(|e| damaged(number, e))?;
        let Json::Object(object) = json else {
            return Err(damaged(number, "not an object"));
        };
        match text(&object, "kind", number)? {
            "register" => {
                let value = object
                    .get("value")
                    .ok_or_else(|| damaged(number, "`value` is missing"))?;
                let hlc = object
                    .get("hlc")
                    .and_then(Json::as_i64)
                    .ok_or_else(|| damaged(number, "`hlc` is missing or is not an integer"))?;
                changes.registers.push(RegisterChange {
                    entity: text(&object, "entity", number)?.to_owned(),
                    entity_id: id(&object, "id", number)?,
                    field: text(&object, "field", number)?.to_owned(),
                    value: decode(value).map_err(|e| damaged(number, e))?,
                    clock: Clock {
                        hlc: Hlc::from_stored(hlc),
                        device: id(&object, "device", number)?,
                    },
                });
            }
            "row" => {
                let pairs = object
                    .get("columns")
                    .and_then(Json::as_array)
                    .ok_or_else(|| damaged(number, "`columns` is missing or is not a list"))?;
                let mut columns = Vec::with_capacity(pairs.len());
                for pair in pairs {
                    let [name, value] = pair.as_array().map(Vec::as_slice).unwrap_or(&[]) else {
                        return Err(damaged(number, "a column is not a [name, value] pair"));
                    };
                    let name = name
                        .as_str()
                        .ok_or_else(|| damaged(number, "a column name is not text"))?;
                    columns.push((
                        name.to_owned(),
                        decode(value).map_err(|e| damaged(number, e))?,
                    ));
                }
                changes.rows.push(RowChange {
                    entity: text(&object, "entity", number)?.to_owned(),
                    id: id(&object, "id", number)?,
                    columns,
                });
            }
            other => return Err(damaged(number, format!("unknown kind \"{other}\""))),
        }
    }
    Ok(changes)
}

pub(super) fn manifest_to_json(manifest: &Manifest) -> Json {
    let scope = match &manifest.scope {
        ManifestScope::Collection => json!({ "kind": "collection" }),
        ManifestScope::Deck { id, name } => {
            json!({ "kind": "deck", "id": id.to_string(), "name": name })
        }
    };
    json!({
        "format": "fc-backup",
        "format_version": manifest.format_version,
        "app_version": manifest.app_version,
        "storage_version": manifest.storage_version,
        "created_ms": manifest.created_ms,
        "device": manifest.device.to_string(),
        "scope": scope,
        "history": manifest.history,
        "registers": manifest.registers,
        "rows": manifest.rows,
        "media_files": manifest.media_files,
    })
}

/// Reads `manifest.json`. A file that says it is not ours, or has no readable manifest, is not a
/// backup. A newer format number is refused. Keys this build does not know are ignored.
pub(super) fn manifest_from_json(json: &Json) -> Result<Manifest, BackupError> {
    let bad = |what: &str| BackupError::Damaged(format!("manifest.json: {what}"));
    let object = json.as_object().ok_or_else(|| bad("not an object"))?;
    if object.get("format").and_then(Json::as_str) != Some("fc-backup") {
        return Err(BackupError::NotABackup);
    }
    let number = |key: &str| {
        object
            .get(key)
            .and_then(Json::as_u64)
            .ok_or_else(|| bad(&format!("`{key}` is missing or is not a number")))
    };
    let found = u32::try_from(number("format_version")?).unwrap_or(u32::MAX);
    if found > FORMAT_VERSION {
        return Err(BackupError::TooNew {
            found,
            supported: FORMAT_VERSION,
        });
    }
    let scope = object
        .get("scope")
        .and_then(Json::as_object)
        .ok_or_else(|| bad("`scope` is missing"))?;
    let scope = match scope.get("kind").and_then(Json::as_str) {
        Some("collection") => ManifestScope::Collection,
        Some("deck") => ManifestScope::Deck {
            id: scope
                .get("id")
                .and_then(Json::as_str)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| bad("the deck `id` is not an ID"))?,
            name: scope
                .get("name")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        _ => return Err(bad("`scope` is not a collection or a deck")),
    };
    Ok(Manifest {
        format_version: found,
        app_version: object
            .get("app_version")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned(),
        storage_version: u32::try_from(number("storage_version")?).unwrap_or(u32::MAX),
        created_ms: object
            .get("created_ms")
            .and_then(Json::as_i64)
            .ok_or_else(|| bad("`created_ms` is missing or is not a number"))?,
        device: object
            .get("device")
            .and_then(Json::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("`device` is not an ID"))?,
        scope,
        history: object
            .get("history")
            .and_then(Json::as_bool)
            .ok_or_else(|| bad("`history` is missing"))?,
        registers: number("registers")?,
        rows: number("rows")?,
        media_files: number("media_files")?,
    })
}
