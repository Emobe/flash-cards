//! Cloze deletions inside a field: `{{c1::answer}}` or `{{c1::answer::hint}}`.
//!
//! A marker is `{{c`, a number from 1 to [`MAX_CLOZE`], `::`, the answer, and `}}`. The answer can
//! hold markers with other numbers, to [`MAX_DEPTH`] levels deep. The first `::` at the top of the
//! answer starts the hint. Anything that does not form a marker (no number, a number out of range,
//! no `}}`) is plain text, so a typo never makes a card or hides a word by accident.

/// The most cloze numbers a note can make cards for. A larger number is plain text. This keeps one
/// typo from making a thousand cards.
pub(crate) const MAX_CLOZE: u32 = 500;

/// The most markers inside one another that count. Deeper ones are plain text.
const MAX_DEPTH: usize = 8;

#[derive(Debug, PartialEq, Eq)]
enum Piece<'a> {
    Text(&'a str),
    Cloze {
        number: u32,
        answer: Vec<Piece<'a>>,
        hint: Option<&'a str>,
    },
}

/// If `text` starts with `{{cN::` for a number in range, the number and the length of that start.
fn marker_start(text: &str) -> Option<(u32, usize)> {
    let rest = text.strip_prefix("{{c")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || !rest[digits..].starts_with("::") {
        return None;
    }
    let number = rest[..digits].parse::<u32>().ok()?;
    (1..=MAX_CLOZE)
        .contains(&number)
        .then_some((number, 3 + digits + 2))
}

