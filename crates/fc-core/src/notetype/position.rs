//! Positions for ordered lists that merge (fields and templates of a note type).
//!
//! A position is a string that sorts between its neighbours (a "fractional index"). Moving an item
//! writes one position, so two devices that reorder at the same time both keep their change, which a
//! renumbering of the whole list or a shared counter could not do (ADR 0006, section 6). Two items
//! can end up with the same position after a merge. Lists are ordered by `(position, id)`, so the
//! order is still the same on every device.
//!
//! Digits are `0-9A-Za-z`, which is also the byte order SQLite and Rust compare strings in. A
//! position never ends in `0`, which is what leaves room to insert before any of them.

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn digit(c: u8) -> usize {
    DIGITS
        .iter()
        .position(|d| *d == c)
        .expect("positions use only the digits")
}

/// The digit of `text` at `index`, counting a missing digit as `0`.
fn digit_at(text: &[u8], index: usize) -> usize {
    text.get(index).map_or(0, |c| digit(*c))
}

/// A position after `before` and before `after`. `None` means no neighbour on that side. Both must
/// be well formed and in order, which `strictly_between` checks.
pub fn between(before: Option<&str>, after: Option<&str>) -> String {
    let before = before.unwrap_or("").as_bytes();
    let after = after.map(str::as_bytes);
    String::from_utf8(midpoint(before, after)).expect("digits are ASCII")
}

/// A well-formed position: only digits, and not empty or ending in `0`. Positions written by this
/// code always are. One that arrives from another device may not be, so it is checked before use.
pub fn is_valid(position: &str) -> bool {
    !position.is_empty()
        && !position.ends_with('0')
        && position.bytes().all(|c| DIGITS.contains(&c))
}

/// True when a position can be put between the two: both are well formed (when given) and in
/// order. When it is false the caller renumbers the list with `evenly`.
pub fn strictly_between(before: Option<&str>, after: Option<&str>) -> bool {
    before.is_none_or(is_valid)
        && after.is_none_or(is_valid)
        && match (before, after) {
            (Some(before), Some(after)) => before < after,
            _ => true,
        }
}

fn midpoint(before: &[u8], after: Option<&[u8]>) -> Vec<u8> {
    if let Some(after) = after {
        // Skip the digits they share, treating a missing digit of `before` as 0.
        let shared = (0..after.len())
            .take_while(|&i| digit_at(before, i) == digit(after[i]))
            .count();
        if shared > 0 {
            let mut result = after[..shared].to_vec();
            result.extend(midpoint(
                before.get(shared..).unwrap_or(&[]),
                Some(&after[shared..]),
            ));
            return result;
        }
    }
    let low = digit_at(before, 0);
    let high = after.map_or(DIGITS.len(), |after| digit(after[0]));
    if high - low > 1 {
        return vec![DIGITS[(low + high).div_ceil(2)]];
    }
    // The first digits are neighbours. If `after` goes on, its first digit already sorts between.
    if let Some(after) = after
        && after.len() > 1
    {
        return vec![after[0]];
    }
    let mut result = vec![DIGITS[low]];
    result.extend(midpoint(before.get(1..).unwrap_or(&[]), None));
    result
}

/// Fresh positions for `count` items in order, used when ties leave no room to insert.
pub fn evenly(count: usize) -> Vec<String> {
    let mut positions: Vec<String> = Vec::with_capacity(count);
    for _ in 0..count {
        let next = between(positions.last().map(String::as_str), None);
        positions.push(next);
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_position_has_room_on_both_sides() {
        let first = between(None, None);
        assert!(between(None, Some(&first)) < first);
        assert!(between(Some(&first), None) > first);
    }

    #[test]
    fn a_position_between_two_sorts_between_them() {
        for (a, b) in [("V", "l"), ("V", "W"), ("1", "2"), ("0V", "1"), ("Vz", "W")] {
            let m = between(Some(a), Some(b));
            assert!(a < m.as_str() && m.as_str() < b, "{a} < {m} < {b}");
        }
    }

    #[test]
    fn positions_never_end_in_zero() {
        let mut positions = vec![between(None, None)];
        for _ in 0..200 {
            let first = positions[0].clone();
            positions.insert(0, between(None, Some(&first)));
        }
        assert!(positions.iter().all(|p| !p.ends_with('0')));
    }

    #[test]
    fn appending_and_prepending_many_times_keeps_the_order() {
        let mut list = vec![between(None, None)];
        for i in 0..300 {
            if i % 2 == 0 {
                list.push(between(list.last().map(String::as_str), None));
            } else {
                list.insert(0, between(None, Some(&list[0])));
            }
        }
        assert!(list.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn inserting_at_pseudo_random_places_keeps_the_order() {
        let mut list: Vec<String> = Vec::new();
        let mut state = 12345u64;
        for _ in 0..500 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let at = (state >> 33) as usize % (list.len() + 1);
            let before = at.checked_sub(1).map(|i| list[i].as_str());
            let after = list.get(at).map(String::as_str);
            let new = between(before, after);
            list.insert(at, new);
        }
        assert!(list.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn equal_neighbours_have_no_room() {
        assert!(!strictly_between(Some("V"), Some("V")));
        assert!(!strictly_between(Some("W"), Some("V")));
        assert!(strictly_between(Some("V"), Some("W")));
        assert!(strictly_between(None, Some("V")));
    }

    #[test]
    fn malformed_neighbours_have_no_room() {
        assert!(!strictly_between(None, Some("")));
        assert!(!strictly_between(Some(""), Some("V")));
        assert!(!strictly_between(Some("V0"), Some("W")));
        assert!(!strictly_between(Some("V"), Some("W-")));
        assert!(is_valid("V") && is_valid("0V"));
    }

    #[test]
    fn evenly_gives_increasing_positions() {
        let positions = evenly(40);
        assert_eq!(positions.len(), 40);
        assert!(positions.windows(2).all(|w| w[0] < w[1]));
    }
}
