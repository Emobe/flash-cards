//! Saved searches: a name, a query, how to sort it and whether it lists cards or notes. A synced
//! table with one register for each, so two devices that edit different parts of one saved search
//! both keep their change (ADR 0006, section 3). Deleting sets the `deleted` register.

use rusqlite::types::Value;

use super::{Mode, SAVED_SEARCH, SearchError, SearchOptions, SearchPage, Sort, parse};
use crate::collection::Collection;
use crate::id::Id;

/// A saved search as it reads now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSearch {
    pub id: Id,
    pub name: String,
    pub query: String,
    pub sort: Sort,
    pub mode: Mode,
}

/// What to change in a saved search. A field left as `None` stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedSearchChange {
    pub name: Option<String>,
    pub query: Option<String>,
    pub sort: Option<Sort>,
    pub mode: Option<Mode>,
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn clean_name(name: &str) -> Result<String, SearchError> {
    let name = name.trim();
    if name.is_empty() {
        Err(SearchError::EmptyName)
    } else {
        Ok(name.to_owned())
    }
}

impl Collection {
    fn saved_rows(&self) -> Result<Vec<SavedSearch>, SearchError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, query, sort, mode FROM saved_search WHERE deleted = 0")?;
        let mut found: Vec<SavedSearch> = statement
            .query_map([], |row| {
                let sort: String = row.get(3)?;
                let mode: String = row.get(4)?;
                Ok(SavedSearch {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    query: row.get(2)?,
                    // A value from a newer app reads as the default, so reading never fails.
                    sort: Sort::parse(&sort).unwrap_or_default(),
                    mode: Mode::parse(&mode).unwrap_or_default(),
                })
            })?
            .collect::<Result<_, _>>()?;
        found.sort_by_cached_key(|s| (s.name.to_lowercase(), s.id));
        Ok(found)
    }

    /// The saved searches, by name (ignoring case).
    pub fn saved_searches(&self) -> Result<Vec<SavedSearch>, SearchError> {
        self.saved_rows()
    }

    pub fn saved_search(&self, id: Id) -> Result<Option<SavedSearch>, SearchError> {
        Ok(self.saved_rows()?.into_iter().find(|s| s.id == id))
    }

    fn check_name_free(&self, name: &str, except: Option<Id>) -> Result<(), SearchError> {
        let wanted = name.to_lowercase();
        if self
            .saved_rows()?
            .iter()
            .any(|s| Some(s.id) != except && s.name.to_lowercase() == wanted)
        {
            Err(SearchError::NameTaken(name.to_owned()))
        } else {
            Ok(())
        }
    }

    /// Saves a search. The query must be one that can be read (it is not run), and the name must
    /// not be another saved search's.
    pub fn save_search(
        &self,
        name: &str,
        query: &str,
        sort: Sort,
        mode: Mode,
    ) -> Result<Id, SearchError> {
        let name = clean_name(name)?;
        parse::parse(query)?;
        self.check_name_free(&name, None)?;
        Ok(self.write(|w| {
            let id = w.new_id()?;
            w.insert(
                SAVED_SEARCH.entity,
                id,
                vec![
                    ("name", text(&name)),
                    ("query", text(query)),
                    ("sort", text(&sort.to_string())),
                    ("mode", text(mode.name())),
                    ("deleted", Value::Integer(0)),
                ],
            )?;
            Ok(id)
        })?)
    }

    /// Changes the parts of a saved search that differ. Writes nothing for the others.
    pub fn update_saved_search(
        &self,
        id: Id,
        change: &SavedSearchChange,
    ) -> Result<(), SearchError> {
        let current = self.saved_search(id)?.ok_or(SearchError::NotFound)?;
        let name = change.name.as_deref().map(clean_name).transpose()?;
        if let Some(query) = &change.query {
            parse::parse(query)?;
        }
        if let Some(name) = &name {
            self.check_name_free(name, Some(id))?;
        }
        Ok(self.write(|w| {
            if let Some(name) = name.filter(|n| *n != current.name) {
                w.set(SAVED_SEARCH.entity, id, "name", text(&name))?;
            }
            if let Some(query) = change.query.as_ref().filter(|q| **q != current.query) {
                w.set(SAVED_SEARCH.entity, id, "query", text(query))?;
            }
            if let Some(sort) = change.sort.filter(|s| *s != current.sort) {
                w.set(SAVED_SEARCH.entity, id, "sort", text(&sort.to_string()))?;
            }
            if let Some(mode) = change.mode.filter(|m| *m != current.mode) {
                w.set(SAVED_SEARCH.entity, id, "mode", text(mode.name()))?;
            }
            Ok(())
        })?)
    }

    pub fn delete_saved_search(&self, id: Id) -> Result<(), SearchError> {
        self.saved_search(id)?.ok_or(SearchError::NotFound)?;
        Ok(self.write(|w| w.set(SAVED_SEARCH.entity, id, "deleted", Value::Integer(1)))?)
    }

    /// Runs a saved search with its own sort and mode, from `offset`, up to `limit` rows.
    pub fn run_saved_search(
        &self,
        id: Id,
        offset: u32,
        limit: u32,
    ) -> Result<SearchPage, SearchError> {
        let saved = self.saved_search(id)?.ok_or(SearchError::NotFound)?;
        self.search(
            &saved.query,
            &SearchOptions {
                sort: saved.sort,
                mode: saved.mode,
                offset,
                limit,
                seed: 0,
            },
        )
    }
}
