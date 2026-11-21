//! Export and import of knowledge libraries (backup, restore, sharing).
//!
//! An export package is a ZIP file:
//!
//! ```text
//! manifest.json        format, library, and the tree of nodes (titles, files)
//! docs/000001.md       one Markdown file per document (names are numbers,
//!                      so special characters in titles cannot hurt)
//! files/<path>         original files of a folder library (optional)
//! docs/000002.mm.json  the data of a mind map (its .md file is the outline)
//! docs/000003.wb.json  the scene of a whiteboard (its .md file lists the texts)
//! ```
//!
//! The documents of read-only libraries (CBETA, HTML articles, folders) are
//! converted from their rendered HTML to Markdown: notes, variant readings and
//! Sanskrit pairs become footnotes (`[^1]` … `[^1]: text`), which the notes
//! reader shows as hover notes again. Importing always makes an internal
//! (notes) library.

use crate::markup::{self, Element, Node};
use crate::notes::NotesStore;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::Path;
use crate::props::Dims;

pub const FORMAT: &str = "smart-library-export";
pub const DIR_FORMAT: &str = "smart-library-dir";

// ------------------------------------------------------------ HTML -> Markdown

#[derive(Default)]
struct Conv {
    out: String,
    notes: Vec<String>,
    /// old-format articles: pictures embedded as base64 and `note` marks are kept
    legacy: bool,
    pictures: Vec<(String, Vec<u8>)>,
}

fn has_class(e: &Element, c: &str) -> bool {
    e.attr("class").is_some_and(|v| v.split_whitespace().any(|t| t == c))
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\r' | '\n' | '\t')).collect()
}

impl Conv {
    fn push_block(&mut self, s: &str) {
        let s = s.trim();
        if !s.is_empty() {
            self.out.push_str(s);
            self.out.push_str("\n\n");
        }
    }

    fn note(&mut self, text: &str) -> String {
        let t = clean(text).trim().to_string();
        if t.is_empty() {
            return String::new();
        }
        self.notes.push(t);
        format!("[^{}]", self.notes.len())
    }

    /// A picture of a legacy article (`src="data:image/png;base64,…"`) kept as an attachment.
    fn picture(&mut self, e: &Element) -> Option<String> {
        if !self.legacy {
            return None;
        }
        let (mime, bytes) = data_uri(e.attr("src")?)?;
        let name = crate::attach::name_for(&bytes, &mime).ok()?;
        if !self.pictures.iter().any(|(n, _)| *n == name) {
            self.pictures.push((name.clone(), bytes));
        }
        Some(name)
    }

    fn inline(&mut self, e: &Element, out: &mut String) {
        for n in &e.children {
            match n {
                Node::Text(t) => out.push_str(&clean(t)),
                Node::Element(c) => self.inline_el(c, out),
            }
        }
    }

    fn inline_el(&mut self, e: &Element, out: &mut String) {
        match e.name.as_str() {
            "br" => out.push_str("  \n"),
            "img" => match self.picture(e) {
                Some(name) => out.push_str(&format!("\n![](attach:{})\n", name)),
                None => out.push_str("[图]"),
            },
            "a" if self.legacy && (e.attr("data-note").is_some() || e.attr("note").is_some()) => {
                let key = e.attr("data-note").or(e.attr("note")).unwrap_or("");
                self.inline(e, out);
                // "@1": the old program kept only the number of the note in the file
                let text = if key.starts_with('@') { format!("标注 {}", key) } else { key.to_string() };
                out.push_str(&self.note(&text));
            }
            "a" if self.legacy && e.attr("href").is_some_and(|h| h.starts_with("http://") || h.starts_with("https://")) => {
                let mut t = String::new();
                self.inline(e, &mut t);
                let t = t.replace(['[', ']'], "");
                let href = e.attr("href").unwrap_or("").replace(' ', "%20").replace(')', "%29");
                if t.trim().is_empty() {
                    out.push_str(&href);
                } else {
                    out.push_str(&format!("[{}]({})", t, href));
                }
            }
            "b" | "strong" => {
                out.push_str("**");
                self.inline(e, out);
                out.push_str("**");
            }
            "em" | "i" => {
                out.push('*');
                self.inline(e, out);
                out.push('*');
            }
            "a" if e.attr("data-src").is_some_and(|s| s.starts_with("smartlib://")) => {
                let mut t = String::new();
                self.inline(e, &mut t);
                let t = t.replace(['[', ']'], "");
                out.push_str(&format!("[{}]({})", t, e.attr("data-src").unwrap_or("").replace('*', "%2A")));
            }
            "span" => {
                if has_class(e, "lb") {
                    return;
                }
                if has_class(e, "note") {
                    // CBETA note: the CBETA text, the original note
                    let cb = e.elements().find(|c| has_class(c, "nb-cb")).map(|c| c.text());
                    let orig = e.elements().find(|c| has_class(c, "nb-orig")).map(|c| c.text());
                    let text = match (cb.filter(|t| !t.is_empty()), orig.filter(|t| !t.is_empty())) {
                        (Some(c), Some(o)) => format!("CBETA：{}　原编注：{}", c, o),
                        (Some(c), None) => format!("CBETA：{}", c),
                        (None, Some(o)) => format!("原编注：{}", o),
                        (None, None) => String::new(),
                    };
                    out.push_str(&self.note(&text));
                } else if has_class(e, "fn") {
                    out.push_str(&self.note(e.attr("data-fn").unwrap_or("")));
                } else if has_class(e, "app") && has_class(e, "v") {
                    self.inline(e, out);
                    out.push_str(&self.note(&format!("异读：{}", e.attr("title").unwrap_or(""))));
                } else if has_class(e, "tt") {
                    self.inline(e, out);
                    out.push_str(&self.note(&format!("对照：{}", e.attr("title").unwrap_or(""))));
                } else if has_class(e, "hi") && has_class(e, "r-bold") {
                    out.push_str("**");
                    self.inline(e, out);
                    out.push_str("**");
                } else {
                    self.inline(e, out);
                }
            }
            _ => self.inline(e, out),
        }
    }

    /// The blocks of an element: text and inline elements between blocks form a paragraph.
    fn blocks(&mut self, e: &Element) {
        let mut pending = String::new();
        for n in &e.children {
            match n {
                Node::Text(t) => pending.push_str(&clean(t)),
                Node::Element(c) => {
                    if self.block_el(c, &mut pending) {
                        continue;
                    }
                    self.inline_el(c, &mut pending);
                }
            }
        }
        let rest = std::mem::take(&mut pending);
        self.push_block(&rest);
    }

