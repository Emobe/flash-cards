//! Reading decks. Nothing here writes. All the read-time rules of the module (cycles, missing
//! parents, same names, "still referenced, so alive") are applied once, in `Tree::load`, so every
//! reader and every write check sees the same decks.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::preset::usable_presets;
use super::{Deck, DeckError, default_deck, default_preset};
use crate::collection::Collection;
use crate::id::Id;

fn nil() -> Id {
    Id::from_bytes([0; 16])
}

/// An ID stored as a blob that is empty for "none".
pub(super) fn optional_id(bytes: &[u8]) -> Option<Id> {
    Id::from_slice(bytes)
}

/// The deck a card's `deck` register names: empty (a card from before decks, step 1.3) or not an ID
/// reads as the Default deck.
pub(crate) fn card_deck(bytes: &[u8]) -> Id {
    optional_id(bytes).unwrap_or_else(default_deck)
}

/// One deck after the read-time rules.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub id: Id,
    pub name: String,
    pub display_name: String,
    pub path: String,
    /// The parent after cycles and missing parents are settled.
    pub parent: Option<Id>,
    pub depth: usize,
    pub preset: Id,
    pub limits_include_subdecks: bool,
    pub cards: usize,
    /// What the deck's own `deleted` register says.
    pub register_deleted: bool,
    /// Whether it is deleted as far as anyone can tell: the register, unless something live keeps
    /// it alive.
    pub deleted: bool,
}

impl Row {
    fn deck(&self) -> Deck {
        Deck {
            id: self.id,
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            path: self.path.clone(),
            parent: self.parent,
            depth: self.depth,
            preset: self.preset,
            limits_include_subdecks: self.limits_include_subdecks,
            cards: self.cards,
            deleted: self.deleted,
        }
    }
}

/// Every deck, in tree order: a deck comes right before its sub-decks, and decks next to each other
/// are ordered by name (ignoring case) and then ID.
pub(crate) struct Tree {
    pub rows: Vec<Row>,
    index: HashMap<Id, usize>,
}

struct Raw {
    id: Id,
    name: String,
    parent: Option<Id>,
    preset: Option<Id>,
    limits_include_subdecks: bool,
    deleted: bool,
    /// The clock of the `parent` register, which decides which move of a cycle is ignored.
    parent_clock: (i64, Id),
}

impl Tree {
    pub fn load(conn: &Connection) -> rusqlite::Result<Self> {
        let raw = load_raw(conn)?;
        let ids: HashMap<Id, usize> = raw.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
        let mut parent: Vec<Option<usize>> = raw
            .iter()
            .map(|r| {
                r.parent
                    .filter(|p| *p != r.id)
                    .and_then(|p| ids.get(&p).copied())
            })
            .collect();
        break_cycles(&raw, &mut parent);

        let counts = card_counts(conn)?;
        let usable = usable_presets(conn)?;
        let default = default_deck();
        let mut alive: Vec<bool> = raw
            .iter()
            .map(|r| !r.deleted || r.id == default || counts.get(&r.id).copied().unwrap_or(0) > 0)
            .collect();
        for start in 0..raw.len() {
            if !alive[start] {
                continue;
            }
            let mut at = parent[start];
            while let Some(above) = at {
                if alive[above] {
                    break;
                }
                alive[above] = true;
                at = parent[above];
            }
        }

        let mut children: Vec<Vec<usize>> = vec![Vec::new(); raw.len()];
        let mut roots = Vec::new();
        for (i, p) in parent.iter().enumerate() {
            match p {
                Some(p) => children[*p].push(i),
                None => roots.push(i),
            }
        }
        let by_name = |list: &mut Vec<usize>| {
            list.sort_by_cached_key(|&i| (raw[i].name.to_lowercase(), raw[i].id));
        };
        by_name(&mut roots);
        for list in &mut children {
            by_name(list);
        }
        let mut display: Vec<String> = raw.iter().map(|r| r.name.clone()).collect();
        for list in std::iter::once(&roots).chain(children.iter()) {
            let mut seen: HashMap<String, usize> = HashMap::new();
            for &i in list.iter().filter(|&&i| alive[i]) {
                let count = seen.entry(raw[i].name.to_lowercase()).or_insert(0);
                *count += 1;
                if *count > 1 {
                    display[i] = format!("{} ({})", raw[i].name, count);
                }
            }
        }

        let mut rows = Vec::with_capacity(raw.len());
        let mut stack: Vec<(usize, usize, String)> =
            roots.iter().rev().map(|&i| (i, 0, String::new())).collect();
        while let Some((i, depth, above)) = stack.pop() {
            let path = if above.is_empty() {
                display[i].clone()
            } else {
                format!("{above}{}{}", super::SEPARATOR, display[i])
            };
            for &child in children[i].iter().rev() {
                stack.push((child, depth + 1, path.clone()));
            }
            let r = &raw[i];
            rows.push(Row {
                id: r.id,
                name: r.name.clone(),
                display_name: display[i].clone(),
                path,
                parent: parent[i].map(|p| raw[p].id),
                depth,
                preset: r
                    .preset
                    .filter(|p| usable.contains(p))
                    .unwrap_or_else(default_preset),
                limits_include_subdecks: r.limits_include_subdecks,
                cards: counts.get(&r.id).copied().unwrap_or(0),
                register_deleted: r.deleted,
                deleted: !alive[i],
            });
        }
        let index = rows.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
        Ok(Self { rows, index })
    }

