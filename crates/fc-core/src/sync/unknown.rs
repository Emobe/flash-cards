//! The generic store for registers this build does not know (ADR 0006, section 10): a new entity
//! type, or a new field of a known one. They are kept unchanged and never written locally, so an
//! older app cannot overwrite them. After an upgrade, a migration applies what was stored.

use rusqlite::types::Value;
use rusqlite::{Transaction, TransactionBehavior, params};

use super::{Hlc, state};
use crate::collection::{Collection, CollectionError};
use crate::id::Id;

/// One register as it arrived from another device.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownRegister {
    pub entity_type: String,
    pub entity_id: Id,
    pub field: String,
    /// Kept exactly as received: any SQLite value.
    pub value: Value,
    pub hlc: Hlc,
    pub device: Id,
}

impl Collection {
    /// Whether this build has a table with this register. Anything else goes to the unknown store.
    pub fn knows_register(&self, entity_type: &str, field: &str) -> bool {
        self.schema
            .tables
            .iter()
            .any(|t| t.entity == entity_type && t.registers.contains(&field))
    }

    /// Keeps a register this build does not know. The higher `(hlc, device)` wins, so applying
    /// the same changes in any order, or twice, gives the same result. Returns whether the stored
    /// value changed. It also moves the saved HLC up to what was received.
    pub fn store_unknown_register(
        &self,
        register: &UnknownRegister,
    ) -> Result<bool, CollectionError> {
        if self.knows_register(&register.entity_type, &register.field) {
            return Err(CollectionError::Storage(format!(
                "`{}.{}` is a known register, not unknown data",
                register.entity_type, register.field
            )));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "INSERT INTO unknown_register (entity_type, entity_id, field, value, hlc, device)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (entity_type, entity_id, field) DO UPDATE
             SET value = excluded.value, hlc = excluded.hlc, device = excluded.device
             WHERE (excluded.hlc, excluded.device) > (hlc, device)",
            params![
                register.entity_type,
                register.entity_id,
                register.field,
                register.value,
                register.hlc.to_stored(),
                register.device
            ],
        )?;
        let last = state::hlc_last(&tx)?.observe(register.hlc);
        state::set_hlc_last(&tx, last)?;
        tx.commit()?;
        Ok(changed == 1)
    }

    /// The unknown registers stored for one entity.
    pub fn unknown_registers(
        &self,
        entity_type: &str,
        entity_id: Id,
    ) -> Result<Vec<UnknownRegister>, CollectionError> {
        let mut statement = self.conn.prepare(
            "SELECT field, value, hlc, device FROM unknown_register
             WHERE entity_type = ?1 AND entity_id = ?2 ORDER BY field",
        )?;
        let rows = statement.query_map(params![entity_type, entity_id], |row| {
            Ok(UnknownRegister {
                entity_type: entity_type.to_owned(),
                entity_id,
                field: row.get(0)?,
                value: row.get(1)?,
                hlc: Hlc::from_stored(row.get(2)?),
                device: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The saved HLC: the newest stamp this collection has written or seen.
    pub fn hlc(&self) -> Result<Hlc, CollectionError> {
        state::hlc_last(&self.conn)
    }

    /// Moves the saved HLC up to a stamp received from another device.
    pub fn observe_hlc(&self, received: Hlc) -> Result<(), CollectionError> {
        let last = state::hlc_last(&self.conn)?.observe(received);
        state::set_hlc_last(&self.conn, last)
    }

    /// This copy's device ID. Never shared by two copies of a collection.
    pub fn device_id(&self) -> Result<Id, CollectionError> {
        state::device_id(&self.conn)
    }

    /// Gives this copy a new device ID. A restore or an import calls this (ADR 0006, section 1).
    pub fn regenerate_device_id(&self) -> Result<Id, CollectionError> {
        state::regenerate_device_id(&self.conn, &self.host)
    }
}
