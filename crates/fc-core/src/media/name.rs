//! Hashes and names. See the module documentation for the format.

use sha2::{Digest, Sha256};

use super::MEDIA_NAMESPACE;
use crate::id::Id;

/// How many hex digits of the hash a name carries (64 bits).
pub const HASH_DIGITS: usize = 16;

const MAX_STEM: usize = 40;
const MAX_EXTENSION: usize = 8;

/// SHA-256 of `bytes` as 64 lowercase hex digits.
pub(super) fn hash_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

pub(super) fn id_of(hash: &str) -> Id {
    Id::new_v5(MEDIA_NAMESPACE, hash.as_bytes())
}

/// The name to use in a field for a file that was added as `original` (a file name or a path) and
/// has this hash. The same arguments always give the same name.
pub fn file_name(original: &str, hash: &str) -> String {
    let base = original.rsplit(['/', '\\']).next().unwrap_or("");
    let (stem, extension) = match base.rsplit_once('.') {
        Some((stem, extension)) => (stem, extension),
        None => (base, ""),
    };
    let extension: String = extension
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .take(MAX_EXTENSION)
        .collect();
    let mut stem_clean = String::new();
    for c in stem.chars() {
        let c = if c.is_alphanumeric() || c == '-' {
            c
        } else {
            '_'
        };
        if c == '_' && stem_clean.ends_with('_') {
            continue;
        }
        stem_clean.push(c);
        if stem_clean.chars().count() >= MAX_STEM {
            break;
        }
    }
    let stem_clean = stem_clean.trim_matches(['_', '-']);
    let stem_clean = if stem_clean.is_empty() {
        "file"
    } else {
        stem_clean
    };
    let digits = &hash[..HASH_DIGITS.min(hash.len())];
    if extension.is_empty() {
        format!("{stem_clean}-{digits}")
    } else {
        format!("{stem_clean}-{digits}.{extension}")
    }
}

/// The hash digits a name carries, in lowercase, or `None` if it is not in the format (a name an
/// import or a person made up, which belongs to no file).
pub(super) fn digits_in(name: &str) -> Option<String> {
    let base = match name.rfind('.') {
        Some(dot)
            if (1..=MAX_EXTENSION).contains(&(name.len() - dot - 1))
                && name[dot + 1..].bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            &name[..dot]
        }
        _ => name,
    };
    let bytes = base.as_bytes();
    if bytes.len() <= HASH_DIGITS {
        return None;
    }
    let (head, tail) = bytes.split_at(bytes.len() - HASH_DIGITS);
    if head.last() == Some(&b'-') && tail.iter().all(u8::is_ascii_hexdigit) {
        Some(String::from_utf8_lossy(tail).to_ascii_lowercase())
    } else {
        None
    }
}