    /// A block element is written out (after the paragraph before it); false for inline ones.
    fn block_el(&mut self, e: &Element, pending: &mut String) -> bool {
        let name = e.name.as_str();
        let is_block = matches!(name, "p" | "div" | "ul" | "ol" | "li" | "table" | "tr" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        if !is_block {
            return false;
        }
        let before = std::mem::take(pending);
        self.push_block(&before);
        match name {
            "div" if has_class(e, "head") => {
                let level = e
                    .attr("class")
                    .and_then(|c| c.split_whitespace().find_map(|t| t.strip_prefix('h').and_then(|n| n.parse::<usize>().ok())))
                    .unwrap_or(2)
                    .clamp(1, 6);
                let mut t = String::new();
                self.inline(e, &mut t);
                if !t.trim().is_empty() {
                    self.push_block(&format!("{} {}", "#".repeat(level), t.trim()));
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level: usize = name[1..].parse().unwrap_or(2);
                let mut t = String::new();
                self.inline(e, &mut t);
                self.push_block(&format!("{} {}", "#".repeat(level), t.trim()));
            }
            "p" | "blockquote" => {
                let mut t = String::new();
                self.inline(e, &mut t);
                let quote = name == "blockquote" || has_class(e, "quote");
                let text = t.trim_matches(|c: char| c == ' ' || c == '\n');
                if quote {
                    let q: Vec<String> = text.lines().map(|l| format!("> {}", l.trim())).collect();
                    self.push_block(&q.join("\n"));
                } else {
                    self.push_block(text);
                }
            }
            "li" => {
                let mut t = String::new();
                self.inline(e, &mut t);
                self.push_block(&format!("- {}", t.trim()));
            }
            "tr" => {
                let cells: Vec<String> = e
                    .elements()
                    .filter(|c| c.name == "td" || c.name == "th")
                    .map(|c| {
                        let mut t = String::new();
                        self.inline(c, &mut t);
                        t.trim().to_string()
                    })
                    .collect();
                self.push_block(&cells.join(" | "));
            }
            // containers: ul, ol, table, div ...
            _ => self.blocks(e),
        }
        true
    }
}

/// Rendered article / volume HTML to Markdown (see the module comment).
pub fn html_to_md(html: &str) -> String {
    convert(html, false).0
}

type Pictures = Vec<(String, Vec<u8>)>;

/// Markdown, the pictures (name, bytes), and the `<meta name=value>` pairs of the head.
fn convert(html: &str, legacy: bool) -> (String, Pictures, Vec<(String, String)>) {
    let root = markup::parse(html);
    let body = root.find_descendant("body").unwrap_or(&root);
    let mut c = Conv { legacy, ..Conv::default() };
    c.blocks(body);
    let mut s = c.out.trim().to_string();
    if !c.notes.is_empty() {
        s.push_str("\n\n");
        for (i, n) in c.notes.iter().enumerate() {
            s.push_str(&format!("[^{}]: {}\n", i + 1, n));
        }
    }
    let metas = root
        .find_descendant("head")
        .map(|h| h.all("meta").filter_map(|m| m.attrs.iter().find(|(k, _)| k != "charset").map(|(k, v)| (k.clone(), v.clone()))).collect())
        .unwrap_or_default();
    (s, c.pictures, metas)
}

/// The properties an old file's meta data gives: `library` = 藏经, `catalog` (+ `album`
/// below it) = 部类, `period` = 时域, `author` = 人物. Other keys (id, searchable …) are dropped.
fn legacy_dims(metas: &[(String, String)]) -> Vec<(String, String)> {
    let first = |k: &str| metas.iter().find(|(n, v)| n == k && !v.trim().is_empty()).map(|(_, v)| v.trim().to_string());
    let mut out = Vec::new();
    for (k, v) in metas {
        let dim = match k.as_str() {
            "library" => "canon",
            "period" => "period",
            "author" => "person",
            _ => continue,
        };
        out.push((dim.to_string(), v.clone()));
    }
    if let Some(c) = first("catalog") {
        out.push(("bulei".into(), match first("album") { Some(a) => format!("{}/{}", c, a), None => c }));
    }
    out
}

/// `data:image/png;base64,AAAA` -> (mime, bytes)
fn data_uri(src: &str) -> Option<(String, Vec<u8>)> {
    let rest = src.trim().strip_prefix("data:")?;
    let (head, data) = rest.split_once(',')?;
    if !head.contains(";base64") {
        return None;
    }
    let mime = head.split(';').next().unwrap_or("").to_string();
    Some((mime, base64_decode(data)?))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

pub type KmindConverted = (String, Pictures, Vec<(String, String)>);
pub type AttachFn<'a> = dyn FnMut(&str, Vec<u8>) -> Result<(), String> + 'a;

/// A KeepMinding / KityMinder file (`.kmind` of the old program) as the data of a Mind-Elixir
/// map, and the pictures (embedded as base64) it had.
pub fn kmind_to_mindmap(text: &str) -> Result<KmindConverted, String> {
    struct Ctx {
        next: u64,
        pictures: Pictures,
    }
    fn node(n: &Value, ctx: &mut Ctx) -> Value {
        let d = &n["data"];
        ctx.next += 1;
        let mut out = json!({ "id": format!("k{:x}", ctx.next), "topic": d["text"].as_str().unwrap_or("") });
        let mut tags: Vec<String> = d["resource"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        if let Some(p) = d["priority"].as_u64() {
            tags.push(format!("优先级{}", p));
        }
        if let Some(p) = d["progress"].as_u64() {
            tags.push(format!("进度{}", p));
        }
        if !tags.is_empty() {
            out["tags"] = json!(tags);
        }
        if let Some(l) = d["hyperlink"].as_str().filter(|l| !l.is_empty()) {
            out["hyperLink"] = json!(l);
        }
        if let Some(src) = d["image"].as_str() {
            if let Some((mime, bytes)) = data_uri(src) {
                if let Ok(name) = crate::attach::name_for(&bytes, &mime) {
                    let size = &d["imageSize"];
                    out["image"] = json!({
                        "url": format!("attach:{}", name),
                        "width": size["width"].as_u64().unwrap_or(120),
                        "height": size["height"].as_u64().unwrap_or(90),
                    });
                    if !ctx.pictures.iter().any(|(n, _)| *n == name) {
                        ctx.pictures.push((name, bytes));
                    }
                }
            } else if src.starts_with("http") {
                out["image"] = json!({ "url": src, "width": 120, "height": 90 });
            }
        }
        let mut kids: Vec<Value> = n["children"].as_array().map(|a| a.iter().map(|c| node(c, ctx)).collect()).unwrap_or_default();
        // the note of a node becomes a first child that says so
        if let Some(note) = d["note"].as_str().filter(|t| !t.trim().is_empty()) {
            ctx.next += 1;
            kids.insert(0, json!({ "id": format!("k{:x}", ctx.next), "topic": format!("注：{}", note.trim()) }));
        }
        if !kids.is_empty() {
            out["children"] = json!(kids);
        }
        out
    }
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("脑图文件不对：{}", e))?;
    if !v["root"].is_object() {
        return Err("脑图文件不对：没有 root".into());
    }
    let mut ctx = Ctx { next: 0, pictures: Vec::new() };
    let mut root = node(&v["root"], &mut ctx);
    root["id"] = json!("root");
    root["root"] = json!(true);
    // template "right": all branches on the right side
    if v["template"].as_str() == Some("right") {
        if let Some(kids) = root["children"].as_array_mut() {
            for k in kids {
                k["direction"] = json!(1);
            }
        }
    }
    // the file's `metadata`: {"library": ["…"], "catalog": ["…"], …}
    let mut metas = Vec::new();
    if let Some(m) = v["metadata"].as_object() {
        for (k, val) in m {
            let list: Vec<&str> = match val {
                Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect(),
                Value::String(t) => vec![t.as_str()],
                _ => vec![],
            };
            metas.extend(list.into_iter().map(|t| (k.clone(), t.to_string())));
        }
    }
    Ok((json!({ "nodeData": root }).to_string(), ctx.pictures, metas))
}

// ------------------------------------------------------------ the package

pub struct PackageWriter {
    zip: zip::ZipWriter<std::fs::File>,
    docs: usize,
}

fn zerr(e: impl std::fmt::Display) -> String {
    format!("数据包出错：{}", e)
}

impl PackageWriter {
    pub fn create(path: &Path) -> Result<PackageWriter, String> {
        let f = std::fs::File::create(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        Ok(PackageWriter { zip: zip::ZipWriter::new(f), docs: 0 })
    }

    fn options() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
    }

    /// Already-compressed formats (images, PDF, EPUB, Office docs, archives, media)
    /// are stored directly so packaging large libraries does not stall CPU on futile deflation.
    fn options_for(name: &str) -> zip::write::SimpleFileOptions {
        let ext = name.rsplit('.').next().map(|s| s.to_ascii_lowercase());
        let stored = matches!(
            ext.as_deref(),
            Some(
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "pdf" | "epub" | "zip" | "docx"
                    | "xlsx" | "pptx" | "7z" | "gz" | "bz2" | "xz" | "mp3" | "mp4" | "m4a"
                    | "mov" | "avi" | "mkv" | "woff" | "woff2"
            )
        );
        zip::write::SimpleFileOptions::default().compression_method(if stored {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        })
    }

    /// Add a document; returns its name in the package.
    pub fn add_doc(&mut self, markdown: &str) -> Result<String, String> {
        self.docs += 1;
        let name = format!("docs/{:06}.md", self.docs);
        self.zip.start_file(&name, Self::options()).map_err(zerr)?;
        self.zip.write_all(markdown.as_bytes()).map_err(zerr)?;
        Ok(name)
    }

    /// Add a file as it is (`name` is the path in the package).
    pub fn add_file(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.zip.start_file(name, Self::options_for(name)).map_err(zerr)?;
        self.zip.write_all(bytes).map_err(zerr)
    }

    /// Add data that is not Markdown (`ext` like `mm.json`); returns its name.
    pub fn add_data(&mut self, content: &str, ext: &str) -> Result<String, String> {
        self.docs += 1;
        let name = format!("docs/{:06}.{}", self.docs, ext);
        self.zip.start_file(&name, Self::options()).map_err(zerr)?;
        self.zip.write_all(content.as_bytes()).map_err(zerr)?;
        Ok(name)
    }

    pub fn finish(mut self, manifest: &Value) -> Result<(), String> {
        self.zip.start_file("manifest.json", Self::options()).map_err(zerr)?;
        let text = serde_json::to_string_pretty(manifest).map_err(zerr)?;
        self.zip.write_all(text.as_bytes()).map_err(zerr)?;
        self.zip.finish().map_err(zerr)?;
        Ok(())
    }
}

/// The manifest: `library` = {title, kind, source}, `scope`, and the `nodes`
/// = [{title, file?, original?, children: […]}].
pub fn manifest(library_title: &str, kind: &str, source: &str, scope: &str, nodes: Vec<Value>) -> Value {
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    json!({
        "format": FORMAT,
        "version": 1,
        "app_version": env!("CARGO_PKG_VERSION"),
        "created": created,
        "library": { "title": library_title, "kind": kind, "source": source },
        "scope": scope,
        "nodes": nodes,
    })
}

/// `数据包-慧灯文集-20220517_160633.smartlib` -> `慧灯文集`
fn package_title(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let s = stem.strip_prefix("数据包-").unwrap_or(stem);
    // a time stamp `-YYYYMMDD_HHMMSS` at the end
    let b = s.as_bytes();
    let cut = if b.len() > 16 {
        let t = &b[b.len() - 16..];
        let ok = t[0] == b'-' && t[9] == b'_' && t.iter().enumerate().all(|(i, c)| i == 0 || i == 9 || c.is_ascii_digit());
        if ok { b.len() - 16 } else { b.len() }
    } else {
        b.len()
    };
    let s = s[..cut].trim();
    if s.is_empty() { "导入的知识库".to_string() } else { s.to_string() }
}

/// A manifest made from the folders and files of an old package: `.html` documents
/// (`legacy_html`), `.kmind` mind maps (`legacy_kmind`) and `.md` / `.txt` notes. A
/// single folder at the top is the library itself. `None` when there is nothing to import.
fn legacy_manifest(names: &[String], title: &str) -> Option<Value> {
    #[derive(Default)]
    struct Dir {
        order: Vec<String>, // first appearance of each entry
        dirs: std::collections::HashMap<String, Dir>,
        files: std::collections::HashMap<String, (String, String)>, // title -> (kind, path)
    }
    fn to_json(d: &Dir) -> Vec<Value> {
        d.order
            .iter()
            .map(|k| match d.dirs.get(k) {
                Some(sub) => json!({ "title": k, "children": to_json(sub) }),
                None => {
                    let (kind, path) = &d.files[k];
                    match kind.as_str() {
                        "md" => json!({ "title": k, "file": path }),
                        _ => json!({ "title": k, "legacy": kind, "file": path }),
                    }
                }
            })
            .collect()
    }
    let mut root = Dir::default();
    let mut found = 0;
    for name in names {
        let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty() && *p != "__MACOSX").collect();
        let Some((last, dirs)) = parts.split_last() else { continue };
        let is_dir = name.ends_with('/');
        let mut cur = &mut root;
        for d in dirs.iter().copied().chain(is_dir.then_some(*last)) {
            if !cur.dirs.contains_key(d) {
                cur.order.push(d.to_string());
                cur.dirs.insert(d.to_string(), Dir::default());
            }
            cur = cur.dirs.get_mut(d).unwrap();
        }
        if is_dir {
            continue;
        }
        let (stem, ext) = match last.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s, e.to_ascii_lowercase()),
            _ => continue,
        };
        let kind = match ext.as_str() {
            "html" | "htm" => "html",
            "kmind" => "kmind",
            "md" | "txt" => "md",
            _ => continue,
        };
        found += 1;
        // `a.html` next to `a.kmind`: the second one is told apart by its extension
        let mut t = stem.to_string();
        if cur.files.contains_key(&t) || cur.dirs.contains_key(&t) {
            t = format!("{}.{}", stem, ext);
        }
        cur.order.push(t.clone());
        cur.files.insert(t, (kind.to_string(), name.clone()));
    }
    if found == 0 {
        return None;
    }
    let mut title = title.to_string();
    let top = if root.dirs.len() == 1 && root.files.is_empty() {
        let (name, only) = root.dirs.iter().next().unwrap();
        title = name.clone();
        to_json(only)
    } else {
        to_json(&root)
    };
    Some(json!({
        "format": FORMAT, "version": 1, "legacy": true, "scope": "library",
        "library": { "title": title, "kind": "legacy", "source": "" },
        "nodes": top,
    }))
}

