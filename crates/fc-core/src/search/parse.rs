//! Reads a query into a tree. Nothing here looks at the collection, so the same text always gives
//! the same tree, and a screen can check what a person typed as they type.
//!
//! ```text
//! query   = or
//! or      = and ("or" and)*
//! and     = unary+                  (a space, or "and", between terms)
//! unary   = ("-" | "not") unary | "(" or ")" | term
//! term    = text | name ":" value   (either may be "quoted"; \" is a quote, \* a star)
//! ```
//!
//! An empty query is the empty `and`, which matches every card. `or`, `and` and `not` are words
//! only when written bare: `"or"` in quotes searches for the text.

use super::SearchError;

/// What the filters before the colon can be. Anything else before a colon is a field name.
pub(super) const FILTERS: [&str; 14] = [
    "deck",
    "deckonly",
    "tag",
    "note",
    "card",
    "is",
    "due",
    "added",
    "rated",
    "introduced",
    "difficulty",
    "stability",
    "lapses",
    "reviews",
];

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Node {
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Term(Term),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Term {
    /// Text anywhere in any field.
    Text(String),
    /// Text in the fields with this name. `text` is empty to find a field with nothing in it.
    Field {
        name: String,
        text: String,
    },
    /// A deck by its path, with what is inside it unless `subdecks` is off.
    Deck {
        pattern: String,
        subdecks: bool,
    },
    /// A tag, with the tags inside it.
    Tag(String),
    NoteType(String),
    /// A card template, by name or by number.
    Card(String),
    Is(Is),
    Due(Span),
    Added(Span),
    Rated {
        span: Span,
        rating: Option<u8>,
    },
    Introduced(Span),
    Number {
        measure: Measure,
        bounds: Bounds,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Is {
    New,
    Learning,
    Review,
    Due,
    Suspended,
    Buried,
}

/// A day: counted from today, or a date (a study day number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Day {
    Relative(i64),
    Date(i64),
}

/// Days from `lo` to `hi`, both included. A side that is `None` is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Span {
    pub lo: Option<Day>,
    pub hi: Option<Day>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Measure {
    Difficulty,
    Stability,
    Lapses,
    Reviews,
}

/// A range of numbers. Each side is the limit and whether the limit itself is included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Bounds {
    pub lo: Option<(f64, bool)>,
    pub hi: Option<(f64, bool)>,
}

pub(super) fn parse(query: &str) -> Result<Node, SearchError> {
    let tokens = lex(query)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        end: query.chars().count(),
    };
    let node = parser.or()?;
    match parser.tokens.get(parser.at) {
        None => Ok(node),
        Some(Token::Close(position)) => Err(syntax(
            *position,
            "There is a closing bracket \")\" with no opening bracket before it.",
        )),
        Some(other) => Err(syntax(other.position(), "Could not read the query here.")),
    }
}

/// The filter a misspelt name is nearest to, if it is close.
pub(super) fn suggest(name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    FILTERS
        .iter()
        .map(|filter| (distance(&name, filter), *filter))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, filter)| filter)
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            let next = (row[j + 1] + 1).min(row[j] + 1).min(previous + cost);
            previous = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

fn syntax(position: usize, problem: impl Into<String>) -> SearchError {
    SearchError::Syntax {
        position,
        problem: problem.into(),
    }
}

#[derive(Debug)]
enum Token {
    Open(usize),
    Close(usize),
    Minus(usize),
    Word(Word),
}

impl Token {
    fn position(&self) -> usize {
        match self {
            Self::Open(p) | Self::Close(p) | Self::Minus(p) => *p,
            Self::Word(word) => word.position,
        }
    }
}

#[derive(Debug)]
struct Word {
    position: usize,
    key: Option<String>,
    value: String,
    /// Whether any part of the value was in quotes.
    quoted: bool,
}

