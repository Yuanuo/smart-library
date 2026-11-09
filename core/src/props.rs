//! User-maintained node properties: the values of the search filters
//! (部类, 时域, 人物, 藏经, 典类) of a book, article, file, folder or note.
//!
//! Stored apart from the libraries (`props.db`), keyed by the node:
//! `(library, book id, path)` — a CBETA book is `(lib, "T0262", "")`, an
//! article or a file its path, a folder its relative path, a note its id.
//! Nothing is written into the source data.
//!
//! A property **replaces** what the data itself gives for that dimension (an
//! empty list: no value at all) and applies to everything below the node
//! (a folder's files, a note's sub-notes); the nearest node wins. A dimension
//! that is not set keeps the value from the data. The values go into the
//! index as facets when it is built (`Overlay::apply`), or are updated later
//! for the affected documents only (`search::update_docs`).

use crate::library::IndexDoc;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;

/// The dimensions, in display order: (name, label).
pub const DIMS: [(&str, &str); 5] = [
    ("bulei", "部类"),
    ("period", "时域"),
    ("person", "人物"),
    ("canon", "藏经"),
    ("kind", "典类"),
];

/// dimension -> values (an empty list: set to nothing)
pub type Dims = BTreeMap<String, Vec<String>>;

pub fn is_dim(name: &str) -> bool {
    DIMS.iter().any(|(d, _)| *d == name)
}

/// Dimensions with two levels (`部類/子類`): written with `/` (`：` and `>` are taken as `/`).
fn two_level(dim: &str) -> bool {
    dim == "bulei" || dim == "kind"
}

/// Clean up what the user typed: trimmed, no empty values, no duplicates,
/// only known dimensions.
pub fn clean(dims: &Dims) -> Dims {
    let mut out = Dims::new();
    for (dim, values) in dims {
        if !is_dim(dim) {
            continue;
        }
        let mut list: Vec<String> = Vec::new();
        for v in values {
            let v = if two_level(dim) {
                v.replace(['：', ':', '>', '›', '＞'], "/").split('/').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/")
            } else {
                v.trim().to_string()
            };
            if !v.is_empty() && !list.contains(&v) {
                list.push(v);
            }
        }
        out.insert(dim.clone(), list);
    }
    out
}

/// The facet path of one value: `["bulei", "華嚴部類", "華嚴部"]`.
pub fn facet_path(dim: &str, value: &str) -> Option<Vec<String>> {
    let mut path = vec![dim.to_string()];
    if two_level(dim) {
        path.extend(value.split('/').map(str::trim).filter(|s| !s.is_empty()).take(2).map(String::from));
    } else if !value.trim().is_empty() {
        path.push(value.trim().to_string());
    }
    (path.len() > 1).then_some(path)
}

/// Facet paths (as the index documents have them) as dimension values:
/// `["bulei", "A", "B"]` -> bulei: `A/B`.
pub fn dims_of(facets: &[Vec<String>]) -> Dims {
    let mut out = Dims::new();
    for f in facets {
        if f.len() > 1 && is_dim(&f[0]) {
            let list = out.entry(f[0].clone()).or_default();
            let v = f[1..].join("/");
            // `A` is implied by `A/B`
            if !list.contains(&v) {
                list.push(v);
            }
        }
    }
    out
}

fn err(e: rusqlite::Error) -> String {
    format!("属性库出错：{}", e)
}

pub struct Props {
    conn: Mutex<Connection>,
}