    /// Where a deck is in `rows`, which is tree order.
    pub fn position(&self, id: Id) -> Option<usize> {
        self.index.get(&id).copied()
    }

    pub fn get(&self, id: Id) -> Option<&Row> {
        self.index.get(&id).map(|&i| &self.rows[i])
    }

    /// A deck that exists and is not deleted.
    pub fn live(&self, id: Id) -> Option<&Row> {
        self.get(id).filter(|r| !r.deleted)
    }

    /// The deck and everything inside it, in tree order. Empty if there is no such deck.
    pub fn subtree(&self, id: Id) -> &[Row] {
        let Some(&start) = self.index.get(&id) else {
            return &[];
        };
        let depth = self.rows[start].depth;
        let end = self.rows[start + 1..]
            .iter()
            .position(|r| r.depth <= depth)
            .map_or(self.rows.len(), |n| start + 1 + n);
        &self.rows[start..end]
    }

    /// Whether `inner` is `outer` or inside it.
    pub fn is_inside(&self, inner: Id, outer: Id) -> bool {
        self.subtree(outer).iter().any(|r| r.id == inner)
    }

    /// The decks that are deleted as far as anyone can tell.
    pub fn dead(&self) -> HashSet<Id> {
        self.rows
            .iter()
            .filter(|r| r.deleted)
            .map(|r| r.id)
            .collect()
    }

    pub fn decks(&self, deleted: bool) -> Vec<Deck> {
        self.rows
            .iter()
            .filter(|r| r.deleted == deleted)
            .map(Row::deck)
            .collect()
    }
}

fn load_raw(conn: &Connection) -> rusqlite::Result<Vec<Raw>> {
    let mut statement = conn.prepare(
        "SELECT d.id, d.name, d.parent, d.options_preset, d.deleted, rc.hlc, rc.device,
                d.limits_include_subdecks
         FROM deck d
         LEFT JOIN register_clock rc ON rc.entity_type = 'deck' AND rc.entity_id = d.id
                                    AND rc.field = 'parent'
         ORDER BY d.id",
    )?;
    statement
        .query_map([], |row| {
            Ok(Raw {
                id: row.get(0)?,
                name: row.get(1)?,
                parent: optional_id(&row.get::<_, Vec<u8>>(2)?),
                preset: optional_id(&row.get::<_, Vec<u8>>(3)?),
                limits_include_subdecks: row.get::<_, i64>(7)? != 0,
                deleted: row.get::<_, i64>(4)? != 0,
                parent_clock: (
                    row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    row.get::<_, Option<Id>>(6)?.unwrap_or_else(nil),
                ),
            })
        })?
        .collect()
}

/// Live cards by deck: a card that is not deleted, in a note that is not deleted.
fn card_counts(conn: &Connection) -> rusqlite::Result<HashMap<Id, usize>> {
    let mut statement = conn.prepare(
        "SELECT c.deck, count(*) FROM card c JOIN note n ON n.id = c.note
         WHERE c.deleted = 0 AND n.deleted = 0 GROUP BY c.deck",
    )?;
    let mut counts = HashMap::new();
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)? as usize))
    })? {
        let (deck, count) = row?;
        *counts.entry(card_deck(&deck)).or_insert(0) += count;
    }
    Ok(counts)
}

/// Each deck has one parent, so a loop is a simple cycle. In every cycle the deck whose `parent`
/// register was written last loses its parent, so it shows at the top level. The clocks are the
/// same on every device, so every device breaks the cycle at the same place.
fn break_cycles(raw: &[Raw], parent: &mut [Option<usize>]) {
    // 0 = not seen, 1 = on the path being followed, 2 = done.
    let mut state = vec![0u8; raw.len()];
    for start in 0..raw.len() {
        if state[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut at = start;
        let cycle = loop {
            match state[at] {
                2 => break None,
                1 => {
                    let from = path.iter().position(|&i| i == at).expect("on the path");
                    break Some(path[from..].to_vec());
                }
                _ => {
                    state[at] = 1;
                    path.push(at);
                    match parent[at] {
                        Some(next) => at = next,
                        None => break None,
                    }
                }
            }
        };
        for &i in &path {
            state[i] = 2;
        }
        if let Some(cycle) = cycle {
            let newest = cycle
                .into_iter()
                .max_by_key(|&i| (raw[i].parent_clock, raw[i].id))
                .expect("a cycle has members");
            parent[newest] = None;
        }
    }
}

impl Collection {
    /// The decks that are not deleted, in tree order: each deck is followed by the decks inside it,
    /// and decks next to each other are ordered by name.
    pub fn decks(&self) -> Result<Vec<Deck>, DeckError> {
        Ok(Tree::load(&self.conn)?.decks(false))
    }

    /// The deleted decks (the trash), in tree order.
    pub fn deleted_decks(&self) -> Result<Vec<Deck>, DeckError> {
        Ok(Tree::load(&self.conn)?.decks(true))
    }

    /// One deck, deleted or not.
    pub fn deck(&self, id: Id) -> Result<Option<Deck>, DeckError> {
        Ok(Tree::load(&self.conn)?.get(id).map(Row::deck))
    }
}

/// The decks that are deleted as far as anyone can tell, for the card code (a card in one of them
/// is not brought back by an edit).
pub(crate) fn dead_decks(conn: &Connection) -> rusqlite::Result<HashSet<Id>> {
    Ok(Tree::load(conn)?.dead())
}
