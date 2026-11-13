//! SQLite storage with versioned migrations.
//!
//! Each migration is a numbered SQL file. `PRAGMA user_version` records the last one applied, and
//! every migration runs inside a transaction, so a failed upgrade leaves the database as it was.

use crate::error::{Error, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

pub const MIGRATIONS: &[(u32, &str)] = &[(1, include_str!("migrations/0001_init.sql"))];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    path: Option<PathBuf>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn, Some(path.to_path_buf()))
    }

    pub fn memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    fn init(mut conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)), path })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The connection. Hold it only for the length of one query or transaction, never across `.await`.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Flushes the WAL into the main file so the database can be copied as a single file.
    pub fn checkpoint(&self) -> Result<()> {
        self.lock().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
}

pub fn schema_version(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

pub fn latest_version() -> u32 {
    MIGRATIONS.last().map(|m| m.0).unwrap_or(0)
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current = schema_version(conn)?;
    if current > latest_version() {
        return Err(Error::msg(format!("This data was made by a newer Marquee (schema {current}; this version knows {}). Update Marquee to open it.", latest_version())));
    }
    for (version, sql) in MIGRATIONS.iter().filter(|m| m.0 > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(version, "applied migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_a_fresh_database_to_latest() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        assert_eq!(schema_version(&conn).unwrap(), latest_version());
        let lists: i64 = conn.query_row("SELECT count(*) FROM lists WHERE kind = 'system'", [], |r| r.get(0)).unwrap();
        assert_eq!(lists, 4);
    }

    #[test]
    fn migrations_are_numbered_in_order_without_gaps() {
        for (i, (version, _)) in MIGRATIONS.iter().enumerate() {
            assert_eq!(*version as usize, i + 1);
        }
    }

    #[test]
    fn running_migrations_twice_changes_nothing() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('k', '1')", []).unwrap();
        migrate(&mut conn).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM settings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn refuses_a_database_from_a_newer_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        conn.pragma_update(None, "user_version", latest_version() + 1).unwrap();
        assert!(migrate(&mut conn).is_err());
    }

    #[test]
    fn every_table_the_spec_names_exists() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        for table in ["items", "sources", "user_events", "lists", "list_items", "follows", "notifications", "cache", "settings"] {
            let n: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get(0)).unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
    }

    #[test]
    fn deleting_an_item_removes_its_events_and_list_entries() {
        let db = Db::memory().unwrap();
        let conn = db.lock();
        conn.execute("INSERT INTO items (id, media, title, sort_title, updated_at) VALUES (1, 'movie', 'A', 'a', 'now')", []).unwrap();
        conn.execute("INSERT INTO user_events (item_id, source, kind, at) VALUES (1, 'marquee', 'rating', 'now')", []).unwrap();
        conn.execute("INSERT INTO list_items (list_id, item_id, added_at) VALUES (1, 1, 'now')", []).unwrap();
        conn.execute("DELETE FROM items WHERE id = 1", []).unwrap();
        let n: i64 = conn.query_row("SELECT (SELECT count(*) FROM user_events) + (SELECT count(*) FROM list_items)", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
