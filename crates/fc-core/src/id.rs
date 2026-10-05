//! 128-bit IDs (ADR 0006, section 1). Stored as 16-byte blobs, shown as lowercase UUID strings.

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id([u8; 16]);

/// The text is not a canonical lowercase UUID.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Not a valid ID.")
    }
}

impl std::error::Error for InvalidId {}

/// The system could not supply random numbers.
#[derive(Debug, PartialEq, Eq)]
pub struct NoRandomness(String);

impl fmt::Display for NoRandomness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "No random numbers are available: {}", self.0)
    }
}

impl std::error::Error for NoRandomness {}

impl Id {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// A version 7 UUID: 48 bits of time, then random bits. Both come from the caller, so this
    /// never reads the clock or the random number generator. New rows sort by creation time.
    pub fn new_v7(unix_ms: i64, random: &[u8; 10]) -> Self {
        let ms = u64::try_from(unix_ms).unwrap_or(0);
        Self(
            *uuid::Builder::from_unix_timestamp_millis(ms, random)
                .into_uuid()
                .as_bytes(),
        )
    }

    /// A new random version 7 ID at `unix_ms`, with the system's random numbers (`getrandom`, which
    /// uses the browser's `crypto` on the web).
    pub fn generate(unix_ms: i64) -> Result<Self, NoRandomness> {
        let mut random = [0u8; 10];
        getrandom::fill(&mut random).map_err(|e| NoRandomness(e.to_string()))?;
        Ok(Self::new_v7(unix_ms, &random))
    }

    /// A version 5 UUID: the same `namespace` and `name` always give the same ID.
    pub fn new_v5(namespace: Id, name: &[u8]) -> Self {
        Self(*Uuid::new_v5(&Uuid::from_bytes(namespace.0), name).as_bytes())
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&Uuid::from_bytes(self.0).hyphenated(), f)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({self})")
    }
}

impl FromStr for Id {
    type Err = InvalidId;

    fn from_str(text: &str) -> Result<Self, InvalidId> {
        let id = Self(*Uuid::try_parse(text).map_err(|_| InvalidId)?.as_bytes());
        // `uuid` also accepts braces, URNs and upper case. We accept the canonical form only.
        if id.to_string() == text {
            Ok(id)
        } else {
            Err(InvalidId)
        }
    }
}

impl ToSql for Id {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.0.as_slice().to_sql()
    }
}

impl FromSql for Id {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Self::from_slice(value.as_blob()?).ok_or(FromSqlError::InvalidType)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v7_ids_carry_the_time_and_the_version() {
        let id = Id::new_v7(0x0123_4567_89ab, &[0xff; 10]);
        assert_eq!(&id.as_bytes()[..6], &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab]);
        assert_eq!(id.as_bytes()[6] >> 4, 7, "version");
        assert_eq!(id.as_bytes()[8] >> 6, 0b10, "variant");
        assert_eq!(id.to_string().len(), 36);
        assert_eq!(&id.to_string()[..13], "01234567-89ab");
    }

    #[test]
    fn generated_ids_are_v7_and_different() {
        let a = Id::generate(1_700_000_000_000).unwrap();
        let b = Id::generate(1_700_000_000_000).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.as_bytes()[6] >> 4, 7);
    }

    #[test]
    fn v7_ids_sort_by_time_first() {
        let early = Id::new_v7(1_000, &[0xff; 10]);
        let late = Id::new_v7(1_001, &[0x00; 10]);
        assert!(early < late);
    }

    #[test]
    fn v7_ids_from_a_negative_time_do_not_panic() {
        let id = Id::new_v7(-5, &[1; 10]);
        assert_eq!(&id.as_bytes()[..6], &[0; 6]);
    }

    #[test]
    fn v5_ids_are_deterministic() {
        let namespace = Id::from_bytes(*b"fc-test-namespc!");
        assert_eq!(Id::new_v5(namespace, b"a"), Id::new_v5(namespace, b"a"));
        assert_ne!(Id::new_v5(namespace, b"a"), Id::new_v5(namespace, b"b"));
        assert_eq!(Id::new_v5(namespace, b"a").as_bytes()[6] >> 4, 5);
    }

    #[test]
    fn text_and_blob_round_trip() {
        let id = Id::new_v7(1_700_000_000_000, &[7; 10]);
        assert_eq!(id.to_string().parse::<Id>(), Ok(id));
        assert_eq!(Id::from_slice(id.as_bytes()), Some(id));
        assert_eq!(Id::from_slice(&[1, 2, 3]), None);
    }

    #[test]
    fn only_the_canonical_text_form_parses() {
        let id = Id::new_v7(1_700_000_000_000, &[7; 10]);
        let text = id.to_string();
        for bad in [
            text.to_uppercase(),
            text.replace('-', ""),
            format!("{{{text}}}"),
            format!("urn:uuid:{text}"),
            text[1..].to_owned(),
            String::new(),
        ] {
            assert_eq!(bad.parse::<Id>(), Err(InvalidId), "{bad}");
        }
    }

    #[test]
    fn stored_as_a_16_byte_blob() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let id = Id::new_v7(1_700_000_000_000, &[9; 10]);
        let (back, length): (Id, i64) = conn
            .query_row("SELECT ?1, length(?1)", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(back, id);
        assert_eq!(length, 16);
        let bad = conn.query_row("SELECT x'0102'", [], |r| r.get::<_, Id>(0));
        assert!(bad.is_err());
    }
}