fn lex(query: &str) -> Result<Vec<Token>, SearchError> {
    let chars: Vec<char> = query.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            tokens.push(Token::Open(i));
            i += 1;
        } else if c == ')' {
            tokens.push(Token::Close(i));
            i += 1;
        } else if c == '-' && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) {
            tokens.push(Token::Minus(i));
            i += 1;
        } else {
            let (word, next) = lex_word(&chars, i)?;
            tokens.push(Token::Word(word));
            i = next;
        }
    }
    Ok(tokens)
}

fn lex_word(chars: &[char], start: usize) -> Result<(Word, usize), SearchError> {
    let mut buffer = String::new();
    let mut key = None;
    let mut quoted = false;
    let mut in_quote = false;
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        let escaped_quote = c == '\\' && chars.get(i + 1) == Some(&'"');
        if escaped_quote {
            buffer.push('"');
            i += 2;
        } else if in_quote {
            if c == '"' {
                in_quote = false;
            } else {
                buffer.push(c);
            }
            i += 1;
        } else if c.is_whitespace() || c == ')' {
            break;
        } else if c == '"' {
            in_quote = true;
            quoted = true;
            i += 1;
        } else if c == ':' && key.is_none() {
            key = Some(std::mem::take(&mut buffer));
            quoted = false;
            i += 1;
        } else {
            buffer.push(c);
            i += 1;
        }
    }
    if in_quote {
        return Err(syntax(
            start,
            "A quote \" is never closed. Add the closing quote.",
        ));
    }
    Ok((
        Word {
            position: start,
            key,
            value: buffer,
            quoted,
        },
        i,
    ))
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    /// The length of the query, for a problem at its end.
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    /// The bare word `or`, `and` or `not` (any case) at the cursor.
    fn is_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(w))
            if w.key.is_none() && !w.quoted && w.value.eq_ignore_ascii_case(keyword))
    }

    fn here(&self) -> usize {
        self.peek().map_or(self.end, Token::position)
    }

    fn or(&mut self) -> Result<Node, SearchError> {
        let mut branches = vec![self.and()?];
        while self.is_keyword("or") {
            self.at += 1;
            branches.push(self.and()?);
        }
        Ok(if branches.len() == 1 {
            branches.remove(0)
        } else {
            Node::Or(branches)
        })
    }

    fn and(&mut self) -> Result<Node, SearchError> {
        let mut items = Vec::new();
        // Where the last "and" is, until a search follows it.
        let mut dangling: Option<usize> = None;
        loop {
            if self.is_keyword("and") {
                if items.is_empty() || dangling.is_some() {
                    return Err(syntax(
                        self.here(),
                        "Put a search on both sides of \"and\".",
                    ));
                }
                dangling = Some(self.here());
                self.at += 1;
            } else if matches!(self.peek(), None | Some(Token::Close(_))) || self.is_keyword("or") {
                break;
            } else {
                items.push(self.unary()?);
                dangling = None;
            }
        }
        if let Some(position) = dangling {
            return Err(syntax(position, "Put a search on both sides of \"and\"."));
        }
        if items.is_empty() && (self.is_keyword("or") || self.after_keyword()) {
            return Err(syntax(self.here(), "Put a search on both sides of \"or\"."));
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Node::And(items)
        })
    }

    /// Whether the token before the cursor is the word `or`.
    fn after_keyword(&self) -> bool {
        matches!(self.tokens.get(self.at.wrapping_sub(1)), Some(Token::Word(w))
            if w.key.is_none() && !w.quoted && w.value.eq_ignore_ascii_case("or"))
    }

    fn unary(&mut self) -> Result<Node, SearchError> {
        if self.is_keyword("not") || matches!(self.peek(), Some(Token::Minus(_))) {
            self.at += 1;
            if matches!(self.peek(), None | Some(Token::Close(_))) {
                return Err(syntax(self.here(), "Nothing follows \"not\" or \"-\"."));
            }
            return Ok(Node::Not(Box::new(self.unary()?)));
        }
        match self.tokens.get(self.at) {
            Some(Token::Open(position)) => {
                let position = *position;
                self.at += 1;
                let inside = self.or()?;
                if matches!(self.peek(), Some(Token::Close(_))) {
                    self.at += 1;
                    Ok(inside)
                } else {
                    Err(syntax(
                        position,
                        "This opening bracket \"(\" is never closed. Add a \")\".",
                    ))
                }
            }
            Some(Token::Word(_)) => {
                let Some(Token::Word(word)) = self.tokens.get(self.at) else {
                    unreachable!("matched a word")
                };
                let term = build_term(word)?;
                self.at += 1;
                Ok(Node::Term(term))
            }
            _ => Err(syntax(self.here(), "Could not read the query here.")),
        }
    }
}

