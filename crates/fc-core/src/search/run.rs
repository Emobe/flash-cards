//! Running a search: the statement, the sort, and the rows of a page.

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, params_from_iter};

use super::compile::{Context, Params, compile, uses_decks};
use super::parse::parse;
use super::{MAX_LIMIT, Mode, SearchError, SearchOptions, SearchPage, SearchRow, SortKey};
use crate::collection::{Collection, CollectionError};
use crate::deck::{Tree, card_deck};
use crate::id::Id;
use crate::notetype::NoteType;
use crate::scheduling::CardState;

const FROM: &str = "FROM card c JOIN note n ON n.id = c.note LEFT JOIN card_schedule s ON s.card = c.id \
     WHERE c.deleted = 0 AND n.deleted = 0";

fn storage(error: impl std::fmt::Display) -> SearchError {
    SearchError::Collection(CollectionError::Storage(error.to_string()))
}

impl Collection {
    /// Runs a query. Cards are the live cards of live notes. See the module doc for the query
    /// language.
    pub fn search(&self, query: &str, options: &SearchOptions) -> Result<SearchPage, SearchError> {
        let node = parse(query)?;
        let types = self.note_types().map_err(storage)?;
        let tree = if uses_decks(&node) || options.sort.key == SortKey::Deck {
            Some(Tree::load(&self.conn)?)
        } else {
            None
        };
        let now = self.host.clock.now();
        let ctx = Context {
            today: self.today().map_err(storage)?,
            now_ms: now.unix_ms,
            utc_offset_minutes: now.utc_offset_minutes,
            day_start_hour: self.day_start_hour().map_err(storage)?,
            tree: tree.as_ref(),
            types: &types,
        };
        let mut params = Params::default();
        let filter = compile(&node, &ctx, &mut params)?;
        let count_sql = match options.mode {
            Mode::Cards => format!("SELECT COUNT(*) {FROM} AND {filter}"),
            Mode::Notes => format!("SELECT COUNT(DISTINCT c.note) {FROM} AND {filter}"),
        };
        let total: i64 =
            self.conn
                .query_row(&count_sql, params_from_iter(params.0.iter()), |row| {
                    row.get(0)
                })?;
        let limit = options.limit.min(MAX_LIMIT);
        if limit == 0 {
            return Ok(SearchPage {
                total: total as u64,
                rows: Vec::new(),
            });
        }
        let key = sort_expression(options, &ctx, &mut params);
        let direction = if options.sort.descending {
            "DESC"
        } else {
            "ASC"
        };
        let limit_arg = params.arg(i64::from(limit));
        let offset_arg = params.arg(i64::from(options.offset));
        let sql = match options.mode {
            Mode::Cards => format!(
                "SELECT c.id FROM (SELECT c.id AS id, {key} AS k {FROM} AND {filter}) c \
                 ORDER BY c.k IS NULL, c.k {direction}, c.id LIMIT {limit_arg} OFFSET {offset_arg}"
            ),
            Mode::Notes => format!(
                "SELECT card FROM (SELECT MIN(c.id) AS card, c.note AS note, {key} AS k \
                 {FROM} AND {filter} GROUP BY c.note) \
                 ORDER BY k IS NULL, k {direction}, note LIMIT {limit_arg} OFFSET {offset_arg}"
            ),
        };
        let mut statement = self.conn.prepare(&sql)?;
        let cards: Vec<Id> = statement
            .query_map(params_from_iter(params.0.iter()), |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let rows = self.rows(&cards, &types)?;
        Ok(SearchPage {
            total: total as u64,
            rows,
        })
    }

    fn rows(&self, cards: &[Id], types: &[NoteType]) -> Result<Vec<SearchRow>, SearchError> {
        let mut statement = self.conn.prepare(
            "SELECT c.note, n.note_type, c.deck, c.suspended, c.buried_until,
                    COALESCE(s.state, 0), s.due_day, s.due_ms
             FROM card c JOIN note n ON n.id = c.note
             LEFT JOIN card_schedule s ON s.card = c.id WHERE c.id = ?1",
        )?;
        let mut title = self
            .conn
            .prepare("SELECT value FROM note_field_value WHERE note = ?1 AND field = ?2")?;
        let today = self.today().map_err(storage)?;
        let mut rows = Vec::with_capacity(cards.len());
        for card in cards {
            let mut row = statement.query_row([card], |row| {
                Ok(SearchRow {
                    card: *card,
                    note: row.get(0)?,
                    note_type: row.get(1)?,
                    deck: card_deck(&row.get::<_, Vec<u8>>(2)?),
                    title: String::new(),
                    state: CardState::from_code(row.get(5)?).unwrap_or(CardState::New),
                    due_day: row.get(6)?,
                    due_ms: row.get(7)?,
                    suspended: row.get::<_, i64>(3)? != 0,
                    buried: row.get::<_, i64>(4)? >= today,
                })
            })?;
            let sort_field = types
                .iter()
                .find(|t| t.id == row.note_type)
                .and_then(|t| t.sort_field);
            if let Some(field) = sort_field {
                let value: Option<String> = title
                    .query_row(rusqlite::params![row.note, field], |r| r.get(0))
                    .optional()?;
                row.title = crate::html::readable(&value.unwrap_or_default());
            }
            rows.push(row);
        }
        Ok(rows)
    }
}

/// `CASE column WHEN id THEN value ... END`, for ranking by something the database cannot order
/// itself (the deck list's order, a name's order, which field is the sort field).
fn case(column: &str, pairs: Vec<(Id, Value)>, otherwise: Value, params: &mut Params) -> String {
    let mut text = format!("CASE {column}");
    for (id, value) in pairs {
        let id = params.arg(Value::Blob(id.as_bytes().to_vec()));
        let value = params.arg(value);
        text.push_str(&format!(" WHEN {id} THEN {value}"));
    }
    let otherwise = params.arg(otherwise);
    text.push_str(&format!(" ELSE {otherwise} END"));
    text
}

/// The SQL for the sort key. In notes mode it is an aggregate over the cards of the note.
fn sort_expression(options: &SearchOptions, ctx: &Context<'_>, params: &mut Params) -> String {
    let notes = options.mode == Mode::Notes;
    let due = |params: &mut Params| {
        let shift = params.arg(
            i64::from(ctx.day_start_hour) * 3_600_000 - i64::from(ctx.utc_offset_minutes) * 60_000,
        );
        format!(
            "CASE WHEN s.state = 2 THEN s.due_day * 86400000 + {shift} \
             WHEN s.state IN (1, 3) THEN s.due_ms END"
        )
    };
    let aggregate = |function: &str, expression: String| {
        if notes {
            format!("{function}({expression})")
        } else {
            expression
        }
    };
    match options.sort.key {
        SortKey::Created => "n.id".to_owned(),
        SortKey::Due => aggregate("MIN", due(params)),
        SortKey::SortField => {
            let pairs = ctx
                .types
                .iter()
                .filter_map(|t| {
                    t.sort_field
                        .map(|f| (t.id, Value::Blob(f.as_bytes().to_vec())))
                })
                .collect();
            let field = case("n.note_type", pairs, Value::Null, params);
            format!(
                "(SELECT fc_fold(v.value) FROM note_field_value v \
                 WHERE v.note = c.note AND v.field = {field})"
            )
        }
        SortKey::Deck => {
            let tree = ctx.tree.expect("loaded for a deck sort");
            let pairs = tree
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| (r.id, Value::Integer(i as i64)))
                .collect();
            let default = tree
                .position(crate::deck::default_deck())
                .map_or(0, |p| p as i64);
            let rank = format!(
                "CASE WHEN length(c.deck) <> 16 THEN {} ELSE {} END",
                params.arg(default),
                case("c.deck", pairs, Value::Integer(default), params)
            );
            aggregate("MIN", rank)
        }
        SortKey::NoteType => {
            let mut names: Vec<(&NoteType, String)> = ctx
                .types
                .iter()
                .map(|t| (t, super::pattern::fold(&t.name)))
                .collect();
            names.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.id.cmp(&b.0.id)));
            let pairs = names
                .iter()
                .enumerate()
                .map(|(i, (t, _))| (t.id, Value::Integer(i as i64)))
                .collect();
            case("n.note_type", pairs, Value::Integer(-1), params)
        }
        SortKey::Difficulty => aggregate("MAX", "s.difficulty".to_owned()),
        SortKey::Stability => aggregate("MIN", "s.stability".to_owned()),
        SortKey::Lapses => aggregate("MAX", "COALESCE(s.lapses, 0)".to_owned()),
        SortKey::Reviews => aggregate("SUM", "COALESCE(s.answers, 0)".to_owned()),
        SortKey::Random => {
            let seed = params.arg(options.seed as i64);
            format!("fc_random(n.id, {seed})")
        }
    }
}