/// `LIKE` pattern of everything below the folder `path` (`path/%`).
pub fn like_below(path: &str) -> String {
    format!("{}/%", path.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}

impl Props {
    pub fn open(path: &Path) -> Result<Props, String> {
        Self::init(Connection::open(path).map_err(err)?)
    }

    pub fn memory() -> Result<Props, String> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<Props, String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS node_props (
                 lib TEXT NOT NULL,
                 book_id TEXT NOT NULL DEFAULT '',
                 path TEXT NOT NULL DEFAULT '',
                 dim TEXT NOT NULL,
                 pos INTEGER NOT NULL,
                 value TEXT NOT NULL,
                 PRIMARY KEY (lib, book_id, path, dim, pos)
             );",
        )
        .map_err(err)?;
        Ok(Props { conn: Mutex::new(conn) })
    }

    /// The properties set on one node (not those of its ancestors).
    pub fn get(&self, lib: &str, book_id: Option<&str>, path: &str) -> Result<Dims, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT dim, value FROM node_props WHERE lib = ?1 AND book_id = ?2 AND path = ?3 ORDER BY dim, pos")
            .map_err(err)?;
        let rows = st
            .query_map(params![lib, book_id.unwrap_or(""), path], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(err)?;
        let mut out = Dims::new();
        for r in rows {
            let (dim, value) = r.map_err(err)?;
            let list = out.entry(dim).or_default();
            if !value.is_empty() {
                list.push(value); // an empty value only marks "set to nothing"
            }
        }
        Ok(out)
    }

    /// Replace the properties of a node (no dimensions: forget the node).
    pub fn set(&self, lib: &str, book_id: Option<&str>, path: &str, dims: &Dims) -> Result<(), String> {
        let dims = clean(dims);
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(err)?;
        let book = book_id.unwrap_or("");
        tx.execute("DELETE FROM node_props WHERE lib = ?1 AND book_id = ?2 AND path = ?3", params![lib, book, path]).map_err(err)?;
        for (dim, values) in &dims {
            if values.is_empty() {
                tx.execute("INSERT INTO node_props VALUES (?1, ?2, ?3, ?4, 0, '')", params![lib, book, path, dim]).map_err(err)?;
            }
            for (i, v) in values.iter().enumerate() {
                tx.execute("INSERT INTO node_props VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![lib, book, path, dim, i as i64, v]).map_err(err)?;
            }
        }
        tx.commit().map_err(err)
    }

    /// Every node of a library that has properties: (book id, path, dims).
    pub fn all(&self, lib: &str) -> Result<Vec<(String, String, Dims)>, String> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT book_id, path, dim, value FROM node_props WHERE lib = ?1 ORDER BY book_id, path, dim, pos")
            .map_err(err)?;
        let rows = st
            .query_map(params![lib], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))
            .map_err(err)?;
        let mut map: BTreeMap<(String, String), Dims> = BTreeMap::new();
        for r in rows {
            let (b, p, dim, value) = r.map_err(err)?;
            let list = map.entry((b, p)).or_default().entry(dim).or_default();
            if !value.is_empty() {
                list.push(value);
            }
        }
        Ok(map.into_iter().map(|((b, p), d)| (b, p, d)).collect())
    }

    /// Forget the properties of notes that are gone (`path` = the note id).
    pub fn remove_nodes(&self, lib: &str, paths: &[String]) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        for p in paths {
            c.execute("DELETE FROM node_props WHERE lib = ?1 AND book_id = '' AND path = ?2", params![lib, p]).map_err(err)?;
        }
        Ok(())
    }

    /// A document or folder was renamed or moved (`from`, `to`: paths in the
    /// library): its properties, and those of everything below it, follow.
    pub fn rekey(&self, lib: &str, from: &str, to: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE OR REPLACE node_props SET path = ?3 || substr(path, ?4)
             WHERE lib = ?1 AND book_id = '' AND (path = ?2 OR path LIKE ?5 ESCAPE '\\')",
            params![lib, from, to, from.chars().count() as i64 + 1, like_below(from)],
        )
        .map_err(err)?;
        Ok(())
    }

    /// Replace everything a library has by `entries` (book id, path, dims).
    pub fn replace_lib(&self, lib: &str, entries: &[(String, String, Dims)]) -> Result<(), String> {
        self.remove_lib(lib)?;
        for (b, p, d) in entries {
            self.set(lib, Some(b.as_str()).filter(|b| !b.is_empty()), p, d)?;
        }
        Ok(())
    }

    pub fn remove_lib(&self, lib: &str) -> Result<(), String> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM node_props WHERE lib = ?1", params![lib]).map_err(err)?;
        Ok(())
    }

    /// The properties of a library, ready to be put into index documents.
    pub fn overlay(&self, lib: &str) -> Result<Overlay, String> {
        Ok(Overlay { map: self.all(lib)?.into_iter().map(|(b, p, d)| ((b, p), d)).collect() })
    }
}

/// The properties of one library, looked up while its documents are indexed.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    map: HashMap<(String, String), Dims>,
}

