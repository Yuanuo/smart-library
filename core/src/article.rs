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
    /// Source attribution, separated from the searchable document text.
    pub attribution: Option<String>,
    /// Original HTML fragment identifiers mapped to reading paragraphs.
    pub anchors: std::collections::HashMap<String, String>,
    /// Plain-text ranges displayed normally but omitted from full-text search.
    pub search_exclusions: Vec<(usize, usize)>,
}

impl Article {
    /// Preserve every original plain-text offset while preventing matches in
    /// excluded headings and across their boundaries.
    pub fn search_view(&self) -> VolumeText {
        let mut text = self.text.clone();
        if !self.search_exclusions.is_empty() {
            let mut ranges = self.search_exclusions.iter().peekable();
            text.plain = text.plain.chars().enumerate().map(|(i, c)| {
                while ranges.peek().is_some_and(|&&(_, end)| end <= i) { ranges.next(); }
                if ranges.peek().is_some_and(|&&(start, end)| start <= i && i < end) { crate::analyzer::INDEX_GAP } else { c }
            }).collect();
        }
        text
    }
}

/// What a data source needs on top of plain HTML. There is one HTML reader
/// for every source (CBETA's `a/` pages, folder files, ZIM articles); the
/// differences are listed here instead of being spread through the walker as
/// `if source == …`, so a quirk added for one source cannot silently change
/// how another one is read. A new source starts from `PLAIN_RULES` and adds
/// only the fields it really needs.
#[derive(Clone, Copy)]
pub struct Rules {
    /// element ids become anchors the reader can jump to
    ids_as_anchors: bool,
    /// links are handed to the window, which resolves them in the source
    resolved_links: bool,
    /// drop wiki-style generic headings (语录 / 参考文献 / 参见) from search
    skip_generic_headings: bool,
}

/// Plain HTML: no anchors, no link resolution, no heading heuristics.
pub const PLAIN_RULES: Rules = Rules { ids_as_anchors: false, resolved_links: false, skip_generic_headings: false };

/// ZIM: the window resolves internal links in the archive, `id`s are the
/// article's chapters, and Wikipedia-style dumps carry headings that are not
/// part of the text.
pub const ZIM_RULES: Rules = Rules { ids_as_anchors: true, resolved_links: true, skip_generic_headings: true };

/// Read an article. `path` is its path in the bookcase (for relative images);
/// `render` asks for the structured HTML too.
pub fn read_article(html: &str, path: &str, render: Option<&RenderOptions>) -> Article {
    read_article_inner(html, path, render, PLAIN_RULES)
}

/// ZIM articles retain safe, inert internal-link data for the reader to resolve.
pub fn read_zim_html(html: &str, path: &str, render: Option<&RenderOptions>) -> Article {
    read_article_inner(html, path, render, ZIM_RULES)
}

