//! Why a template is wrong. `Display` says where, what, and what to do.

use std::fmt;

use crate::collection::CollectionError;

/// The two sides of a card template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Front => "front",
            Self::Back => "back",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// A `{{` with no `}}` after it.
    UnclosedTag,
    /// `{{}}`, `{{#}}` or `{{cloze:}}`: no field name.
    EmptyTag,
    /// `{{#Name}}` or `{{^Name}}` that is never closed.
    UnclosedSection { name: String, inverted: bool },
    /// `{{/Name}}` with no section of that name open.
    StrayEnd { name: String },
    /// A word before a colon that is not a filter. Known filters are `cloze`, `text` and `hint`.
    UnknownFilter { filter: String },
    /// `{{FrontSide}}` on the front of a card.
    FrontSideOnFront,
}

/// One problem in one side of a template, with its place (counted from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    pub kind: ErrorKind,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}: ", self.line, self.column)?;
        match &self.kind {
            ErrorKind::UnclosedTag => f.write_str(
                "this \"{{\" has no matching \"}}\". Close the tag with \"}}\", or remove the braces.",
            ),
            ErrorKind::EmptyTag => f.write_str(
                "this tag has no field name. Put a field name between the braces, like {{Front}}.",
            ),
            ErrorKind::UnclosedSection { name, inverted } => write!(
                f,
                "the section {{{{{}{name}}}}} is never closed. Add {{{{/{name}}}}} where it ends.",
                if *inverted { '^' } else { '#' },
            ),
            ErrorKind::StrayEnd { name } => write!(
                f,
                "{{{{/{name}}}}} closes a section that was never opened. Remove it, or add \
                 {{{{#{name}}}}} before it."
            ),
            ErrorKind::UnknownFilter { filter } => write!(
                f,
                "\"{filter}:\" is not a filter. The filters are cloze, text and hint. If \
                 \"{filter}:\" is part of a field name, rename the field."
            ),
            ErrorKind::FrontSideOnFront => f.write_str(
                "{{FrontSide}} is for the back of a card. The front cannot include itself. Remove \
                 it from the front.",
            ),
        }
    }
}

impl std::error::Error for TemplateError {}

/// A template that cannot be used, with where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateProblem {
    /// The template's name.
    pub template: String,
    pub side: Side,
    /// At least one.
    pub errors: Vec<TemplateError>,
}

impl fmt::Display for TemplateProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The {} of the card template \"{}\" has a mistake: ",
            self.side, self.template
        )?;
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            error.fmt(f)?;
        }
        Ok(())
    }
}

/// Why a card could not be rendered.
#[derive(Debug, PartialEq, Eq)]
pub enum RenderError {
    /// No such card, or it or its note or template is deleted.
    NotFound,
    /// The card's template has a mistake. Nothing is shown for it until the template is fixed.
    Template(TemplateProblem),
    Collection(CollectionError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("That card no longer exists."),
            Self::Template(problem) => {
                problem.fmt(f)?;
                f.write_str(" Fix the template to see this card.")
            }
            Self::Collection(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<CollectionError> for RenderError {
    fn from(error: CollectionError) -> Self {
        Self::Collection(error)
    }
}

impl From<rusqlite::Error> for RenderError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Collection(error.into())
    }
}
