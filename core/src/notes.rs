//! Internal (read-write) libraries (design-spec 6.4): notes kept in SQLite,
//! one database per library (`<data>/repos/<id>/notes.db`). The tree is made
//! by `parent_id` and `position`. Every note is the same kind of node: it
//! has a title, Markdown content (maybe empty) and maybe children
//! (Everything is a Note). SQLite is the truth; the search index is derived
//! and rebuilt on demand.

use crate::article::read_article;
use crate::library::IndexDoc;
use crate::markdown;
use crate::props::Dims;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

pub struct NotesStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct NoteRef {
    pub id: i64,
    pub title: String,
    pub has_children: bool,
    /// a mind map (see `mindmap.rs`), else a Markdown note
    pub mind: bool,
    /// a whiteboard (see `whiteboard.rs`)
    pub board: bool,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub id: i64,
    pub parent: Option<i64>,
    pub title: String,
    pub content: String,
    pub content_type: String,
    /// place among its siblings
    pub position: i64,
    pub created: i64,
    pub updated: i64,
}

/// The marker of the JSON kept in a `deleted` snapshot.
pub const DELETED_FORMAT: &str = "smart-library-deleted-notes";

/// A deleted note and everything below it, as kept for the trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletedNode {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub position: i64,
    #[serde(default)]
    pub children: Vec<DeletedNode>,
}

/// The properties, tags and other names of one deleted note (kept apart from
/// the tree: the note store does not know them).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletedMeta {
    /// the note's id before it was deleted
    pub id: i64,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub props: Dims,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// A deleted subtree together with where it stood: the snapshot's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletedTree {
    pub format: String,
    /// the titles from the top of the library down to the parent: restoring a
    /// missing level makes a new (empty) note with that title
    #[serde(default)]
    pub path: Vec<String>,
    /// snapshots of the first version: the parent and ancestor ids, used when
    /// `path` is empty (they are not filled in any more)
    #[serde(default)]
    pub parent: Option<i64>,
    #[serde(default)]
    pub ancestors: Vec<i64>,
    /// properties / tags / other names, by the note's old id
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta: Vec<DeletedMeta>,
    pub root: DeletedNode,
}

impl DeletedTree {
    pub fn encode(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| format!("删除快照写入失败：{}", e))
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        let tree: DeletedTree = serde_json::from_str(text).map_err(|e| format!("删除快照格式不对：{}", e))?;
        if tree.format != DELETED_FORMAT {
            return Err("这不是已删除笔记的快照".into());
        }
        Ok(tree)
    }

    /// How many notes it holds (the root included).
    pub fn len(&self) -> usize {
        fn count(n: &DeletedNode) -> usize {
            1 + n.children.iter().map(count).sum::<usize>()
        }
        count(&self.root)
    }

    /// A deleted tree always holds its root, so this is never true; it exists
    /// because clippy asks for it next to `len`.
    pub fn is_empty(&self) -> bool {
        false
    }
}

/// What restoring a deleted tree gives back: the new root id, the `(old, new)`
/// id of every note it made, and the parent titles it had to create.
type Restored = (i64, Vec<(i64, i64)>, Vec<String>);

/// A note with the titles of the folders above it.
#[derive(Debug, Clone)]
pub struct NoteRow {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub content_type: String,
    pub folders: Vec<String>,
    /// ids of the notes above it, top first
    pub ancestors: Vec<i64>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Change the count of each picture by `delta`; one that drops below one starts its 24 hours.
fn bump(c: &Connection, names: &[String], delta: i64) -> Result<(), String> {
    let t = now();
    for n in names {
        c.execute(
            "INSERT INTO attach_refs (name, refs, zero_since) VALUES (?1, MAX(?2, 0), CASE WHEN ?2 < 1 THEN ?3 END)
             ON CONFLICT(name) DO UPDATE SET refs = MAX(refs + ?2, 0),
                 zero_since = CASE WHEN refs + ?2 < 1 THEN COALESCE(zero_since, ?3) END",
            params![n, delta, t],
        )
        .map_err(err)?;
    }
    Ok(())
}

fn err(e: rusqlite::Error) -> String {
    format!("笔记库出错：{}", e)
}

impl NotesStore {
    pub fn open(path: &Path) -> Result<NotesStore, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(err)?;
        Self::init(conn)
    }

