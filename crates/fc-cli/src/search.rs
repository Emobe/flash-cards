//! `fc search` and the saved-search commands: the search the browser screens will use, tried from a
//! terminal.

use fc_core::search::{Mode, SearchOptions, SearchRow, Sort};

use super::study::{date, open, state_name, time};
use super::{Failure, plural};

/// The options these commands take, taken out of the arguments they were given.
#[derive(Default)]
pub struct Options {
    pub sort: Option<Sort>,
    pub notes: bool,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub seed: Option<u64>,
}

impl Options {
    /// Takes `--sort`, `--notes`, `--limit`, `--offset` and `--seed` out of `args`. What is left
    /// over is returned.
    pub fn take(args: &[String]) -> Result<(Self, Vec<String>), Failure> {
        let mut options = Self::default();
        let mut rest = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .cloned()
                    .ok_or_else(|| Failure::Usage(format!("{name} needs a value.")))
            };
            let number = |name: &str, text: String| -> Result<u64, Failure> {
                text.parse()
                    .map_err(|_| Failure::Usage(format!("\"{text}\" is not a number for {name}.")))
            };
            match arg.as_str() {
                "--sort" => {
                    let text = value("--sort")?;
                    options.sort =
                        Some(Sort::parse(&text).map_err(|e| Failure::Usage(e.to_string()))?);
                }
                "--notes" => options.notes = true,
                "--limit" => options.limit = Some(number("--limit", value("--limit")?)? as u32),
                "--offset" => options.offset = Some(number("--offset", value("--offset")?)? as u32),
                "--seed" => options.seed = Some(number("--seed", value("--seed")?)?),
                _ => rest.push(arg.clone()),
            }
        }
        Ok((options, rest))
    }

    fn mode(&self) -> Mode {
        if self.notes { Mode::Notes } else { Mode::Cards }
    }
}

fn row_text(
    open: &super::study::Open,
    decks: &[(fc_core::id::Id, String)],
    row: &SearchRow,
) -> String {
    let deck = decks
        .iter()
        .find(|(id, _)| *id == row.deck)
        .map_or("?", |(_, path)| path.as_str());
    let mut line = format!(
        "{}  {:<10}  {deck}  {}",
        row.card,
        state_name(row.state),
        row.title
    );
    match (row.due_ms, row.due_day) {
        (Some(ms), _) => line.push_str(&format!("  due {}", time(ms, open.now.utc_offset_minutes))),
        (None, Some(day)) => line.push_str(&format!("  due {}", date(day))),
        (None, None) => {}
    }
    if row.suspended {
        line.push_str("  [suspended]");
    }
    if row.buried {
        line.push_str("  [buried]");
    }
    line
}

fn page_text(
    open: &super::study::Open,
    page: &fc_core::search::SearchPage,
    mode: Mode,
    offset: u32,
) -> Result<String, Failure> {
    let decks: Vec<_> = open
        .collection
        .decks()?
        .into_iter()
        .map(|d| (d.id, d.path))
        .collect();
    let noun = if mode == Mode::Notes { "note" } else { "card" };
    let mut text = format!("Found {}", plural(page.total as usize, noun));
    if !page.rows.is_empty() {
        text.push_str(&format!(
            ", showing {} to {}:",
            offset + 1,
            offset as usize + page.rows.len()
        ));
    }
    for row in &page.rows {
        text.push_str(&format!("\n  {}", row_text(open, &decks, row)));
    }
    Ok(text)
}

fn search_options(options: &Options) -> SearchOptions {
    let defaults = SearchOptions::default();
    SearchOptions {
        sort: options.sort.unwrap_or(defaults.sort),
        mode: options.mode(),
        offset: options.offset.unwrap_or(0),
        limit: options.limit.unwrap_or(defaults.limit),
        seed: options.seed.unwrap_or(0),
    }
}

pub fn search(file: &str, query: &str, options: &Options) -> Result<String, Failure> {
    let open = open(file)?;
    let wanted = search_options(options);
    let page = open.collection.search(query, &wanted)?;
    page_text(&open, &page, wanted.mode, wanted.offset)
}

pub fn list(file: &str) -> Result<String, Failure> {
    let open = open(file)?;
    let saved = open.collection.saved_searches()?;
    if saved.is_empty() {
        return Ok("No saved searches. Save one with `fc save-search`.".to_owned());
    }
    let mut text = format!("{}:", plural(saved.len(), "saved search"));
    for s in saved {
        text.push_str(&format!(
            "\n  {}  [{}, {}]  {}",
            s.name,
            s.mode.name(),
            s.sort,
            s.query
        ));
    }
    Ok(text)
}

fn find(open: &super::study::Open, name: &str) -> Result<fc_core::search::SavedSearch, Failure> {
    let wanted = name.to_lowercase();
    let saved = open.collection.saved_searches()?;
    match saved.iter().find(|s| s.name.to_lowercase() == wanted) {
        Some(found) => Ok(found.clone()),
        None => {
            let names: Vec<&str> = saved.iter().map(|s| s.name.as_str()).collect();
            Err(Failure::Core(format!(
                "No saved search called \"{name}\". {}",
                if names.is_empty() {
                    "There are none.".to_owned()
                } else {
                    format!("The saved searches are: {}.", names.join(", "))
                }
            )))
        }
    }
}

pub fn save(file: &str, name: &str, query: &str, options: &Options) -> Result<String, Failure> {
    let open = open(file)?;
    let defaults = SearchOptions::default();
    open.collection.save_search(
        name,
        query,
        options.sort.unwrap_or(defaults.sort),
        options.mode(),
    )?;
    Ok(format!("Saved \"{}\".", name.trim()))
}

pub fn delete(file: &str, name: &str) -> Result<String, Failure> {
    let open = open(file)?;
    let saved = find(&open, name)?;
    open.collection.delete_saved_search(saved.id)?;
    Ok(format!("Deleted \"{}\".", saved.name))
}

pub fn run_saved(file: &str, name: &str, options: &Options) -> Result<String, Failure> {
    let open = open(file)?;
    let saved = find(&open, name)?;
    let offset = options.offset.unwrap_or(0);
    let page = open.collection.run_saved_search(
        saved.id,
        offset,
        options.limit.unwrap_or(SearchOptions::default().limit),
    )?;
    page_text(&open, &page, saved.mode, offset)
}
