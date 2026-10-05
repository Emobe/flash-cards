//! The small part of the template language that card generation needs (step 1.3): which fields a
//! front side uses, whether it would show anything, which cloze numbers a note has, and renaming a
//! field inside a template. Step 1.4 defines the whole grammar, with its errors, and replaces this
//! with its parser. Until then this is deliberately lenient: it never fails, and a template that is
//! malformed is read as far as it makes sense.
//!
//! A tag is `{{...}}`. Its text, trimmed, is a field (`Name`, or `filter:Name`, with any number of
//! filters), a section start (`#Name` or `^Name`, shown when the field is not empty or is empty), a
//! section end (`/Name`). Names are matched exactly.

use std::ops::Range;

/// The most cloze numbers a note can make cards for. A larger number is plain text. This keeps one
/// typo from making a thousand cards.
pub(crate) const MAX_CLOZE: u32 = 500;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Kind<'a> {
    /// A field, with the filters before its name (`cloze`, `hint`, ...).
    Field {
        filters: Vec<&'a str>,
    },
    Section,
    Inverted,
    End,
}

#[derive(Debug)]
pub(crate) struct Tag<'a> {
    pub kind: Kind<'a>,
    pub name: &'a str,
    /// Where the name is in the text, so a rename can replace just that.
    pub name_range: Range<usize>,
}

/// Every tag in `text`, in order. A `{{` with no `}}` after it is plain text.
pub(crate) fn tags(text: &str) -> Vec<Tag<'_>> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(open) = text[at..].find("{{") {
        let inner_start = at + open + 2;
        let Some(close) = text[inner_start..].find("}}") else {
            break;
        };
        let inner_end = inner_start + close;
        at = inner_end + 2;
        let inner = &text[inner_start..inner_end];
        let trimmed = inner.trim();
        let lead = inner.len() - inner.trim_start().len();
        let base = inner_start + lead;
        let (kind, name, name_start) = match trimmed.chars().next() {
            Some('#') => (Kind::Section, &trimmed[1..], base + 1),
            Some('^') => (Kind::Inverted, &trimmed[1..], base + 1),
            Some('/') => (Kind::End, &trimmed[1..], base + 1),
            _ => {
                let split = trimmed.rfind(':').map_or(0, |i| i + 1);
                let filters = trimmed[..split.saturating_sub(1)]
                    .split(':')
                    .filter(|f| !f.is_empty())
                    .collect();
                (Kind::Field { filters }, &trimmed[split..], base + split)
            }
        };
        let lead_name = name.len() - name.trim_start().len();
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let start = name_start + lead_name;
        found.push(Tag {
            kind,
            name,
            name_range: start..start + name.len(),
        });
    }
    found
}

/// The field tags of a front that would show something, given a way to read a field. A section is
/// shown or hidden by its field, and a tag inside a hidden section counts for nothing. An end tag
/// closes the nearest open section of that name, and one with none is ignored, so a malformed
/// template is still read.
fn visible_fields<'a>(
    front: &'a str,
    is_empty: &dyn Fn(&str) -> bool,
) -> Vec<(&'a str, Vec<&'a str>)> {
    let mut open: Vec<(&str, bool)> = Vec::new();
    let mut shown = Vec::new();
    for tag in tags(front) {
        match tag.kind {
            Kind::Section => open.push((tag.name, !is_empty(tag.name))),
            Kind::Inverted => open.push((tag.name, is_empty(tag.name))),
            Kind::End => {
                if let Some(at) = open.iter().rposition(|(name, _)| *name == tag.name) {
                    open.truncate(at);
                }
            }
            Kind::Field { filters } => {
                if open.iter().all(|(_, active)| *active) {
                    shown.push((tag.name, filters));
                }
            }
        }
    }
    shown
}

/// Whether the front of a standard template would show a field that has something in it.
pub(crate) fn front_shows_content(front: &str, value: &dyn Fn(&str) -> Option<String>) -> bool {
    let is_empty = |name: &str| value(name).is_none_or(|v| !has_content(&v));
    visible_fields(front, &is_empty)
        .iter()
        .any(|(name, _)| !is_empty(name))
}