pub struct Package {
    archive: zip::ZipArchive<std::fs::File>,
    pub manifest: Value,
}

impl Package {
    pub fn open(path: &Path) -> Result<Package, String> {
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let mut archive = zip::ZipArchive::new(f).map_err(|_| "这不是知识库数据包（不是 ZIP 文件）".to_string())?;
        let text = match archive.by_name("manifest.json") {
            Ok(mut m) => {
                let mut s = String::new();
                m.read_to_string(&mut s).map_err(zerr)?;
                Some(s)
            }
            Err(_) => None,
        };
        let manifest: Value = match text {
            Some(s) => serde_json::from_str(&s).map_err(|e| format!("manifest.json 不对：{}", e))?,
            // the old (Java) program: no manifest, only folders and files
            None => {
                let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
                legacy_manifest(&names, &package_title(path)).ok_or("这不是知识库数据包（没有 manifest.json，也没有旧版的 html / kmind 文件）")?
            }
        };
        let fmt = manifest["format"].as_str().unwrap_or("");
        if fmt != FORMAT && fmt != DIR_FORMAT {
            return Err("这不是知识库数据包".into());
        }
        if manifest["version"].as_u64().unwrap_or(0) > 1 {
            return Err("数据包的版本比这个程序新，请升级程序".into());
        }
        Ok(Package { archive, manifest })
    }

    pub fn is_dir_package(&self) -> bool {
        self.manifest["format"] == DIR_FORMAT
    }