fn read_article_inner(html: &str, path: &str, render: Option<&RenderOptions>, rules: Rules) -> Article {
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
        rules,
        anchors: std::collections::HashMap::new(),
        pending_anchors: Vec::new(),
        plain_len: 0,
        search_exclusions: Vec::new(),
    };
    w.walk_children(body);
    let text = assemble(String::new(), &w.pieces, &w.lbs, &[]);
    Article {
        text,
        meta,
        headings: w.headings,
        html: w.html.map(|h| h.buf),
        attribution: None,
        anchors: w.anchors,
        search_exclusions: w.search_exclusions,
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

/// Content the page hides with inline CSS or the `hidden` attribute. It is
/// not rendered by a browser, so it must not become text here either: the
/// reader drops `style` attributes, and Wikihow (among others) keeps image
/// metadata such as `{"smallUrl":…,"licensing":…}` in a `display:none` span.
fn hidden(el: &Element) -> bool {
    if el.attr("hidden").is_some() {
        return true;
    }
    let Some(style) = el.attr("style") else { return false };
    let s: String = style
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    s.contains("display:none") || s.contains("visibility:hidden")
}

fn generic_heading(text: &str) -> bool {
    matches!(crate::analyzer::match_key(text).into_iter().collect::<String>().as_str(), "语录" | "参考文献" | "参见")
}

struct Walker {
    pieces: Vec<(String, Option<usize>)>,
    lbs: Vec<String>,
    cur_lb: Option<usize>,
    html: Option<HtmlOut>,
    dir: String,
    headings: Vec<Heading>,
    rules: Rules,
    anchors: std::collections::HashMap<String, String>,
    pending_anchors: Vec<String>,
    plain_len: usize,
    search_exclusions: Vec<(usize, usize)>,
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
        for id in self.pending_anchors.drain(..) {
            self.anchors.entry(id).or_insert_with(|| lb.clone());
        }
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
        self.plain_len += s.chars().filter(|&c| !crate::volume::is_dropped(c)).count();
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
        if SKIP.contains(&name) || el.attr("data-trilium-h1").is_some() || hidden(el) {
            return;
        }
        if self.rules.ids_as_anchors {
            if let Some(id) = el.attr("id").filter(|id| !id.is_empty()) {
                if let Some(i) = self.cur_lb {
                    self.anchors.entry(id.to_string()).or_insert_with(|| self.lbs[i].clone());
                } else {
                    self.pending_anchors.push(id.to_string());
                }
            }
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<u8>().unwrap_or(2);
                let text = el.text().trim().to_string();
                let exclude = self.rules.skip_generic_headings && level >= 2 && generic_heading(&text);
                self.open("div", &format!("head h{}", level));
                let lb = self.start_block();
                if !text.is_empty() {
                    self.headings.push(Heading { text, lb, level });
                }
                let start = self.plain_len;
                self.walk_children(el);
                if exclude && self.plain_len > start {
                    self.search_exclusions.push((start, self.plain_len));
                }
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
            "img" if el.attr("src").is_some_and(|s| s.starts_with("attach:")) => {
                // a picture pasted into a note: kept in the library's attachments
                let name = el.attr("src").unwrap_or("").trim_start_matches("attach:");
                if crate::attach::is_name(name) {
                    if let Some(h) = self.html.as_mut() {
                        h.img(&format!("attachments/{}", name));
                    }
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
            // a link back to a place of a document (a clipping's source)
            "a" if el.attr("data-src").is_some_and(|s| s.starts_with("smartlib://")) => {
                if let Some(h) = self.html.as_mut() {
                    h.open("a", &[("class", "src"), ("data-src", el.attr("data-src").unwrap_or(""))]);
                }
                self.walk_children(el);
                self.close("a");
            }
            "a" if self.rules.resolved_links && el.attr("href").is_some_and(|href| crate::zim::is_internal_href(href) || crate::zim::external_url(href).is_some()) => {
                // `#/File:…` is a ZIM path to a file (the link around an
                // image), not an anchor in this article: keep the image, but
                // no link — clicking it would look for a chapter by that name
                if el.attr("href").is_some_and(|href| href.trim_start().starts_with("#/")) {
                    self.walk_children(el);
                    return;
                }
                if let Some(h) = self.html.as_mut() {
                    let href = el.attr("href").unwrap();
                    let kind = if crate::zim::external_url(href).is_some() { "external" } else { "internal" };
                    h.open("a", &[("class", "zim-link"), ("href", "#"), ("data-zim-href", href), ("data-link-kind", kind)]);
                }
                self.walk_children(el);
                self.close("a");
            }
            "a" if el.attr("href").is_some_and(|href| crate::zim::external_url(href).is_some()) => {
                if let Some(h) = self.html.as_mut() {
                    let href = el.attr("href").unwrap();
                    h.open("a", &[("class", "ext-link"), ("href", "#"), ("data-ext-href", href)]);
                }
                self.walk_children(el);
                self.close("a");
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

    /// The reader drops `style` attributes, so text a page hides with CSS has
    /// to be left out here too (Wikihow keeps image JSON in a hidden span).
    #[test]
    fn leaves_out_content_hidden_by_style_or_attribute() {
        let src = r#"<body><div><p>正文</p>
        <div class="image_details" style="display:none;"><span style="display: none;">{"smallUrl":"https:\/\/x","licensing":"&lt;div&gt;"}</span></div>
        <span style="visibility: hidden">看不见</span><span>可见</span><p hidden>隐藏段落</p>
        <span style="display:inline">行内</span></div></body>"#;
        let a = read_article(src, "C/测试", Some(&RenderOptions::default()));
        assert_eq!(a.text.plain, "正文可见行内");
        let html = a.html.unwrap();
        assert!(!html.contains("smallUrl") && !html.contains("看不见") && !html.contains("隐藏段落"));
    }

    /// Wikihow wraps images in `<a href="#/File:…">`; that is a ZIM file
    /// path, not an anchor, so the image stays but the link goes away.
    #[test]
    fn zim_image_links_are_not_chapters() {
        let src = r##"<body><p>正文</p><a class="image" href="#/File:A.jpg"><img src="./images/a.webp" alt="图"/></a><a href="#章节">跳到章节</a></body>"##;
        let a = read_zim_html(src, "C/某条目", Some(&RenderOptions::default()));
        let html = a.html.unwrap();
        assert!(html.contains("data-zim-href=\"#章节\""), "{html}");
        assert!(!html.contains("File:A.jpg"), "{html}");
        assert!(html.contains("<img"), "{html}");
    }

    #[test]
    fn resolves_only_paths_inside_the_bookcase() {
        assert_eq!(resolve_relative("a/ZB/x", "a/69.jpg").as_deref(), Some("a/ZB/x/a/69.jpg"));
        assert_eq!(resolve_relative("a/ZB/x", "../y.jpg").as_deref(), Some("a/ZB/y.jpg"));
        assert_eq!(resolve_relative("a", "../../etc/passwd"), None);
        assert_eq!(resolve_relative("a", "http://x/y.jpg"), None);
    }
}
