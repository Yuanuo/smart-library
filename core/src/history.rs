//! Reading history (the "Recent" list): every document ever opened, with the
//! last reading position. SQLite, one small table; the file is user data
//! (`<data>/history.db`) but losing it costs nothing but the list.

use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct History {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub lib: String,
    pub path: String,
    pub book_id: Option<String>,
    pub title: String,
    /// line / paragraph at the top of the view when last read
    pub lb: Option<String>,
    pub mode: Option<String>,
    /// last opened, unix seconds
    pub opened: i64,
    pub visits: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn err(e: rusqlite::Error) -> String {
    format!("历史记录出错：{}", e)
}

impl History {
    pub fn open(path: &Path) -> Result<History, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Self::init(Connection::open(path).map_err(err)?)
    }

    pub fn memory() -> Result<History, String> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<History, String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                 lib TEXT NOT NULL,
                 path TEXT NOT NULL,
                 book_id TEXT,
                 title TEXT NOT NULL,
                 lb TEXT,
                 mode TEXT,
                 opened INTEGER NOT NULL,
                 visits INTEGER NOT NULL DEFAULT 1,
                 PRIMARY KEY (lib, path)
             );
             CREATE INDEX IF NOT EXISTS history_opened ON history(opened DESC);",
        )
        .map_err(err)?;
        Ok(History { conn: Mutex::new(conn) })
    }

    /// A document was opened: new entry, or move the old one to the top.
    pub fn touch(&self, lib: &str, path: &str, book_id: Option<&str>, title: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO history (lib, path, book_id, title, opened) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(lib, path) DO UPDATE SET title = ?4, book_id = ?3, opened = ?5, visits = visits + 1",
            params![lib, path, book_id, title, now()],
        )
        .map_err(err)?;
        Ok(())
    }

    /// The reading position of a document (no change if it is not listed).
    pub fn set_progress(&self, lib: &str, path: &str, lb: &str, mode: Option<&str>) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE history SET lb = ?3, mode = COALESCE(?4, mode) WHERE lib = ?1 AND path = ?2",
            params![lib, path, lb, mode],
        )
        .map_err(err)?;
        Ok(())
    }

    /// Newest first; `filter` keeps titles containing it (case-insensitive).
    pub fn list(&self, filter: &str, limit: usize) -> Result<Vec<HistoryEntry>, String> {
        let c = self.conn.lock().unwrap();
        let like = format!("%{}%", filter.trim().replace('%', "\\%").replace('_', "\\_"));
        let mut st = c
            .prepare(
                "SELECT lib, path, book_id, title, lb, mode, opened, visits FROM history
                 WHERE title LIKE ?1 ESCAPE '\\' ORDER BY opened DESC LIMIT ?2",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![like, limit as i64], |r| {
                Ok(HistoryEntry {
                    lib: r.get(0)?,
                    path: r.get(1)?,
                    book_id: r.get(2)?,
                    title: r.get(3)?,
                    lb: r.get(4)?,
                    mode: r.get(5)?,
                    opened: r.get(6)?,
                    visits: r.get(7)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    /// What was ever opened in a library: `(path, book id)` pairs.
    pub fn opened(&self, lib: &str) -> Result<Vec<(String, Option<String>)>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT path, book_id FROM history WHERE lib = ?1").map_err(err)?;
        let rows = st.query_map(params![lib], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }

    pub fn remove(&self, lib: &str, path: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM history WHERE lib = ?1 AND path = ?2", params![lib, path]).map_err(err)?;
        Ok(())
    }

    /// Forget a whole library (when it is unbound).
    pub fn remove_lib(&self, lib: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM history WHERE lib = ?1", params![lib]).map_err(err)?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM history", []).map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_opens_and_progress() {
        let h = History::memory().unwrap();
        h.touch("cbeta", "XML/T/T01/T01n0001_001.xml", Some("T0001"), "長阿含經 卷一").unwrap();
        h.touch("zb", "a/ZB/z/1.html", None, "童年的梦").unwrap();
        h.set_progress("cbeta", "XML/T/T01/T01n0001_001.xml", "0003a05", Some("para")).unwrap();
        h.set_progress("nobody", "x", "1", None).unwrap(); // not listed: ignored
        // opening again counts a visit and moves it to the top
        std::thread::sleep(std::time::Duration::from_millis(1100));
        h.touch("cbeta", "XML/T/T01/T01n0001_001.xml", Some("T0001"), "長阿含經 卷一").unwrap();
        let l = h.list("", 10).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].lib, "cbeta");
        assert_eq!(l[0].visits, 2);
        assert_eq!(l[0].lb.as_deref(), Some("0003a05"));
        assert_eq!(h.list("童年", 10).unwrap().len(), 1);
        assert_eq!(h.opened("cbeta").unwrap(), vec![("XML/T/T01/T01n0001_001.xml".to_string(), Some("T0001".to_string()))]);
        h.remove_lib("zb").unwrap();
        assert_eq!(h.list("", 10).unwrap().len(), 1);
        h.clear().unwrap();
        assert!(h.list("", 10).unwrap().is_empty());
    }
}