    pub fn dir_filter(&self) -> crate::folder::FolderFilter {
        let mut f = crate::folder::FolderFilter::default();
        if let Some(arr) = self.manifest["library"]["exts"].as_array() {
            let exts: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_ascii_lowercase())).filter(|s| !s.is_empty()).collect();
            if !exts.is_empty() {
                f.exts = exts;
            }
        }
        if let Some(mb) = self.manifest["library"]["max_mb"].as_u64() {
            f.max_mb = mb;
        }
        f
    }

    pub fn data_file_count(&self) -> usize {
        self.archive
            .file_names()
            .filter(|n| {
                n.starts_with("data/")
                    && !n.ends_with('/')
                    && !n.starts_with("data/.smart-library/")
                    && !n.ends_with("/.meta.json")
                    && *n != "data/.meta.json"
            })
            .count()
    }

    pub fn title(&self) -> String {
        self.manifest["library"]["title"].as_str().unwrap_or("导入的知识库").to_string()
    }

    /// The names of all files of the package.
    pub fn names(&self) -> Vec<String> {
        self.archive.file_names().map(|s| s.to_string()).collect()
    }

    pub fn read_bytes(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let mut f = self.archive.by_name(name).map_err(|_| format!("数据包里缺少 {}", name))?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).map_err(zerr)?;
        Ok(v)
    }

    pub fn read_text(&mut self, name: &str) -> Result<String, String> {
        let mut f = self.archive.by_name(name).map_err(|_| format!("数据包里缺少 {}", name))?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(zerr)?;
        Ok(s)
    }
}

/// Make the notes of a package below `parent` (`None`: the top); `wrap`: below
/// one more note named like the package's library. The number of notes made.
/// `attach` is given each picture of `attachments/` (name, bytes) to keep.
pub fn import_into(
    store: &NotesStore,
    pkg: &mut Package,
    parent: Option<i64>,
    wrap: bool,
    attach: &mut AttachFn<'_>,
    props: &mut dyn FnMut(i64, &Dims),
) -> Result<usize, String> {
    for name in pkg.names() {
        if let Some(file) = name.strip_prefix("attachments/").filter(|f| crate::attach::is_name(f)) {
            let bytes = pkg.read_bytes(&name)?;
            attach(file, bytes)?;
        }
    }
    let nodes = pkg.manifest["nodes"].as_array().cloned().unwrap_or_default();
    let mut n = 0;
    // thousands of notes (old packages): one transaction instead of one disk write each
    store.begin()?;
    let done = (|| {
        let mut parent = parent;
        if wrap {
            parent = Some(store.create(parent, &pkg.title(), "")?);
            n += 1;
        }
        import_nodes(store, pkg, parent, &nodes, &mut n, attach, props)
    })();
    match done {
        Ok(()) => store.commit()?,
        Err(e) => {
            store.rollback();
            return Err(e);
        }
    }
    Ok(n)
}

/// The user properties a manifest node carries: `"props": {"period": ["唐"], ...}`.
pub fn props_of(node: &Value) -> Dims {
    let mut out = Dims::new();
    if let Some(o) = node["props"].as_object() {
        for (dim, v) in o {
            let list = v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            out.insert(dim.clone(), list);
        }
    }
    crate::props::clean(&out)
}

fn import_nodes(
    store: &NotesStore,
    pkg: &mut Package,
    parent: Option<i64>,
    nodes: &[Value],
    n: &mut usize,
    attach: &mut AttachFn<'_>,
    props: &mut dyn FnMut(i64, &Dims),
) -> Result<(), String> {
    for node in nodes {
        let title = node["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).unwrap_or("（无标题）");
        let mut own = props_of(node);
        let mut legacy_map: Option<String> = None;
        let mut content = match (node["legacy"].as_str(), node["file"].as_str()) {
            (Some("html"), Some(f)) => {
                let html = String::from_utf8_lossy(&pkg.read_bytes(f)?).into_owned();
                let (md, pictures, metas) = convert(&html, true);
                for (name, bytes) in pictures {
                    attach(&name, bytes)?;
                }
                for (dim, value) in legacy_dims(&metas) {
                    own.entry(dim).or_default().push(value);
                }
                md
            }
            (Some("kmind"), Some(f)) => {
                let text = String::from_utf8_lossy(&pkg.read_bytes(f)?).into_owned();
                match kmind_to_mindmap(&text) {
                    Ok((data, pictures, metas)) => {
                        for (name, bytes) in pictures {
                            attach(&name, bytes)?;
                        }
                        for (dim, value) in legacy_dims(&metas) {
                            own.entry(dim).or_default().push(value);
                        }
                        legacy_map = Some(data);
                        String::new()
                    }
                    Err(e) => format!("（这个脑图文件读不了：{}）", e),
                }
            }
            (_, Some(f)) => pkg.read_text(f)?,
            _ => String::new(),
        };
        // the heading with the title that the export put first is the note's title
        let first = format!("# {}", title);
        if let Some(rest) = content.strip_prefix(&first) {
            if rest.is_empty() || rest.starts_with('\n') {
                content = rest.trim_start().to_string();
            }
        }
        // a mind map comes with its data (the Markdown file is only its outline)
        let id = match (node["mindmap"].as_str(), node["whiteboard"].as_str()) {
            _ if legacy_map.is_some() => store.create_typed(parent, title, legacy_map.as_deref().unwrap_or(""), crate::mindmap::TYPE)?,
            (Some(f), _) => store.create_typed(parent, title, &pkg.read_text(f)?, crate::mindmap::TYPE)?,
            (_, Some(f)) => store.create_typed(parent, title, &pkg.read_text(f)?, crate::whiteboard::TYPE)?,
            _ => store.create(parent, title, &content)?,
        };
        *n += 1;
        let own = crate::props::clean(&own);
        if !own.is_empty() {
            props(id, &own);
        }
        let children = node["children"].as_array().cloned().unwrap_or_default();
        import_nodes(store, pkg, Some(id), &children, n, attach, props)?;
    }
    Ok(())
}

/// `# title`, the text, and the line with the source, as written to a document.
pub fn document(title: &str, body: &str, source: &str) -> String {
    let mut s = format!("# {}\n\n{}", title, body.trim());
    if !source.is_empty() {
        s.push_str(&format!("\n\n来源：{}", source));
    }
    s.push('\n');
    s
}

// ------------------------------------------------------------ .smartdir (folder package)

pub struct DirExportReport {
    pub docs: usize,
    pub files: usize,
}

fn is_ignored_dir_export_name(name: &str) -> bool {
    matches!(name, crate::folder::SIDECAR_DIR | ".git" | ".DS_Store" | "Thumbs.db" | "desktop.ini" | "__MACOSX")
}

fn drop_nested_rels(rels: &[String]) -> Vec<String> {
    let clean: Vec<String> = rels
        .iter()
        .map(|r| r.trim_matches('/').to_string())
        .filter(|r| !r.is_empty())
        .collect();
    clean
        .iter()
        .filter(|k| !clean.iter().any(|o| o != *k && k.starts_with(&format!("{}/", o))))
        .cloned()
        .collect()
}

fn common_parent_dir(rels: &[String]) -> String {
    let parents: Vec<Vec<&str>> = rels
        .iter()
        .map(|r| match r.rsplit_once('/') {
            Some((p, _)) => p.split('/').filter(|s| !s.is_empty()).collect(),
            None => Vec::new(),
        })
        .collect();
    let Some(first) = parents.first() else { return String::new() };
    let mut common = Vec::new();
    for (i, seg) in first.iter().enumerate() {
        if parents.iter().all(|p| p.get(i) == Some(seg)) {
            common.push(*seg);
        } else {
            break;
        }
    }
    common.join("/")
}

fn strip_rel_prefix(rel: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return rel.to_string();
    }
    rel.strip_prefix(&format!("{}/", prefix)).unwrap_or(rel).to_string()
}

