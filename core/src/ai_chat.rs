//! AI assistant chat history (`<data>/ai_chat.db`): keeps conversations per
//! document `(lib, path)` or as standalone / search chats, without touching
//! the documents themselves. Can be listed, restored, deleted or cleared.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

pub struct AiChatStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct AiSession {
    pub id: i64,
    /// `doc` | `search` | `chat`
    pub scope_kind: String,
    pub lib: String,
    pub path: String,
    pub book_id: Option<String>,
    pub title: String,
    pub doc_title: String,
    pub messages_json: String,
    pub refs_json: String,
    pub updated: i64,
}

#[derive(Debug, Clone)]
pub struct AiSessionSummary {
    pub id: i64,
    pub scope_kind: String,
    pub lib: String,
    pub path: String,
    pub book_id: Option<String>,
    pub title: String,
    pub doc_title: String,
    pub updated: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn err(e: rusqlite::Error) -> String {
    format!("AI 会话记录出错：{}", e)
}

impl AiChatStore {
    pub fn open(path: &Path) -> Result<AiChatStore, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Self::init(Connection::open(path).map_err(err)?)
    }

    pub fn memory() -> Result<AiChatStore, String> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<AiChatStore, String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ai_sessions (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 scope_kind TEXT NOT NULL DEFAULT 'doc',
                 lib TEXT NOT NULL DEFAULT '',
                 path TEXT NOT NULL DEFAULT '',
                 book_id TEXT,
                 title TEXT NOT NULL,
                 doc_title TEXT NOT NULL DEFAULT '',
                 messages_json TEXT NOT NULL DEFAULT '[]',
                 refs_json TEXT NOT NULL DEFAULT '[]',
                 updated INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS ai_sessions_doc ON ai_sessions(lib, path, updated DESC);
             CREATE INDEX IF NOT EXISTS ai_sessions_updated ON ai_sessions(updated DESC);",
        )
        .map_err(err)?;
        Ok(AiChatStore { conn: Mutex::new(conn) })
    }

    /// Create (`id <= 0` or `None`) or update an AI chat session; returns its `id`.
    #[allow(clippy::too_many_arguments)]
    pub fn save_session(
        &self,
        id: Option<i64>,
        scope_kind: &str,
        lib: &str,
        path: &str,
        book_id: Option<&str>,
        title: &str,
        doc_title: &str,
        messages_json: &str,
        refs_json: &str,
    ) -> Result<i64, String> {
        let c = self.conn.lock().unwrap();
        let ts = now();
        if let Some(existing_id) = id.filter(|&x| x > 0) {
            let n = c
                .execute(
                    "UPDATE ai_sessions
                     SET scope_kind = ?2, lib = ?3, path = ?4, book_id = ?5,
                         title = ?6, doc_title = ?7, messages_json = ?8, refs_json = ?9, updated = ?10
                     WHERE id = ?1",
                    params![existing_id, scope_kind, lib, path, book_id, title, doc_title, messages_json, refs_json, ts],
                )
                .map_err(err)?;
            if n > 0 {
                return Ok(existing_id);
            }
        }
        c.execute(
            "INSERT INTO ai_sessions (scope_kind, lib, path, book_id, title, doc_title, messages_json, refs_json, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![scope_kind, lib, path, book_id, title, doc_title, messages_json, refs_json, ts],
        )
        .map_err(err)?;
        Ok(c.last_insert_rowid())
    }

    pub fn get_session(&self, id: i64) -> Result<Option<AiSession>, String> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id, scope_kind, lib, path, book_id, title, doc_title, messages_json, refs_json, updated
             FROM ai_sessions WHERE id = ?1",
            params![id],
            |r| {
                Ok(AiSession {
                    id: r.get(0)?,
                    scope_kind: r.get(1)?,
                    lib: r.get(2)?,
                    path: r.get(3)?,
                    book_id: r.get(4)?,
                    title: r.get(5)?,
                    doc_title: r.get(6)?,
                    messages_json: r.get(7)?,
                    refs_json: r.get(8)?,
                    updated: r.get(9)?,
                })
            },
        )
        .optional()
        .map_err(err)
    }

    pub fn latest_for_doc(&self, lib: &str, path: &str) -> Result<Option<AiSession>, String> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id, scope_kind, lib, path, book_id, title, doc_title, messages_json, refs_json, updated
             FROM ai_sessions WHERE lib = ?1 AND path = ?2 ORDER BY updated DESC, id DESC LIMIT 1",
            params![lib, path],
            |r| {
                Ok(AiSession {
                    id: r.get(0)?,
                    scope_kind: r.get(1)?,
                    lib: r.get(2)?,
                    path: r.get(3)?,
                    book_id: r.get(4)?,
                    title: r.get(5)?,
                    doc_title: r.get(6)?,
                    messages_json: r.get(7)?,
                    refs_json: r.get(8)?,
                    updated: r.get(9)?,
                })
            },
        )
        .optional()
        .map_err(err)
    }

    /// List session summaries; when `lib` and `path` are given (non-empty),
    /// only sessions for that document are returned; otherwise all sessions.
    pub fn list_sessions(&self, lib: Option<&str>, path: Option<&str>, limit: usize) -> Result<Vec<AiSessionSummary>, String> {
        let c = self.conn.lock().unwrap();
        let row_map = |r: &rusqlite::Row<'_>| {
            Ok(AiSessionSummary {
                id: r.get(0)?,
                scope_kind: r.get(1)?,
                lib: r.get(2)?,
                path: r.get(3)?,
                book_id: r.get(4)?,
                title: r.get(5)?,
                doc_title: r.get(6)?,
                updated: r.get(7)?,
            })
        };
        match (lib.filter(|s| !s.is_empty()), path.filter(|s| !s.is_empty())) {
            (Some(l), Some(p)) => {
                let mut st = c
                    .prepare(
                        "SELECT id, scope_kind, lib, path, book_id, title, doc_title, updated
                         FROM ai_sessions WHERE lib = ?1 AND path = ?2 ORDER BY updated DESC, id DESC LIMIT ?3",
                    )
                    .map_err(err)?;
                let rows = st.query_map(params![l, p, limit as i64], row_map).map_err(err)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(err)
            }
            _ => {
                let mut st = c
                    .prepare(
                        "SELECT id, scope_kind, lib, path, book_id, title, doc_title, updated
                         FROM ai_sessions ORDER BY updated DESC, id DESC LIMIT ?1",
                    )
                    .map_err(err)?;
                let rows = st.query_map(params![limit as i64], row_map).map_err(err)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(err)
            }
        }
    }

    pub fn delete_session(&self, id: i64) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM ai_sessions WHERE id = ?1", params![id]).map_err(err)?;
        Ok(())
    }

    pub fn rename_session(&self, id: i64, title: &str) -> Result<(), String> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err("标题不能为空".into());
        }
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE ai_sessions SET title = ?2 WHERE id = ?1",
            params![id, trimmed],
        )
        .map_err(err)?;
        Ok(())
    }

    pub fn rekey(&self, lib: &str, from: &str, to: &str, title: Option<&str>) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE ai_sessions SET path = ?3 || substr(path, ?4)
             WHERE lib = ?1 AND (path = ?2 OR path LIKE ?5 ESCAPE '\\')",
            params![lib, from, to, from.chars().count() as i64 + 1, crate::props::like_below(from)],
        )
        .map_err(err)?;
        if let Some(t) = title {
            c.execute("UPDATE ai_sessions SET doc_title = ?3 WHERE lib = ?1 AND path = ?2", params![lib, to, t])
                .map_err(err)?;
        }
        Ok(())
    }

    pub fn remove_lib(&self, lib: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM ai_sessions WHERE lib = ?1", params![lib]).map_err(err)?;
        Ok(())
    }

    pub fn clear(&self) -> Result<usize, String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM ai_sessions", []).map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_lists_and_clears_sessions() {
        let store = AiChatStore::memory().unwrap();
        let id1 = store
            .save_session(
                None,
                "doc",
                "cbeta",
                "XML/T/T01/T01n0001_001.xml",
                Some("T0001"),
                "关于阿含经的提问",
                "長阿含經 卷一",
                r#"[{"role":"user","content":"你好"}]"#,
                "[]",
            )
            .unwrap();
        let _id2 = store
            .save_session(
                None,
                "chat",
                "",
                "",
                None,
                "独立对话",
                "",
                r#"[{"role":"user","content":"总结"}]"#,
                r#"[{"lib":"cbeta","path":"XML/T/T01/T01n0001_001.xml","title":"長阿含經"}]"#,
            )
            .unwrap();

        let s1 = store.get_session(id1).unwrap().unwrap();
        assert_eq!(s1.title, "关于阿含经的提问");
        assert_eq!(s1.doc_title, "長阿含經 卷一");

        // update existing session
        let id1_again = store
            .save_session(
                Some(id1),
                "doc",
                "cbeta",
                "XML/T/T01/T01n0001_001.xml",
                Some("T0001"),
                "更新后的标题",
                "長阿含經 卷一",
                r#"[{"role":"user","content":"你好"},{"role":"assistant","content":"您好"}]"#,
                "[]",
            )
            .unwrap();
        assert_eq!(id1, id1_again);
        assert_eq!(store.latest_for_doc("cbeta", "XML/T/T01/T01n0001_001.xml").unwrap().unwrap().title, "更新后的标题");

        // list by doc vs all
        assert_eq!(store.list_sessions(Some("cbeta"), Some("XML/T/T01/T01n0001_001.xml"), 20).unwrap().len(), 1);
        assert_eq!(store.list_sessions(None, None, 20).unwrap().len(), 2);

        // rekey
        store.rekey("cbeta", "XML/T/T01/T01n0001_001.xml", "XML/T/T01/moved.xml", Some("新题")).unwrap();
        assert_eq!(store.latest_for_doc("cbeta", "XML/T/T01/moved.xml").unwrap().unwrap().doc_title, "新题");

        store.delete_session(id1).unwrap();
        assert_eq!(store.list_sessions(None, None, 20).unwrap().len(), 1);
        assert_eq!(store.clear().unwrap(), 1);
        assert!(store.list_sessions(None, None, 20).unwrap().is_empty());
    }
}
