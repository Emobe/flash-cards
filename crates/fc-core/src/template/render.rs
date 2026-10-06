//! Turning a parsed template and a note's values into the HTML of a card.
//!
//! A field's value is inserted as it is written (it is HTML, ADR 0006 section 3) and is never read
//! as template syntax, so a note cannot add tags to a template. Nothing here makes HTML safe: the
//! output is only ever shown in the card frame (ADR 0005), which is the sandbox.

use super::cloze;
use super::error::{Side, TemplateProblem};
use super::lex::Filter;
use super::parse::{Node, Parsed, is_empty, parse};
use crate::html::{css_media_names, escape, expand_sound, has_content, media_names, plain_text};
use crate::notetype::{NoteType, Template};

/// A card ready for the card frame: the HTML of each side, and the media files the HTML names (to
/// be supplied to the frame, ADR 0005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedCard {
    /// A complete HTML document: the note type's CSS, and the front of the card.
    pub front: String,
    /// A complete HTML document: the note type's CSS, and the back of the card.
    pub back: String,
    /// Names of media files either side refers to, in order, without repeats: first the `src` of
    /// media tags and `[sound:]` (front, then back), then the CSS `url(...)` names of the note
    /// type's CSS. Web addresses and data URLs are not listed: the frame does not load them.
    pub media: Vec<String>,
}

struct Context<'a, 'v> {
    value: &'a dyn Fn(&str) -> Option<&'v str>,
    side: Side,
    ordinal: u32,
    /// The rendered front, for `{{FrontSide}}` on the back.
    front: &'a str,
}

fn apply(filter: &Filter, name: &str, text: String, ctx: &Context<'_, '_>) -> String {
    match filter {
        Filter::Cloze => cloze::render(&text, ctx.ordinal, ctx.side == Side::Back),
        Filter::Text => plain_text(&text),
        Filter::Hint if has_content(&text) => format!(
            "<details class=\"hint\"><summary>{}</summary>{text}</details>",
            escape(name)
        ),
        Filter::Hint => String::new(),
        Filter::Unknown(_) => text,
    }
}

fn write(nodes: &[Node], ctx: &Context<'_, '_>, out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Field { name, filters } => {
                let mut text = (ctx.value)(name).unwrap_or("").to_owned();
                // The filter next to the field applies first.
                for filter in filters.iter().rev() {
                    text = apply(filter, name, text, ctx);
                }
                out.push_str(&text);
            }
            Node::FrontSide => {
                if ctx.side == Side::Back {
                    out.push_str(ctx.front);
                }
            }
            Node::Section {
                name,
                inverted,
                body,
            } => {
                if is_empty((ctx.value)(name)) == *inverted {
                    write(body, ctx, out);
                }
            }
        }
    }
}

/// Where `</style` cannot appear in the CSS, so the CSS cannot end its own `<style>` element.
fn safe_css(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if tail
            .as_bytes()
            .get(..7)
            .is_some_and(|b| b.eq_ignore_ascii_case(b"</style"))
        {
            out.push_str("<\\/style");
            rest = &tail[7..];
        } else {
            out.push('<');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

fn document(css: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>{}</style></head>\
         <body class=\"card\">{body}</body></html>",
        safe_css(css)
    )
}

fn strict(text: &str, side: Side, template: &str) -> Result<Parsed, TemplateProblem> {
    let (parsed, errors) = parse(text, side);
    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(TemplateProblem {
            template: template.to_owned(),
            side,
            errors,
        })
    }
}

/// Renders one card of `template`. `value` reads a field of the note by name (`None` for a name
/// the note type does not have), and `ordinal` is the cloze number, or 0 for a standard template.
/// Fails if either side of the template has a mistake, saying where.
pub(crate) fn render<'v>(
    note_type: &NoteType,
    template: &Template,
    value: &dyn Fn(&str) -> Option<&'v str>,
    ordinal: u32,
) -> Result<RenderedCard, TemplateProblem> {
    let front = strict(&template.front, Side::Front, &template.name)?;
    let back = strict(&template.back, Side::Back, &template.name)?;
    let mut front_body = String::new();
    write(
        &front.nodes,
        &Context {
            value,
            side: Side::Front,
            ordinal,
            front: "",
        },
        &mut front_body,
    );
    let mut back_body = String::new();
    write(
        &back.nodes,
        &Context {
            value,
            side: Side::Back,
            ordinal,
            front: &front_body,
        },
        &mut back_body,
    );
    let front_html = document(&note_type.css, &expand_sound(&front_body));
    let back_html = document(&note_type.css, &expand_sound(&back_body));
    let mut media = media_names(&front_html);
    for name in media_names(&back_html)
        .into_iter()
        .chain(css_media_names(&note_type.css))
    {
        if !media.contains(&name) {
            media.push(name);
        }
    }
    Ok(RenderedCard {
        front: front_html,
        back: back_html,
        media,
    })
}