/// The index in `text` of the `}}` that ends a marker whose answer starts at 0, counting markers
/// inside it. `None` if it never ends.
fn marker_end(text: &str) -> Option<usize> {
    let mut depth = 1;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if let Some((_, len)) = marker_start(rest) {
            depth += 1;
            at += len;
        } else if rest.starts_with("}}") {
            depth -= 1;
            if depth == 0 {
                return Some(at);
            }
            at += 2;
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// The first `::` that is not inside a marker.
fn hint_split(answer: &str) -> Option<usize> {
    let mut depth = 0;
    let mut at = 0;
    while at < answer.len() {
        let rest = &answer[at..];
        if let Some((_, len)) = marker_start(rest) {
            depth += 1;
            at += len;
        } else if depth > 0 && rest.starts_with("}}") {
            depth -= 1;
            at += 2;
        } else if depth == 0 && rest.starts_with("::") {
            return Some(at);
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

fn pieces(text: &str, depth: usize) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut plain_from = 0;
    let mut at = 0;
    while let Some(found) = text[at..].find("{{c").map(|i| at + i) {
        let rest = &text[found..];
        let marker = marker_start(rest)
            .filter(|_| depth < MAX_DEPTH)
            .and_then(|(number, len)| marker_end(&rest[len..]).map(|end| (number, len, end)));
        let Some((number, len, end)) = marker else {
            at = found + 3;
            continue;
        };
        if found > plain_from {
            out.push(Piece::Text(&text[plain_from..found]));
        }
        let inside = &rest[len..len + end];
        let (answer, hint) = match hint_split(inside) {
            Some(split) => (&inside[..split], Some(&inside[split + 2..])),
            None => (inside, None),
        };
        out.push(Piece::Cloze {
            number,
            answer: pieces(answer, depth + 1),
            hint,
        });
        at = found + len + end + 2;
        plain_from = at;
    }
    if plain_from < text.len() {
        out.push(Piece::Text(&text[plain_from..]));
    }
    out
}

fn collect(pieces: &[Piece<'_>], numbers: &mut Vec<u32>) {
    for piece in pieces {
        if let Piece::Cloze { number, answer, .. } = piece {
            numbers.push(*number);
            collect(answer, numbers);
        }
    }
}

/// The numbers of the markers in a field's text, sorted and without repeats.
pub(crate) fn numbers(text: &str) -> Vec<u32> {
    let mut found = Vec::new();
    collect(&pieces(text, 0), &mut found);
    found.sort_unstable();
    found.dedup();
    found
}

fn write(pieces: &[Piece<'_>], active: u32, answer_side: bool, out: &mut String) {
    for piece in pieces {
        match piece {
            Piece::Text(text) => out.push_str(text),
            Piece::Cloze {
                number,
                answer,
                hint,
            } if *number == active => {
                out.push_str("<span class=\"cloze\">");
                if answer_side {
                    write(answer, active, answer_side, out);
                } else {
                    out.push('[');
                    out.push_str(hint.filter(|h| !h.trim().is_empty()).unwrap_or("..."));
                    out.push(']');
                }
                out.push_str("</span>");
            }
            Piece::Cloze { answer, .. } => write(answer, active, answer_side, out),
        }
    }
}

/// A field's text with the markers of cloze number `active` hidden (`answer_side` false: `[...]`
/// or `[hint]`) or shown (`answer_side` true), and every other marker shown as plain text.
pub(crate) fn render(text: &str, active: u32, answer_side: bool) -> String {
    let mut out = String::with_capacity(text.len());
    write(&pieces(text, 0), active, answer_side, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_sorted_without_repeats() {
        assert_eq!(numbers("{{c3::a}} {{c1::b::hint}} {{c1::again}}"), [1, 3]);
        assert_eq!(numbers("plain"), Vec::<u32>::new());
        assert_eq!(numbers(""), Vec::<u32>::new());
    }

    #[test]
    fn only_real_markers_in_range_count() {
        for (text, expected) in [
            ("{{c0::zero}}", vec![]),
            ("{{c501::big}}", vec![]),
            ("{{c500::edge}}", vec![500]),
            ("{{c::none}}", vec![]),
            ("{{cx::none}}", vec![]),
            ("{{c2:single colon}}", vec![]),
            ("{{c99999999999::huge}}", vec![]),
            ("{{c12::twelve}}", vec![12]),
            ("{{c1::never ends", vec![]),
            ("{{c1::a}} {{c2::b", vec![1]),
        ] {
            assert_eq!(numbers(text), expected, "{text}");
        }
    }

    #[test]
    fn markers_can_nest_to_a_limit() {
        assert_eq!(numbers("{{c1::a {{c2::b}} c}}"), [1, 2]);
        let deep = format!("{}x{}", "{{c1::".repeat(20), "}}".repeat(20));
        assert_eq!(numbers(&deep), [1]);
        let html = render(&deep, 1, true);
        assert_eq!(html.matches("<span").count(), 8);
        assert!(html.contains("{{c1::"));
    }

    #[test]
    fn the_front_hides_the_active_cloze_and_shows_the_rest() {
        let text = "{{c1::Warszawa}} is in {{c2::Polska}}";
        assert_eq!(
            render(text, 1, false),
            "<span class=\"cloze\">[...]</span> is in Polska"
        );
        assert_eq!(
            render(text, 2, false),
            "Warszawa is in <span class=\"cloze\">[...]</span>"
        );
    }

    #[test]
    fn the_back_shows_the_active_answer_marked() {
        let text = "{{c1::Warszawa}} is in {{c2::Polska}}";
        assert_eq!(
            render(text, 1, true),
            "<span class=\"cloze\">Warszawa</span> is in Polska"
        );
    }

    #[test]
    fn a_hint_replaces_the_dots_on_the_front_only() {
        let text = "{{c1::pies::dog}}";
        assert_eq!(render(text, 1, false), "<span class=\"cloze\">[dog]</span>");
        assert_eq!(render(text, 1, true), "<span class=\"cloze\">pies</span>");
        assert_eq!(
            render("{{c1::pies::  }}", 1, false),
            "<span class=\"cloze\">[...]</span>"
        );
        assert_eq!(
            render("{{c1::a::b::c}}", 1, true),
            "<span class=\"cloze\">a</span>"
        );
    }

    #[test]
    fn the_same_number_hides_every_marker_with_it() {
        assert_eq!(
            render("{{c1::a}} {{c1::b}}", 1, false),
            "<span class=\"cloze\">[...]</span> <span class=\"cloze\">[...]</span>"
        );
    }

    #[test]
    fn nested_markers_hide_with_the_outer_one_and_show_one_by_one() {
        let text = "{{c1::big {{c2::small}}}}";
        assert_eq!(render(text, 1, false), "<span class=\"cloze\">[...]</span>");
        assert_eq!(
            render(text, 1, true),
            "<span class=\"cloze\">big small</span>"
        );
        assert_eq!(
            render(text, 2, false),
            "big <span class=\"cloze\">[...]</span>"
        );
    }

    #[test]
    fn text_that_is_not_a_marker_is_left_alone() {
        let text = "{{c0::a}} {{c1::b";
        assert_eq!(render(text, 1, false), text);
        assert_eq!(render("żółć {{c1::ż}}", 2, true), "żółć ż");
        assert_eq!(render("{{c1::x}}", 0, false), "x");
    }
}
