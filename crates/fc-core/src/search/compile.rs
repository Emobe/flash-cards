//! Turns a parsed query into one SQL `WHERE` clause. Names (decks, note types, fields, templates)
//! are looked up here, in Rust, with the same matching as text, and reach SQL as lists of IDs. The
//! clause works on `card c JOIN note n LEFT JOIN card_schedule s`, with every value a parameter.

use std::collections::HashSet;

use rusqlite::types::Value;

use super::SearchError;
use super::parse::{Bounds, Day, Is, Measure, Node, Span, Term, suggest};
use super::pattern::{Pattern, fold};
use crate::deck::{SEPARATOR, Tree, default_deck};
use crate::id::Id;
use crate::notetype::{Kind, NoteType};

/// What a query needs to know about the collection and the moment.
pub(super) struct Context<'a> {
    pub today: i64,
    pub now_ms: i64,
    pub utc_offset_minutes: i32,
    pub day_start_hour: u8,
    /// Loaded only when something needs the decks.
    pub tree: Option<&'a Tree>,
    pub types: &'a [NoteType],
}

impl Context<'_> {
    /// The first moment of a study day, in Unix milliseconds.
    pub fn day_start_ms(&self, day: i64) -> i64 {
        day * 86_400_000 - i64::from(self.utc_offset_minutes) * 60_000
            + i64::from(self.day_start_hour) * 3_600_000
    }

    fn absolute(&self, day: Day) -> i64 {
        match day {
            Day::Relative(n) => self.today.saturating_add(n),
            Day::Date(day) => day,
        }
    }

    /// The days of a span, from today.
    fn days(&self, span: Span) -> (Option<i64>, Option<i64>) {
        (
            span.lo.map(|d| self.absolute(d)),
            span.hi.map(|d| self.absolute(d)),
        )
    }
}

/// The values of the statement being built. `arg` returns the placeholder to put in the SQL.
#[derive(Default)]
pub(super) struct Params(pub Vec<Value>);

impl Params {
    pub fn arg(&mut self, value: impl Into<Value>) -> String {
        self.0.push(value.into());
        format!("?{}", self.0.len())
    }

    fn ids(&mut self, column: &str, ids: &[Id]) -> String {
        if ids.is_empty() {
            return "0".to_owned();
        }
        let marks: Vec<String> = ids
            .iter()
            .map(|id| self.arg(Value::Blob(id.as_bytes().to_vec())))
            .collect();
        format!("{column} IN ({})", marks.join(", "))
    }
}

/// Whether a query needs the deck tree.
pub(super) fn uses_decks(node: &Node) -> bool {
    match node {
        Node::And(items) | Node::Or(items) => items.iter().any(uses_decks),
        Node::Not(inner) => uses_decks(inner),
        Node::Term(term) => matches!(term, Term::Deck { .. }),
    }
}

pub(super) fn compile(
    node: &Node,
    ctx: &Context<'_>,
    params: &mut Params,
) -> Result<String, SearchError> {
    Ok(match node {
        Node::And(items) if items.is_empty() => "1".to_owned(),
        Node::And(items) => join(items, " AND ", ctx, params)?,
        Node::Or(items) => join(items, " OR ", ctx, params)?,
        Node::Not(inner) => format!("NOT {}", compile(inner, ctx, params)?),
        Node::Term(term) => term_sql(term, ctx, params)?,
    })
}

fn join(
    items: &[Node],
    glue: &str,
    ctx: &Context<'_>,
    params: &mut Params,
) -> Result<String, SearchError> {
    let parts: Result<Vec<String>, SearchError> = items
        .iter()
        .map(|item| compile(item, ctx, params))
        .collect();
    Ok(format!("({})", parts?.join(glue)))
}