    pub fn memory() -> Result<NotesStore, String> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<NotesStore, String> {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS notes (
                 id INTEGER PRIMARY KEY,
                 parent_id INTEGER REFERENCES notes(id) ON DELETE CASCADE,
                 title TEXT NOT NULL,
                 content TEXT NOT NULL DEFAULT '',
                 content_type TEXT NOT NULL DEFAULT 'md',
                 position INTEGER NOT NULL DEFAULT 0,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS notes_parent ON notes(parent_id, position);
             -- pictures a deleted snapshot still needs: the sweep leaves them alone
             CREATE TABLE IF NOT EXISTS trash_refs (
                 snapshot_id INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 PRIMARY KEY (snapshot_id, name)
             );",
        )
        .map_err(err)?;
        // The picture table is new in this version: count what the notes refer to once.
        let fresh: bool = conn
            .query_row("SELECT NOT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'attach_refs')", [], |r| r.get(0))
            .map_err(err)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS attach_refs (
                 name TEXT PRIMARY KEY,
                 refs INTEGER NOT NULL DEFAULT 0,
                 zero_since INTEGER
             );",
        )
        .map_err(err)?;
        let store = NotesStore { conn: Mutex::new(conn) };
        if fresh {
            store.recount()?;
        }
        Ok(store)
    }

    /// Count the pictures again from the notes (what each name is referred to by).
    pub fn recount(&self) -> Result<(), String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.savepoint().map_err(err)?;
        let mut counts: HashMap<String, i64> = HashMap::new();
        {
            let mut st = tx.prepare("SELECT content FROM notes").map_err(err)?;
            let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
            for content in rows {
                for n in crate::attach::refs(&content.map_err(err)?) {
                    *counts.entry(n).or_default() += 1;
                }
            }
        }
        let t = now();
        tx.execute("UPDATE attach_refs SET refs = 0", []).map_err(err)?;
        for (n, k) in &counts {
            tx.execute(
                "INSERT INTO attach_refs (name, refs, zero_since) VALUES (?1, ?2, NULL)
                 ON CONFLICT(name) DO UPDATE SET refs = ?2, zero_since = NULL",
                params![n, k],
            )
            .map_err(err)?;
        }
        tx.execute("UPDATE attach_refs SET zero_since = ?1 WHERE refs < 1 AND zero_since IS NULL", params![t]).map_err(err)?;
        tx.commit().map_err(err)
    }

    /// A picture was stored but no note refers to it yet: it starts its 24 hours.
    pub fn register_picture(&self, name: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("INSERT OR IGNORE INTO attach_refs (name, refs, zero_since) VALUES (?1, 0, ?2)", params![name, now()]).map_err(err)?;
        Ok(())
    }

    /// Names whose count has been below one for at least `older_than` seconds are forgotten and
    /// returned (the caller deletes the files). Pictures in `on_disk` that have no record start
    /// their time now.
    pub fn sweep_pictures(&self, older_than: i64, on_disk: &[String]) -> Result<Vec<String>, String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.savepoint().map_err(err)?;
        let t = now();
        for n in on_disk {
            tx.execute("INSERT OR IGNORE INTO attach_refs (name, refs, zero_since) VALUES (?1, 0, ?2)", params![n, t]).map_err(err)?;
        }
        let gone: Vec<String> = {
            let mut st = tx
                .prepare(
                    "SELECT name FROM attach_refs a
                     WHERE refs < 1 AND COALESCE(zero_since, 0) <= ?1
                       AND NOT EXISTS (SELECT 1 FROM trash_refs t WHERE t.name = a.name)",
                )
                .map_err(err)?;
            let rows = st.query_map(params![t - older_than], |r| r.get::<_, String>(0)).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        for n in &gone {
            tx.execute("DELETE FROM attach_refs WHERE name = ?1", params![n]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(gone)
    }

    /// `(name, refs)` of every known picture (for tests and the settings page).
    pub fn picture_refs(&self) -> Result<Vec<(String, i64)>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT name, refs FROM attach_refs ORDER BY name").map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    /// Group many writes into one transaction (import); end with `commit` or `rollback`.
    pub fn begin(&self) -> Result<(), String> {
        self.conn.lock().unwrap().execute_batch("BEGIN").map_err(err)
    }

    pub fn commit(&self) -> Result<(), String> {
        self.conn.lock().unwrap().execute_batch("COMMIT").map_err(err)
    }

    pub fn rollback(&self) {
        let _ = self.conn.lock().unwrap().execute_batch("ROLLBACK");
    }

    /// One level of the tree (`None`: the top of the library).
    pub fn children(&self, parent: Option<i64>) -> Result<Vec<NoteRef>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare(
                "SELECT id, title, EXISTS(SELECT 1 FROM notes k WHERE k.parent_id = n.id), content_type
                 FROM notes n WHERE parent_id IS ?1 ORDER BY position, id",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![parent], |r| {
                let kind: String = r.get(3)?;
                Ok(NoteRef {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    has_children: r.get(2)?,
                    mind: kind == crate::mindmap::TYPE,
                    board: kind == crate::whiteboard::TYPE,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub fn get(&self, id: i64) -> Result<Option<Note>, String> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id, parent_id, title, content, content_type, position, created, updated FROM notes WHERE id = ?1",
            params![id],
            |r| {
                Ok(Note {
                    id: r.get(0)?,
                    parent: r.get(1)?,
                    title: r.get(2)?,
                    content: r.get(3)?,
                    content_type: r.get(4)?,
                    position: r.get(5)?,
                    created: r.get(6)?,
                    updated: r.get(7)?,
                })
            },
        )
        .optional()
        .map_err(err)
    }

    /// Add a note as the last child of `parent`.
    pub fn create(&self, parent: Option<i64>, title: &str, content: &str) -> Result<i64, String> {
        self.create_typed(parent, title, content, "md")
    }

    /// [`NotesStore::create`] with a content type (`md` or `mindmap`).
    pub fn create_typed(&self, parent: Option<i64>, title: &str, content: &str, content_type: &str) -> Result<i64, String> {
        self.create_inner(None, parent, title, content, content_type)
    }

    /// Create a note with an explicit numeric `id` (used for fixed diary node IDs).
    pub fn create_with_id(&self, id: i64, parent: Option<i64>, title: &str, content: &str, content_type: &str) -> Result<i64, String> {
        self.create_inner(Some(id), parent, title, content, content_type)
    }

    fn create_inner(&self, fixed_id: Option<i64>, parent: Option<i64>, title: &str, content: &str, content_type: &str) -> Result<i64, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("标题不能为空".into());
        }
        let mut c = self.conn.lock().unwrap();
        let tx = c.savepoint().map_err(err)?;
        if let Some(p) = parent {
            let ok: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1)", params![p], |r| r.get(0))
                .map_err(err)?;
            if !ok {
                return Err("上级笔记不存在".into());
            }
        }
        let pos: i64 = tx
            .query_row("SELECT COALESCE(MAX(position) + 1, 0) FROM notes WHERE parent_id IS ?1", params![parent], |r| r.get(0))
            .map_err(err)?;
        let t = now();
        let id = if let Some(fid) = fixed_id {
            tx.execute(
                "INSERT INTO notes (id, parent_id, title, content, content_type, position, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![fid, parent, title, content, content_type, pos, t],
            )
            .map_err(err)?;
            fid
        } else {
            tx.execute(
                "INSERT INTO notes (parent_id, title, content, content_type, position, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![parent, title, content, content_type, pos, t],
            )
            .map_err(err)?;
            tx.last_insert_rowid() // before `bump` writes another table
        };
        bump(&tx, &crate::attach::refs(content), 1)?;
        tx.commit().map_err(err)?;
        Ok(id)
    }

    pub fn update(&self, id: i64, title: Option<&str>, content: Option<&str>) -> Result<(), String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.savepoint().map_err(err)?;
        if let Some(t) = title {
            if t.trim().is_empty() {
                return Err("标题不能为空".into());
            }
            tx.execute("UPDATE notes SET title = ?1, updated = ?2 WHERE id = ?3", params![t.trim(), now(), id]).map_err(err)?;
        }
        if let Some(t) = content {
            let old: Option<String> = tx.query_row("SELECT content FROM notes WHERE id = ?1", params![id], |r| r.get(0)).ok();
            if let Some(old) = old {
                let (before, after) = (crate::attach::refs(&old), crate::attach::refs(t));
                bump(&tx, &after.iter().filter(|n| !before.contains(n)).cloned().collect::<Vec<_>>(), 1)?;
                bump(&tx, &before.iter().filter(|n| !after.contains(n)).cloned().collect::<Vec<_>>(), -1)?;
            }
            tx.execute("UPDATE notes SET content = ?1, updated = ?2 WHERE id = ?3", params![t, now(), id]).map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    /// Add text at the end of a note, after a blank line.
    pub fn append(&self, id: i64, text: &str) -> Result<(), String> {
        let note = self.get(id)?.ok_or("笔记不存在")?;
        if note.content_type == crate::mindmap::TYPE {
            return Err("脑图不能追加文字，请用「加入脑图」".into());
        }
        if note.content_type == crate::whiteboard::TYPE {
            return Err("白板不能追加文字".into());
        }
        let old = note.content.trim_end();
        let content = if old.is_empty() { format!("{}
", text.trim()) } else { format!("{}

{}
", old, text.trim()) };
        self.update(id, None, Some(&content))
    }

    /// Delete a note with everything below it; the number of notes removed.
    pub fn delete(&self, id: i64) -> Result<usize, String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.savepoint().map_err(err)?;
        let n: i64 = tx
            .query_row(
                "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION ALL SELECT n.id FROM notes n JOIN sub ON n.parent_id = sub.id)
                 SELECT COUNT(*) FROM sub WHERE EXISTS(SELECT 1 FROM notes WHERE id = sub.id)",
                params![id],
                |r| r.get(0),
            )
            .map_err(err)?;
        let contents: Vec<String> = {
            let mut st = tx
                .prepare(
                    "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION ALL SELECT n.id FROM notes n JOIN sub ON n.parent_id = sub.id)
                     SELECT content FROM notes WHERE id IN (SELECT id FROM sub)",
                )
                .map_err(err)?;
            let rows = st.query_map(params![id], |r| r.get::<_, String>(0)).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        for content in &contents {
            bump(&tx, &crate::attach::refs(content), -1)?;
        }
        tx.execute("DELETE FROM notes WHERE id = ?1", params![id]).map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(n as usize)
    }

    /// `id` and everything below it, as the tree kept in a deleted snapshot.
    pub fn subtree(&self, id: i64) -> Result<DeletedNode, String> {
        let note = self.get(id)?.ok_or("笔记不存在")?;
        let mut children = Vec::new();
        for c in self.children(Some(id))? {
            children.push(self.subtree(c.id)?);
        }
        Ok(DeletedNode {
            id: note.id,
            title: note.title,
            content: note.content,
            content_type: note.content_type,
            position: note.position,
            children,
        })
    }

    /// A title for a restored note: the old one, or `old 2`, `old 3` … when a
    /// sibling already has it.
    pub fn free_restore_title(&self, parent: Option<i64>, base: &str) -> Result<String, String> {
        let base = base.trim();
        let base = if base.is_empty() { "笔记" } else { base };
        if self.find_child(parent, base)?.is_none() {
            return Ok(base.to_string());
        }
        for n in 2..10000 {
            let t = format!("{} {}", base, n);
            if self.find_child(parent, &t)?.is_none() {
                return Ok(t);
            }
        }
        Err("没有可用的标题".into())
    }

    /// The note at a path of titles, making the missing levels (`None`: the top);
    /// also tells which titles were made, for the restore notice.
    pub fn ensure_path_report(&self, parts: &[String]) -> Result<(Option<i64>, Vec<String>), String> {
        let mut cur = None;
        let mut made = Vec::new();
        for p in parts {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            cur = Some(match self.find_child(cur, p)? {
                Some(id) => id,
                None => {
                    made.push(p.to_string());
                    self.create(cur, p, "")?
                }
            });
        }
        Ok((cur, made))
    }

    /// Put a deleted tree back at the path of titles it was under: an existing
    /// level is reused, a missing one is made as an empty note. Only the
    /// selected note itself is numbered when its title is taken; its children
    /// are new below it. A note whose original id is free gets it again, so a
    /// diary day and the marks of that note find it; a taken id gets a new one.
    /// Returns the new root id, `(old id, new id)` for every note made, and the
    /// parent titles newly made.
    pub fn restore_deleted(&self, tree: &DeletedTree) -> Result<Restored, String> {
        // all notes or none: a half-made subtree would be hard to tell apart
        self.begin()?;
        match self.restore_deleted_inner(tree) {
            Ok(v) => {
                if let Err(e) = self.commit() {
                    self.rollback();
                    return Err(e);
                }
                Ok(v)
            }
            Err(e) => {
                self.rollback();
                Err(e)
            }
        }
    }

    fn restore_deleted_inner(&self, tree: &DeletedTree) -> Result<Restored, String> {
        let (parent, created) = if tree.path.is_empty() {
            // an entry of the first version: only ids, so the nearest one still there
            (self.deleted_parent(tree)?, Vec::new())
        } else {
            self.ensure_path_report(&tree.path)?
        };
        let mut made = Vec::new();
        let root = self.restore_node(parent, &tree.root, true, &mut made)?;
        // back to its old place among its siblings (clamped when they changed)
        let _ = self.move_to(root, parent, tree.root.position.max(0) as usize);
        Ok((root, made, created))
    }

    /// Snapshots of the first version have no title path: go below the original
    /// parent when it is still there, else the nearest ancestor, else the top.
    pub fn deleted_parent(&self, tree: &DeletedTree) -> Result<Option<i64>, String> {
        if let Some(p) = tree.parent {
            if self.get(p)?.is_some() {
                return Ok(Some(p));
            }
        }
        for id in tree.ancestors.iter().rev() {
            if self.get(*id)?.is_some() {
                return Ok(Some(*id));
            }
        }
        Ok(None)
    }

    fn restore_node(&self, parent: Option<i64>, node: &DeletedNode, is_root: bool, made: &mut Vec<(i64, i64)>) -> Result<i64, String> {
        let title = if is_root {
            self.free_restore_title(parent, &node.title)?
        } else {
            let t = node.title.trim();
            if t.is_empty() {
                "笔记".to_string()
            } else {
                t.to_string()
            }
        };
        let id = if self.get(node.id)?.is_none() {
            self.create_with_id(node.id, parent, &title, &node.content, &node.content_type)?
        } else {
            self.create_typed(parent, &title, &node.content, &node.content_type)?
        };
        made.push((node.id, id));
        for c in &node.children {
            self.restore_node(Some(id), c, false, made)?;
        }
        Ok(id)
    }

    /// Keep the pictures a deleted tree uses alive while it waits in the trash
    /// (the daily sweep leaves held names alone).
    pub fn hold_deleted_pictures(&self, snapshot_id: i64, tree: &DeletedTree) -> Result<(), String> {
        fn walk(n: &DeletedNode, out: &mut Vec<String>) {
            for r in crate::attach::refs(&n.content) {
                if !out.contains(&r) {
                    out.push(r);
                }
            }
            for c in &n.children {
                walk(c, out);
            }
        }
        let mut names = Vec::new();
        walk(&tree.root, &mut names);
        let c = self.conn.lock().unwrap();
        for n in &names {
            c.execute("INSERT OR IGNORE INTO trash_refs (snapshot_id, name) VALUES (?1, ?2)", params![snapshot_id, n]).map_err(err)?;
        }
        Ok(())
    }

    /// A deleted snapshot was restored or thrown away: stop keeping its pictures.
    pub fn release_deleted_pictures(&self, snapshot_id: i64) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM trash_refs WHERE snapshot_id = ?1", params![snapshot_id]).map_err(err)?;
        Ok(())
    }

    /// Move a note below `new_parent` at `index` among its children.
    pub fn move_to(&self, id: i64, new_parent: Option<i64>, index: usize) -> Result<(), String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.savepoint().map_err(err)?;
        if let Some(mut p) = new_parent {
            // not into itself or its own descendants
            loop {
                if p == id {
                    return Err("不能移动到自己里面".into());
                }
                match tx.query_row("SELECT parent_id FROM notes WHERE id = ?1", params![p], |r| r.get::<_, Option<i64>>(0)).optional().map_err(err)? {
                    Some(Some(up)) => p = up,
                    Some(None) => break,
                    None => return Err("目标笔记不存在".into()),
                }
            }
        }
        let mut ids: Vec<i64> = {
            let mut st = tx
                .prepare("SELECT id FROM notes WHERE parent_id IS ?1 AND id != ?2 ORDER BY position, id")
                .map_err(err)?;
            let rows = st.query_map(params![new_parent, id], |r| r.get(0)).map_err(err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(err)?
        };
        ids.insert(index.min(ids.len()), id);
        tx.execute("UPDATE notes SET parent_id = ?1 WHERE id = ?2", params![new_parent, id]).map_err(err)?;
        for (pos, i) in ids.iter().enumerate() {
            tx.execute("UPDATE notes SET position = ?1 WHERE id = ?2", params![pos as i64, i]).map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    /// A title for a new note below `parent`: `base 1`, `base 2`, … the first
    /// one that no sibling has.
    pub fn free_title(&self, parent: Option<i64>, base: &str) -> Result<String, String> {
        let mut n = 1;
        loop {
            let t = format!("{} {}", base, n);
            if self.find_child(parent, &t)?.is_none() {
                return Ok(t);
            }
            n += 1;
        }
    }

    /// The child of `parent` with this title (the first one).
    pub fn find_child(&self, parent: Option<i64>, title: &str) -> Result<Option<i64>, String> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id FROM notes WHERE parent_id IS ?1 AND title = ?2 ORDER BY position, id LIMIT 1",
            params![parent, title],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)
    }

    /// The note at a path of titles, making the missing ones on the way
    /// (`None`: the top of the library, for an empty path).
    pub fn ensure_path(&self, parts: &[String]) -> Result<Option<i64>, String> {
        Ok(self.ensure_path_report(parts)?.0)
    }

    /// `id` and every note below it.
    pub fn descendants(&self, id: i64) -> Result<Vec<i64>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare(
                "WITH RECURSIVE sub(id) AS (SELECT id FROM notes WHERE id = ?1
                 UNION ALL SELECT n.id FROM notes n JOIN sub ON n.parent_id = sub.id) SELECT id FROM sub",
            )
            .map_err(err)?;
        let rows = st.query_map(params![id], |r| r.get(0)).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }

    /// Ids from the top down to `id` (itself last).
    pub fn chain(&self, id: i64) -> Result<Vec<i64>, String> {
        let mut out = vec![id];
        let mut cur = self.get(id)?.and_then(|n| n.parent);
        while let Some(p) = cur {
            out.push(p);
            cur = self.get(p)?.and_then(|n| n.parent);
        }
        out.reverse();
        Ok(out)
    }

    /// Titles of the notes above `id`, top first.
    pub fn folders_of(&self, id: i64) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        let mut cur = self.get(id)?.and_then(|n| n.parent);
        while let Some(p) = cur {
            let n = self.get(p)?.ok_or("笔记树损坏")?;
            out.push(n.title);
            cur = n.parent;
        }
        out.reverse();
        Ok(out)
    }

    /// Every note with its folder chain, in tree order.
    pub fn rows(&self) -> Result<Vec<NoteRow>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT id, parent_id, title, content, content_type FROM notes ORDER BY position, id")
            .map_err(err)?;
        let all: Vec<(i64, Option<i64>, String, String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let info: HashMap<i64, (Option<i64>, &str)> = all.iter().map(|(i, p, t, _, _)| (*i, (*p, t.as_str()))).collect();
        let mut children: HashMap<Option<i64>, Vec<usize>> = HashMap::new();
        for (n, (_, p, _, _, _)) in all.iter().enumerate() {
            children.entry(*p).or_default().push(n);
        }
        // depth-first, so that notes come out in tree order
        let mut out = Vec::with_capacity(all.len());
        let mut stack: Vec<usize> = children.get(&None).map(|v| v.iter().rev().copied().collect()).unwrap_or_default();
        while let Some(n) = stack.pop() {
            let (id, _, title, content, content_type) = &all[n];
            let mut folders = Vec::new();
            let mut ancestors = Vec::new();
            let mut cur = info[id].0;
            while let Some(p) = cur {
                let (up, t) = info[&p];
                folders.push(t.to_string());
                ancestors.push(p);
                cur = up;
            }
            folders.reverse();
            ancestors.reverse();
            out.push(NoteRow { id: *id, title: title.clone(), content: content.clone(), content_type: content_type.clone(), folders, ancestors });
            if let Some(k) = children.get(&Some(*id)) {
                stack.extend(k.iter().rev());
            }
        }
        Ok(out)
    }

    /// Changes whenever a note is added, removed or edited.
    pub fn fingerprint(&self) -> String {
        let c = self.conn.lock().unwrap();
        c.query_row("SELECT COUNT(*), COALESCE(MAX(updated), 0), COALESCE(SUM(LENGTH(content)), 0) FROM notes", [], |r| {
            Ok(format!("{}:{}:{}", r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })
        .unwrap_or_default()
    }
}

/// The article view of a note (Markdown -> paragraphs).
pub fn article(content: &str, render: Option<&crate::render::RenderOptions>) -> crate::article::Article {
    read_article(&markdown::to_html(content), "", render)
}

/// Index documents for the notes with content; the title counts as content
/// too, so a note with only a title is still found by it.
pub fn docs(rows: &[NoteRow], library_title: &str) -> Vec<IndexDoc> {
    rows.iter()
        .map(|r| {
            let a = article(&r.content, None);
            let text = match r.content_type.as_str() {
                crate::mindmap::TYPE => crate::mindmap::text(&r.content),
                crate::whiteboard::TYPE => crate::whiteboard::text(&r.content),
                _ => a.text.text,
            };
            let mut facets = Vec::new();
            let mut bulei = vec!["bulei".to_string()];
            bulei.extend(r.folders.iter().take(2).cloned());
            if bulei.len() > 1 {
                facets.push(bulei);
            }
            let book = r.folders.last().cloned().unwrap_or_else(|| library_title.to_string());
            facets.push(vec!["book".to_string(), book.clone()]);
            IndexDoc {
                path: r.id.to_string(),
                book,
                juan: 0,
                title: r.title.clone(),
                text,
                facets,
                keys: r.ancestors.iter().chain(std::iter::once(&r.id)).map(|i| (String::new(), i.to_string())).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_picture_references() {
        let a = "00112233445566778899aabbccddeeff.png";
        let b = "ffeeddccbbaa99887766554433221100.png";
        let s = NotesStore::memory().unwrap();
        let n1 = s.create(None, "一", &format!("![](attach:{})", a)).unwrap();
        let n2 = s.create(None, "二", &format!("![](attach:{}) ![](attach:{})", a, b)).unwrap();
        assert_eq!(s.picture_refs().unwrap(), vec![(a.into(), 2), (b.into(), 1)]);
        s.update(n2, None, Some("没有图了")).unwrap();
        assert_eq!(s.picture_refs().unwrap(), vec![(a.into(), 1), (b.into(), 0)]);
        // below one but younger than the time: kept; the one never seen on disk is added
        assert!(s.sweep_pictures(86400, &[]).unwrap().is_empty());
        s.delete(n1).unwrap();
        assert_eq!(s.picture_refs().unwrap(), vec![(a.into(), 0), (b.into(), 0)]);
        let mut gone = s.sweep_pictures(0, &[]).unwrap();
        gone.sort();
        assert_eq!(gone, vec![a.to_string(), b.to_string()]);
        assert!(s.picture_refs().unwrap().is_empty());
        // a picture that is only pasted
        s.register_picture(a).unwrap();
        assert_eq!(s.picture_refs().unwrap(), vec![(a.into(), 0)]);
        s.create(None, "三", &format!("![](attach:{})", a)).unwrap();
        s.recount().unwrap();
        assert_eq!(s.picture_refs().unwrap(), vec![(a.into(), 1)]);
    }

    #[test]
    fn tree_crud_and_move() {
        let s = NotesStore::memory().unwrap();
        let a = s.create(None, "甲", "# 甲\n正文").unwrap();
        let b = s.create(None, "乙", "").unwrap();
        let a1 = s.create(Some(a), "甲一", "如是我闻").unwrap();
        let a2 = s.create(Some(a), "甲二", "").unwrap();
        assert!(s.create(None, "  ", "").is_err());
        let top = s.children(None).unwrap();
        assert_eq!(top.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["甲", "乙"]);
        assert!(top[0].has_children && !top[1].has_children);

        s.append(a2, "> 引文").unwrap();
        s.append(a2, "> 又一段
").unwrap();
        assert_eq!(s.get(a2).unwrap().unwrap().content, "> 引文

> 又一段
");
        assert!(s.append(9999, "x").is_err());
        s.update(a1, Some("甲·一"), Some("如是我闻，一时")).unwrap();
        assert_eq!(s.get(a1).unwrap().unwrap().title, "甲·一");
        assert_eq!(s.folders_of(a1).unwrap(), vec!["甲"]);

        // order: a2 goes first among a's children, then b moves under a2
        s.move_to(a2, Some(a), 0).unwrap();
        assert_eq!(s.children(Some(a)).unwrap()[0].id, a2);
        s.move_to(b, Some(a2), 0).unwrap();
        assert!(s.children(Some(a2)).unwrap().len() == 1);
        // not into its own subtree
        assert!(s.move_to(a, Some(b), 0).is_err());
        assert!(s.move_to(a, Some(a), 0).is_err());

        let rows = s.rows().unwrap();
        assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["甲", "甲二", "乙", "甲·一"]);
        assert_eq!(rows[2].folders, vec!["甲", "甲二"]);

        let before = s.fingerprint();
        assert_eq!(s.delete(a).unwrap(), 4);
        assert!(s.children(None).unwrap().is_empty());
        assert_ne!(before, s.fingerprint());
    }

    #[test]
    fn makes_missing_path_nodes() {
        let s = NotesStore::memory().unwrap();
        let a = s.create(None, "a", "").unwrap();
        let path: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let c = s.ensure_path(&path).unwrap().unwrap();
        // `a` was reused, `b` and `c` made; a second call changes nothing
        assert_eq!(s.chain(c).unwrap().len(), 3);
        assert_eq!(s.chain(c).unwrap()[0], a);
        assert_eq!(s.ensure_path(&path).unwrap(), Some(c));
        assert_eq!(s.rows().unwrap().len(), 3);
        assert_eq!(s.ensure_path(&[]).unwrap(), None);
    }

    #[test]
    fn index_docs_read_markdown() {
        let s = NotesStore::memory().unwrap();
        let a = s.create(None, "章", "").unwrap();
        s.create(Some(a), "节", "# 节\n如是**我**闻").unwrap();
        let docs = docs(&s.rows().unwrap(), "笔记");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[1].book, "章");
        assert!(docs[1].text.contains("如是我闻"));
    }

    #[test]
    fn free_titles_and_boards() {
        let s = NotesStore::memory().unwrap();
        assert_eq!(s.free_title(None, "笔记").unwrap(), "笔记 1");
        s.create(None, "笔记 1", "").unwrap();
        s.create(None, "笔记 3", "").unwrap();
        assert_eq!(s.free_title(None, "笔记").unwrap(), "笔记 2");
        let b = s.create_typed(None, "白板 1", &crate::whiteboard::new_data(), crate::whiteboard::TYPE).unwrap();
        assert!(s.children(None).unwrap().iter().any(|n| n.board && n.id == b));
        assert!(s.append(b, "x").is_err());
    }

    #[test]
    fn nested_savepoint_and_rollback_atomic() {
        let pic = "00112233445566778899aabbccddeeff.png";
        let s = NotesStore::memory().unwrap();
        s.begin().unwrap();
        let id = s.create(None, "事务内笔记", &format!("![](attach:{pic})")).unwrap();
        assert!(s.get(id).unwrap().is_some());
        assert_eq!(s.picture_refs().unwrap(), vec![(pic.into(), 1)]);
        s.rollback();
        assert!(s.get(id).unwrap().is_none());
        assert!(s.picture_refs().unwrap().is_empty());
    }

    fn deleted_tree(s: &NotesStore, id: i64, path: Vec<&str>) -> DeletedTree {
        DeletedTree {
            format: DELETED_FORMAT.into(),
            path: path.into_iter().map(String::from).collect(),
            parent: None,
            ancestors: Vec::new(),
            meta: Vec::new(),
            root: s.subtree(id).unwrap(),
        }
    }

    #[test]
    fn deleted_subtrees_come_back_with_their_ids_and_place() {
        let s = NotesStore::memory().unwrap();
        let before = s.create(None, "前", "").unwrap();
        let p = s.create(None, "书", "题").unwrap();
        let a = s.create(Some(p), "笔记 1", "甲").unwrap();
        let b = s.create(Some(p), "笔记 2", "乙").unwrap();
        let after = s.create(None, "后", "").unwrap();
        let text = deleted_tree(&s, p, vec![]).encode().unwrap();
        let tree = DeletedTree::decode(&text).unwrap();
        assert_eq!(tree.len(), 3);
        s.delete(p).unwrap();
        assert!(s.get(a).unwrap().is_none());

        // nothing took the ids: the same notes come back at the same place
        let (root, made, created) = s.restore_deleted(&tree).unwrap();
        assert_eq!(root, p);
        assert_eq!(made, vec![(p, p), (a, a), (b, b)]);
        assert!(created.is_empty());
        assert_eq!(s.children(None).unwrap().iter().map(|n| n.id).collect::<Vec<_>>(), vec![before, p, after]);
        assert_eq!(s.get(root).unwrap().unwrap().title, "书");
        assert_eq!(s.get(a).unwrap().unwrap().content, "甲");
        assert_eq!(s.get(a).unwrap().unwrap().parent, Some(p));
    }

    #[test]
    fn a_taken_id_and_title_are_not_reused() {
        let s = NotesStore::memory().unwrap();
        let p = s.create(None, "书", "题").unwrap();
        let _a = s.create(Some(p), "笔记 1", "甲").unwrap();
        let tree = deleted_tree(&s, p, vec![]);
        s.delete(p).unwrap();
        // the table is empty, so the next note takes the old root id
        let other = s.create(None, "书", "新的").unwrap();
        assert_eq!(other, p);
        let (root, made, _) = s.restore_deleted(&tree).unwrap();
        assert_ne!(root, p);
        assert_eq!(made.len(), 2);
        assert_eq!(s.get(root).unwrap().unwrap().title, "书 2");
        assert_eq!(s.get(root).unwrap().unwrap().parent, None);
        assert_eq!(s.get(other).unwrap().unwrap().content, "新的");
        for (_, m) in &made {
            assert!(s.get(*m).unwrap().is_some());
        }
    }

    #[test]
    fn a_deleted_note_rebuilds_its_path_without_touching_the_parents_trash() {
        let s = NotesStore::memory().unwrap();
        let book = s.create(None, "Book", "书的内容").unwrap();
        let note1 = s.create(Some(book), "Note1", "一").unwrap();
        let chapter = s.create(Some(book), "Chapter1", "章").unwrap();
        let chapter_tree = deleted_tree(&s, chapter, vec!["Book"]);
        let book_tree = deleted_tree(&s, book, vec![]);
        s.delete(chapter).unwrap();
        s.delete(book).unwrap(); // Note1 goes with it
        assert!(s.get(note1).unwrap().is_none());

        // Chapter1 alone: Book is made again as an empty parent, Note1 stays deleted
        let (root, made, created) = s.restore_deleted(&chapter_tree).unwrap();
        assert_eq!(made, vec![(chapter, root)]);
        assert_eq!(created, vec!["Book"]);
        let parent = s.get(root).unwrap().unwrap().parent.unwrap();
        let rebuilt = s.get(parent).unwrap().unwrap();
        assert_eq!((rebuilt.title.as_str(), rebuilt.content.as_str()), ("Book", ""));
        assert_eq!(s.get(root).unwrap().unwrap().title, "Chapter1");
        assert!(s.get(note1).unwrap().is_none());

        // Book's own entry, restored later: the title is taken, so `Book 2`, with Note1
        let (book2, _, created2) = s.restore_deleted(&book_tree).unwrap();
        assert!(created2.is_empty());
        assert_eq!(s.get(book2).unwrap().unwrap().title, "Book 2");
        assert_eq!(s.get(book2).unwrap().unwrap().parent, None);
        assert_eq!(s.get(note1).unwrap().unwrap().parent, Some(book2));
        // the rebuilt parent keeps Chapter1 and is not overwritten
        assert_eq!(s.get(parent).unwrap().unwrap().content, "");
    }

    #[test]
    fn a_missing_multi_level_path_is_made_top_down() {
        let s = NotesStore::memory().unwrap();
        let lib = s.create(None, "库", "").unwrap();
        let book = s.create(Some(lib), "书", "").unwrap();
        let ch = s.create(Some(book), "章", "文").unwrap();
        let tree = deleted_tree(&s, ch, vec!["库", "书"]);
        s.delete(ch).unwrap();
        s.delete(book).unwrap();
        s.delete(lib).unwrap();
        let (root, _, created) = s.restore_deleted(&tree).unwrap();
        assert_eq!(created, vec!["库", "书"]);
        let book_note = s.get(s.get(root).unwrap().unwrap().parent.unwrap()).unwrap().unwrap();
        let lib_note = s.get(book_note.parent.unwrap()).unwrap().unwrap();
        assert_eq!(lib_note.title, "库");
        assert_eq!(lib_note.parent, None);
        assert_eq!(book_note.title, "书");
    }

    #[test]
    fn a_legacy_deleted_tree_falls_back_to_the_nearest_parent() {
        let s = NotesStore::memory().unwrap();
        let q = s.create(None, "库", "").unwrap();
        let p = s.create(Some(q), "章", "").unwrap();
        let c = s.create(Some(p), "节", "文").unwrap();
        // an entry of the first version: ids only, no title path
        let mut tree = deleted_tree(&s, p, vec![]);
        tree.parent = Some(q);
        tree.ancestors = s.chain(q).unwrap();
        s.delete(p).unwrap();
        s.delete(q).unwrap();
        let (root, _, created) = s.restore_deleted(&tree).unwrap();
        assert!(created.is_empty());
        assert_eq!(s.get(root).unwrap().unwrap().parent, None);
        assert_eq!(s.get(root).unwrap().unwrap().title, "章");
        assert!(s.get(c).unwrap().is_some());
    }

    #[test]
    fn held_pictures_outlive_the_deleted_note() {
        let pic = "00112233445566778899aabbccddeeff.png";
        let s = NotesStore::memory().unwrap();
        let id = s.create(None, "图", &format!("![](attach:{pic})")).unwrap();
        let tree = deleted_tree(&s, id, vec![]);
        s.delete(id).unwrap();
        assert_eq!(s.picture_refs().unwrap(), vec![(pic.into(), 0)]);
        // without a hold the sweep would take it (the age is ignored here)
        s.hold_deleted_pictures(7, &tree).unwrap();
        assert!(s.sweep_pictures(-1, &[]).unwrap().is_empty());
        s.release_deleted_pictures(7).unwrap();
        assert_eq!(s.sweep_pictures(-1, &[]).unwrap(), vec![pic.to_string()]);
    }

    #[test]
    fn deleted_tree_decoding_checks_the_marker_and_the_path() {
        assert!(DeletedTree::decode("not json").is_err());
        assert!(DeletedTree::decode(r#"{"format":"other","root":{"id":1,"title":"x"}}"#).is_err());
        let tree = DeletedTree::decode(
            r#"{"format":"smart-library-deleted-notes","parent":null,"root":{"id":1,"title":"x","children":[]}}"#,
        )
        .unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root.title, "x");
        assert!(tree.path.is_empty());
        let tree = DeletedTree::decode(
            r#"{"format":"smart-library-deleted-notes","path":["库","书"],"root":{"id":1,"title":"x","children":[]}}"#,
        )
        .unwrap();
        assert_eq!(tree.path, vec!["库", "书"]);
        // properties, tags and other names travel with the tree
        let tree = DeletedTree::decode(
            r#"{"format":"smart-library-deleted-notes","path":["库"],"meta":[{"id":5,"props":{"period":["唐"]},"tags":["精读"],"aliases":["别称"]}],"root":{"id":1,"title":"x","children":[]}}"#,
        )
        .unwrap();
        assert_eq!(tree.meta.len(), 1);
        assert_eq!(tree.meta[0].id, 5);
        assert_eq!(tree.meta[0].props["period"], vec!["唐"]);
        assert_eq!(tree.meta[0].tags, vec!["精读"]);
        assert_eq!(tree.meta[0].aliases, vec!["别称"]);
        let round = DeletedTree::decode(&tree.encode().unwrap()).unwrap();
        assert_eq!(round, tree);
    }
}
