//! Everything that makes the collection syncable (ADR 0006, section 13).
//!
//! # The rules for synced tables
//!
//! A synced table is one whose rows (or fields of rows) merge between devices. Every one of them:
//!
//! - is declared in a [`SyncedTable`] and listed in [`SYNCED_TABLES`], with a 16-byte `id`
//!   primary key and one column per register (the register name is the column name);
//! - is created in a migration with [`install_guard`], which makes the database refuse any write
//!   that does not come through [`WriteTx`];
//! - has no `UNIQUE` constraint on user-visible values, no foreign key and no counter, so a merge
//!   can always apply (ADR 0006, section 6);
//! - is never hard-deleted. Deleting sets a `deleted` register (section 5).
//!
//! `WriteTx` records `(hlc, device, pushed)` for every register it writes, in the same
//! transaction. The tests in `registry` fail if a table is added without being declared.

mod hlc;
mod registry;
mod requires;
pub(crate) mod state;
mod unknown;
mod write;

pub use hlc::Hlc;
pub use registry::{LOCAL_TABLES, SYNCED_TABLES, SyncedTable, install_guard};
pub use requires::SUPPORTED_FEATURES;
pub use unknown::UnknownRegister;
pub use write::{RegisterClock, WriteTx};

#[cfg(test)]
pub(crate) use registry::check_schema;

#[cfg(test)]
mod tests;