/// A bare `NOT` of a clause that can be NULL would drop the row either way, so every clause here
/// is a plain true or false.
fn term_sql(term: &Term, ctx: &Context<'_>, params: &mut Params) -> Result<String, SearchError> {
    Ok(match term {
        Term::Text(text) => {
            let pattern = params.arg(text.clone());
            format!(
                "EXISTS (SELECT 1 FROM note_field_value v WHERE v.note = c.note \
                 AND v.field IN (SELECT id FROM note_type_field WHERE deleted = 0) \
                 AND fc_contains(v.value, {pattern}))"
            )
        }
        Term::Field { name, text } => field_sql(name, text, ctx, params)?,
        Term::Deck { pattern, subdecks } => deck_sql(pattern, *subdecks, ctx, params),
        Term::Tag(pattern) => {
            let pattern = params.arg(pattern.clone());
            format!(
                "EXISTS (SELECT 1 FROM note_tag t WHERE t.note = c.note AND t.present = '1' \
                 AND fc_has(t.tag, {pattern}))"
            )
        }
        Term::NoteType(pattern) => {
            let pattern = Pattern::new(pattern);
            let ids: Vec<Id> = ctx
                .types
                .iter()
                .filter(|t| pattern.is_inside(&fold(&t.name)))
                .map(|t| t.id)
                .collect();
            params.ids("n.note_type", &ids)
        }
        Term::Card(text) => card_sql(text, ctx, params),
        Term::Is(is) => is_sql(*is, ctx, params),
        Term::Due(span) => due_sql(*span, ctx, params),
        Term::Added(span) => {
            let (lo, hi) = ctx.days(*span);
            let time = "fc_id_ms(n.id)";
            format!("COALESCE({}, 0)", time_range(time, lo, hi, ctx, params))
        }
        Term::Rated { span, rating } => event_sql(*span, *rating, false, ctx, params),
        Term::Introduced(span) => event_sql(*span, None, true, ctx, params),
        Term::Number { measure, bounds } => {
            let column = match measure {
                Measure::Difficulty => "s.difficulty",
                Measure::Stability => "s.stability",
                Measure::Lapses => "COALESCE(s.lapses, 0)",
                Measure::Reviews => "COALESCE(s.answers, 0)",
            };
            format!("COALESCE({}, 0)", bounds_sql(column, *bounds, params))
        }
    })
}

/// `column` between two moments, as the SQL for "at or after the start of `lo`, before the end of
/// `hi`". An open side is left out. Empty when both are.
fn time_range(
    column: &str,
    lo: Option<i64>,
    hi: Option<i64>,
    ctx: &Context<'_>,
    params: &mut Params,
) -> String {
    let mut parts = Vec::new();
    if let Some(lo) = lo {
        parts.push(format!("{column} >= {}", params.arg(ctx.day_start_ms(lo))));
    }
    if let Some(hi) = hi {
        parts.push(format!(
            "{column} < {}",
            params.arg(ctx.day_start_ms(hi.saturating_add(1)))
        ));
    }
    if parts.is_empty() {
        "1".to_owned()
    } else {
        format!("({})", parts.join(" AND "))
    }
}

fn bounds_sql(column: &str, bounds: Bounds, params: &mut Params) -> String {
    let mut parts = Vec::new();
    if let Some((n, inclusive)) = bounds.lo {
        let op = if inclusive { ">=" } else { ">" };
        parts.push(format!("{column} {op} {}", params.arg(n)));
    }
    if let Some((n, inclusive)) = bounds.hi {
        let op = if inclusive { "<=" } else { "<" };
        parts.push(format!("{column} {op} {}", params.arg(n)));
    }
    format!("({})", parts.join(" AND "))
}

fn field_sql(
    name: &str,
    text: &str,
    ctx: &Context<'_>,
    params: &mut Params,
) -> Result<String, SearchError> {
    let pattern = Pattern::new(name);
    let mut fields = Vec::new();
    let mut types = Vec::new();
    for note_type in ctx.types {
        let found: Vec<Id> = note_type
            .fields
            .iter()
            .filter(|f| pattern.is_all_of(&fold(&f.name)))
            .map(|f| f.id)
            .collect();
        if !found.is_empty() {
            types.push(note_type.id);
            fields.extend(found);
        }
    }
    if fields.is_empty() {
        return Err(SearchError::UnknownField {
            name: name.to_owned(),
            suggestion: suggest(name),
        });
    }
    let in_fields = params.ids("v.field", &fields);
    if text.is_empty() {
        let in_types = params.ids("n.note_type", &types);
        return Ok(format!(
            "({in_types} AND NOT EXISTS (SELECT 1 FROM note_field_value v \
             WHERE v.note = c.note AND {in_fields} AND fc_fold(v.value) <> ''))"
        ));
    }
    let text = params.arg(text.to_owned());
    Ok(format!(
        "EXISTS (SELECT 1 FROM note_field_value v WHERE v.note = c.note AND {in_fields} \
         AND fc_contains(v.value, {text}))"
    ))
}

fn deck_sql(pattern: &str, subdecks: bool, ctx: &Context<'_>, params: &mut Params) -> String {
    let Some(tree) = ctx.tree else {
        return "0".to_owned();
    };
    // `deckonly:Polish` means a deck called Polish and not what is inside it, so with no `::` in
    // the pattern it looks at the deck's own name. `deck:` and a pattern with `::` look at the path.
    let by_path = subdecks || pattern.contains(SEPARATOR);
    let pattern = Pattern::new(pattern);
    let mut wanted: Vec<Id> = Vec::new();
    let mut seen = HashSet::new();
    for row in tree.rows.iter().filter(|r| !r.deleted) {
        let name = if by_path {
            &row.path
        } else {
            &row.display_name
        };
        if !pattern.is_inside(&fold(name)) {
            continue;
        }
        let rows = if subdecks {
            tree.subtree(row.id)
        } else {
            std::slice::from_ref(row)
        };
        for inside in rows.iter().filter(|r| !r.deleted) {
            if seen.insert(inside.id) {
                wanted.push(inside.id);
            }
        }
    }
    let list = params.ids("c.deck", &wanted);
    if seen.contains(&default_deck()) {
        // A card with no deck, or one that is not an ID, is in the Default deck.
        format!("({list} OR length(c.deck) <> 16)")
    } else {
        list
    }
}

