//! Media: images and audio that notes name (ADR 0006, section 8). Step 1.10.
//!
//! **Identity.** A file is its SHA-256. The synced entity `media_file` has an ID made from the hash
//! (`UUIDv5`), so two devices that add the same bytes write the same entity, and a merge never has
//! two rows for one file. Its registers are the hash (as lowercase hex), the size and `deleted`.
//!
//! **Bytes.** `media_blob` holds each file's bytes once, by hash. It is a local table, never synced:
//! Phase 4 moves blobs by hash and asks the server which it lacks. Keeping the bytes in the
//! collection file means one code path on every platform (`fc-core` never touches the file system,
//! ADR 0003) and a backup that is one file. A `media_file` row with no blob is a file whose bytes
//! have not arrived on this device.
//!
//! **Names.** A name in a field is `<stem>-<first 16 hex of the hash>.<extension>`. The stem and the
//! extension come from the file the person added, cut down to letters, digits, `-` and `_`, so a
//! name is safe in `src="..."`, `[sound:...]` and CSS `url(...)`. The name is never stored: a name
//! belongs to the file whose hash starts with the 16 hex digits in it, and the stem is only a hint
//! for people. Two files called `image.jpg` get different names, and the same bytes added under two
//! stems give two names for one file. If two files ever shared those 16 digits, the name would read
//! as no file at all rather than pick one.
//!
//! **References.** A file is used when a note field, a template or a note type's CSS names it.
//! Notes in the trash and templates that are removed still count: restoring them brings the
//! reference back. Unused and missing files are found by comparing the two lists.
//!
//! **Deleting.** Deleting sets `deleted` and keeps the bytes (freeing them belongs to "Empty trash").
//! A deleted file that something still names reads as alive, decided when reading, so a merge that
//! deletes a file on one device while a note on another names it keeps the file (ADR 0006, rule in
//! section 5). Adding the same bytes again brings a deleted file back.

mod check;
mod error;
mod name;
mod ops;
mod read;
#[cfg(test)]
mod tests;

use crate::id::Id;
use crate::sync::SyncedTable;

pub use check::{MediaReport, Reference};
pub use error::MediaError;
pub use name::{HASH_DIGITS, file_name};
pub use ops::AddedMedia;

/// The sync entity type and register names are part of the sync format and never change (ADR 0006,
/// section 10). `hash` is lowercase hex of the SHA-256.
pub const MEDIA_FILE: SyncedTable = SyncedTable {
    entity: "media_file",
    table: "media_file",
    registers: &["hash", "size", "deleted"],
};

/// A media file's ID is `UUIDv5(MEDIA_NAMESPACE, hash as lowercase hex)`.
pub(crate) const MEDIA_NAMESPACE: Id = Id::from_bytes(*b"fc-media-ids-001");

/// A media file as it reads now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFile {
    pub id: Id,
    /// SHA-256 of the bytes, 64 lowercase hex digits.
    pub hash: String,
    pub size: u64,
    /// Deleted, and nothing names it any more. A deleted file that is still named reads as alive.
    pub deleted: bool,
    /// The bytes are on this device.
    pub has_bytes: bool,
}
