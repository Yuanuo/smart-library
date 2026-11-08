//! HTML articles: the content of user-made trees in a bookcase (`a/LZ/…`
//! Trilium exports, `a/ZB/…` with `<meta author= book= …>`), and later any
//! HTML document.
//!
//! An article is read into the same [`VolumeText`] as a volume, with
//! **paragraphs in place of lines**: every block (p, h1–h6, li, td …) starts
//! a "line" named `¶1`, `¶2` … So search (`search::locate`, `snippets`),
//! highlighting and jumping work unchanged; the position in a "line" is the
//! position in the paragraph. The structured HTML is written by the same walk
//! through [`HtmlOut`], so its char counting matches `plain` exactly (as for
//! volumes, see `render.rs`).
//!
//! The HTML is rebuilt from a small allow-list of elements: no scripts,
//! styles, event handlers or links leave the reader.

use crate::markup::{self, Element, Node};
use crate::render::{HtmlOut, RenderOptions};
use crate::volume::{assemble, VolumeText};

#[derive(Debug, Clone, Default)]
pub struct ArticleMeta {
    /// `<title>`, else the `name` meta
    pub title: String,
    /// `<meta author=…>` (several allowed)
    pub authors: Vec<String>,
    pub period: String,
    pub book: String,
    pub library: String,
    pub catalog: String,
}

#[derive(Debug, Clone)]
pub struct Heading {
    pub text: String,
    /// paragraph marker, e.g. `¶3`
    pub lb: String,
    /// 1..=6
    pub level: u8,
}

#[derive(Debug, Clone)]
pub struct Article {
    pub text: VolumeText,
    pub meta: ArticleMeta,
    pub headings: Vec<Heading>,
    /// structured HTML when rendering was asked for
    pub html: Option<String>,
}

/// Read an article. `path` is its path in the bookcase (for relative images);
/// `render` asks for the structured HTML too.
pub fn read_article(html: &str, path: &str, render: Option<&RenderOptions>) -> Article {
    let root = markup::parse(html);
    let meta = read_meta(&root);
    let body = root.find_descendant("body").unwrap_or(&root);
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("").to_string();
    let mut w = Walker {
        pieces: Vec::new(),
        lbs: Vec::new(),
        cur_lb: None,
        html: render.map(HtmlOut::new),
        dir,
        headings: Vec::new(),
    };
    w.walk_children(body);
    let text = assemble(String::new(), &w.pieces, &w.lbs, &[]);
    Article {
        text,
        meta,
        headings: w.headings,
        html: w.html.map(|h| h.buf),
    }
}

