//! From tokens to a tree, with every mistake reported and the tree still usable. `parse` never
//! fails: a template that is malformed is read as far as it makes sense (an unclosed section ends
//! at the end of the template, a stray end tag is ignored). Card generation uses that reading,
//! because edits and merges never refuse (ADR 0006). Saving and rendering use the errors.

use super::error::{ErrorKind, Side, TemplateError};
use super::lex::{Filter, TagKind, Token, lex};
use crate::html::has_content;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Text(String),
    Field {
        name: String,
        filters: Vec<Filter>,
    },
    /// `{{FrontSide}}`: the front of the card, on the back.
    FrontSide,
    Section {
        name: String,
        inverted: bool,
        body: Vec<Node>,
    },
}

/// A parsed template side.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Parsed {
    pub nodes: Vec<Node>,
}

/// Where a byte offset is in `text`: line and column, from 1, in characters.
fn place(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rfind('\n')
        .map_or(before, |i| &before[i + 1..])
        .chars()
        .count()
        + 1;
    (line, column)
}

struct Open {
    name: String,
    inverted: bool,
    start: usize,
    body: Vec<Node>,
}

pub(crate) fn parse(text: &str, side: Side) -> (Parsed, Vec<TemplateError>) {
    let mut errors = Vec::new();
    let mut error = |kind: ErrorKind, offset: usize| {
        let (line, column) = place(text, offset);
        errors.push(TemplateError { kind, line, column });
    };
    let mut stack: Vec<Open> = Vec::new();
    let mut top: Vec<Node> = Vec::new();

    // Closes the innermost open section into the one below it (or the top level).
    fn close(stack: &mut Vec<Open>, top: &mut Vec<Node>) {
        if let Some(open) = stack.pop() {
            let node = Node::Section {
                name: open.name,
                inverted: open.inverted,
                body: open.body,
            };
            stack
                .last_mut()
                .map_or(&mut *top, |o| &mut o.body)
                .push(node);
        }
    }

    for token in lex(text) {
        match token {
            Token::Text(t) => push(&mut stack, &mut top, Node::Text(t.to_owned())),
            Token::Unclosed { text: t, start } => {
                error(ErrorKind::UnclosedTag, start);
                push(&mut stack, &mut top, Node::Text(t.to_owned()));
            }
            Token::Tag(tag) => {
                if tag.name.is_empty() {
                    error(ErrorKind::EmptyTag, tag.start);
                    continue;
                }
                match tag.kind {
                    TagKind::Field(filters) => {
                        for filter in &filters {
                            if let Filter::Unknown(word) = filter {
                                error(
                                    ErrorKind::UnknownFilter {
                                        filter: word.clone(),
                                    },
                                    tag.start,
                                );
                            }
                        }
                        if filters.is_empty() && tag.name == "FrontSide" {
                            if side == Side::Front {
                                error(ErrorKind::FrontSideOnFront, tag.start);
                            }
                            push(&mut stack, &mut top, Node::FrontSide);
                        } else {
                            push(
                                &mut stack,
                                &mut top,
                                Node::Field {
                                    name: tag.name.to_owned(),
                                    filters,
                                },
                            );
                        }
                    }
                    TagKind::Section | TagKind::Inverted => stack.push(Open {
                        name: tag.name.to_owned(),
                        inverted: tag.kind == TagKind::Inverted,
                        start: tag.start,
                        body: Vec::new(),
                    }),
                    TagKind::End => {
                        // An end tag closes the nearest open section of that name. Sections inside
                        // it that were never closed end here, and are reported.
                        if let Some(at) = stack.iter().rposition(|o| o.name == tag.name) {
                            while stack.len() > at + 1 {
                                let inner = &stack[stack.len() - 1];
                                error(
                                    ErrorKind::UnclosedSection {
                                        name: inner.name.clone(),
                                        inverted: inner.inverted,
                                    },
                                    inner.start,
                                );
                                close(&mut stack, &mut top);
                            }
                            close(&mut stack, &mut top);
                        } else {
                            error(
                                ErrorKind::StrayEnd {
                                    name: tag.name.to_owned(),
                                },
                                tag.start,
                            );
                        }
                    }
                }
            }
        }
    }
    while let Some(open) = stack.last() {
        error(
            ErrorKind::UnclosedSection {
                name: open.name.clone(),
                inverted: open.inverted,
            },
            open.start,
        );
        close(&mut stack, &mut top);
    }
    errors.sort_by_key(|e| (e.line, e.column));
    (Parsed { nodes: top }, errors)
}

fn push(stack: &mut [Open], top: &mut Vec<Node>, node: Node) {
    stack
        .last_mut()
        .map_or(top, |open| &mut open.body)
        .push(node);
}

/// Whether a field has nothing to show.
pub(crate) fn is_empty(value: Option<&str>) -> bool {
    value.is_none_or(|v| !has_content(v))
}

impl Parsed {
    /// The fields this template would show, in order, with their filters: those not inside a
    /// section that is hidden. A section is shown or hidden by whether its field is empty.
    pub(crate) fn visible_fields<'a, 'v>(
        &'a self,
        value: &dyn Fn(&str) -> Option<&'v str>,
    ) -> Vec<(&'a str, &'a [Filter])> {
        fn walk<'a, 'v>(
            nodes: &'a [Node],
            value: &dyn Fn(&str) -> Option<&'v str>,
            shown: &mut Vec<(&'a str, &'a [Filter])>,
        ) {
            for node in nodes {
                match node {
                    Node::Field { name, filters } => shown.push((name, filters)),
                    Node::Section {
                        name,
                        inverted,
                        body,
                    } => {
                        if is_empty(value(name)) == *inverted {
                            walk(body, value, shown);
                        }
                    }
                    Node::Text(_) | Node::FrontSide => {}
                }
            }
        }
        let mut shown = Vec::new();
        walk(&self.nodes, value, &mut shown);
        shown
    }

