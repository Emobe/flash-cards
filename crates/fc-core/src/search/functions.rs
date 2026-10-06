//! The SQL functions search uses, registered on every connection when a collection is opened.
//! They are deterministic, so SQLite may call each as few times as it can.
//!
//! - `fc_contains(text, pattern)`: the field text (HTML) has the pattern inside it.
//! - `fc_equals(text, pattern)`: the name is the pattern from end to end.
//! - `fc_fold(text)`: field text reduced for sorting.
//! - `fc_id_ms(id)`: the time in an ID made by `Id::new_v7`, in Unix milliseconds.
//! - `fc_random(id, seed)`: a number that depends only on the two, for a repeatable shuffle.

use rusqlite::Connection;
use rusqlite::functions::{Context, FunctionFlags};

use super::pattern::{Pattern, fold, fold_html};

pub(crate) fn register(conn: &Connection) -> rusqlite::Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    conn.create_scalar_function("fc_contains", 2, flags, |ctx| {
        let (Some(text), Some(pattern)) = (text_arg(ctx, 0)?, text_arg(ctx, 1)?) else {
            return Ok(false);
        };
        Ok(Pattern::new(&pattern).is_inside(&fold_html(&text)))
    })?;
    conn.create_scalar_function("fc_equals", 2, flags, |ctx| {
        let (Some(text), Some(pattern)) = (text_arg(ctx, 0)?, text_arg(ctx, 1)?) else {
            return Ok(false);
        };
        Ok(Pattern::new(&pattern).is_all_of(&fold(&text)))
    })?;
    conn.create_scalar_function("fc_fold", 1, flags, |ctx| {
        Ok(text_arg(ctx, 0)?.map(|text| fold_html(&text)))
    })?;
    conn.create_scalar_function("fc_id_ms", 1, flags, |ctx| {
        let bytes: Option<Vec<u8>> = ctx.get(0)?;
        Ok(bytes.filter(|b| b.len() == 16).map(|b| {
            let mut ms = [0u8; 8];
            ms[2..].copy_from_slice(&b[..6]);
            i64::from_be_bytes(ms)
        }))
    })?;
    conn.create_scalar_function("fc_random", 2, flags, |ctx| {
        let bytes: Vec<u8> = ctx.get(0)?;
        let seed: i64 = ctx.get(1)?;
        Ok(shuffle_key(&bytes, seed as u64))
    })?;
    Ok(())
}

fn text_arg(ctx: &Context<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    ctx.get(index)
}

/// FNV-1a over the bytes and the seed, then mixed so close IDs land far apart. SQLite integers are
/// signed, so the top bit is cleared.
fn shuffle_key(bytes: &[u8], seed: u64) -> i64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    (hash >> 1) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        register(&conn).unwrap();
        conn
    }

    #[test]
    fn contains_reads_through_html_and_case() {
        let conn = connection();
        let hit = |text: &str, pattern: &str| -> bool {
            conn.query_row("SELECT fc_contains(?1, ?2)", [text, pattern], |r| r.get(0))
                .unwrap()
        };
        assert!(hit("<b>Mój</b>&nbsp;KOT", "mój kot"));
        assert!(hit("Łódź", "łódź"));
        assert!(!hit("<b>cat</b>", "<b>"), "tags are not text");
        assert!(
            !conn
                .query_row("SELECT fc_contains(NULL, 'a')", [], |r| r.get::<_, bool>(0))
                .unwrap()
        );
    }

    #[test]
    fn the_time_of_an_id_comes_back_out() {
        let conn = connection();
        let id = crate::id::Id::new_v7(1_700_000_123_456, &[7; 10]);
        let ms: i64 = conn
            .query_row("SELECT fc_id_ms(?1)", [id.as_bytes().to_vec()], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(ms, 1_700_000_123_456);
    }

    #[test]
    fn the_shuffle_depends_on_the_seed_and_nothing_else() {
        assert_eq!(shuffle_key(&[1, 2, 3], 5), shuffle_key(&[1, 2, 3], 5));
        assert_ne!(shuffle_key(&[1, 2, 3], 5), shuffle_key(&[1, 2, 3], 6));
        assert!(shuffle_key(&[255; 16], u64::MAX) >= 0);
    }
}
