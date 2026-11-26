//! Document snapshots: manual and temporary AI snapshots for any writable
//! document (`notes` or `rw_folder`, across `md`, `mindmap`, `whiteboard`).
//! Stored in `<data>/snapshots.db`.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub struct SnapshotStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: i64,
    pub lib: String,
    pub doc_id: String,
    /// `"manual"` or `"ai"`
    pub kind: String,
    pub label: String,
    pub content_type: String,
    pub size_bytes: usize,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub id: i64,
    pub lib: String,
    pub doc_id: String,
    pub kind: String,
    pub label: String,
    pub content_type: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SnapshotPolicy {
    pub ai_max_per_doc: usize,
    pub ai_ttl_days: u64,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self {
            ai_max_per_doc: 10,
            ai_ttl_days: 7,
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn err(e: rusqlite::Error) -> String {
    format!("快照数据库出错：{}", e)
}

impl SnapshotStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("无法创建快照目录：{}", e))?;
        }
        let conn = Connection::open(path).map_err(err)?;
        Self::init(conn)
    }

    pub fn memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(err)?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, String> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS snapshots (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 lib TEXT NOT NULL,
                 doc_id TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 label TEXT NOT NULL DEFAULT '',
                 content_type TEXT NOT NULL DEFAULT 'md',
                 content TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_snapshots_doc ON snapshots(lib, doc_id, created_at DESC, id DESC);
             CREATE INDEX IF NOT EXISTS idx_snapshots_ai_time ON snapshots(kind, created_at);
             CREATE TABLE IF NOT EXISTS snapshot_policy (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 ai_max_per_doc INTEGER NOT NULL DEFAULT 10,
                 ai_ttl_days INTEGER NOT NULL DEFAULT 7
             );
             INSERT OR IGNORE INTO snapshot_policy (id, ai_max_per_doc, ai_ttl_days) VALUES (1, 10, 7);",
        )
        .map_err(err)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn read_policy_locked(c: &Connection) -> Result<SnapshotPolicy, String> {
        let row = c
            .query_row(
                "SELECT ai_max_per_doc, ai_ttl_days FROM snapshot_policy WHERE id = 1",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(err)?;
        Ok(match row {
            Some((m, d)) => SnapshotPolicy {
                ai_max_per_doc: (m.max(1) as usize).min(100),
                ai_ttl_days: (d.max(1) as u64).min(365),
            },
            None => SnapshotPolicy::default(),
        })
    }

    pub fn policy(&self) -> Result<SnapshotPolicy, String> {
        let c = self.conn.lock().unwrap();
        Self::read_policy_locked(&c)
    }

    pub fn set_policy(&self, ai_max_per_doc: usize, ai_ttl_days: u64) -> Result<SnapshotPolicy, String> {
        let max = ai_max_per_doc.clamp(1, 100);
        let days = ai_ttl_days.clamp(1, 365);
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO snapshot_policy (id, ai_max_per_doc, ai_ttl_days) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET ai_max_per_doc = excluded.ai_max_per_doc, ai_ttl_days = excluded.ai_ttl_days",
            params![max as i64, days as i64],
        )
        .map_err(err)?;
        Ok(SnapshotPolicy {
            ai_max_per_doc: max,
            ai_ttl_days: days,
        })
    }

    pub fn create(
        &self,
        lib: &str,
        doc_id: &str,
        kind: &str,
        label: &str,
        content_type: &str,
        content: &str,
    ) -> Result<SnapshotMeta, String> {
        let kind = if kind == "ai" { "ai" } else { "manual" };
        self.insert(lib, doc_id, kind, label, content_type, content)
    }

    /// A deleted note and its subtree, kept for the library's trash (`kind =
    /// deleted`). It is not shown among a document's snapshots and never expires.
    pub fn create_deleted(&self, lib: &str, doc_id: &str, label: &str, content: &str) -> Result<SnapshotMeta, String> {
        self.insert(lib, doc_id, "deleted", label, "tree", content)
    }

    fn insert(
        &self,
        lib: &str,
        doc_id: &str,
        kind: &str,
        label: &str,
        content_type: &str,
        content: &str,
    ) -> Result<SnapshotMeta, String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.savepoint().map_err(err)?;

        if kind == "ai" {
            let latest: Option<(i64, String, String, String, String, String, i64)> = tx
                .query_row(
                    "SELECT id, lib, doc_id, kind, label, content_type, content, created_at
                     FROM snapshots WHERE lib = ?1 AND doc_id = ?2 AND kind != 'deleted'
                     ORDER BY created_at DESC, id DESC LIMIT 1",
                    params![lib, doc_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(6)?, r.get(7)?)),
                )
                .optional()
                .map_err(err)?;
            if let Some((id, l, d, k, lbl, prev_content, ts)) = latest {
                if prev_content == content {
                    tx.commit().map_err(err)?;
                    return Ok(SnapshotMeta {
                        id,
                        lib: l,
                        doc_id: d,
                        kind: k,
                        label: lbl,
                        content_type: content_type.to_string(),
                        size_bytes: content.len(),
                        created_at: ts,
                    });
                }
            }
        }

        let ts = now();
        tx.execute(
            "INSERT INTO snapshots (lib, doc_id, kind, label, content_type, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![lib, doc_id, kind, label, content_type, content, ts],
        )
        .map_err(err)?;
        let id = tx.last_insert_rowid();

        if kind == "ai" {
            let policy = Self::read_policy_locked(&tx)?;
            tx.execute(
                "DELETE FROM snapshots
                 WHERE lib = ?1 AND doc_id = ?2 AND kind = 'ai'
                   AND id NOT IN (
                       SELECT id FROM snapshots
                       WHERE lib = ?1 AND doc_id = ?2 AND kind = 'ai'
                       ORDER BY created_at DESC, id DESC
                       LIMIT ?3
                   )",
                params![lib, doc_id, policy.ai_max_per_doc as i64],
            )
            .map_err(err)?;
        }

        tx.commit().map_err(err)?;
        Ok(SnapshotMeta {
            id,
            lib: lib.to_string(),
            doc_id: doc_id.to_string(),
            kind: kind.to_string(),
            label: label.to_string(),
            content_type: content_type.to_string(),
            size_bytes: content.len(),
            created_at: ts,
        })
    }

    pub fn list(&self, lib: &str, doc_id: &str) -> Result<Vec<SnapshotMeta>, String> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c
            .prepare(
                "SELECT id, lib, doc_id, kind, label, content_type, LENGTH(CAST(content AS BLOB)), created_at
                 FROM snapshots WHERE lib = ?1 AND doc_id = ?2 AND kind != 'deleted'
                 ORDER BY created_at DESC, id DESC",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![lib, doc_id], |r| {
                Ok(SnapshotMeta {
                    id: r.get(0)?,
                    lib: r.get(1)?,
                    doc_id: r.get(2)?,
                    kind: r.get(3)?,
                    label: r.get(4)?,
                    content_type: r.get(5)?,
                    size_bytes: r.get::<_, i64>(6)?.max(0) as usize,
                    created_at: r.get(7)?,
                })
            })
            .map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(err)?);
        }
        Ok(out)
    }

    /// The deleted subtrees of a library (its trash), newest first.
    pub fn list_deleted(&self, lib: &str) -> Result<Vec<SnapshotMeta>, String> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c
            .prepare(
                "SELECT id, lib, doc_id, kind, label, content_type, LENGTH(CAST(content AS BLOB)), created_at
                 FROM snapshots WHERE lib = ?1 AND kind = 'deleted'
                 ORDER BY created_at DESC, id DESC",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![lib], |r| {
                Ok(SnapshotMeta {
                    id: r.get(0)?,
                    lib: r.get(1)?,
                    doc_id: r.get(2)?,
                    kind: r.get(3)?,
                    label: r.get(4)?,
                    content_type: r.get(5)?,
                    size_bytes: r.get::<_, i64>(6)?.max(0) as usize,
                    created_at: r.get(7)?,
                })
            })
            .map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(err)?);
        }
        Ok(out)
    }

    /// Throw away every deleted subtree of a library for good; returns how many.
    pub fn purge_deleted(&self, lib: &str) -> Result<usize, String> {
        let c = self.conn.lock().unwrap();
        let n = c
            .execute("DELETE FROM snapshots WHERE lib = ?1 AND kind = 'deleted'", params![lib])
            .map_err(err)?;
        Ok(n)
    }

    /// Move document snapshots to the new library-relative path.
    pub fn rekey(&self, lib: &str, from: &str, to: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE snapshots SET doc_id = ?3 || substr(doc_id, ?4)
             WHERE lib = ?1 AND kind != 'deleted'
               AND (doc_id = ?2 OR substr(doc_id, 1, ?4) = ?2 || '/')",
            params![lib, from, to, from.chars().count() as i64 + 1],
        ).map_err(err)?;
        Ok(())
    }

    /// A library was removed: so are the snapshots that belonged to its documents.
    pub fn remove_lib(&self, lib: &str) -> Result<usize, String> {
        let c = self.conn.lock().unwrap();
        let n = c.execute("DELETE FROM snapshots WHERE lib = ?1", params![lib]).map_err(err)?;
        Ok(n)
    }

    pub fn get(&self, id: i64) -> Result<Option<SnapshotRecord>, String> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id, lib, doc_id, kind, label, content_type, content, created_at FROM snapshots WHERE id = ?1",
            params![id],
            |r| {
                Ok(SnapshotRecord {
                    id: r.get(0)?,
                    lib: r.get(1)?,
                    doc_id: r.get(2)?,
                    kind: r.get(3)?,
                    label: r.get(4)?,
                    content_type: r.get(5)?,
                    content: r.get(6)?,
                    created_at: r.get(7)?,
                })
            },
        )
        .optional()
        .map_err(err)
    }

    pub fn delete(&self, id: i64) -> Result<bool, String> {
        let c = self.conn.lock().unwrap();
        let n = c.execute("DELETE FROM snapshots WHERE id = ?1", params![id]).map_err(err)?;
        Ok(n > 0)
    }

    pub fn cleanup_expired_ai(&self) -> Result<usize, String> {
        let c = self.conn.lock().unwrap();
        let policy = Self::read_policy_locked(&c)?;
        let cutoff = now() - (policy.ai_ttl_days as i64) * 86400;
        let n = c
            .execute("DELETE FROM snapshots WHERE kind = 'ai' AND created_at < ?1", params![cutoff])
            .map_err(err)?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_lists_gets_and_deletes_snapshots() {
        let store = SnapshotStore::memory().unwrap();
        let m1 = store.create("notes", "1", "manual", "初稿", "md", "# A").unwrap();
        let m2 = store.create("notes", "1", "ai", "AI 改前", "md", "# B").unwrap();
        let list = store.list("notes", "1").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, m2.id);
        assert_eq!(list[1].id, m1.id);
        assert_eq!(store.get(m1.id).unwrap().unwrap().content, "# A");
        assert!(store.delete(m1.id).unwrap());
        assert_eq!(store.list("notes", "1").unwrap().len(), 1);
    }

    #[test]
    fn follows_folder_moves_and_note_renames() {
        let store = SnapshotStore::memory().unwrap();
        let before = "专题_甲/文档.md";
        let manual = store.create("folder", before, "manual", "初稿", "md", "# 原文").unwrap();
        let ai = store.create("folder", before, "ai", "AI 改前", "md", "# 修改前").unwrap();
        let sibling = store.create("folder", "专题_甲集/文档.md", "manual", "", "md", "").unwrap();
        let elsewhere = store.create("elsewhere", before, "manual", "", "md", "").unwrap();
        let after = "资料/专题_甲/新文档.md";

        store.rekey("folder", "专题_甲", "资料/专题_甲").unwrap();
        store.rekey("folder", "资料/专题_甲/文档.md", after).unwrap();

        assert!(store.list("folder", before).unwrap().is_empty());
        assert_eq!(store.list("folder", after).unwrap().len(), 2);
        assert_eq!(store.get(manual.id).unwrap().unwrap().doc_id, after);
        assert_eq!(store.get(ai.id).unwrap().unwrap().doc_id, after);
        assert_eq!(store.get(sibling.id).unwrap().unwrap().doc_id, "专题_甲集/文档.md");
        assert_eq!(store.get(elsewhere.id).unwrap().unwrap().doc_id, before);
    }

    #[test]
    fn prunes_excess_ai_snapshots_without_touching_manual() {
        let store = SnapshotStore::memory().unwrap();
        store.set_policy(3, 7).unwrap();
        store.create("notes", "9", "manual", "手动保留", "mindmap", "{}").unwrap();
        for i in 1..=5 {
            store
                .create("notes", "9", "ai", &format!("AI-{}", i), "mindmap", &format!("{{\"v\":{}}}", i))
                .unwrap();
        }
        let list = store.list("notes", "9").unwrap();
        let ai_count = list.iter().filter(|s| s.kind == "ai").count();
        let manual_count = list.iter().filter(|s| s.kind == "manual").count();
        assert_eq!(ai_count, 3);
        assert_eq!(manual_count, 1);
    }

    #[test]
    fn cleans_up_expired_ai_snapshots_only() {
        let store = SnapshotStore::memory().unwrap();
        store.set_policy(10, 3).unwrap();
        let m_manual = store.create("f", "a.md", "manual", "手动", "md", "keep").unwrap();
        let m_ai = store.create("f", "a.md", "ai", "AI", "md", "old").unwrap();
        {
            let c = store.conn.lock().unwrap();
            let old_ts = now() - 5 * 86400;
            c.execute("UPDATE snapshots SET created_at = ?1 WHERE id IN (?2, ?3)", params![old_ts, m_manual.id, m_ai.id])
                .unwrap();
        }
        let removed = store.cleanup_expired_ai().unwrap();
        assert_eq!(removed, 1);
        let list = store.list("f", "a.md").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, m_manual.id);
    }

    #[test]
    fn deleted_snapshots_are_the_trash_not_document_snapshots() {
        let store = SnapshotStore::memory().unwrap();
        let doc = store.create("notes", "1", "manual", "手动", "md", "a").unwrap();
        let gone = store.create_deleted("notes", "1", "笔记 1", "{\"root\":{}}").unwrap();
        assert_eq!(gone.kind, "deleted");
        // the document dialog does not show it, the trash does
        assert_eq!(store.list("notes", "1").unwrap().len(), 1);
        assert_eq!(store.list_deleted("notes").unwrap().len(), 1);
        assert_eq!(store.list_deleted("other").unwrap().len(), 0);
        // it never expires
        {
            let c = store.conn.lock().unwrap();
            c.execute("UPDATE snapshots SET created_at = ?1 WHERE id = ?2", params![now() - 400 * 86400, gone.id]).unwrap();
        }
        assert_eq!(store.cleanup_expired_ai().unwrap(), 0);
        assert_eq!(store.list_deleted("notes").unwrap().len(), 1);
        // thrown away for good, or with the whole library
        store.create_deleted("notes", "2", "笔记 2", "{\"root\":{}}").unwrap();
        assert_eq!(store.purge_deleted("notes").unwrap(), 2);
        assert_eq!(store.list_deleted("notes").unwrap().len(), 0);
        assert_eq!(store.list("notes", "1").unwrap().len(), 1);
        store.create_deleted("notes", "2", "笔记 2", "{\"root\":{}}").unwrap();
        assert_eq!(store.remove_lib("notes").unwrap(), 2);
        assert!(store.get(doc.id).unwrap().is_none());
    }
}
