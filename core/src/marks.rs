//! Bookmarks, collections and tags: one kind of record, a *mark*, kept in
//! our own SQLite file so that read-only libraries can be marked too.
//!
//! * `node`: a tag target only, a whole book / article (`path` empty for the
//!   CBETA tree's book level, a volume path when tagged from the reader);
//! * `bookmark`: a position (`lb`) with the text there;
//! * `clip`: selected text (`quote`) at a position, with an optional note.
//!
//! Every mark has tags. A mark remembers where it came from (library title,
//! document title, author), so it can still be shown after its library was
//! unbound; only opening it needs the library.

use rusqlite::{params, params_from_iter, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct Marks {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Default)]
pub struct MarkNew {
    /// `bookmark` or `clip`
    pub kind: String,
    pub lib: String,
    pub path: String,
    pub book_id: Option<String>,
    pub lb: Option<String>,
    pub quote: String,
    pub note: String,
    pub doc_title: String,
    pub lib_title: String,
    pub author: String,
}

#[derive(Debug, Clone)]
pub struct Mark {
    pub id: i64,
    pub kind: String,
    pub lib: String,
    pub path: String,
    pub book_id: Option<String>,
    pub lb: Option<String>,
    pub quote: String,
    pub note: String,
    pub doc_title: String,
    pub lib_title: String,
    pub author: String,
    pub created: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MarkFilter {
    pub tag: Option<String>,
    /// words found in the text, note, title or a tag
    pub text: String,
    /// one document: its library and path (and, for CBETA, the book)
    pub lib: Option<String>,
    pub path: Option<String>,
    pub book_id: Option<String>,
    /// empty: every kind
    pub kinds: Vec<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn err(e: rusqlite::Error) -> String {
    format!("收藏与标签出错：{}", e)
}

/// `"a, b，c"` -> `["a", "b", "c"]`: split on commas and semicolons, trimmed,
/// no empty or repeated tags.
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split([',', '，', '、', ';', '；']) {
        let t: String = t.trim().chars().take(40).collect();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

impl Marks {
    pub fn open(path: &Path) -> Result<Marks, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Self::init(Connection::open(path).map_err(err)?)
    }

    pub fn memory() -> Result<Marks, String> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<Marks, String> {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS marks (
                 id INTEGER PRIMARY KEY,
                 kind TEXT NOT NULL,
                 lib TEXT NOT NULL,
                 path TEXT NOT NULL DEFAULT '',
                 book_id TEXT,
                 lb TEXT,
                 quote TEXT NOT NULL DEFAULT '',
                 note TEXT NOT NULL DEFAULT '',
                 doc_title TEXT NOT NULL DEFAULT '',
                 lib_title TEXT NOT NULL DEFAULT '',
                 author TEXT NOT NULL DEFAULT '',
                 created INTEGER NOT NULL
             );
             CREATE UNIQUE INDEX IF NOT EXISTS marks_node
                 ON marks(lib, COALESCE(book_id, ''), path) WHERE kind = 'node';
             CREATE INDEX IF NOT EXISTS marks_doc ON marks(lib, path);
             CREATE TABLE IF NOT EXISTS mark_tags (
                 mark_id INTEGER NOT NULL REFERENCES marks(id) ON DELETE CASCADE,
                 tag TEXT NOT NULL,
                 PRIMARY KEY (mark_id, tag)
             );
             CREATE INDEX IF NOT EXISTS mark_tags_tag ON mark_tags(tag);",
        )
        .map_err(err)?;
        Ok(Marks { conn: Mutex::new(conn) })
    }

    fn put_tags(c: &Connection, id: i64, tags: &[String]) -> Result<(), String> {
        c.execute("DELETE FROM mark_tags WHERE mark_id = ?1", params![id]).map_err(err)?;
        for t in tags {
            c.execute("INSERT OR IGNORE INTO mark_tags (mark_id, tag) VALUES (?1, ?2)", params![id, t]).map_err(err)?;
        }
        Ok(())
    }

    pub fn add(&self, m: &MarkNew, tags: &[String]) -> Result<i64, String> {
        if m.kind != "bookmark" && m.kind != "clip" {
            return Err("不是书签或收藏".into());
        }
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO marks (kind, lib, path, book_id, lb, quote, note, doc_title, lib_title, author, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![m.kind, m.lib, m.path, m.book_id, m.lb, m.quote, m.note, m.doc_title, m.lib_title, m.author, now()],
        )
        .map_err(err)?;
        let id = c.last_insert_rowid();
        Self::put_tags(&c, id, tags)?;
        Ok(id)
    }

    /// Replace the tags of a whole book / article (no tags: forget it).
    pub fn set_node_tags(
        &self,
        lib: &str,
        book_id: Option<&str>,
        path: &str,
        doc_title: &str,
        lib_title: &str,
        tags: &[String],
    ) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        let found: Option<i64> = c
            .query_row(
                "SELECT id FROM marks WHERE kind = 'node' AND lib = ?1 AND COALESCE(book_id, '') = ?2 AND path = ?3",
                params![lib, book_id.unwrap_or(""), path],
                |r| r.get(0),
            )
            .ok();
        match (found, tags.is_empty()) {
            (Some(id), true) => {
                c.execute("DELETE FROM marks WHERE id = ?1", params![id]).map_err(err)?;
            }
            (Some(id), false) => {
                c.execute("UPDATE marks SET doc_title = ?2, lib_title = ?3 WHERE id = ?1", params![id, doc_title, lib_title]).map_err(err)?;
                Self::put_tags(&c, id, tags)?;
            }
            (None, true) => {}
            (None, false) => {
                c.execute(
                    "INSERT INTO marks (kind, lib, path, book_id, doc_title, lib_title, created) VALUES ('node', ?1, ?2, ?3, ?4, ?5, ?6)",
                    params![lib, path, book_id, doc_title, lib_title, now()],
                )
                .map_err(err)?;
                let id = c.last_insert_rowid();
                Self::put_tags(&c, id, tags)?;
            }
        }
        Ok(())
    }