/// Filter a `.meta.json` so it keeps `"*"` and only `"list"` entries whose name
/// is in `keep_names`. Returns `None` if nothing remains.
fn filter_dir_meta(raw: &str, keep_names: &std::collections::HashSet<String>) -> Option<String> {
    let val: Value = serde_json::from_str(raw).ok()?;
    let obj = val.as_object()?;
    let mut out = serde_json::Map::new();
    if let Some(star) = obj.get("*") {
        out.insert("*".into(), star.clone());
    }
    if let Some(arr) = obj.get("list").and_then(|v| v.as_array()) {
        let kept: Vec<Value> = arr
            .iter()
            .filter(|item| {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.as_str());
                name.is_some_and(|n| {
                    keep_names.contains(n)
                        || keep_names.iter().any(|k| k.rsplit_once('.').is_some_and(|(s, _)| s == n))
                })
            })
            .cloned()
            .collect();
        if !kept.is_empty() {
            out.insert("list".into(), Value::Array(kept));
        }
    }
    if out.is_empty() {
        return None;
    }
    serde_json::to_string_pretty(&Value::Object(out)).ok().map(|s| s + "\n")
}

/// Export a local folder library (whole or picked nodes) as a `.smartdir` ZIP package.
/// Preserves folder structure, `.meta.json`, original files, and only referenced attachments
/// (excluding `.smart-library/trash` and VCS/OS junk). Rewrites Markdown attachment links when
/// a partial export changes directory depth.
pub fn export_folder_dir(
    folder: &crate::folder::FolderLibrary,
    title: &str,
    rels: &[String],
    whole: bool,
    out: &Path,
    on_progress: &mut dyn FnMut(usize, usize, &str),
) -> Result<DirExportReport, String> {
    use std::collections::{BTreeMap, BTreeSet, HashSet};

    // (abs_path, src_rel, dst_rel)
    let mut entries: Vec<(std::path::PathBuf, String, String)> = Vec::new();
    let mut seen_dst: HashSet<String> = HashSet::new();

    fn walk_dir(
        dir: &Path,
        src_prefix: &str,
        strip: &str,
        entries: &mut Vec<(std::path::PathBuf, String, String)>,
        seen_dst: &mut HashSet<String>,
    ) -> Result<(), String> {
        let mut items: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {}", dir.display(), e))?
            .flatten()
            .collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let name = e.file_name().to_string_lossy().into_owned();
            if is_ignored_dir_export_name(&name) {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            let src_rel = if src_prefix.is_empty() { name.clone() } else { format!("{}/{}", src_prefix, name) };
            if ft.is_dir() {
                walk_dir(&e.path(), &src_rel, strip, entries, seen_dst)?;
            } else if ft.is_file() {
                let dst_rel = strip_rel_prefix(&src_rel, strip);
                if seen_dst.insert(dst_rel.clone()) {
                    entries.push((e.path(), src_rel, dst_rel));
                }
            }
        }
        Ok(())
    }

    let is_whole = whole || rels.is_empty();
    let mut partial_meta: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    if is_whole {
        walk_dir(&folder.root, "", "", &mut entries, &mut seen_dst)?;
    } else {
        let picked = drop_nested_rels(rels);
        if picked.is_empty() {
            return Err("没有选中要导出的文件或目录".into());
        }
        let single_dir = picked.len() == 1 && folder.abs(&picked[0]).map(|p| p.is_dir()).unwrap_or(false);
        let strip = if single_dir {
            picked[0].clone()
        } else {
            common_parent_dir(&picked)
        };
        for rel in &picked {
            let abs = folder.abs(rel)?;
            if abs.is_dir() {
                walk_dir(&abs, rel, &strip, &mut entries, &mut seen_dst)?;
            } else if abs.is_file() {
                let dst_rel = strip_rel_prefix(rel, &strip);
                if seen_dst.insert(dst_rel.clone()) {
                    entries.push((abs, rel.clone(), dst_rel));
                }
            }
        }
        // Preserve filtered `.meta.json` on ancestor directories that were partially included
        if !single_dir {
            let mut dir_children: BTreeMap<String, HashSet<String>> = BTreeMap::new();
            for (_, src_rel, _) in &entries {
                let mut cur = src_rel.as_str();
                while let Some((parent, child)) = cur.rsplit_once('/') {
                    if !strip.is_empty() && parent.len() < strip.len() {
                        break;
                    }
                    dir_children.entry(parent.to_string()).or_default().insert(child.to_string());
                    if parent == strip {
                        break;
                    }
                    cur = parent;
                }
                if strip.is_empty() && !cur.contains('/') {
                    dir_children.entry(String::new()).or_default().insert(cur.to_string());
                }
            }
            for (src_dir, kept) in dir_children {
                let dst_dir = if src_dir == strip {
                    String::new()
                } else {
                    strip_rel_prefix(&src_dir, &strip)
                };
                let dst_meta_rel = if dst_dir.is_empty() {
                    crate::folder::DIR_META_FILE.to_string()
                } else {
                    format!("{}/{}", dst_dir, crate::folder::DIR_META_FILE)
                };
                if seen_dst.contains(&dst_meta_rel) {
                    continue;
                }
                let src_meta = if src_dir.is_empty() {
                    folder.root.join(crate::folder::DIR_META_FILE)
                } else {
                    folder.root.join(&src_dir).join(crate::folder::DIR_META_FILE)
                };
                if let Ok(raw) = std::fs::read_to_string(&src_meta) {
                    if let Some(filtered) = filter_dir_meta(&raw, &kept) {
                        partial_meta.insert(dst_meta_rel, filtered.into_bytes());
                    }
                }
            }
        }
    }

    let total = entries.len();
    let mut w = PackageWriter::create(out)?;
    let mut used_attachments: BTreeSet<String> = BTreeSet::new();
    let mut docs = 0usize;

    for (idx, (abs, src_rel, dst_rel)) in entries.iter().enumerate() {
        let name = dst_rel.rsplit('/').next().unwrap_or(dst_rel);
        let zip_path = format!("data/{}", dst_rel);
        match crate::folder::EditKind::of(name) {
            Some(crate::folder::EditKind::Markdown) => {
                let raw = std::fs::read(abs).map_err(|e| format!("{}: {}", src_rel, e))?;
                let text = String::from_utf8_lossy(&raw);
                let editor = crate::folder::to_editor(&text);
                for att in crate::attach::refs(&editor) {
                    used_attachments.insert(att);
                }
                let depth_changed = src_rel.matches('/').count() != dst_rel.matches('/').count();
                if depth_changed && !crate::attach::refs(&editor).is_empty() {
                    let rewritten = crate::folder::attach_to_links(&editor, dst_rel);
                    w.add_file(&zip_path, rewritten.as_bytes())?;
                } else {
                    w.add_file(&zip_path, &raw)?;
                }
                docs += 1;
            }
            Some(crate::folder::EditKind::MindMap | crate::folder::EditKind::Whiteboard) => {
                let raw = std::fs::read(abs).map_err(|e| format!("{}: {}", src_rel, e))?;
                let text = String::from_utf8_lossy(&raw);
                for att in crate::attach::refs(&text) {
                    used_attachments.insert(att);
                }
                w.add_file(&zip_path, &raw)?;
                docs += 1;
            }
            None => {
                let raw = std::fs::read(abs).map_err(|e| format!("{}: {}", src_rel, e))?;
                w.add_file(&zip_path, &raw)?;
                if name != crate::folder::DIR_META_FILE {
                    docs += 1;
                }
            }
        }
        on_progress(idx + 1, total, "");
    }

    for (dst_meta_rel, bytes) in partial_meta {
        w.add_file(&format!("data/{}", dst_meta_rel), &bytes)?;
    }

    let attach_root = folder.root.join(crate::folder::SIDECAR_DIR).join("attachments");
    let mut files = 0usize;
    let total_att = used_attachments.len();
    for (att_idx, att) in used_attachments.iter().enumerate() {
        if att_idx == 0 || (att_idx + 1) % 20 == 0 || att_idx + 1 == total_att {
            on_progress(att_idx + 1, total_att, "正在打包附件…");
        }
        let disk = crate::attach::sharded(&attach_root, att)
            .filter(|p| p.is_file())
            .or_else(|| crate::attach::path_of(&attach_root, att).filter(|p| p.is_file()));
        if let Some(p) = disk {
            if let Ok(bytes) = std::fs::read(p) {
                let shard = crate::attach::shard_of(att).unwrap_or_default();
                let zip_path = format!("data/{}/attachments/{}{}", crate::folder::SIDECAR_DIR, shard, att);
                w.add_file(&zip_path, &bytes)?;
                files += 1;
            }
        }
    }

    on_progress(total, total, "正在写入压缩包，请稍候…");

    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let manifest = json!({
        "format": DIR_FORMAT,
        "version": 1,
        "app_version": env!("CARGO_PKG_VERSION"),
        "created": created,
        "library": {
            "title": title,
            "kind": "folder",
            "source": "",
            "exts": folder.filter.exts,
            "max_mb": folder.filter.max_mb,
        },
        "scope": if is_whole { "library" } else { "nodes" },
    });
    w.finish(&manifest)?;
    Ok(DirExportReport { docs, files })
}

