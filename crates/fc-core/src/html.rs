//! Small readings of HTML that notes and templates need: whether a field shows anything, what
//! it says as plain text, and which media files it names. Nothing here parses HTML fully. Field
//! content is untrusted and is only ever shown inside the card frame (ADR 0005), so these helpers
//! never decide what is safe: they only read.

/// Tags that show something by themselves, so a field holding only one is not empty.
const CONTENT_TAGS: [&str; 8] = [
    "img", "audio", "video", "picture", "svg", "object", "embed", "iframe",
];

/// Tags whose `src` names a media file the card frame can supply (ADR 0005).
const MEDIA_TAGS: [&str; 4] = ["img", "audio", "video", "source"];

/// The lower-case name of a tag, from the text between `<` and `>`, without a leading `/`.
fn tag_name(inner: &str) -> String {
    inner
        .trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The text of a tag after its name.
fn attributes(inner: &str) -> &str {
    let name_end = inner
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(inner.len());
    &inner[name_end..]
}

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
        let tag = tag_name(&rest[open + 1..open + close]);
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

/// Tags that end a line of text, so `a<br>b` reads as two words.
const BREAKS: [&str; 7] = ["br", "p", "div", "li", "tr", "td", "hr"];

/// `html` as plain text: no tags, a space where a line ends, every other character as written
/// (entities are kept). A stray `<` or `>` is escaped, so the result is safe to put in HTML.
pub(crate) fn plain_text(html: &str) -> String {
    let mut plain = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        plain.push_str(&escape_angles(&rest[..open]));
        let Some(close) = rest[open..].find('>') else {
            // A `<` that never closes is text, not a tag.
            plain.push_str("&lt;");
            rest = &rest[open + 1..];
            continue;
        };
        if BREAKS.contains(&tag_name(&rest[open + 1..open + close]).as_str()) {
            plain.push(' ');
        }
        rest = &rest[open + close + 1..];
    }
    plain.push_str(&escape_angles(rest));
    plain
}

fn escape_angles(text: &str) -> String {
    text.replace('<', "&lt;").replace('>', "&gt;")
}

/// `text` safe to put in HTML text or in a double-quoted attribute.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// A field's text reduced for comparing two notes: no tags, spaces collapsed, lower case.
pub(crate) fn comparison_key(html: &str) -> String {
    let mut plain = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        plain.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        if BREAKS.contains(&tag_name(&rest[open + 1..open + close]).as_str()) {
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

/// The end of the tag that starts at `html[0]` (a `<`): the index of its `>`, not counting a `>`
/// inside a quoted attribute value.
fn tag_end(html: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (at, c) in html.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(at),
            (None, _) => {}
        }
    }
    None
}

/// The value of the `src` attribute in the text of a tag after its name, decoded.
fn src_attribute(attributes: &str) -> Option<String> {
    let mut rest = attributes;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == '/' || c == '>')
            .unwrap_or(rest.len());
        if name_end == 0 {
            return None;
        }
        let name = &rest[..name_end];
        rest = rest[name_end..].trim_start();
        let value = if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (value, tail) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let body = &after[1..];
                    let end = body.find(q).unwrap_or(body.len());
                    (&body[..end], body.get(end + 1..).unwrap_or(""))
                }
                _ => {
                    let end = after
                        .find(|c: char| c.is_whitespace() || c == '>')
                        .unwrap_or(after.len());
                    (&after[..end], &after[end..])
                }
            };
            rest = tail;
            Some(value)
        } else {
            None
        };
        if name.eq_ignore_ascii_case("src") {
            return value.map(decode);
        }
    }
}

fn decode(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Whether a `src` names a file of the collection, not a web address or a data URL (which the card
/// frame's CSP blocks, or which needs no file).
fn is_local(name: &str) -> bool {
    if name.is_empty() || name.starts_with("//") || name.starts_with('#') {
        return false;
    }
    let scheme = name.split(':').next().unwrap_or("");
    let has_scheme = name.contains(':')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    !has_scheme
}

/// The media files that `html` names in the `src` of an `<img>`, `<audio>`, `<video>` or `<source>`
/// tag, in order and without repeats. These are the names the card frame looks up (ADR 0005).
pub(crate) fn media_names(html: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        rest = &rest[open..];
        let Some(end) = tag_end(rest) else { break };
        let inner = &rest[1..end];
        let name = tag_name(inner);
        if MEDIA_TAGS.contains(&name.as_str())
            && let Some(src) = src_attribute(attributes(inner))
            && is_local(&src)
            && !names.contains(&src)
        {
            names.push(src);
        }
        rest = &rest[end + 1..];
    }
    names
}

/// `html` with each `[sound:name]` shorthand turned into an `<audio controls>` element. A name
/// that is empty or spans more than one line is left as text.
pub(crate) fn expand_sound(html: &str) -> String {
    const OPEN: &str = "[sound:";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let after = &rest[at + OPEN.len()..];
        match after.find(']') {
            Some(end) if end > 0 && !after[..end].contains(['\n', '[', '<']) => {
                out.push_str("<audio controls src=\"");
                out.push_str(&escape(after[..end].trim()));
                out.push_str("\"></audio>");
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(OPEN);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn plain_text_drops_tags_and_keeps_the_rest_safe() {
        assert_eq!(plain_text("<b>a</b>&amp;b"), "a&amp;b");
        assert_eq!(plain_text("a<br>b"), "a b");
        assert_eq!(plain_text("1 < 2"), "1 &lt; 2");
        assert_eq!(plain_text("<script>x</script>"), "x");
        assert_eq!(plain_text("a > b"), "a &gt; b");
    }

    #[test]
    fn media_names_come_from_src_attributes() {
        let html = concat!(
            "<img src=\"a.png\"> <IMG SRC='b c.jpg'/> <audio src=d.mp3></audio>",
            "<video controls><source src=\"e.mp4\" type=video/mp4></video>",
            "<img src=\"a.png\"><img src=\"x&amp;y.png\">",
        );
        assert_eq!(
            media_names(html),
            ["a.png", "b c.jpg", "d.mp3", "e.mp4", "x&y.png"]
        );
    }

    #[test]
    fn media_names_skip_web_addresses_and_other_tags() {
        let html = concat!(
            "<img src=\"https://example.com/a.png\"><img src=\"//x/y.png\">",
            "<img src=\"data:image/png;base64,AAAA\"><img src=\"blob:x\">",
            "<a href=\"b.png\">b</a><script src=\"c.js\"></script><img alt=x>",
            "<img src=\"\"><img data-src=\"d.png\">",
        );
        assert!(media_names(html).is_empty(), "{:?}", media_names(html));
    }

    #[test]
    fn a_quoted_angle_bracket_does_not_end_a_tag() {
        assert_eq!(media_names("<img alt=\"a > b\" src=\"x.png\">"), ["x.png"]);
        assert!(media_names("<img src=\"x.png\"").is_empty());
    }

    #[test]
    fn sound_shorthand_becomes_audio() {
        assert_eq!(
            expand_sound("a [sound:pies.mp3] b"),
            "a <audio controls src=\"pies.mp3\"></audio> b"
        );
        assert_eq!(
            expand_sound("[sound:a\"b.mp3]"),
            "<audio controls src=\"a&quot;b.mp3\"></audio>"
        );
        for text in ["[sound:]", "[sound:a\nb]", "[sound:a", "[sound:<b>]"] {
            assert_eq!(expand_sound(text), text);
        }
        assert_eq!(media_names(&expand_sound("[sound:a&b.mp3]")), ["a&b.mp3"]);
    }
}