fn build_term(word: &Word) -> Result<Term, SearchError> {
    let at = word.position;
    let Some(key) = &word.key else {
        if word.value.is_empty() {
            return Err(syntax(
                at,
                "Empty quotes search for nothing. Put some text between them.",
            ));
        }
        return Ok(Term::Text(word.value.clone()));
    };
    if key.is_empty() {
        return Err(syntax(
            at,
            "There is a colon with no name before it. Write a filter such as deck:Polish, or put \
             the text in quotes.",
        ));
    }
    let value = word.value.as_str();
    let name = key.to_lowercase();
    let needs_value = |what: &str| {
        if value.is_empty() {
            Err(syntax(
                at,
                format!("\"{name}:\" needs {what} after the colon."),
            ))
        } else {
            Ok(())
        }
    };
    Ok(match name.as_str() {
        "deck" | "deckonly" => {
            needs_value("a deck")?;
            Term::Deck {
                pattern: value.to_owned(),
                subdecks: name == "deck",
            }
        }
        "tag" => {
            needs_value("a tag")?;
            Term::Tag(value.to_owned())
        }
        "note" => {
            needs_value("a note type")?;
            Term::NoteType(value.to_owned())
        }
        "card" => {
            needs_value("a template name or a number")?;
            Term::Card(value.to_owned())
        }
        "is" => Term::Is(parse_is(value, at)?),
        "due" => Term::Due(parse_span(value, false, "due", at)?),
        "added" => Term::Added(parse_span(value, true, "added", at)?),
        "introduced" => Term::Introduced(parse_span(value, true, "introduced", at)?),
        "rated" => {
            let (days, rating) = match value.split_once(':') {
                Some((days, rating)) => (days, Some(parse_rating(rating, at)?)),
                None => (value, None),
            };
            Term::Rated {
                span: parse_span(days, true, "rated", at)?,
                rating,
            }
        }
        "difficulty" => number(Measure::Difficulty, value, &name, at)?,
        "stability" => number(Measure::Stability, value, &name, at)?,
        "lapses" => number(Measure::Lapses, value, &name, at)?,
        "reviews" => number(Measure::Reviews, value, &name, at)?,
        _ => Term::Field {
            name: key.clone(),
            text: value.to_owned(),
        },
    })
}

fn parse_is(value: &str, at: usize) -> Result<Is, SearchError> {
    Ok(match value.to_lowercase().as_str() {
        "new" => Is::New,
        "learning" | "learn" => Is::Learning,
        "review" => Is::Review,
        "due" => Is::Due,
        "suspended" => Is::Suspended,
        "buried" => Is::Buried,
        _ => {
            return Err(syntax(
                at,
                format!(
                    "\"is:{value}\" is not a card state. Use is:new, is:learning, is:review, \
                     is:due, is:suspended or is:buried."
                ),
            ));
        }
    })
}

fn parse_rating(text: &str, at: usize) -> Result<u8, SearchError> {
    match text.to_lowercase().as_str() {
        "1" | "again" => Ok(1),
        "2" | "hard" => Ok(2),
        "3" | "good" => Ok(3),
        "4" | "easy" => Ok(4),
        _ => Err(syntax(
            at,
            format!(
                "\"{text}\" is not an answer. After the days use 1 to 4, or again, hard, good \
                 or easy, for example rated:7:1."
            ),
        )),
    }
}

