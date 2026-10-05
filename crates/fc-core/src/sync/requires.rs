//! Collection-level `requires` (ADR 0006, section 10): features a collection uses that an older app
//! would misinterpret. An app that does not know one of them keeps studying locally but pauses
//! sync, with a message to update. It does not refuse to open the collection.
//!
//! Each feature is one synced `requirement` row, with an ID derived from the feature name, so two
//! devices that add the same feature merge into one row and different features never conflict.

use rusqlite::types::Value;

use super::SyncedTable;
use crate::collection::{Collection, CollectionError};
use crate::id::Id;

pub(super) const TABLE: SyncedTable = SyncedTable {
    entity: "requirement",
    table: "requirement",
    registers: &["feature", "active"],
};

/// Features this build understands. A feature is added here, and to the code that needs it, in the
/// same change that first writes it.
pub const SUPPORTED_FEATURES: &[&str] = &[];

const NAMESPACE: Id = Id::from_bytes(*b"fc-requirement-1");

fn requirement_id(feature: &str) -> Id {
    Id::new_v5(NAMESPACE, feature.as_bytes())
}

impl Collection {
    /// Records that this collection now needs `feature`.
    pub fn require_feature(&self, feature: &str) -> Result<(), CollectionError> {
        self.write(|w| {
            let id = requirement_id(feature);
            match w.get("requirement", id, "active")? {
                None => w.insert(
                    "requirement",
                    id,
                    vec![
                        ("feature", Value::Text(feature.to_owned())),
                        ("active", Value::Integer(1)),
                    ],
                ),
                Some(Value::Integer(1)) => Ok(()),
                Some(_) => w.set("requirement", id, "active", Value::Integer(1)),
            }
        })
    }

    /// The features the collection needs that this build does not know. While this is not empty,
    /// sync must stay paused.
    pub fn unsupported_features(&self) -> Result<Vec<String>, CollectionError> {
        // A row that arrived without its `feature` register yet has an empty name: skip it.
        let mut statement = self.conn.prepare(
            "SELECT feature FROM requirement WHERE active = 1 AND feature <> '' ORDER BY feature",
        )?;
        let features = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(features
            .into_iter()
            .filter(|feature| !SUPPORTED_FEATURES.contains(&feature.as_str()))
            .collect())
    }
}
