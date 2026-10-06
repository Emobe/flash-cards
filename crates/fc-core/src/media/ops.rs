//! Adding, deleting and restoring media files.

use rusqlite::params;
use rusqlite::types::Value;

use super::name::{file_name, hash_of, id_of};
use super::{MEDIA_FILE, MediaError, MediaFile};
use crate::collection::Collection;

/// What [`Collection::add_media`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedMedia {
    /// The name to use in a field (`<img src="...">` or `[sound:...]`).
    pub name: String,
    pub file: MediaFile,
    /// False when the collection already had these exact bytes, so nothing new was stored.
    pub new: bool,
}

impl Collection {
    /// Adds a file, given the name or path it came with and its bytes. The same bytes are stored
    /// once, whatever they were called: adding them again stores nothing, brings a deleted file back
    /// and fills in the bytes of a file whose bytes were missing here. Returns the name to use.
    pub fn add_media(&self, original_name: &str, bytes: &[u8]) -> Result<AddedMedia, MediaError> {
        if bytes.is_empty() {
            return Err(MediaError::Empty);
        }
        let hash = hash_of(bytes);
        let id = id_of(&hash);
        let size = bytes.len() as i64;
        let new = self.write(|w| {
            let stored = w.local().execute(
                "INSERT OR IGNORE INTO media_blob (hash, data) VALUES (?1, ?2)",
                params![hash, bytes],
            )? > 0;
            let new = match w.get(MEDIA_FILE.entity, id, "deleted")? {
                None => {
                    w.insert(
                        MEDIA_FILE.entity,
                        id,
                        vec![
                            ("hash", Value::Text(hash.clone())),
                            ("size", Value::Integer(size)),
                            ("deleted", Value::Integer(0)),
                        ],
                    )?;
                    true
                }
                Some(Value::Integer(0)) => stored,
                Some(_) => {
                    w.set(MEDIA_FILE.entity, id, "deleted", Value::Integer(0))?;
                    true
                }
            };
            Ok(new)
        })?;
        let name = file_name(original_name, &hash);
        let file = self
            .media_file(&name)?
            .expect("a file that was just added has a name");
        Ok(AddedMedia { name, file, new })
    }

    /// Deletes the file a name belongs to, unless something still names it. The bytes stay (freeing
    /// them is for "Empty trash"), so the file can be brought back.
    pub fn delete_media(&self, name: &str) -> Result<(), MediaError> {
        let row = self
            .media_row(name)?
            .ok_or_else(|| MediaError::NotFound(name.to_owned()))?;
        if row.deleted {
            return Ok(());
        }
        if self.media_is_named(&row.hash)? {
            return Err(MediaError::StillUsed(name.to_owned()));
        }
        Ok(self.write(|w| w.set(MEDIA_FILE.entity, row.id, "deleted", Value::Integer(1)))?)
    }

    /// Brings back a deleted file by its name.
    pub fn restore_media(&self, name: &str) -> Result<(), MediaError> {
        let row = self
            .media_row(name)?
            .ok_or_else(|| MediaError::NotFound(name.to_owned()))?;
        if !row.deleted {
            return Ok(());
        }
        Ok(self.write(|w| w.set(MEDIA_FILE.entity, row.id, "deleted", Value::Integer(0)))?)
    }

    /// Deletes every file that nothing names, in one transaction, and returns them.
    pub fn delete_unused_media(&self) -> Result<Vec<MediaFile>, MediaError> {
        let unused = self.check_media()?.unused;
        self.write(|w| {
            for file in &unused {
                w.set(MEDIA_FILE.entity, file.id, "deleted", Value::Integer(1))?;
            }
            Ok(())
        })?;
        Ok(unused
            .into_iter()
            .map(|f| MediaFile { deleted: true, ..f })
            .collect())
    }
}
