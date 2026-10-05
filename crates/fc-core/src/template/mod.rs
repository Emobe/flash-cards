//! The template language (ADR 0006, build notes for step 1.4): how a card template turns a note
//! into the HTML of a card.
//!
//! A template has a front and a back. Text is HTML and is kept as written. Tags are in `{{...}}`:
//!
//! - `{{Field}}` shows a field. `{{filter:Field}}` changes how, and filters chain
//!   (`{{hint:text:Field}}`, the one next to the field first). The filters are `cloze`, `text`
//!   (plain text) and `hint` (a collapsed block with the field's name as its label).
//! - `{{#Field}}...{{/Field}}` shows its content when the field has some, `{{^Field}}...{{/Field}}`
//!   when it has none. Sections nest.
//! - `{{FrontSide}}` is the rendered front, and is for the back only.
//!
//! Field names are matched exactly, spaces included. A field the note type does not have reads as
//! empty, as a removed field does. A field's value is never read as template syntax.
//!
//! Three layers: `lex` cuts text into tags and never fails, `parse` builds a tree and reports every
//! mistake while still returning a usable tree, and `render` writes HTML. Card generation and
//! renaming use the lenient reading, so a merge that produces a bad template never breaks a note.
//! Saving a template and rendering a card use the errors.

mod cloze;
mod error;
mod lex;
mod parse;
mod render;
#[cfg(test)]
mod tests;

pub use error::{ErrorKind, RenderError, Side, TemplateError, TemplateProblem};
pub use render::RenderedCard;

pub(crate) use parse::{Parsed, is_empty};
pub(crate) use render::render;

use lex::{Filter, Token, lex};

/// The mistakes in one side of a template, in order. Empty if it is well formed.
pub fn check(text: &str, side: Side) -> Vec<TemplateError> {
    parse::parse(text, side).1
}

/// The field names a template reads that are not in `known`, in order. A template that names a
/// field the note type does not have still works (the field reads as empty), so a screen can show
/// this as a warning.
pub fn unknown_fields(text: &str, known: &[&str]) -> Vec<String> {
    let (parsed, _) = parse::parse(text, Side::Back);
    parsed
        .field_names()
        .into_iter()
        .filter(|name| !known.contains(name))
        .map(str::to_owned)
        .collect()
}

/// `text` with every tag that names the field `old` changed to name `new`. Filters and the spacing
/// inside the braces stay as written. Works on a template that has mistakes.
pub(crate) fn rename_field(text: &str, old: &str, new: &str) -> String {
    let mut result = text.to_owned();
    for token in lex(text).into_iter().rev() {
        if let Token::Tag(tag) = token
            && tag.name == old
        {
            result.replace_range(tag.name_range, new);
        }
    }
    result
}

/// Reads the front of a template the lenient way, for card generation.
pub(crate) fn parse_front(text: &str) -> Parsed {
    parse::parse(text, Side::Front).0
}

/// Whether the front of a standard template would show a field that has something in it.
pub(crate) fn front_shows_content<'v>(
    front: &Parsed,
    value: &dyn Fn(&str) -> Option<&'v str>,
) -> bool {
    front
        .visible_fields(value)
        .iter()
        .any(|(name, _)| !is_empty(value(name)))
}

/// The cloze numbers a cloze front asks for: those in the fields it shows through the `cloze`
/// filter, sorted and without repeats.
pub(crate) fn cloze_numbers<'v>(
    front: &Parsed,
    value: &dyn Fn(&str) -> Option<&'v str>,
) -> Vec<u32> {
    let mut numbers = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (name, filters) in front.visible_fields(value) {
        if filters.contains(&Filter::Cloze) && !seen.contains(&name) {
            seen.push(name);
            if let Some(text) = value(name) {
                numbers.extend(cloze::numbers(text));
            }
        }
    }
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}