/// `card:3` is the third template (or cloze number 3), and anything else is part of a template name.
fn card_sql(text: &str, ctx: &Context<'_>, params: &mut Params) -> String {
    let pattern = Pattern::new(text);
    let number: Option<usize> = text.parse().ok().filter(|n| *n >= 1);
    let mut clauses = Vec::new();
    for note_type in ctx.types {
        let named: Vec<Id> = match number {
            Some(_) => Vec::new(),
            None => note_type
                .templates
                .iter()
                .filter(|t| pattern.is_inside(&fold(&t.name)))
                .map(|t| t.id)
                .collect(),
        };
        if !named.is_empty() {
            clauses.push(params.ids("c.template", &named));
        }
        let Some(number) = number else { continue };
        match note_type.kind {
            Kind::Standard => {
                if let Some(template) = note_type.templates.get(number - 1) {
                    clauses.push(params.ids("c.template", &[template.id]));
                }
            }
            Kind::Cloze => {
                let templates: Vec<Id> = note_type.templates.iter().map(|t| t.id).collect();
                let in_templates = params.ids("c.template", &templates);
                let ordinal = params.arg(number as i64);
                clauses.push(format!("({in_templates} AND c.ordinal = {ordinal})"));
            }
        }
    }
    if clauses.is_empty() {
        "0".to_owned()
    } else {
        format!("({})", clauses.join(" OR "))
    }
}

fn is_sql(is: Is, ctx: &Context<'_>, params: &mut Params) -> String {
    match is {
        Is::New => "COALESCE(s.state, 0) = 0".to_owned(),
        Is::Learning => "COALESCE(s.state, 0) IN (1, 3)".to_owned(),
        Is::Review => "COALESCE(s.state, 0) = 2".to_owned(),
        Is::Due => {
            let today = params.arg(ctx.today);
            let now = params.arg(ctx.now_ms);
            let today_again = params.arg(ctx.today);
            format!(
                "COALESCE(((s.state = 2 AND s.due_day <= {today}) \
                 OR (s.state IN (1, 3) AND s.due_ms <= {now})) \
                 AND c.suspended = 0 AND c.buried_until < {today_again}, 0)"
            )
        }
        Is::Suspended => "c.suspended <> 0".to_owned(),
        Is::Buried => format!("c.buried_until >= {}", params.arg(ctx.today)),
    }
}

/// Review cards by their due day, learning cards by the day their due time falls in.
fn due_sql(span: Span, ctx: &Context<'_>, params: &mut Params) -> String {
    let (lo, hi) = ctx.days(span);
    let mut review = vec!["s.state = 2".to_owned()];
    if let Some(lo) = lo {
        review.push(format!("s.due_day >= {}", params.arg(lo)));
    }
    if let Some(hi) = hi {
        review.push(format!("s.due_day <= {}", params.arg(hi)));
    }
    let learning = time_range("s.due_ms", lo, hi, ctx, params);
    format!(
        "COALESCE(({}) OR (s.state IN (1, 3) AND {learning}), 0)",
        review.join(" AND ")
    )
}

/// Answers that were not undone: `rated` in a span of days (optionally one answer), `introduced`
/// when the answer was the card's first.
fn event_sql(
    span: Span,
    rating: Option<u8>,
    first_only: bool,
    ctx: &Context<'_>,
    params: &mut Params,
) -> String {
    let (lo, hi) = ctx.days(span);
    let mut parts = vec!["e.card = c.id".to_owned(), "e.kind = 'review'".to_owned()];
    if let Some(lo) = lo {
        parts.push(format!("e.day >= {}", params.arg(lo)));
    }
    if let Some(hi) = hi {
        parts.push(format!("e.day <= {}", params.arg(hi)));
    }
    if let Some(rating) = rating {
        parts.push(format!("e.rating = {}", params.arg(i64::from(rating))));
    }
    if first_only {
        parts.push("e.state_before = 0".to_owned());
    }
    parts.push(
        "NOT EXISTS (SELECT 1 FROM card_event v WHERE v.kind = 'void' AND v.target = e.id)"
            .to_owned(),
    );
    format!(
        "EXISTS (SELECT 1 FROM card_event e WHERE {})",
        parts.join(" AND ")
    )
}
