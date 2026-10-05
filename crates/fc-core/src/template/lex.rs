//! Cutting a template into text and tags. The lexer never fails and keeps the place of every name,
//! so renaming a field works even on a template that does not parse.

use std::ops::Range;

/// A filter changes how a field is shown: `{{cloze:Text}}`, `{{text:Front}}`, `{{hint:Extra}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Filter {
    Cloze,
    Text,
    Hint,
    /// A word before a colon that is not a filter this version knows (it may come from a newer
    /// app). The parser reports it, and card generation ignores it.
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TagKind {
    /// A field, with its filters in the order written.
    Field(Vec<Filter>),
    /// `{{#Name}}`: shown when the field has content.
    Section,
    /// `{{^Name}}`: shown when the field is empty.
    Inverted,
    /// `{{/Name}}`.
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tag<'a> {
    pub kind: TagKind,
    /// The name, trimmed. It can be empty (`{{}}`).
    pub name: &'a str,
    /// Where the name is in the text, so a rename can replace just that.
    pub name_range: Range<usize>,
    /// Where the tag starts, at its `{{`.
    pub start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    Text(&'a str),
    Tag(Tag<'a>),
    /// A `{{` with no `}}` after it, and everything after it. Read as text when leniently parsed.
    Unclosed {
        text: &'a str,
        start: usize,
    },
}

pub(crate) fn lex(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut at = 0;
    loop {
        let Some(open) = text[at..].find("{{").map(|i| at + i) else {
            if at < text.len() {
                tokens.push(Token::Text(&text[at..]));
            }
            break;
        };
        if open > at {
            tokens.push(Token::Text(&text[at..open]));
        }
        let inner_start = open + 2;
        let Some(inner_end) = text[inner_start..].find("}}").map(|i| inner_start + i) else {
            tokens.push(Token::Unclosed {
                text: &text[open..],
                start: open,
            });
            break;
        };
        tokens.push(Token::Tag(lex_tag(text, open, inner_start..inner_end)));
        at = inner_end + 2;
    }
    tokens
}

/// The place of `part` inside `whole`, where `part` is a slice of `whole`.
fn range_in(whole: &str, part: &str) -> Range<usize> {
    let start = part.as_ptr() as usize - whole.as_ptr() as usize;
    start..start + part.len()
}

fn lex_tag(text: &str, start: usize, inner: Range<usize>) -> Tag<'_> {
    let trimmed = text[inner].trim();
    let special = match trimmed.chars().next() {
        Some('#') => Some(TagKind::Section),
        Some('^') => Some(TagKind::Inverted),
        Some('/') => Some(TagKind::End),
        _ => None,
    };
    if let Some(kind) = special {
        let name = trimmed[1..].trim();
        return Tag {
            kind,
            name,
            name_range: range_in(text, name),
            start,
        };
    }
    let mut filters = Vec::new();
    let mut rest = trimmed;
    while let Some((head, tail)) = rest.split_once(':') {
        let head = head.trim();
        let word = !head.is_empty()
            && head
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
        if !word {
            break;
        }
        filters.push(match head {
            "cloze" => Filter::Cloze,
            "text" => Filter::Text,
            "hint" => Filter::Hint,
            other => Filter::Unknown(other.to_owned()),
        });
        rest = tail;
    }
    let name = rest.trim();
    Tag {
        kind: TagKind::Field(filters),
        name,
        name_range: range_in(text, name),
        start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(text: &str) -> Vec<Tag<'_>> {
        lex(text)
            .into_iter()
            .filter_map(|t| match t {
                Token::Tag(tag) => Some(tag),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn reads_fields_filters_and_sections() {
        let found = tags("a {{Front}} b {{ cloze:Text }} {{#Extra}}x{{/Extra}} {{^Z}}{{/Z}}");
        let names: Vec<_> = found.iter().map(|t| t.name).collect();
        assert_eq!(names, ["Front", "Text", "Extra", "Extra", "Z", "Z"]);
        assert_eq!(found[1].kind, TagKind::Field(vec![Filter::Cloze]));
        assert_eq!(found[2].kind, TagKind::Section);
        assert_eq!(found[4].kind, TagKind::Inverted);
        assert_eq!(found[5].kind, TagKind::End);
    }

    #[test]
    fn filters_chain_in_the_order_written() {
        let found = tags("{{hint:text:Front}} {{foo:Back}}");
        assert_eq!(
            found[0].kind,
            TagKind::Field(vec![Filter::Hint, Filter::Text])
        );
        assert_eq!(found[0].name, "Front");
        assert_eq!(
            found[1].kind,
            TagKind::Field(vec![Filter::Unknown("foo".into())])
        );
    }

    #[test]
    fn a_colon_that_is_not_a_filter_stays_in_the_name() {
        let found = tags("{{Time zone: x}} {{cloze:Time zone: x}}");
        assert_eq!(found[0].name, "Time zone: x");
        assert_eq!(found[0].kind, TagKind::Field(vec![]));
        assert_eq!(found[1].name, "Time zone: x");
        assert_eq!(found[1].kind, TagKind::Field(vec![Filter::Cloze]));
    }

    #[test]
    fn names_have_places() {
        let text = "x {{ cloze:Text }} y {{#Add reverse}}";
        let found = tags(text);
        assert_eq!(&text[found[0].name_range.clone()], "Text");
        assert_eq!(&text[found[1].name_range.clone()], "Add reverse");
        assert_eq!(found[0].start, 2);
    }

    #[test]
    fn a_tag_that_never_closes_is_kept() {
        assert_eq!(
            lex("a {{Front"),
            [
                Token::Text("a "),
                Token::Unclosed {
                    text: "{{Front",
                    start: 2
                }
            ]
        );
        assert_eq!(tags("{{}}")[0].name, "");
        assert_eq!(tags("{{ }} {{Back}}").len(), 2);
        assert_eq!(lex("").len(), 0);
    }
}