fn read_meta(root: &Element) -> ArticleMeta {
    let mut m = ArticleMeta::default();
    let mut metas = Vec::new();
    root.descendants("meta", &mut metas);
    let mut name = String::new();
    for e in metas {
        for (k, v) in &e.attrs {
            let v = v.trim().to_string();
            if v.is_empty() {
                continue;
            }
            match k.as_str() {
                "author" => m.authors.push(v),
                "period" => m.period = v,
                "book" => m.book = v,
                "library" => m.library = v,
                "catalog" => m.catalog = v,
                "name" if e.attr("content").is_none() => name = v,
                _ => {}
            }
        }
    }
    m.title = root
        .find_descendant("title")
        .map(|t| t.text().trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or(name);
    m
}

/// `a/ZB/ZBkc006` + `a/69.jpg` -> `a/ZB/ZBkc006/a/69.jpg`; no absolute or
/// remote sources, no way out of the bookcase.
pub fn resolve_relative(dir: &str, src: &str) -> Option<String> {
    let src = src.split(['?', '#']).next().unwrap_or("");
    if src.is_empty() || src.contains(':') || src.starts_with('/') || src.starts_with('\\') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in src.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// `%E3%80%8A` -> `《` (invalid escapes are kept as they are).
fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        (b as char).to_digit(16).map(|d| d as u8)
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const SKIP: [&str; 12] = [
    "script", "style", "head", "title", "meta", "link", "noscript", "svg", "button", "input", "select", "template",
];
const BLOCKS: [&str; 10] = ["p", "li", "blockquote", "pre", "dt", "dd", "figcaption", "td", "th", "caption"];
const CONTAINERS: [&str; 9] = ["div", "section", "article", "main", "body", "header", "footer", "aside", "nav"];

struct Walker {
    pieces: Vec<(String, Option<usize>)>,
    lbs: Vec<String>,
    cur_lb: Option<usize>,
    html: Option<HtmlOut>,
    dir: String,
    headings: Vec<Heading>,
}

impl Walker {
    /// A new paragraph ("line") starts here.
    fn start_block(&mut self) -> String {
        // paragraphs are separated by a space in `text` (not counted in
        // `plain`, not written to the HTML): snippets do not run together
        if !self.pieces.is_empty() {
            self.pieces.push((" ".to_string(), self.cur_lb));
        }
        let lb = format!("¶{}", self.lbs.len() + 1);
        self.lbs.push(lb.clone());
        self.cur_lb = Some(self.lbs.len() - 1);
        if let Some(h) = self.html.as_mut() {
            h.open("span", &[("class", "lb"), ("data-lb", &lb)]);
            h.close("span");
        }
        lb
    }

    fn emit(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !matches!(c, '\r' | '\n' | '\t')).collect();
        if s.trim().is_empty() && self.cur_lb.is_none() {
            return;
        }
        if s.is_empty() {
            return;
        }
        if self.cur_lb.is_none() {
            self.start_block();
        }
        self.pieces.push((s.clone(), self.cur_lb));
        if let Some(h) = self.html.as_mut() {
            h.text(&s);
        }
    }

    fn walk_children(&mut self, el: &Element) {
        for n in &el.children {
            match n {
                Node::Text(t) => self.emit(t),
                Node::Element(e) => self.walk(e),
            }
        }
    }

    fn open(&mut self, tag: &str, class: &str) {
        if let Some(h) = self.html.as_mut() {
            h.open(tag, &[("class", class)]);
        }
    }

    fn close(&mut self, tag: &str) {
        if let Some(h) = self.html.as_mut() {
            h.close(tag);
        }
    }

    fn block(&mut self, el: &Element, tag: &str, class: &str) {
        self.open(tag, class);
        self.start_block();
        self.walk_children(el);
        self.close(tag);
        // text after the block (outside any block) starts a new paragraph
        self.cur_lb = None;
    }

    fn walk(&mut self, el: &Element) {
        let name = el.name.as_str();
        if SKIP.contains(&name) || el.attr("data-trilium-h1").is_some() {
            return;
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<u8>().unwrap_or(2);
                let text = el.text().trim().to_string();
                self.open("div", &format!("head h{}", level));
                let lb = self.start_block();
                if !text.is_empty() {
                    self.headings.push(Heading { text, lb, level });
                }
                self.walk_children(el);
                self.close("div");
                self.cur_lb = None;
            }
            n if BLOCKS.contains(&n) => {
                let class_attr = el.attr("class").unwrap_or("");
                let verse = class_attr.contains("l-js") || class_attr.contains("颂");
                let (tag, class) = match n {
                    "li" => ("li", "item"),
                    "td" | "th" => ("td", ""),
                    "pre" => ("p", "p t-pre"),
                    "blockquote" => ("p", "p quote"),
                    _ if verse => ("p", "p verse"),
                    _ => ("p", "p"),
                };
                self.block(el, tag, class);
            }
            n if CONTAINERS.contains(&n) => {
                // a container with its own text is a paragraph too
                let has_text = el.children.iter().any(|c| matches!(c, Node::Text(t) if !t.trim().is_empty()));
                if has_text {
                    self.block(el, "p", "p");
                } else {
                    self.walk_children(el);
                }
            }
            "ul" | "ol" => {
                self.open("ul", "list");
                self.walk_children(el);
                self.close("ul");
                self.cur_lb = None;
            }
            "table" => {
                self.open("table", "table");
                self.walk_children(el);
                self.close("table");
                self.cur_lb = None;
            }
            "tr" => {
                self.open("tr", "");
                self.walk_children(el);
                self.close("tr");
            }
            "br" => {
                if let Some(h) = self.html.as_mut() {
                    h.raw("<br>");
                }
            }
            "img" => {
                if let Some(path) = el.attr("src").and_then(|s| resolve_relative(&self.dir, s)) {
                    if let Some(h) = self.html.as_mut() {
                        h.img(&path);
                    }
                }
            }
            "b" | "strong" => {
                self.open("span", "hi r-bold");
                self.walk_children(el);
                self.close("span");
            }
            "i" | "em" => {
                self.open("em", "");
                self.walk_children(el);
                self.close("em");
            }
            // a footnote marker `<a id="footnote-1" data-note="%E3…">`: the note
            // text is not part of the text; the page shows a mark and the text on hover
            "a" if el.attr("data-note").is_some() => {
                let note = percent_decode(el.attr("data-note").unwrap_or("")).trim().to_string();
                if !note.is_empty() {
                    if let Some(h) = self.html.as_mut() {
                        h.open("span", &[("class", "fn"), ("data-fn", &note)]);
                        h.close("span");
                    }
                }
            }
            "sup" | "sub" => {
                self.open(name, "");
                self.walk_children(el);
                self.close(name);
            }
            _ => self.walk_children(el),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{locate, MatchMode};

    // shapes of the two real formats (bookcase 2025.Q1 with the user's a/ trees)
    const LZ: &str = r#"<html><head><meta charset="utf-8"><link rel="stylesheet" href="../../style.css">
<title data-trilium-title>一切法高王经</title></head><body><div class="content">
<h1 data-trilium-h1>一切法高王经</h1><div class="ck-content"><div class="content"><div class="ck-content">
<h2 class="head">一切法高王经</h2>
<p>如是我闻。一时婆伽婆。</p>
<p class="l-js-5c">我既新出家　　朝日始受戒</p>
<script>alert(1)</script></div></div></div></div></body></html>"#;

    const ZB: &str = r#"<!doctype html><html data-ver="467"><head>
<meta author="托嘎如意宝"><meta author="索达吉堪布"><meta library="智悲文集"><meta catalog="课程">
<meta period="现代"><meta book="赞戒论讲解"><meta name="第5课"><meta charset="UTF-8"></head><body>
<p id="a1" class="正文_正文-科判">戊一、<a id="a-7"></a>持戒之功德：</p>
<p class="正文_正文-颂词" id="aE">无误取舍因果持戒士，<a id="a-F"></a>死时勇士空行作迎接，</p>
<p><img class="x" src="a/69.jpg" alt="" id="av"></p></body></html>"#;

    #[test]
    fn reads_a_trilium_export_paragraph_by_paragraph() {
        let a = read_article(LZ, "a/LZ/大乘五大部外重译经/一切法高王经.html", Some(&RenderOptions::default()));
        assert_eq!(a.meta.title, "一切法高王经");
        // the duplicated h1 is skipped; head + 2 paragraphs
        let lbs: Vec<&str> = a.text.lines.iter().map(|l| l.lb.as_deref().unwrap()).collect();
        assert_eq!(lbs, vec!["¶1", "¶2", "¶3"]);
        assert_eq!(a.text.plain, "一切法高王经如是我闻一时婆伽婆我既新出家朝日始受戒");
        assert_eq!(a.headings.len(), 1);
        let html = a.html.unwrap();
        assert!(!html.contains("script") && !html.contains("alert"));
        assert!(html.contains("class=\"p verse\""));
    }

    #[test]
    fn keeps_footnote_markers_out_of_the_text() {
        let src = r#"<body><p>见断是112种。<a id="a-1"></a><a id="footnote-133" data-note="%E3%80%8A%E6%99%BA%E3%80%8B%EF%BC%9A%E2%80%9C%E4%B8%80%E2%80%9D"></a>修断。</p></body>"#;
        let a = read_article(src, "a/ZB/x/1.html", Some(&RenderOptions::default()));
        assert_eq!(a.text.plain, "见断是112种修断");
        let html = a.html.unwrap();
        assert!(html.contains("class=\"fn\"") && html.contains("data-fn=\"《智》：“一”\""), "{html}");
    }

    #[test]
    fn reads_metadata_images_and_locates_hits() {
        let opts = RenderOptions { asset_prefix: "@/".into(), ..Default::default() };
        let a = read_article(ZB, "a/ZB/ZBkc006/ZBkc006_006.html", Some(&opts));
        assert_eq!(a.meta.authors, vec!["托嘎如意宝", "索达吉堪布"]);
        assert_eq!((a.meta.book.as_str(), a.meta.period.as_str()), ("赞戒论讲解", "现代"));
        assert_eq!(a.meta.title, "第5课");
        assert!(a.html.as_ref().unwrap().contains("src=\"@/a/ZB/ZBkc006/a/69.jpg\""));
        let occ = locate(&a.text, "持戒", MatchMode::Phrase);
        let at: Vec<(&str, usize)> = occ.iter().map(|o| (o.lb.as_str(), o.pos)).collect();
        assert_eq!(at, vec![("¶1", 3), ("¶2", 7)]);
    }

    #[test]
    fn resolves_only_paths_inside_the_bookcase() {
        assert_eq!(resolve_relative("a/ZB/x", "a/69.jpg").as_deref(), Some("a/ZB/x/a/69.jpg"));
        assert_eq!(resolve_relative("a/ZB/x", "../y.jpg").as_deref(), Some("a/ZB/y.jpg"));
        assert_eq!(resolve_relative("a", "../../etc/passwd"), None);
        assert_eq!(resolve_relative("a", "http://x/y.jpg"), None);
    }
}
