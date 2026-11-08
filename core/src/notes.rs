//! Internal (read-write) libraries (design-spec 6.4): notes kept in SQLite,
//! one database per library (`<data>/repos/<id>/notes.db`). The tree is made
//! by `parent_id` and `position`. Every note is the same kind of node: it
//! has a title, Markdown content (maybe empty) and maybe children
//! (Everything is a Note). SQLite is the truth; the search index is derived
//! and rebuilt on demand.

use crate::article::read_article;
use crate::library::IndexDoc;
use crate::markdown;
use rusqlite::{params, Connection, OptionalExtension};
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
    pub created: i64,
    pub updated: i64,
}

/// A note with the titles of the folders above it.
#[derive(Debug, Clone)]
pub struct NoteRow {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub content_type: String,
    pub folders: Vec<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
             CREATE INDEX IF NOT EXISTS notes_parent ON notes(parent_id, position);",
        )
        .map_err(err)?;
        Ok(NotesStore { conn: Mutex::new(conn) })
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
            "SELECT id, parent_id, title, content, content_type, created, updated FROM notes WHERE id = ?1",
            params![id],
            |r| {
                Ok(Note {
                    id: r.get(0)?,
                    parent: r.get(1)?,
                    title: r.get(2)?,
                    content: r.get(3)?,
                    content_type: r.get(4)?,
                    created: r.get(5)?,
                    updated: r.get(6)?,
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
        let title = title.trim();
        if title.is_empty() {
            return Err("标题不能为空".into());
        }
        let c = self.conn.lock().unwrap();
        if let Some(p) = parent {
            let ok: bool = c
                .query_row("SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1)", params![p], |r| r.get(0))
                .map_err(err)?;
            if !ok {
                return Err("上级笔记不存在".into());
            }
        }
        let pos: i64 = c
            .query_row("SELECT COALESCE(MAX(position) + 1, 0) FROM notes WHERE parent_id IS ?1", params![parent], |r| r.get(0))
            .map_err(err)?;
        let t = now();
        c.execute(
            "INSERT INTO notes (parent_id, title, content, content_type, position, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![parent, title, content, content_type, pos, t],
        )
        .map_err(err)?;
        Ok(c.last_insert_rowid())
    }

    pub fn update(&self, id: i64, title: Option<&str>, content: Option<&str>) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        if let Some(t) = title {
            if t.trim().is_empty() {
                return Err("标题不能为空".into());
            }
            c.execute("UPDATE notes SET title = ?1, updated = ?2 WHERE id = ?3", params![t.trim(), now(), id]).map_err(err)?;
        }
        if let Some(t) = content {
            c.execute("UPDATE notes SET content = ?1, updated = ?2 WHERE id = ?3", params![t, now(), id]).map_err(err)?;
        }
        Ok(())
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
        let c = self.conn.lock().unwrap();
        let n: i64 = c
            .query_row(
                "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION ALL SELECT n.id FROM notes n JOIN sub ON n.parent_id = sub.id)
                 SELECT COUNT(*) FROM sub WHERE EXISTS(SELECT 1 FROM notes WHERE id = sub.id)",
                params![id],
                |r| r.get(0),
            )
            .map_err(err)?;
        c.execute("DELETE FROM notes WHERE id = ?1", params![id]).map_err(err)?;
        Ok(n as usize)
    }

    /// Move a note below `new_parent` at `index` among its children.
    pub fn move_to(&self, id: i64, new_parent: Option<i64>, index: usize) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        if let Some(mut p) = new_parent {
            // not into itself or its own descendants
            loop {
                if p == id {
                    return Err("不能移动到自己里面".into());
                }
                match c.query_row("SELECT parent_id FROM notes WHERE id = ?1", params![p], |r| r.get::<_, Option<i64>>(0)).optional().map_err(err)? {
                    Some(Some(up)) => p = up,
                    Some(None) => break,
                    None => return Err("目标笔记不存在".into()),
                }
            }
        }
        let mut ids: Vec<i64> = {
            let mut st = c
                .prepare("SELECT id FROM notes WHERE parent_id IS ?1 AND id != ?2 ORDER BY position, id")
                .map_err(err)?;
            let rows = st.query_map(params![new_parent, id], |r| r.get(0)).map_err(err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(err)?
        };
        ids.insert(index.min(ids.len()), id);
        c.execute("UPDATE notes SET parent_id = ?1 WHERE id = ?2", params![new_parent, id]).map_err(err)?;
        for (pos, i) in ids.iter().enumerate() {
            c.execute("UPDATE notes SET position = ?1 WHERE id = ?2", params![pos as i64, i]).map_err(err)?;
        }
        Ok(())
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
        let mut cur = None;
        for p in parts {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            cur = Some(match self.find_child(cur, p)? {
                Some(id) => id,
                None => self.create(cur, p, "")?,
            });
        }
        Ok(cur)
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
            let mut cur = info[id].0;
            while let Some(p) = cur {
                let (up, t) = info[&p];
                folders.push(t.to_string());
                cur = up;
            }
            folders.reverse();
            out.push(NoteRow { id: *id, title: title.clone(), content: content.clone(), content_type: content_type.clone(), folders });
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
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