/// Extract a `.smartdir` package into `dest`. `dest` must exist and be completely empty.
pub fn extract_dir_package(pkg: &mut Package, dest: &Path) -> Result<usize, String> {
    if !pkg.is_dir_package() {
        return Err("这不是文件夹数据包（.smartdir）".into());
    }
    if !dest.is_dir() {
        return Err(format!("目标文件夹不存在：{}", dest.display()));
    }
    let mut iter = std::fs::read_dir(dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    if iter.next().is_some() {
        return Err("目标文件夹不为空，请选择一个空目录".into());
    }
    let names = pkg.names();
    let mut count = 0usize;
    for name in names {
        let Some(rel) = name.strip_prefix("data/") else { continue };
        if rel.is_empty() || rel.ends_with('/') {
            continue;
        }
        if rel.starts_with('/') || rel.contains('\\') || rel.split('/').any(|s| s == ".." || s.contains(':')) {
            return Err(format!("数据包里有不安全的路径：{}", rel));
        }
        let target = rel.split('/').filter(|s| !s.is_empty()).fold(dest.to_path_buf(), |p, s| p.join(s));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
        }
        let bytes = pkg.read_bytes(&name)?;
        std::fs::write(&target, &bytes).map_err(|e| format!("{}: {}", target.display(), e))?;
        if !rel.starts_with(&format!("{}/", crate::folder::SIDECAR_DIR)) && !rel.ends_with(crate::folder::DIR_META_FILE) {
            count += 1;
        }
    }
    Ok(count)
}

