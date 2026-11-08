//! Export and import of knowledge libraries (backup, restore, sharing).
//!
//! An export package is a ZIP file:
//!
//! ```text
//! manifest.json        format, library, and the tree of nodes (titles, files)
//! docs/000001.md       one Markdown file per document (names are numbers,
//!                      so special characters in titles cannot hurt)
//! files/<path>         original files of a folder library (optional)
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

pub const FORMAT: &str = "smart-library-export";

// ------------------------------------------------------------ HTML -> Markdown

#[derive(Default)]
struct Conv {
    out: String,
    notes: Vec<String>,
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
            "img" => out.push_str("[图]"),
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
    let root = markup::parse(html);
    let body = root.find_descendant("body").unwrap_or(&root);
    let mut c = Conv::default();
    c.blocks(body);
    let mut s = c.out.trim().to_string();
    if !c.notes.is_empty() {
        s.push_str("\n\n");
        for (i, n) in c.notes.iter().enumerate() {
            s.push_str(&format!("[^{}]: {}\n", i + 1, n));
        }
    }
    s
}

// ------------------------------------------------------------ the package

pub struct PackageWriter {
    zip: zip::ZipWriter<std::fs::File>,
    docs: usize,
}

fn zerr(e: impl std::fmt::Display) -> String {
    format!("导出包出错：{}", e)
}

impl PackageWriter {
    pub fn create(path: &Path) -> Result<PackageWriter, String> {
        let f = std::fs::File::create(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        Ok(PackageWriter { zip: zip::ZipWriter::new(f), docs: 0 })
    }

    fn options() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
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
        self.zip.start_file(name, Self::options()).map_err(zerr)?;
        self.zip.write_all(bytes).map_err(zerr)
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

pub struct Package {
    archive: zip::ZipArchive<std::fs::File>,
    pub manifest: Value,
}

impl Package {
    pub fn open(path: &Path) -> Result<Package, String> {
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let mut archive = zip::ZipArchive::new(f).map_err(|_| "这不是知识库导出包（不是 ZIP 文件）".to_string())?;
        let manifest: Value = {
            let mut m = archive.by_name("manifest.json").map_err(|_| "这不是知识库导出包（没有 manifest.json）".to_string())?;
            let mut s = String::new();
            m.read_to_string(&mut s).map_err(zerr)?;
            serde_json::from_str(&s).map_err(|e| format!("manifest.json 不对：{}", e))?
        };
        if manifest["format"] != FORMAT {
            return Err("这不是知识库导出包".into());
        }
        if manifest["version"].as_u64().unwrap_or(0) > 1 {
            return Err("导出包的版本比这个程序新，请升级程序".into());
        }
        Ok(Package { archive, manifest })
    }

    pub fn title(&self) -> String {
        self.manifest["library"]["title"].as_str().unwrap_or("导入的知识库").to_string()
    }

    pub fn read_text(&mut self, name: &str) -> Result<String, String> {
        let mut f = self.archive.by_name(name).map_err(|_| format!("导出包里缺少 {}", name))?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(zerr)?;
        Ok(s)
    }
}

/// Make the notes of a package below `parent`; the number of notes made.
pub fn import_into(store: &NotesStore, pkg: &mut Package) -> Result<usize, String> {
    let nodes = pkg.manifest["nodes"].as_array().cloned().unwrap_or_default();
    let mut n = 0;
    import_nodes(store, pkg, None, &nodes, &mut n)?;
    Ok(n)
}

fn import_nodes(store: &NotesStore, pkg: &mut Package, parent: Option<i64>, nodes: &[Value], n: &mut usize) -> Result<(), String> {
    for node in nodes {
        let title = node["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).unwrap_or("（无标题）");
        let mut content = match node["file"].as_str() {
            Some(f) => pkg.read_text(f)?,
            None => String::new(),
        };
        // the heading with the title that the export put first is the note's title
        let first = format!("# {}", title);
        if let Some(rest) = content.strip_prefix(&first) {
            if rest.is_empty() || rest.starts_with('\n') {
                content = rest.trim_start().to_string();
            }
        }
        let id = store.create(parent, title, &content)?;
        *n += 1;
        let children = node["children"].as_array().cloned().unwrap_or_default();
        import_nodes(store, pkg, Some(id), &children, n)?;
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
    fn writes_and_reads_a_package() {
        let dir = std::env::temp_dir().join(format!("sl-exchange-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.zip");
        let mut w = PackageWriter::create(&path).unwrap();
        let f1 = w.add_doc(&document("甲/乙:丙", "正文 **粗**", "测试库 · 1")).unwrap();
        let f2 = w.add_doc("无标题行的内容").unwrap();
        w.add_file("files/图/a.bin", &[1, 2, 3]).unwrap();
        let nodes = vec![json!({
            "title": "甲/乙:丙", "file": f1,
            "children": [ { "title": "子", "file": f2 }, { "title": "夹", "children": [] } ]
        })];
        w.finish(&manifest("测试库", "folder", "C:\\x", "nodes", nodes)).unwrap();

        let mut p = Package::open(&path).unwrap();
        assert_eq!(p.title(), "测试库");
        let store = NotesStore::memory().unwrap();
        assert_eq!(import_into(&store, &mut p).unwrap(), 3);
        let top = store.children(None).unwrap();
        assert_eq!(top[0].title, "甲/乙:丙");
        let note = store.get(top[0].id).unwrap().unwrap();
        // the title heading went into the title
        assert!(note.content.starts_with("正文 **粗**") && note.content.contains("来源：测试库"), "{}", note.content);
        assert_eq!(store.children(Some(top[0].id)).unwrap().len(), 2);
        assert!(Package::open(&dir.join("missing.zip")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod real {
    use super::*;

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
}