/// The cloze numbers a cloze front asks for: those in the fields it shows through the `cloze`
/// filter, sorted and without repeats.
pub(crate) fn cloze_numbers(front: &str, value: &dyn Fn(&str) -> Option<String>) -> Vec<u32> {
    let is_empty = |name: &str| value(name).is_none_or(|v| !has_content(&v));
    let mut numbers = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (name, filters) in visible_fields(front, &is_empty) {
        if filters.contains(&"cloze") && !seen.contains(&name) {
            seen.push(name);
            if let Some(text) = value(name) {
                numbers.extend(numbers_in(&text));
            }
        }
    }
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// The numbers of `{{c1::...}}` markers in a field's text.
fn numbers_in(text: &str) -> Vec<u32> {
    let mut numbers = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("{{c") {
        rest = &rest[at + 3..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0
            && rest[digits..].starts_with("::")
            && let Ok(number) = rest[..digits].parse::<u32>()
            && (1..=MAX_CLOZE).contains(&number)
        {
            numbers.push(number);
        }
    }
    numbers
}

/// `text` with every tag that names the field `old` changed to name `new`. Filters and the
/// spacing inside the braces stay as written.
pub(crate) fn rename_field(text: &str, old: &str, new: &str) -> String {
    let mut result = text.to_owned();
    for tag in tags(text).into_iter().rev() {
        if tag.name == old {
            result.replace_range(tag.name_range, new);
        }
    }
    result
}

/// Tags that show something by themselves, so a field holding only one is not empty.
const CONTENT_TAGS: [&str; 8] = [
    "img", "audio", "video", "picture", "svg", "object", "embed", "iframe",
];

/// Whether a field's HTML shows anything: some text that is not only spaces, or a tag like `<img>`.
pub(crate) fn has_content(html: &str) -> bool {
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        if !visible_text(&rest[..open]).is_empty() {
            return true;
        }
        let Some(close) = rest[open..].find('>') else {
            return !visible_text(&rest[open..]).is_empty();
        };
        let tag = rest[open + 1..open + close]
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if CONTENT_TAGS.contains(&tag.as_str()) {
            return true;
        }
        rest = &rest[open + close + 1..];
    }
    !visible_text(rest).is_empty()
}

fn visible_text(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .trim()
        .to_owned()
}

/// A field's text reduced for comparing two notes: no tags, spaces collapsed, lower case.
pub(crate) fn comparison_key(html: &str) -> String {
    const BREAKS: [&str; 7] = ["br", "p", "div", "li", "tr", "td", "hr"];
    let mut plain = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        plain.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close]
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if BREAKS.contains(&tag.as_str()) {
            plain.push(' ');
        }
        rest = &rest[open + close + 1..];
    }
    plain.push_str(rest);
    let plain = plain
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    plain
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn reads_fields_filters_and_sections() {
        let found = tags("a {{Front}} b {{ cloze:Text }} {{#Extra}}x{{/Extra}} {{^Z}}{{/Z}}");
        let summary: Vec<_> = found.iter().map(|t| t.name).collect();
        assert_eq!(summary, ["Front", "Text", "Extra", "Extra", "Z", "Z"]);
        assert_eq!(
            found[1].kind,
            Kind::Field {
                filters: vec!["cloze"]
            }
        );
        assert_eq!(found[2].kind, Kind::Section);
        assert_eq!(found[4].kind, Kind::Inverted);
        assert_eq!(found[5].kind, Kind::End);
    }

    #[test]
    fn a_tag_that_never_closes_is_text() {
        assert!(tags("{{Front").is_empty());
        assert!(tags("{{}}").is_empty());
        assert_eq!(tags("{{ }} {{Back}}").len(), 1);
    }

    #[test]
    fn a_front_shows_content_when_a_field_it_shows_has_some() {
        let v = values(&[("Front", "pies"), ("Back", "")]);
        assert!(front_shows_content("{{Front}}", &v));
        assert!(!front_shows_content("{{Back}}", &v));
        assert!(front_shows_content("{{Back}} {{Front}}", &v));
        assert!(!front_shows_content("no tags here", &v));
        assert!(!front_shows_content("{{Missing}}", &v));
        assert!(!front_shows_content("{{FrontSide}}", &v));
    }

    #[test]
    fn sections_decide_what_counts() {
        let v = values(&[
            ("Front", "pies"),
            ("Back", "dog"),
            ("Add", "y"),
            ("Empty", ""),
        ]);
        let optional = "{{#Add}}{{Back}}{{/Add}}";
        assert!(front_shows_content(optional, &v));
        assert!(!front_shows_content("{{#Empty}}{{Back}}{{/Empty}}", &v));
        assert!(front_shows_content("{{^Empty}}{{Back}}{{/Empty}}", &v));
        assert!(!front_shows_content("{{^Add}}{{Back}}{{/Add}}", &v));
        assert!(!front_shows_content(
            "{{#Add}}{{#Empty}}{{Back}}{{/Empty}}{{/Add}}",
            &v
        ));
        assert!(front_shows_content(
            "{{#Empty}}{{Back}}{{/Empty}}{{Front}}",
            &v
        ));
    }

    #[test]
    fn a_malformed_front_is_still_read() {
        let v = values(&[("Front", "pies"), ("Empty", "")]);
        assert!(!front_shows_content("{{#Empty}}{{Front}}", &v));
        assert!(front_shows_content("{{/Nothing}}{{Front}}", &v));
        assert!(front_shows_content("{{#Front}}{{Front}}", &v));
        assert!(front_shows_content(
            "{{#A}}{{/B}}{{Front}}",
            &values(&[("A", "x"), ("Front", "f")])
        ));
    }

    #[test]
    fn cloze_numbers_come_from_the_fields_the_front_shows_through_the_filter() {
        let v = values(&[
            ("Text", "{{c1::a}} {{c3::b::hint}} {{c1::again}}"),
            ("Extra", "{{c2::not here}}"),
        ]);
        assert_eq!(cloze_numbers("{{cloze:Text}}", &v), [1, 3]);
        assert_eq!(cloze_numbers("{{Text}}", &v), Vec::<u32>::new());
        assert_eq!(
            cloze_numbers("{{cloze:Text}}{{cloze:Extra}}", &v),
            [1, 2, 3]
        );
        assert_eq!(cloze_numbers("{{cloze:Text}}{{cloze:Text}}", &v), [1, 3]);
    }

    #[test]
    fn only_real_cloze_markers_in_range_count() {
        for (text, expected) in [
            ("{{c0::zero}}", vec![]),
            ("{{c501::big}}", vec![]),
            ("{{c500::edge}}", vec![500]),
            ("{{c::none}}", vec![]),
            ("{{cx::none}}", vec![]),
            ("{{c2:single colon}}", vec![]),
            ("{{c99999999999::huge}}", vec![]),
            ("{{c12::twelve}}", vec![12]),
        ] {
            assert_eq!(numbers_in(text), expected, "{text}");
        }
    }

    #[test]
    fn renaming_changes_only_the_tags_that_name_the_field() {
        let text = "{{Front}} Front {{ hint:Front }} {{#Front}}x{{/Front}} {{Back}} {{FrontSide}}";
        assert_eq!(
            rename_field(text, "Front", "Question"),
            "{{Question}} Front {{ hint:Question }} {{#Question}}x{{/Question}} {{Back}} {{FrontSide}}"
        );
        assert_eq!(rename_field("{{Front", "Front", "Q"), "{{Front");
    }

    #[test]
    fn content_is_text_or_media() {
        for yes in [
            "a",
            " <b>a</b> ",
            "<img src=x>",
            "<IMG SRC=x/>",
            "<audio src=a></audio>",
            "[sound:a.mp3]",
            "a<br>",
        ] {
            assert!(has_content(yes), "{yes}");
        }
        for no in [
            "",
            "   ",
            "<br>",
            "<div><br></div>",
            "&nbsp;",
            "<p>&nbsp; </p>",
            "<b></b>",
        ] {
            assert!(!has_content(no), "{no}");
        }
    }

    #[test]
    fn comparison_ignores_tags_spacing_and_case() {
        assert_eq!(comparison_key("  <b>Pies</b>&nbsp; "), "pies");
        assert_eq!(comparison_key("a<br>b"), "a b");
        assert_eq!(comparison_key("Ab<i>c</i>"), "abc");
        assert_eq!(comparison_key("Żółć  &amp; x"), "żółć & x");
        assert_eq!(comparison_key("<br>"), "");
    }
}