    pub fn node_tags(&self, lib: &str, book_id: Option<&str>, path: &str) -> Result<Vec<String>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare(
                "SELECT t.tag FROM mark_tags t JOIN marks m ON m.id = t.mark_id
                 WHERE m.kind = 'node' AND m.lib = ?1 AND COALESCE(m.book_id, '') = ?2 AND m.path = ?3 ORDER BY t.tag",
            )
            .map_err(err)?;
        let rows = st.query_map(params![lib, book_id.unwrap_or(""), path], |r| r.get(0)).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }

    pub fn update(&self, id: i64, note: Option<&str>, tags: Option<&[String]>) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        if let Some(n) = note {
            c.execute("UPDATE marks SET note = ?1 WHERE id = ?2", params![n, id]).map_err(err)?;
        }
        if let Some(t) = tags {
            Self::put_tags(&c, id, t)?;
        }
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM marks WHERE id = ?1", params![id]).map_err(err)?;
        Ok(())
    }

    /// Newest first.
    pub fn list(&self, f: &MarkFilter, limit: usize) -> Result<Vec<Mark>, String> {
        let c = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, kind, lib, path, book_id, lb, quote, note, doc_title, lib_title, author, created FROM marks m WHERE 1 = 1",
        );
        let mut args: Vec<String> = Vec::new();
        if let Some(t) = &f.tag {
            sql.push_str(" AND EXISTS (SELECT 1 FROM mark_tags t WHERE t.mark_id = m.id AND t.tag = ?)");
            args.push(t.clone());
        }
        if let Some(lib) = &f.lib {
            sql.push_str(" AND m.lib = ?");
            args.push(lib.clone());
            if let Some(path) = &f.path {
                // this document's marks, and the tags of its book
                sql.push_str(" AND (m.path = ? OR (m.kind = 'node' AND ? != '' AND m.book_id = ?))");
                args.push(path.clone());
                args.push(f.book_id.clone().unwrap_or_default());
                args.push(f.book_id.clone().unwrap_or_default());
            }
        }
        if !f.kinds.is_empty() {
            sql.push_str(&format!(" AND m.kind IN ({})", vec!["?"; f.kinds.len()].join(", ")));
            args.extend(f.kinds.iter().cloned());
        }
        for w in f.text.split_whitespace() {
            sql.push_str(
                " AND (m.quote LIKE ? ESCAPE '\\' OR m.note LIKE ? ESCAPE '\\' OR m.doc_title LIKE ? ESCAPE '\\'
                   OR EXISTS (SELECT 1 FROM mark_tags t WHERE t.mark_id = m.id AND t.tag LIKE ? ESCAPE '\\'))",
            );
            let like = format!("%{}%", w.replace('%', "\\%").replace('_', "\\_"));
            args.extend(std::iter::repeat(like).take(4));
        }
        sql.push_str(&format!(" ORDER BY m.created DESC, m.id DESC LIMIT {}", limit.max(1)));
        let mut st = c.prepare(&sql).map_err(err)?;
        let rows = st
            .query_map(params_from_iter(args.iter()), |r| {
                Ok(Mark {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    lib: r.get(2)?,
                    path: r.get(3)?,
                    book_id: r.get(4)?,
                    lb: r.get(5)?,
                    quote: r.get(6)?,
                    note: r.get(7)?,
                    doc_title: r.get(8)?,
                    lib_title: r.get(9)?,
                    author: r.get(10)?,
                    created: r.get(11)?,
                    tags: Vec::new(),
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let mut rows = rows;
        let mut tq = c.prepare("SELECT tag FROM mark_tags WHERE mark_id = ?1 ORDER BY tag").map_err(err)?;
        for m in &mut rows {
            m.tags = tq
                .query_map(params![m.id], |r| r.get(0))
                .map_err(err)?
                .collect::<Result<Vec<String>, _>>()
                .map_err(err)?;
        }
        Ok(rows)
    }

    /// A document or folder was renamed or moved (`from`, `to`: paths in the
    /// library): its marks, and those of everything below it, follow. `title`
    /// is the new title of the document itself.
    pub fn rekey(&self, lib: &str, from: &str, to: &str, title: Option<&str>) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE OR REPLACE marks SET path = ?3 || substr(path, ?4)
             WHERE lib = ?1 AND (path = ?2 OR path LIKE ?5 ESCAPE '\\')",
            params![lib, from, to, from.chars().count() as i64 + 1, crate::props::like_below(from)],
        )
        .map_err(err)?;
        if let Some(t) = title {
            c.execute("UPDATE marks SET doc_title = ?3 WHERE lib = ?1 AND path = ?2", params![lib, to, t]).map_err(err)?;
        }
        Ok(())
    }

    /// Every mark of a library (with its tags).
    pub fn export_lib(&self, lib: &str) -> Result<Vec<Mark>, String> {
        self.list(&MarkFilter { lib: Some(lib.to_string()), ..Default::default() }, 1_000_000)
    }

    /// Replace all the marks of a library by `marks` (their ids are not kept).
    pub fn replace_lib(&self, lib: &str, marks: &[Mark]) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM marks WHERE lib = ?1", params![lib]).map_err(err)?;
        for m in marks {
            c.execute(
                "INSERT OR IGNORE INTO marks (kind, lib, path, book_id, lb, quote, note, doc_title, lib_title, author, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![m.kind, lib, m.path, m.book_id, m.lb, m.quote, m.note, m.doc_title, m.lib_title, m.author, m.created],
            )
            .map_err(err)?;
            let id = c.last_insert_rowid();
            Self::put_tags(&c, id, &m.tags)?;
        }
        Ok(())
    }

    /// Every tag with the number of marks that have it.
    pub fn tags(&self) -> Result<Vec<(String, i64)>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT tag, COUNT(*) FROM mark_tags GROUP BY tag ORDER BY COUNT(*) DESC, tag")
            .map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(lib: &str, path: &str, lb: &str, quote: &str) -> MarkNew {
        MarkNew {
            kind: "clip".into(),
            lib: lib.into(),
            path: path.into(),
            book_id: Some("T0262".into()),
            lb: Some(lb.into()),
            quote: quote.into(),
            doc_title: "妙法蓮華經 卷一".into(),
            lib_title: "大藏经".into(),
            ..Default::default()
        }
    }

    #[test]
    fn parses_tags() {
        assert_eq!(parse_tags(" 反馈, 重要，反馈 ;; 校对 "), vec!["反馈", "重要", "校对"]);
    }

    #[test]
    fn clips_bookmarks_and_tags() {
        let m = Marks::memory().unwrap();
        let a = m.add(&clip("cbeta", "XML/T/T08/T08n0262_001.xml", "0001a05", "如是我聞"), &parse_tags("反馈, 重要")).unwrap();
        let mut b = clip("cbeta", "XML/T/T08/T08n0262_002.xml", "0010b01", "");
        b.kind = "bookmark".into();
        m.add(&b, &[]).unwrap();
        assert!(m.add(&MarkNew { kind: "node".into(), ..Default::default() }, &[]).is_err());

        assert_eq!(m.list(&MarkFilter::default(), 10).unwrap().len(), 2);
        let f = MarkFilter { tag: Some("反馈".into()), ..Default::default() };
        let r = m.list(&f, 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, a);
        assert_eq!(r[0].tags, vec!["反馈", "重要"]);
        assert_eq!(m.tags().unwrap(), vec![("反馈".to_string(), 1), ("重要".to_string(), 1)]);

        // text filter looks at the quote, note, title and tags
        let f = MarkFilter { text: "如是 重要".into(), ..Default::default() };
        assert_eq!(m.list(&f, 10).unwrap().len(), 1);
        let f = MarkFilter { kinds: vec!["bookmark".into()], ..Default::default() };
        assert_eq!(m.list(&f, 10).unwrap().len(), 1);

        m.update(a, Some("译文有误"), Some(&["校对".to_string()])).unwrap();
        let r = m.list(&MarkFilter { text: "有误".into(), ..Default::default() }, 10).unwrap();
        assert_eq!(r[0].tags, vec!["校对"]);
        m.delete(a).unwrap();
        assert!(m.tags().unwrap().is_empty()); // tags go with the mark
    }

    #[test]
    fn marks_props_and_history_follow_a_moved_path() {
        let m = Marks::memory().unwrap();
        let mut b = clip("dir", "甲/内/文.md", "3", "引文");
        b.book_id = None;
        m.add(&b, &["旧".to_string()]).unwrap();
        m.set_node_tags("dir", None, "甲/内", "内", "库", &["夹".to_string()]).unwrap();
        m.set_node_tags("dir", None, "甲_内", "别的", "库", &["别".to_string()]).unwrap(); // not below 甲/内
        m.rekey("dir", "甲/内", "乙/内", None).unwrap();
        m.rekey("dir", "乙/内/文.md", "乙/内/新名.md", Some("新名")).unwrap();
        let all = m.export_lib("dir").unwrap();
        assert!(all.iter().any(|x| x.path == "乙/内/新名.md" && x.doc_title == "新名" && x.tags == vec!["旧"]));
        assert_eq!(m.node_tags("dir", None, "乙/内").unwrap(), vec!["夹"]);
        assert_eq!(m.node_tags("dir", None, "甲_内").unwrap(), vec!["别"]);
        // the library file replaces everything the library has
        m.replace_lib("dir", &all[..1]).unwrap();
        assert_eq!(m.export_lib("dir").unwrap().len(), 1);

        let p = crate::props::Props::memory().unwrap();
        let mut d = crate::props::Dims::new();
        d.insert("period".to_string(), vec!["唐".to_string()]);
        p.set("dir", None, "甲/内/文.md", &d).unwrap();
        p.set("dir", None, "甲", &d).unwrap();
        p.rekey("dir", "甲", "丙").unwrap();
        assert_eq!(p.get("dir", None, "丙/内/文.md").unwrap(), d);
        assert_eq!(p.get("dir", None, "丙").unwrap(), d);
        assert!(p.get("dir", None, "甲").unwrap().is_empty());
        let all = p.all("dir").unwrap();
        p.replace_lib("dir", &[]).unwrap();
        assert!(p.all("dir").unwrap().is_empty());
        p.replace_lib("dir", &all).unwrap();
        assert_eq!(p.all("dir").unwrap().len(), 2);

        let h = crate::history::History::memory().unwrap();
        h.touch("dir", "甲/文.md", None, "文").unwrap();
        h.rekey("dir", "甲", "丙", None).unwrap();
        h.rekey("dir", "丙/文.md", "丙/新.md", Some("新")).unwrap();
        let l = h.list("", 10).unwrap();
        assert_eq!((l[0].path.as_str(), l[0].title.as_str()), ("丙/新.md", "新"));
    }

    #[test]
    fn node_tags_and_document_filter() {
        let m = Marks::memory().unwrap();
        m.set_node_tags("cbeta", Some("T0262"), "", "妙法蓮華經", "大藏经", &["待读".to_string()]).unwrap();
        m.set_node_tags("cbeta", Some("T0262"), "XML/T/T08/T08n0262_001.xml", "妙法蓮華經 卷一", "大藏经", &["精读".to_string()]).unwrap();
        assert_eq!(m.node_tags("cbeta", Some("T0262"), "").unwrap(), vec!["待读"]);
        m.set_node_tags("cbeta", Some("T0262"), "", "妙法蓮華經", "大藏经", &["待读".to_string(), "法华".to_string()]).unwrap();
        assert_eq!(m.node_tags("cbeta", Some("T0262"), "").unwrap(), vec!["待读", "法华"]);
        m.add(&clip("cbeta", "XML/T/T08/T08n0262_002.xml", "1", "x"), &[]).unwrap();

        // the marks of one volume: its own and the tags of its book
        let f = MarkFilter {
            lib: Some("cbeta".into()),
            path: Some("XML/T/T08/T08n0262_001.xml".into()),
            book_id: Some("T0262".into()),
            ..Default::default()
        };
        assert_eq!(m.list(&f, 10).unwrap().len(), 2);
        // no tags: forgotten
        m.set_node_tags("cbeta", Some("T0262"), "", "", "", &[]).unwrap();
        assert!(m.node_tags("cbeta", Some("T0262"), "").unwrap().is_empty());
        assert_eq!(m.list(&MarkFilter { kinds: vec!["node".into()], ..Default::default() }, 10).unwrap().len(), 1);
    }
}