/// Import a `.smartdir` package into an internal notes library (SQLite).
/// Extracts to a temporary folder, reads via `FolderLibrary` (preserving `.meta.json` order
/// and properties, mind maps, whiteboards, attachments, and converting other documents to Markdown).
pub fn import_dir_into_notes(
    store: &NotesStore,
    pkg: &mut Package,
    parent: Option<i64>,
    wrap: bool,
    attach: &mut AttachFn<'_>,
    props: &mut dyn FnMut(i64, &Dims),
) -> Result<usize, String> {
    if !pkg.is_dir_package() {
        return Err("这不是文件夹数据包（.smartdir）".into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("smart-library-dir-import-{}-{}", std::process::id(), stamp));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("无法创建临时目录：{}", e))?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(tmp.clone());

    extract_dir_package(pkg, &tmp)?;
    let filter = pkg.dir_filter();
    let folder = crate::folder::FolderLibrary::open(&tmp, filter)?;

    // Copy all attachments into the target internal library's attachment store
    fn walk_attachments(dir: &Path, attach: &mut AttachFn<'_>) -> Result<(), String> {
        let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                walk_attachments(&e.path(), attach)?;
            } else if ft.is_file() {
                let name = e.file_name().to_string_lossy().into_owned();
                if crate::attach::is_name(&name) {
                    let bytes = std::fs::read(e.path()).map_err(|err| err.to_string())?;
                    attach(&name, bytes)?;
                }
            }
        }
        Ok(())
    }
    walk_attachments(&tmp.join(crate::folder::SIDECAR_DIR).join("attachments"), attach)?;

    fn import_folder_children(
        folder: &crate::folder::FolderLibrary,
        rel: &str,
        store: &NotesStore,
        parent: Option<i64>,
        n: &mut usize,
        props: &mut dyn FnMut(i64, &Dims),
    ) -> Result<(), String> {
        for e in folder.children(rel)? {
            if e.is_dir {
                let id = store.create(parent, &e.name, "")?;
                *n += 1;
                if let Some(dims) = folder.dir_meta_dims(&e.rel) {
                    let cleaned = crate::props::clean(&dims);
                    if !cleaned.is_empty() {
                        props(id, &cleaned);
                    }
                }
                import_folder_children(folder, &e.rel, store, Some(id), n, props)?;
            } else {
                let title = match crate::folder::EditKind::of(&e.name) {
                    Some(crate::folder::EditKind::MindMap) => e.name.strip_suffix(".map.json").unwrap_or(&e.name),
                    Some(crate::folder::EditKind::Whiteboard) => e.name.strip_suffix(".wbd.json").unwrap_or(&e.name),
                    _ => e.name.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(&e.name),
                };
                let id = match crate::folder::EditKind::of(&e.name) {
                    Some(crate::folder::EditKind::MindMap) => {
                        let text = std::fs::read_to_string(folder.abs(&e.rel)?).map_err(|err| err.to_string())?;
                        store.create_typed(parent, title, &text, crate::mindmap::TYPE)?
                    }
                    Some(crate::folder::EditKind::Whiteboard) => {
                        let text = std::fs::read_to_string(folder.abs(&e.rel)?).map_err(|err| err.to_string())?;
                        store.create_typed(parent, title, &text, crate::whiteboard::TYPE)?
                    }
                    Some(crate::folder::EditKind::Markdown) => {
                        let raw = std::fs::read(folder.abs(&e.rel)?).map_err(|err| err.to_string())?;
                        let text = String::from_utf8_lossy(&raw);
                        let md = crate::folder::to_editor(&text);
                        store.create(parent, title, &md)?
                    }
                    None => {
                        let md = match folder.article(&e.rel, Some(&crate::render::RenderOptions::default())) {
                            Ok(a) => html_to_md(&a.html.unwrap_or_default()),
                            Err(_) => continue,
                        };
                        store.create(parent, title, &md)?
                    }
                };
                *n += 1;
                if let Some(dims) = folder.dir_meta_dims(&e.rel) {
                    let cleaned = crate::props::clean(&dims);
                    if !cleaned.is_empty() {
                        props(id, &cleaned);
                    }
                }
            }
        }
        Ok(())
    }

    let mut n = 0;
    store.begin()?;
    let done = (|| {
        let mut parent = parent;
        if wrap {
            parent = Some(store.create(parent, &pkg.title(), "")?);
            n += 1;
        }
        import_folder_children(&folder, "", store, parent, &mut n, props)
    })();
    match done {
        Ok(()) => store.commit()?,
        Err(e) => {
            store.rollback();
            return Err(e);
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown;

    const CBETA: &str = concat!(
        "<html><body><div class=\"head h2\">序品</div><p class=\"p\" id=\"x\"><span class=\"lb\" data-lb=\"0001a03\"></span>",
        "如是我聞<span class=\"note\" tabindex=\"0\" data-note=\"1\" data-n=\"1\"><sup>1</sup>",
        "<span class=\"nb-orig\">〔死〕－【三】</span><span class=\"nb-cb\">〔－〕【CB】，死【大】</span></span>。",
        "<span class=\"app v\" data-n=\"1\" title=\"【CB】無；【大】死\">死</span>後<span class=\"hi r-bold\">粗</span>",
        "<br><span class=\"lb\" data-lb=\"0001a04\"></span>下行</p>",
        "<ul class=\"list\"><li class=\"item\">甲</li></ul><p class=\"p quote\">引</p></body></html>"
    );

    #[test]
    fn converts_notes_to_footnotes() {
        let md = html_to_md(CBETA);
        assert!(md.starts_with("## 序品\n\n如是我聞[^1]。死[^2]後**粗**  \n下行"), "{md}");
        assert!(md.contains("- 甲") && md.contains("> 引"), "{md}");
        assert!(md.contains("[^1]: CBETA：〔－〕【CB】，死【大】　原编注：〔死〕－【三】"), "{md}");
        assert!(md.contains("[^2]: 异读：【CB】無；【大】死"), "{md}");
        // and the notes reader shows them again as hover notes
        let h = markdown::to_html(&md);
        assert!(h.contains("data-note=") && !h.contains("[^1]"), "{h}");
    }

    #[test]
    fn imports_an_old_package_without_manifest() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("sl-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("数据包-旧库-20220517_160633.smartlib");
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let html = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta period=\"现代\"><meta author=\"甲\"><meta author=\"乙\"><meta library=\"旧库\"><meta catalog=\"目\"><meta album=\"辑一\"></head><body>             <h1 id=\"a1\">文章</h1><p id=\"a2\">人<a data-note=\"@1\" id=\"x\"></a>有三种<a id=\"a-3\"></a>，<a note=\"自写的标注\">见</a>。</p>             <p><img style=\"width:10px\" src=\"data:image/png;base64,{png}\"></p></body></html>"
        );
        let kmind = r#"{"root":{"data":{"text":"根"},"children":[{"data":{"id":"a","text":"枝","resource":["alias"],"hyperlink":"http://x.y","note":"说明"},"children":[{"data":{"text":"叶"}}]}]},"template":"right","metadata":{"id":"x","searchable":["lookup"],"library":["经目对照"],"catalog":["登噶目錄"]}}"#;
        let mut zw = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in [("库/", ""), ("库/文/", ""), ("库/文/文章.html", html.as_str()), ("库/图.kmind", kmind), ("库/说明.md", "# 说明

正文"), ("库/x.bin", "?")] {
            if n.ends_with('/') {
                zw.add_directory(n, o).unwrap();
            } else {
                zw.start_file(n, o).unwrap();
                zw.write_all(c.as_bytes()).unwrap();
            }
        }
        zw.finish().unwrap();

        let mut p = Package::open(&path).unwrap();
        assert_eq!(p.title(), "库");
        let store = NotesStore::memory().unwrap();
        let mut pics: Vec<String> = Vec::new();
        let mut with_props: Vec<(i64, Dims)> = Vec::new();
        let n = import_into(&store, &mut p, None, false, &mut |n, _| (pics.push(n.to_string()), Ok(())).1, &mut |id, d| with_props.push((id, d.clone()))).unwrap();
        assert_eq!(n, 4); // 文 / 文章 / 图 / 说明
        let top = store.children(None).unwrap();
        assert_eq!(top.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["文", "图", "说明"]);
        let article = store.children(Some(top[0].id)).unwrap();
        let note = store.get(article[0].id).unwrap().unwrap();
        // the title heading went into the title; marks became footnotes; the picture an attachment
        assert!(note.content.starts_with("人[^1]有三种，[见](") || note.content.starts_with("人[^1]有三种，见[^2]。"), "{}", note.content);
        assert!(note.content.contains("[^1]: 标注 @1") && note.content.contains("[^2]: 自写的标注"), "{}", note.content);
        assert_eq!(pics.len(), 1);
        assert!(note.content.contains(&format!("![](attach:{})", pics[0])), "{}", note.content);
        let dims = &with_props.iter().find(|(id, _)| *id == note.id).unwrap().1;
        assert_eq!(dims.get("period"), Some(&vec!["现代".to_string()]));
        assert_eq!(dims.get("person"), Some(&vec!["甲".to_string(), "乙".to_string()]));
        assert_eq!(dims.get("canon"), Some(&vec!["旧库".to_string()]));
        assert_eq!(dims.get("bulei"), Some(&vec!["目/辑一".to_string()]));
        // the mind map
        let map = store.get(top[1].id).unwrap().unwrap();
        assert_eq!(map.content_type, crate::mindmap::TYPE);
        let v: Value = serde_json::from_str(&map.content).unwrap();
        assert_eq!(v["nodeData"]["topic"], "根");
        let branch = &v["nodeData"]["children"][0];
        assert_eq!((branch["topic"].as_str(), branch["direction"].as_u64(), branch["hyperLink"].as_str()), (Some("枝"), Some(1), Some("http://x.y")));
        assert_eq!(branch["tags"][0], "alias");
        assert_eq!(branch["children"][0]["topic"], "注：说明");
        assert_eq!(branch["children"][1]["topic"], "叶");
        let md = &with_props.iter().find(|(id, _)| *id == map.id).unwrap().1;
        assert_eq!((md.get("canon"), md.get("bulei")), (Some(&vec!["经目对照".to_string()]), Some(&vec!["登噶目錄".to_string()])));
        assert!(!md.contains_key("searchable"));
        assert_eq!(store.get(top[2].id).unwrap().unwrap().content, "正文");
        assert!(Package::open(&dir.join("none.smartlib")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_and_reads_a_package() {
        let dir = std::env::temp_dir().join(format!("sl-exchange-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.zip");
        let mut w = PackageWriter::create(&path).unwrap();
        let f1 = w.add_doc(&document("甲/乙:丙", "正文 **粗**", "测试库 · 1")).unwrap();
        let f2 = w.add_doc("无标题行的内容").unwrap();
        w.add_file("files/图/a.bin", &[1, 2, 3]).unwrap();
        w.add_file("attachments/0123456789abcdef0123456789abcdef.png", &[9]).unwrap();
        w.add_file("attachments/evil.png", &[9]).unwrap();
        let nodes = vec![json!({
            "title": "甲/乙:丙", "file": f1, "props": { "period": ["唐"], "kind": [], "bogus": ["x"] },
            "children": [ { "title": "子", "file": f2 }, { "title": "夹", "children": [] } ]
        })];
        w.finish(&manifest("测试库", "folder", "C:\\x", "nodes", nodes)).unwrap();

        let mut p = Package::open(&path).unwrap();
        assert_eq!(p.title(), "测试库");
        let store = NotesStore::memory().unwrap();
        let mut got: Vec<(String, Vec<u8>)> = Vec::new();
        let mut with_props: Vec<(i64, Dims)> = Vec::new();
        assert_eq!(
            import_into(&store, &mut p, None, false, &mut |n, b| (got.push((n.to_string(), b)), Ok(())).1, &mut |id, d| with_props.push((id, d.clone())))
                .unwrap(),
            3
        );
        // the node's properties come along (unknown dimensions are dropped; "set to nothing" is kept)
        assert_eq!(with_props.len(), 1);
        assert_eq!(with_props[0].1.get("period"), Some(&vec!["唐".to_string()]));
        assert_eq!(with_props[0].1.get("kind"), Some(&Vec::new()));
        assert!(!with_props[0].1.contains_key("bogus"));
        // only well-formed picture names are handed over
        assert_eq!(got, vec![("0123456789abcdef0123456789abcdef.png".to_string(), vec![9u8])]);
        let top = store.children(None).unwrap();
        assert_eq!(top[0].title, "甲/乙:丙");
        let note = store.get(top[0].id).unwrap().unwrap();
        // the title heading went into the title
        assert!(note.content.starts_with("正文 **粗**") && note.content.contains("来源：测试库"), "{}", note.content);
        assert_eq!(store.children(Some(top[0].id)).unwrap().len(), 2);
        // below a note, with a note for the library around it
        let mut p2 = Package::open(&path).unwrap();
        assert_eq!(import_into(&store, &mut p2, Some(top[0].id), true, &mut |_, _| Ok(()), &mut |_, _| {}).unwrap(), 4);
        let under = store.children(Some(top[0].id)).unwrap();
        assert_eq!(under.last().unwrap().title, "测试库");
        assert_eq!(store.children(Some(under.last().unwrap().id)).unwrap()[0].title, "甲/乙:丙");
        assert!(Package::open(&dir.join("missing.zip")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// Needs the old packages: `cargo test -p smart-library-core real_old -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_old_packages() {
        for f in ["数据包-慧灯文集-20220517_160633.smartlib", "数据包-经目信息-20221002_113537.smartlib"] {
            let t = std::time::Instant::now();
            let mut p = Package::open(&std::path::Path::new("C:/Devx/smartLib.d").join(f)).unwrap();
            let store = NotesStore::memory().unwrap();
            let mut pics = 0;
            let n = import_into(&store, &mut p, None, false, &mut |_, _| (pics += 1, Ok(())).1, &mut |_, _| {}).unwrap();
            println!("{} -> {} notes, {} pictures, {:?}, top {:?}", p.title(), n, pics, t.elapsed(), store.children(None).unwrap().iter().take(6).map(|r| r.title.clone()).collect::<Vec<_>>());
            let rows = store.rows().unwrap();
            let md = rows.len();
            println!("rows {}", md);
            for r in store.rows().unwrap() {
                let note = store.get(r.id).unwrap().unwrap();
                if note.title.starts_with("二谛") || note.title.starts_with("2011.04") {
                    let c = &note.content;
                    let at = c.find("![](").or(c.find("[^1]")).unwrap_or(0);
                    let from = (0..=at.saturating_sub(40)).rev().find(|i| c.is_char_boundary(*i)).unwrap_or(0);
                    println!("== {} ({} chars)
{}
...
{}", note.title, c.len(), c.chars().take(80).collect::<String>(), c[from..].chars().take(200).collect::<String>());
                    println!("tail: {}", c.chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect::<String>());
                }
            }
        }
    }

    /// Needs the test data: `cargo test -p smart-library-core real_volume -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_volume_to_markdown() {
        let p = std::path::Path::new("C:/Devx/smartLib.d/bookcase.zip");
        let bc = crate::bookcase::Bookcase::open(Box::new(crate::source::ZipSource::open(p).unwrap())).unwrap();
        let vols = bc.volumes("T0262");
        let v = vols.iter().find(|v| v.exists).unwrap();
        let html = bc.volume_html(&v.path, &crate::render::RenderOptions::default()).unwrap().html;
        let md = html_to_md(&html);
        println!("{} chars, {} footnotes\n{}", md.len(), md.matches("]: ").count(), md.chars().take(1500).collect::<String>());
        assert!(md.contains("[^1]"));
        let back = crate::markdown::to_html(&md);
        assert!(back.contains("data-note="));
    }

    #[test]
    fn smartdir_export_extract_and_import_notes() {
        let base = std::env::temp_dir().join(format!("sl-smartdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src");
        std::fs::create_dir_all(src.join("专题/子节")).unwrap();
        std::fs::create_dir_all(src.join(".smart-library/attachments/5c/99")).unwrap();
        std::fs::create_dir_all(src.join(".smart-library/trash")).unwrap();
        std::fs::write(src.join(".smart-library/trash/deleted.md"), "deleted").unwrap();

        let used_pic = "5c9907840123456789abcdef01234567.png";
        let unused_pic = "5c9900000123456789abcdef01234567.png";
        std::fs::write(src.join(".smart-library/attachments/5c/99").join(used_pic), &[1, 2, 3]).unwrap();
        std::fs::write(src.join(".smart-library/attachments/5c/99").join(unused_pic), &[9, 9]).unwrap();

        std::fs::write(
            src.join("专题/.meta.json"),
            r#"{"*": {"canon": "测试藏"}, "list": [{"name": "子节"}, {"name": "说明.md", "period": "现代"}]}"#,
        ).unwrap();
        std::fs::write(
            src.join("专题/说明.md"),
            format!("正文 ![]( ../.smart-library/attachments/5c/99/{})\n![](../.smart-library/attachments/5c/99/{})", used_pic, used_pic),
        ).unwrap();
        std::fs::write(
            src.join("专题/子节/图.map.json"),
            crate::mindmap::new_data("图"),
        ).unwrap();
        std::fs::write(src.join("其他.txt"), "其他文本").unwrap();

        let folder = crate::folder::FolderLibrary::open(&src, crate::folder::FolderFilter::default()).unwrap();

        // 1. Partial export of single directory "专题"
        let pkg_path = base.join("topic.smartdir");
        let rep = export_folder_dir(&folder, "专题", &["专题".to_string()], false, &pkg_path, &mut |_, _, _| {}).unwrap();
        assert_eq!(rep.files, 1); // only used_pic, not unused_pic or trash
        assert!(rep.docs >= 2);

        let mut pkg = Package::open(&pkg_path).unwrap();
        assert!(pkg.is_dir_package());
        assert_eq!(pkg.title(), "专题");

        // 2. Extract to non-empty dir fails
        let nonempty = base.join("nonempty");
        std::fs::create_dir_all(&nonempty).unwrap();
        std::fs::write(nonempty.join("x.txt"), "x").unwrap();
        assert!(extract_dir_package(&mut pkg, &nonempty).is_err());

        // 3. Extract to empty dir succeeds and rewrites markdown attachment link depth
        let dest = base.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        extract_dir_package(&mut pkg, &dest).unwrap();
        assert!(!dest.join(".smart-library/trash").exists());
        assert!(dest.join(".smart-library/attachments/5c/99").join(used_pic).is_file());
        assert!(!dest.join(".smart-library/attachments/5c/99").join(unused_pic).exists());
        assert!(dest.join(".meta.json").is_file());
        let extracted_md = std::fs::read_to_string(dest.join("说明.md")).unwrap();
        assert!(extracted_md.contains(&format!("](.smart-library/attachments/5c/99/{})", used_pic)), "{}", extracted_md);
        assert!(!extracted_md.contains("](../.smart-library/"));

        // 4. Import .smartdir into internal notes library
        let store = NotesStore::memory().unwrap();
        let mut pics = Vec::new();
        let mut props_got: Vec<(i64, Dims)> = Vec::new();
        let mut pkg2 = Package::open(&pkg_path).unwrap();
        let count = import_dir_into_notes(
            &store,
            &mut pkg2,
            None,
            false,
            &mut |n, b| { pics.push((n.to_string(), b)); Ok(()) },
            &mut |id, d| props_got.push((id, d.clone())),
        ).unwrap();
        assert_eq!(count, 3); // 子节 + 图 + 说明
        assert_eq!(pics.len(), 1);
        assert_eq!(pics[0].0, used_pic);
        let top = store.children(None).unwrap();
        assert_eq!(top[0].title, "子节");
        assert_eq!(top[1].title, "说明");
        let note = store.get(top[1].id).unwrap().unwrap();
        assert!(note.content.contains(&format!("![](attach:{})", used_pic)), "{}", note.content);
        let note_dims = &props_got.iter().find(|(id, _)| *id == note.id).unwrap().1;
        assert_eq!(note_dims.get("canon"), Some(&vec!["测试藏".to_string()]));
        assert_eq!(note_dims.get("period"), Some(&vec!["现代".to_string()]));

        let _ = std::fs::remove_dir_all(&base);
    }
}
