//! Reading media files and their bytes.

use super::check::named_digits;
use super::name::{digits_in, hash_of};
use super::{HASH_DIGITS, MediaError, MediaFile};
use crate::collection::Collection;
use crate::id::Id;

/// A `media_file` row as stored.
#[derive(Debug, Clone)]
pub(super) struct Row {
    pub id: Id,
    pub hash: String,
    pub size: u64,
    /// The `deleted` register, before the rule that a named file stays alive.
    pub deleted: bool,
    pub has_bytes: bool,
}

impl Row {
    pub(super) fn file(&self, still_named: bool) -> MediaFile {
        MediaFile {
            id: self.id,
            hash: self.hash.clone(),
            size: self.size,
            deleted: self.deleted && !still_named,
            has_bytes: self.has_bytes,
        }
    }
}

const SELECT: &str = "SELECT id, hash, size, deleted,
        EXISTS (SELECT 1 FROM media_blob WHERE media_blob.hash = media_file.hash)
    FROM media_file";

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        id: row.get(0)?,
        hash: row.get(1)?,
        size: row.get::<_, i64>(2)?.max(0) as u64,
        deleted: row.get::<_, i64>(3)? != 0,
        has_bytes: row.get(4)?,
    })
}

impl Collection {
    /// Every `media_file` row, ordered by hash.
    pub(super) fn media_rows(&self) -> Result<Vec<Row>, MediaError> {
        let mut statement = self.conn.prepare(&format!("{SELECT} ORDER BY hash, id"))?;
        let rows = statement.query_map([], row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// The rows whose hash starts with `digits` (lowercase hex).
    pub(super) fn media_rows_with(&self, digits: &str) -> Result<Vec<Row>, MediaError> {
        // `g` sorts after every hex digit, so this is the range of hashes that start with `digits`.
        let mut statement = self.conn.prepare(&format!(
            "{SELECT} WHERE hash >= ?1 AND hash < ?2 ORDER BY hash, id"
        ))?;
        let rows = statement
            .query_map([digits, &format!("{digits}g")], row)?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// The one row a name belongs to, or `None` for a name that is not in the format, belongs to no
    /// file or (never in practice) to two.
    pub(super) fn media_row(&self, name: &str) -> Result<Option<Row>, MediaError> {
        let Some(digits) = digits_in(name) else {
            return Ok(None);
        };
        let mut rows = self.media_rows_with(&digits)?;
        Ok(if rows.len() == 1 { rows.pop() } else { None })
    }

    /// The file a name belongs to. A deleted file that a note, template or style still names is
    /// alive. A deleted file nothing names is returned with `deleted` set, so it can be restored.
    pub fn media_file(&self, name: &str) -> Result<Option<MediaFile>, MediaError> {
        let Some(found) = self.media_row(name)? else {
            return Ok(None);
        };
        let named = found.deleted && self.media_is_named(&found.hash)?;
        Ok(Some(found.file(named)))
    }

    /// Whether anything names the file with this hash.
    pub(super) fn media_is_named(&self, hash: &str) -> Result<bool, MediaError> {
        let digits = &hash[..HASH_DIGITS.min(hash.len())];
        Ok(named_digits(self)?.contains(digits))
    }

    /// Every media file, ordered by hash. A deleted file that is still named reads as alive.
    pub fn media_files(&self) -> Result<Vec<MediaFile>, MediaError> {
        let rows = self.media_rows()?;
        let named = if rows.iter().any(|r| r.deleted) {
            named_digits(self)?
        } else {
            Default::default()
        };
        Ok(rows
            .iter()
            .map(|r| r.file(named.contains(&r.hash[..HASH_DIGITS.min(r.hash.len())])))
            .collect())
    }

    /// The bytes of the file a name belongs to. A file that is deleted and unnamed is not served.
    pub fn media_bytes(&self, name: &str) -> Result<Vec<u8>, MediaError> {
        let file = self
            .media_file(name)?
            .filter(|f| !f.deleted)
            .ok_or_else(|| MediaError::NotFound(name.to_owned()))?;
        let bytes: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT data FROM media_blob WHERE hash = ?1",
                [&file.hash],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        let bytes = bytes.ok_or_else(|| MediaError::NoBytes(name.to_owned()))?;
        // The bytes are checked against the name's hash, so a damaged copy is never served.
        if hash_of(&bytes) == file.hash {
            Ok(bytes)
        } else {
            Err(MediaError::NoBytes(name.to_owned()))
        }
    }
}