impl Overlay {
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// What applies to a document: the properties of its nodes, from the top
    /// down (`keys`, itself last); a lower node replaces a higher one.
    pub fn merged(&self, keys: &[(String, String)]) -> Dims {
        let mut out = Dims::new();
        for k in keys {
            if let Some(d) = self.map.get(k) {
                for (dim, v) in d {
                    out.insert(dim.clone(), v.clone());
                }
            }
        }
        out
    }

    /// Put the properties into the document's facets.
    pub fn apply(&self, doc: &mut IndexDoc) {
        if self.map.is_empty() {
            return;
        }
        for (dim, values) in self.merged(&doc.keys) {
            doc.facets.retain(|f| f.first().map(String::as_str) != Some(dim.as_str()));
            for v in values {
                if let Some(p) = facet_path(&dim, &v) {
                    // a two-level value also counts for its first level
                    doc.facets.push(p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(list: &[(&str, &[&str])]) -> Dims {
        list.iter().map(|(d, v)| (d.to_string(), v.iter().map(|s| s.to_string()).collect())).collect()
    }

    #[test]
    fn stores_replaces_and_forgets() {
        let p = Props::memory().unwrap();
        p.set("cbeta", Some("T0262"), "", &dims(&[("period", &["唐"]), ("person", &["鳩摩羅什", " 鳩摩羅什", "  "])])).unwrap();
        assert_eq!(p.get("cbeta", Some("T0262"), "").unwrap(), dims(&[("period", &["唐"]), ("person", &["鳩摩羅什"])]));
        // an empty list is kept: "set to nothing"
        p.set("cbeta", Some("T0262"), "", &dims(&[("period", &[])])).unwrap();
        assert_eq!(p.get("cbeta", Some("T0262"), "").unwrap(), dims(&[("period", &[])]));
        assert_eq!(p.all("cbeta").unwrap().len(), 1);
        p.set("cbeta", Some("T0262"), "", &Dims::new()).unwrap();
        assert!(p.all("cbeta").unwrap().is_empty());
        // two levels accept the full-width colon
        p.set("n", None, "3", &dims(&[("kind", &["论：经释"]), ("bulei", &["A > B"])])).unwrap();
        assert_eq!(p.get("n", None, "3").unwrap(), dims(&[("bulei", &["A/B"]), ("kind", &["论/经释"])]));
        p.remove_nodes("n", &["3".into()]).unwrap();
        assert!(p.get("n", None, "3").unwrap().is_empty());
    }

    #[test]
    fn overlay_replaces_a_dimension_and_inherits_down_the_tree() {
        let p = Props::memory().unwrap();
        p.set("f", None, "a", &dims(&[("period", &["宋"]), ("bulei", &["甲/乙"])])).unwrap();
        p.set("f", None, "a/b", &dims(&[("period", &[])])).unwrap();
        let o = p.overlay("f").unwrap();
        let mk = |rel: &str| {
            let mut keys = Vec::new();
            let mut cur = String::new();
            for part in rel.split('/') {
                if !cur.is_empty() {
                    cur.push('/');
                }
                cur.push_str(part);
                keys.push((String::new(), cur.clone()));
            }
            IndexDoc {
                path: rel.into(),
                book: "b".into(),
                juan: 0,
                title: "t".into(),
                text: "x".into(),
                facets: vec![vec!["period".into(), "唐".into()], vec!["kind".into(), "txt".into()]],
                keys,
            }
        };
        let mut d = mk("a/x.txt");
        o.apply(&mut d);
        assert!(d.facets.contains(&vec!["period".to_string(), "宋".to_string()]));
        assert!(!d.facets.contains(&vec!["period".to_string(), "唐".to_string()]));
        assert!(d.facets.contains(&vec!["bulei".to_string(), "甲".to_string(), "乙".to_string()]));
        assert!(d.facets.contains(&vec!["kind".to_string(), "txt".to_string()])); // not set: kept
        // the nearest node wins: no period at all below a/b
        let mut d = mk("a/b/y.txt");
        o.apply(&mut d);
        assert!(!d.facets.iter().any(|f| f[0] == "period"));
        assert_eq!(dims_of(&d.facets).get("bulei").unwrap(), &vec!["甲/乙".to_string()]);
    }
}
