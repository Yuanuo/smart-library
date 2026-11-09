//! The data a read-write local folder carries with it (design-spec 6.5): the
//! properties of its nodes, and the marks (bookmarks, clips, tags) made in it,
//! kept in `<folder>/.smart-library/meta.json`. So a folder that is backed up
//! or synchronised with git takes this data along, and a copy on another
//! computer shows it too. The program's own SQLite files stay the working copy;
//! this file is written after every change and read when the library is opened.
//!
//! One record per line, in a fixed order, so git diffs and merges stay small.
//! Nothing here is about the library's name or place on this computer.

use crate::folder::SIDECAR_DIR;
use crate::marks::Mark;
use crate::props::Dims;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FILE: &str = "meta.json";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sidecar {
    /// path of a node -> its properties
    pub props: Vec<(String, Dims)>,
    /// `lib`, `id` and `lib_title` are not kept in the file
    pub marks: Vec<Mark>,
}

pub fn path_of(root: &Path) -> PathBuf {
    root.join(SIDECAR_DIR).join(FILE)
}

/// The text of the file (records sorted, one per line).
pub fn render(sc: &Sidecar) -> String {
    let mut props: Vec<String> = sc
        .props
        .iter()
        .map(|(path, dims)| json!({ "path": path, "dims": dims }).to_string())
        .collect();
    props.sort();
    let mut marks: Vec<&Mark> = sc.marks.iter().collect();
    marks.sort_by(|a, b| {
        (&a.path, &a.kind, &a.lb, &a.quote, a.created).cmp(&(&b.path, &b.kind, &b.lb, &b.quote, b.created))
    });
    let marks: Vec<String> = marks
        .into_iter()
        .map(|m| {
            json!({
                "kind": m.kind, "path": m.path, "lb": m.lb, "quote": m.quote, "note": m.note,
                "doc_title": m.doc_title, "author": m.author, "created": m.created, "tags": m.tags,
            })
            .to_string()
        })
        .collect();
    let block = |v: &[String]| if v.is_empty() { String::new() } else { format!("\n  {}\n ", v.join(",\n  ")) };
    format!(
        "{{\n \"format\": \"smart-library-sidecar\",\n \"version\": 1,\n \"props\": [{}],\n \"marks\": [{}]\n}}\n",
        block(&props),
        block(&marks)
    )
}

/// Read the text of a file made by [`render`] (the marks get `lib`).
pub fn parse(text: &str, lib: &str) -> Result<Sidecar, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("{} 格式不对：{}", FILE, e))?;
    if v["format"] != "smart-library-sidecar" {
        return Err(format!("{} 不是 Smart Library 的数据文件", FILE));
    }
    let mut sc = Sidecar::default();
    for p in v["props"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let mut dims = Dims::new();
        if let Some(o) = p["dims"].as_object() {
            for (k, vals) in o {
                dims.insert(k.clone(), vals.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default());
            }
        }
        sc.props.push((p["path"].as_str().unwrap_or("").to_string(), dims));
    }
    let s = |m: &Value, k: &str| m[k].as_str().unwrap_or("").to_string();
    for m in v["marks"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        sc.marks.push(Mark {
            id: 0,
            kind: s(m, "kind"),
            lib: lib.to_string(),
            path: s(m, "path"),
            book_id: None,
            lb: m["lb"].as_str().map(String::from),
            quote: s(m, "quote"),
            note: s(m, "note"),
            doc_title: s(m, "doc_title"),
            lib_title: String::new(),
            author: s(m, "author"),
            created: m["created"].as_i64().unwrap_or(0),
            tags: m["tags"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        });
    }
    Ok(sc)
}

/// The file of a folder, if there is one.
pub fn read(root: &Path, lib: &str) -> Result<Option<Sidecar>, String> {
    match std::fs::read_to_string(path_of(root)) {
        Ok(t) => parse(&t, lib).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {}", path_of(root).display(), e)),
    }
}

/// Write the file (through a temporary file); false when it already says the same.
pub fn write(root: &Path, sc: &Sidecar) -> Result<bool, String> {
    let text = render(sc);
    let path = path_of(root);
    if std::fs::read_to_string(&path).map(|old| old == text).unwrap_or(false) {
        return Ok(false);
    }
    let dir = root.join(SIDECAR_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let tmp = dir.join(format!(".{}.tmp", FILE));
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("{}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {}", path.display(), e)
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(kind: &str, path: &str, lb: Option<&str>, quote: &str, tags: &[&str]) -> Mark {
        Mark {
            id: 7,
            kind: kind.into(),
            lib: "x".into(),
            path: path.into(),
            book_id: None,
            lb: lb.map(String::from),
            quote: quote.into(),
            note: "批注".into(),
            doc_title: "文".into(),
            lib_title: "库".into(),
            author: String::new(),
            created: 100,
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn round_trips_in_a_stable_order() {
        let mut d = Dims::new();
        d.insert("period".into(), vec!["唐".into(), "宋".into()]);
        let sc = Sidecar {
            props: vec![("乙/丙.md".into(), d.clone()), ("甲".into(), d.clone())],
            marks: vec![mark("clip", "乙/丙.md", Some("3"), "引文", &["旧", "新"]), mark("node", "甲", None, "", &["夹"])],
        };
        let text = render(&sc);
        assert!(text.lines().count() >= 8 && text.contains("\"path\":\"甲\""));
        let back = parse(&text, "L").unwrap();
        assert_eq!(back.props.len(), 2);
        assert_eq!(back.marks.len(), 2);
        let c = back.marks.iter().find(|m| m.kind == "clip").unwrap();
        assert_eq!((c.lb.as_deref(), c.quote.as_str(), c.lib.as_str(), c.created), (Some("3"), "引文", "L", 100));
        assert_eq!(c.tags, vec!["旧", "新"]);
        // the same data in another order gives the same text
        let mut other = sc.clone();
        other.props.reverse();
        other.marks.reverse();
        assert_eq!(render(&other), text);
        assert!(parse("{}", "L").is_err());
        assert!(parse(&render(&Sidecar::default()), "L").unwrap().marks.is_empty());
    }

    #[test]
    fn writes_only_what_changed() {
        let d = std::env::temp_dir().join(format!("sl-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(read(&d, "L").unwrap().is_none());
        let sc = Sidecar { props: vec![], marks: vec![mark("bookmark", "a.md", Some("1"), "", &[])] };
        assert!(write(&d, &sc).unwrap());
        assert!(!write(&d, &sc).unwrap());
        assert_eq!(read(&d, "L").unwrap().unwrap().marks.len(), 1);
        std::fs::write(path_of(&d), "kaputt").unwrap();
        assert!(read(&d, "L").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