    /// Every field name this template reads, including section names, without repeats.
    pub(crate) fn field_names(&self) -> Vec<&str> {
        fn walk<'a>(nodes: &'a [Node], names: &mut Vec<&'a str>) {
            for node in nodes {
                match node {
                    Node::Field { name, .. } => {
                        if !names.contains(&name.as_str()) {
                            names.push(name);
                        }
                    }
                    Node::Section { name, body, .. } => {
                        if !names.contains(&name.as_str()) {
                            names.push(name);
                        }
                        walk(body, names);
                    }
                    Node::Text(_) | Node::FrontSide => {}
                }
            }
        }
        let mut names = Vec::new();
        walk(&self.nodes, &mut names);
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, side: Side) -> Vec<(ErrorKind, usize, usize)> {
        parse(text, side)
            .1
            .into_iter()
            .map(|e| (e.kind, e.line, e.column))
            .collect()
    }

    fn lookup(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<&'static str> {
        move |name| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }

    #[test]
    fn a_good_template_has_no_errors_and_a_tree() {
        let (parsed, errors) = parse(
            "a {{Front}} {{#X}}{{^Y}}{{hint:Z}}{{/Y}}{{/X}}",
            Side::Front,
        );
        assert!(errors.is_empty());
        assert_eq!(parsed.nodes.len(), 4);
        let Node::Section { name, body, .. } = &parsed.nodes[3] else {
            panic!("{parsed:?}");
        };
        assert_eq!(name, "X");
        assert!(matches!(&body[0], Node::Section { inverted: true, .. }));
    }

    #[test]
    fn every_kind_of_mistake_is_reported_where_it_is() {
        assert_eq!(
            kinds("a\n  {{Front", Side::Front),
            [(ErrorKind::UnclosedTag, 2, 3)]
        );
        assert_eq!(kinds("{{}}", Side::Front), [(ErrorKind::EmptyTag, 1, 1)]);
        assert_eq!(
            kinds("{{cloze:}}", Side::Front),
            [(ErrorKind::EmptyTag, 1, 1)]
        );
        assert_eq!(
            kinds("x {{#A}} y", Side::Front),
            [(
                ErrorKind::UnclosedSection {
                    name: "A".into(),
                    inverted: false
                },
                1,
                3
            )]
        );
        assert_eq!(
            kinds("{{/A}}", Side::Front),
            [(ErrorKind::StrayEnd { name: "A".into() }, 1, 1)]
        );
        assert_eq!(
            kinds("{{foo:Front}}", Side::Front),
            [(
                ErrorKind::UnknownFilter {
                    filter: "foo".into()
                },
                1,
                1
            )]
        );
        assert_eq!(
            kinds("{{FrontSide}}", Side::Front),
            [(ErrorKind::FrontSideOnFront, 1, 1)]
        );
        assert!(kinds("{{FrontSide}}", Side::Back).is_empty());
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        assert_eq!(
            kinds("żółć {{}}", Side::Front),
            [(ErrorKind::EmptyTag, 1, 6)]
        );
    }

    #[test]
    fn an_end_tag_closes_the_inner_sections_it_skips() {
        let errors = kinds("{{#A}}{{^B}}x{{/A}}", Side::Front);
        assert_eq!(
            errors,
            [(
                ErrorKind::UnclosedSection {
                    name: "B".into(),
                    inverted: true
                },
                1,
                7
            )]
        );
        let (parsed, _) = parse("{{#A}}{{^B}}x{{/A}}", Side::Front);
        assert_eq!(parsed.nodes.len(), 1);
    }

    #[test]
    fn a_malformed_template_is_still_read() {
        let v = lookup(&[("Front", "pies"), ("Empty", "")]);
        let shown = |text: &str| {
            let (parsed, _) = parse(text, Side::Front);
            parsed.visible_fields(&v).len()
        };
        assert_eq!(shown("{{#Empty}}{{Front}}"), 0);
        assert_eq!(shown("{{/Nothing}}{{Front}}"), 1);
        assert_eq!(shown("{{#Front}}{{Front}}"), 1);
        assert_eq!(shown("{{Front"), 0);
    }

    #[test]
    fn sections_decide_which_fields_are_visible() {
        let v = lookup(&[("Front", "pies"), ("Add", "y"), ("Empty", "<br>")]);
        let names = |text: &str| -> Vec<String> {
            let (parsed, _) = parse(text, Side::Front);
            parsed
                .visible_fields(&v)
                .into_iter()
                .map(|(n, _)| n.to_owned())
                .collect()
        };
        assert_eq!(names("{{#Add}}{{Front}}{{/Add}}"), ["Front"]);
        assert!(names("{{#Empty}}{{Front}}{{/Empty}}").is_empty());
        assert_eq!(names("{{^Empty}}{{Front}}{{/Empty}}"), ["Front"]);
        assert!(names("{{^Add}}{{Front}}{{/Add}}").is_empty());
        assert!(names("{{#Add}}{{#Empty}}{{Front}}{{/Empty}}{{/Add}}").is_empty());
        assert!(names("{{#Missing}}{{Front}}{{/Missing}}").is_empty());
        assert_eq!(names("{{^Missing}}{{Front}}{{/Missing}}"), ["Front"]);
    }

    #[test]
    fn field_names_include_sections_once() {
        let (parsed, _) = parse("{{A}}{{#B}}{{A}}{{cloze:C}}{{/B}}{{FrontSide}}", Side::Back);
        assert_eq!(parsed.field_names(), ["A", "B", "C"]);
    }
}