fn parse_day(text: &str, at: usize) -> Result<Day, SearchError> {
    let wrong = || {
        syntax(
            at,
            format!(
                "\"{text}\" is not a day. Use a number of days from today (0 is today, -1 is \
                 yesterday) or a date such as 2026-09-30."
            ),
        )
    };
    if let Some(date) = parse_date(text) {
        return Ok(Day::Date(date.ok_or_else(wrong)?));
    }
    text.parse::<i64>()
        .ok()
        .filter(|n| n.abs() < 1_000_000)
        .map(Day::Relative)
        .ok_or_else(wrong)
}

/// `None` if `text` is not shaped like a date, `Some(None)` if it is but is not a real one.
fn parse_date(text: &str) -> Option<Option<i64>> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return None;
    };
    Some(days_from_civil(year, month, day))
}

/// Days from 1970-01-01 to a date of the Gregorian calendar, or `None` if there is no such date.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || day < 1 {
        return None;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let length = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > length {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y.rem_euclid(400);
    let shifted = (month + 9) % 12;
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

/// A day or a range. `past` filters (added, rated, introduced) read a single number as the last
/// that many days, and a due filter as that one day.
fn parse_span(value: &str, past: bool, name: &str, at: usize) -> Result<Span, SearchError> {
    if let Some((from, to)) = value.split_once("..") {
        let side = |text: &str| {
            if text.is_empty() {
                Ok(None)
            } else {
                parse_day(text, at).map(Some)
            }
        };
        let (lo, hi) = (side(from)?, side(to)?);
        if lo.is_none() && hi.is_none() {
            return Err(syntax(
                at,
                format!("\"{name}:..\" needs a day on at least one side, for example {name}:..0."),
            ));
        }
        return Ok(Span { lo, hi });
    }
    match parse_day(value, at)? {
        Day::Date(day) => Ok(Span {
            lo: Some(Day::Date(day)),
            hi: Some(Day::Date(day)),
        }),
        Day::Relative(n) if past => {
            if n < 1 {
                return Err(syntax(
                    at,
                    format!(
                        "\"{name}:{value}\" needs 1 or more days, for example {name}:7 for the \
                         last 7 days. For other days write a range such as {name}:-30..-8."
                    ),
                ));
            }
            Ok(Span {
                lo: Some(Day::Relative(1 - n)),
                hi: Some(Day::Relative(0)),
            })
        }
        Day::Relative(n) => Ok(Span {
            lo: Some(Day::Relative(n)),
            hi: Some(Day::Relative(n)),
        }),
    }
}

fn number(measure: Measure, value: &str, name: &str, at: usize) -> Result<Term, SearchError> {
    Ok(Term::Number {
        measure,
        bounds: parse_bounds(value, name, at)?,
    })
}

fn parse_bounds(value: &str, name: &str, at: usize) -> Result<Bounds, SearchError> {
    let wrong = |text: &str| {
        syntax(
            at,
            format!(
                "\"{text}\" is not a number for \"{name}:\". Use a number, a comparison such as \
                 >5 or <=3, or a range such as 2..6."
            ),
        )
    };
    let parse = |text: &str| -> Result<f64, SearchError> {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| wrong(text))
    };
    if let Some((from, to)) = value.split_once("..") {
        let side = |text: &str| -> Result<Option<(f64, bool)>, SearchError> {
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some((parse(text)?, true)))
            }
        };
        let (lo, hi) = (side(from)?, side(to)?);
        if lo.is_none() && hi.is_none() {
            return Err(wrong(value));
        }
        return Ok(Bounds { lo, hi });
    }
    for (prefix, lower, upper, inclusive) in [
        (">=", true, false, true),
        ("<=", false, true, true),
        (">", true, false, false),
        ("<", false, true, false),
        ("=", true, true, true),
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            let n = parse(rest)?;
            return Ok(Bounds {
                lo: lower.then_some((n, inclusive)),
                hi: upper.then_some((n, inclusive)),
            });
        }
    }
    let n = parse(value)?;
    Ok(Bounds {
        lo: Some((n, true)),
        hi: Some((n, true)),
    })
}

/// The study day number of a date, for the compiler. Public to the module for its tests.
#[cfg(test)]
pub(super) fn date_number(year: i64, month: i64, day: i64) -> Option<i64> {
    days_from_civil(year, month, day)
}
