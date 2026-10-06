//! Text matching for search: a pattern where `*` matches anything, compared after both sides are
//! reduced the same way (tags and entities removed from field text, lower case). SQLite's own
//! `LIKE` folds only ASCII, which would make `ł` and `Ł` different letters, so the matching is
//! done here and registered on the connection as SQL functions (`functions.rs`).

use crate::html;

/// A pattern split at its wildcards. Built from the text a person typed: `*` is a wildcard, `\*`
/// is a star and `\\` is a backslash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pattern {
    pieces: Vec<String>,
}

impl Pattern {
    pub fn new(text: &str) -> Self {
        let mut pieces = vec![String::new()];
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '*' => pieces.push(String::new()),
                '\\' if matches!(chars.peek(), Some('*' | '\\')) => {
                    let literal = chars.next().expect("peeked");
                    pieces.last_mut().expect("never empty").push(literal);
                }
                c => pieces.last_mut().expect("never empty").push(c),
            }
        }
        for piece in &mut pieces {
            *piece = fold(piece);
        }
        Self { pieces }
    }

    /// Whether the folded `text` has this pattern inside it.
    pub fn is_inside(&self, text: &str) -> bool {
        self.matches(text, false)
    }

    /// Whether the folded `text` is this pattern from end to end.
    pub fn is_all_of(&self, text: &str) -> bool {
        self.matches(text, true)
    }

    fn matches(&self, text: &str, whole: bool) -> bool {
        let [only] = self.pieces.as_slice() else {
            return self.matches_pieces(text, whole);
        };
        if whole {
            text == only
        } else {
            text.contains(only)
        }
    }

    fn matches_pieces(&self, text: &str, whole: bool) -> bool {
        let mut rest = text;
        let last = self.pieces.len() - 1;
        for (i, piece) in self.pieces.iter().enumerate() {
            if whole && i == 0 {
                let Some(after) = rest.strip_prefix(piece.as_str()) else {
                    return false;
                };
                rest = after;
            } else if whole && i == last {
                return rest.ends_with(piece.as_str());
            } else {
                let Some(at) = rest.find(piece.as_str()) else {
                    return false;
                };
                rest = &rest[at + piece.len()..];
            }
        }
        true
    }
}

/// A piece of a pattern, or a name, reduced the way field text is.
pub(crate) fn fold(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Field text (HTML) reduced for matching.
pub(crate) fn fold_html(html: &str) -> String {
    html::comparison_key(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(pattern: &str, text: &str) -> bool {
        Pattern::new(pattern).is_inside(&fold(text))
    }

    fn all_of(pattern: &str, text: &str) -> bool {
        Pattern::new(pattern).is_all_of(&fold(text))
    }

    #[test]
    fn a_plain_pattern_is_a_substring_ignoring_case_in_any_alphabet() {
        assert!(inside("kot", "Mój KOT śpi"));
        assert!(inside("ł", "Łódź"));
        assert!(inside("ŁÓD", "łódź"));
        assert!(!inside("kota", "kot"));
    }

    #[test]
    fn stars_match_anything() {
        assert!(inside("k*t", "kocie ta"));
        assert!(inside("*", ""));
        assert!(inside("a*b*c", "xxaxxbxxcxx"));
        assert!(!inside("a*b*c", "xxcxxbxxaxx"));
    }

    #[test]
    fn whole_matches_are_anchored_at_both_ends() {
        assert!(all_of("lang", "Lang"));
        assert!(!all_of("lang", "lang::polish"));
        assert!(all_of("lang::*", "lang::polish"));
        assert!(all_of("*polish", "lang::polish"));
        assert!(!all_of("*polish", "polish::lang"));
        assert!(!all_of("a*a", "a"), "the pieces may not overlap");
    }

    #[test]
    fn an_escaped_star_is_a_star() {
        assert!(inside("a\\*b", "a*b"));
        assert!(!inside("a\\*b", "axb"));
        assert!(inside("a\\\\b", "a\\b"));
    }
}
