//! Temporary storage spike (step 0.4): one table of notes, to prove the same SQLite code persists
//! on every target. Deleted in step 1.1, when the real schema arrives.

use rusqlite::Connection;

/// Longest note, in characters.
pub const MAX_NOTE_CHARS: usize = 1000;

#[derive(Debug, PartialEq, Eq)]
pub enum SpikeError {
    /// The note is empty or longer than `MAX_NOTE_CHARS`.
    InvalidNote,
    /// SQLite failed. The text goes to the log, not to the user.
    Storage(String),
}

impl From<rusqlite::Error> for SpikeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

#[derive(Debug)]
pub struct SpikeStore {
    conn: Connection,
}

impl SpikeStore {
    /// Opens a database at a path or `file:` URI and creates the table if needed.
    pub fn open(path: &str) -> Result<Self, SpikeError> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, SpikeError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, SpikeError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
        )?;
        Ok(Self { conn })
    }

    pub fn add_note(&self, body: &str) -> Result<(), SpikeError> {
        let length = body.chars().count();
        if length == 0 || length > MAX_NOTE_CHARS {
            return Err(SpikeError::InvalidNote);
        }
        self.conn
            .execute("INSERT INTO notes (body) VALUES (?1)", [body])?;
        Ok(())
    }

    /// Every note, oldest first.
    pub fn list_notes(&self) -> Result<Vec<String>, SpikeError> {
        let mut statement = self.conn.prepare("SELECT body FROM notes ORDER BY id")?;
        let notes = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_listed_in_the_order_added() {
        let store = SpikeStore::open_in_memory().unwrap();
        store.add_note("first").unwrap();
        store.add_note("second").unwrap();
        assert_eq!(store.list_notes().unwrap(), ["first", "second"]);
    }

    #[test]
    fn empty_and_overlong_notes_are_rejected() {
        let store = SpikeStore::open_in_memory().unwrap();
        assert_eq!(store.add_note(""), Err(SpikeError::InvalidNote));
        assert_eq!(
            store.add_note(&"x".repeat(MAX_NOTE_CHARS + 1)),
            Err(SpikeError::InvalidNote)
        );
        assert!(store.add_note(&"x".repeat(MAX_NOTE_CHARS)).is_ok());
    }

    #[test]
    fn notes_survive_reopening_a_file() {
        let path = std::env::temp_dir().join(format!("fc-spike-{}.db", std::process::id()));
        let path_str = path.to_str().unwrap();
        SpikeStore::open(path_str)
            .unwrap()
            .add_note("kept")
            .unwrap();
        let reopened = SpikeStore::open(path_str).unwrap();
        assert_eq!(reopened.list_notes().unwrap(), ["kept"]);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
}
