//! Sync facts kept in `meta`: the saved HLC, the device ID and the installation ID.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use super::Hlc;
use crate::clock::Host;
use crate::collection::CollectionError;
use crate::id::Id;

const HLC_LAST: &str = "hlc_last";
const DEVICE_ID: &str = "device_id";
const INSTALLATION_ID: &str = "installation_id";

fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()
}

fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

pub(crate) fn hlc_last(conn: &Connection) -> Result<Hlc, CollectionError> {
    let saved = get(conn, HLC_LAST)?;
    Ok(saved
        .and_then(|text| text.parse().ok())
        .map_or(Hlc::ZERO, Hlc::from_stored))
}

pub(crate) fn set_hlc_last(conn: &Connection, hlc: Hlc) -> Result<(), CollectionError> {
    Ok(set(conn, HLC_LAST, &hlc.to_stored().to_string())?)
}

pub(crate) fn device_id(conn: &Connection) -> Result<Id, CollectionError> {
    get(conn, DEVICE_ID)?
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| CollectionError::Storage("the collection has no device ID".to_owned()))
}

/// A new UUIDv7 from the host's time and the system's random numbers.
pub(crate) fn new_id(host: &Host) -> Result<Id, CollectionError> {
    let mut random = [0u8; 10];
    getrandom::fill(&mut random)
        .map_err(|e| CollectionError::Storage(format!("no random numbers: {e}")))?;
    Ok(Id::new_v7(host.clock.now().unix_ms, &random))
}

/// Gives the collection a new device ID and records which installation holds it.
pub(crate) fn regenerate_device_id(conn: &Connection, host: &Host) -> Result<Id, CollectionError> {
    let device = new_id(host)?;
    set(conn, DEVICE_ID, &device.to_string())?;
    set(conn, INSTALLATION_ID, &host.installation_id.to_string())?;
    Ok(device)
}

/// Run on every open, after migrations: a collection that has no device ID yet (new, or from
/// before 1.1b), or that was last opened by another installation (a copied file), gets a new one.
/// Two copies must never write with the same device ID (ADR 0006, section 1).
pub(crate) fn identify(conn: &mut Connection, host: &Host) -> Result<(), CollectionError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let installation = get(&tx, INSTALLATION_ID)?;
    let has_device = get(&tx, DEVICE_ID)?.is_some();
    if !has_device || installation.as_deref() != Some(&host.installation_id.to_string()) {
        regenerate_device_id(&tx, host)?;
    }
    tx.commit()?;
    Ok(())
}
