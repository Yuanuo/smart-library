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
use crate::html_out::{HtmlOut, RenderOptions};
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct KepanEntry {
    /// paragraph marker, e.g. `¶3`
    pub lb: String,
    /// true for `kepan-split` (`分二：…`), false for `kepan-head`
    pub is_split: bool,
    /// visible text of the kepan paragraph
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerseEntry {
    /// paragraph marker, e.g. `¶4`
    pub lb: String,
    /// visible text of the verse paragraph
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Article {
    pub text: VolumeText,
    pub meta: ArticleMeta,
    pub headings: Vec<Heading>,
    /// detected 科判 paragraphs (populated when rendered with `detect_structure`)
    pub kepans: Vec<KepanEntry>,
    /// detected 颂词 paragraphs (populated when rendered with `detect_structure`)
    pub verses: Vec<VerseEntry>,
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
    /// detect 科判 and 颂词 paragraphs for display styling
    detect_structure: bool,
}

/// Plain HTML: no anchors, no link resolution, no heading heuristics.
pub const PLAIN_RULES: Rules = Rules {
    ids_as_anchors: false,
    resolved_links: false,
    skip_generic_headings: false,
    detect_structure: true,
};

/// ZIM: the window resolves internal links in the archive, `id`s are the
/// article's chapters, and Wikipedia-style dumps carry headings that are not
/// part of the text.
pub const ZIM_RULES: Rules = Rules {
    ids_as_anchors: true,
    resolved_links: true,
    skip_generic_headings: true,
    detect_structure: false,
};

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
    let block_kinds = if render.is_some() && rules.detect_structure {
        classify_blocks(body)
    } else {
        std::collections::HashMap::new()
    };
    let mut w = Walker {
        pieces: Vec::new(),
        lbs: Vec::new(),
        cur_lb: None,
        html: render.map(HtmlOut::new),
        dir,
        headings: Vec::new(),
        kepans: Vec::new(),
        verses: Vec::new(),
        rules,
        anchors: std::collections::HashMap::new(),
        pending_anchors: Vec::new(),
        plain_len: 0,
        search_exclusions: Vec::new(),
        block_kinds,
    };
    w.walk_children(body);
    let text = assemble(String::new(), &w.pieces, &w.lbs, &[]);
    Article {
        text,
        meta,
        headings: w.headings,
        kepans: w.kepans,
        verses: w.verses,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockKind {
    KepanHead,
    KepanSplit,
    Verse,
    VerseQuote,
}

#[derive(Clone, Copy, Debug)]
struct VerseMetrics {
    clause_len: usize,
    clauses: usize,
    ends_with_comma: bool,
    has_punct: bool,
    starts_with_quote: bool,
}

fn is_hanzi(c: char) -> bool {
    matches!(
        c,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{2FA1F}'
    )
}

fn is_ganzhi(c: char) -> bool {
    "甲乙丙丁戊己庚辛壬癸子丑寅卯辰巳午未申酉戌亥".contains(c)
}

fn is_kepan_num(c: char) -> bool {
    "一二三四五六七八九十百千零〇廿卅".contains(c) || c.is_ascii_digit()
}

/// Strip leading `<天干/地支><中文或阿拉伯数字>`, returning the remainder.
fn strip_ganzhi_num(s: &str) -> Option<&str> {
    let mut it = s.chars();
    let first = it.next()?;
    if !is_ganzhi(first) {
        return None;
    }
    let rest = it.as_str();
    let num_bytes: usize = rest.chars().take_while(|&c| is_kepan_num(c)).map(char::len_utf8).sum();
    if num_bytes == 0 {
        return None;
    }
    Some(&rest[num_bytes..])
}

/// Find `分<数字>：` (or `:`) in `s`, returning `(before_分, after_colon)`.
fn find_fen_colon(s: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    while let Some(pos) = s[offset..].find('分') {
        let at = offset + pos;
        let after_fen = &s[at + '分'.len_utf8()..];
        let num_bytes: usize = after_fen.chars().take_while(|&c| is_kepan_num(c)).map(char::len_utf8).sum();
        if num_bytes > 0 {
            let after_num = &after_fen[num_bytes..];
            if let Some(after_colon) = after_num.strip_prefix('：').or_else(|| after_num.strip_prefix(':')) {
                return Some((&s[..at], after_colon));
            }
        }
        offset = at + '分'.len_utf8();
    }
    None
}

fn detect_kepan(raw: &str) -> Option<BlockKind> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(after) = strip_ganzhi_num(t) {
        if let Some((before, _)) = find_fen_colon(after) {
            if before.chars().count() <= 32 && !before.chars().any(|c| matches!(c, '。' | '！' | '？')) {
                return Some(BlockKind::KepanSplit);
            }
        }
        if let Some(body) = after.strip_prefix('、') {
            let body = body.trim();
            if body.is_empty() {
                return None;
            }
            let has_sent_end = body.chars().any(|c| matches!(c, '。' | '！' | '？'));
            if !has_sent_end && (body.ends_with('：') || body.ends_with(':') || body.chars().count() <= 80) {
                return Some(BlockKind::KepanHead);
            }
        }
        return None;
    }
    if let Some((before, after_colon)) = find_fen_colon(t) {
        if before.chars().count() <= 24 && !before.chars().any(|c| matches!(c, '。' | '！' | '？')) {
            let tail = after_colon.trim_start();
            if let Some(after_gz) = strip_ganzhi_num(tail) {
                if after_gz.starts_with(['、', '（', '(', '分']) {
                    return Some(BlockKind::KepanSplit);
                }
            }
        }
    }
    None
}

/// Strip balanced inline parenthetical notes `（…）` / `(…)` (such as Tibetan
/// seed syllables or variant readings inside a verse). Returns `None` if
/// parentheses are unbalanced.
fn strip_inline_parens(s: &str) -> Option<String> {
    if !s.contains(['（', '）', '(', ')']) {
        return Some(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '（' | '(' => depth += 1,
            '）' | ')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    Some(out)
}

fn starts_with_open_quote(s: &str) -> bool {
    s.trim_start().starts_with(['“', '‘', '「', '『'])
}

fn verse_metrics(raw: &str) -> Option<VerseMetrics> {
    let stripped = strip_inline_parens(raw)?;
    let t = stripped.trim();
    if t.is_empty() {
        return None;
    }
    // Allow a trailing colon (e.g. `欲令大众悦，即时说颂言：`), but reject internal colons (`答：…`).
    let without_trailing_colon = t.strip_suffix('：').or_else(|| t.strip_suffix(':')).unwrap_or(t);
    if without_trailing_colon.contains('：') || without_trailing_colon.contains(':') {
        return None;
    }
    let mut clause_len: Option<usize> = None;
    let mut clauses = 0usize;
    for seg in without_trailing_colon
        .split(|c: char| matches!(c, '，' | '。' | '！' | '？' | '；' | ',' | ';' | '!' | '?') || c.is_whitespace())
    {
        if seg.is_empty() {
            continue;
        }
        let mut len = 0usize;
        for c in seg.chars() {
            if matches!(c, '“' | '”' | '‘' | '’' | '「' | '」' | '『' | '』') {
                continue;
            }
            if !is_hanzi(c) {
                return None;
            }
            len += 1;
        }
        if len == 0 {
            continue;
        }
        if !(3..=17).contains(&len) {
            return None;
        }
        match clause_len {
            None => clause_len = Some(len),
            Some(prev) if prev != len => return None,
            Some(_) => {}
        }
        clauses += 1;
    }
    let clause_len = clause_len?;
    let trimmed_end = t.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '”' | '’' | '」' | '』'));
    let ends_with_comma = trimmed_end.ends_with('，') || trimmed_end.ends_with(',');
    let has_punct = t.chars().any(|c| matches!(c, '，' | '。' | '！' | '？' | '；' | '：' | ',' | ';' | '!' | '?' | ':'));
    let starts_with_quote = starts_with_open_quote(raw);
    Some(VerseMetrics { clause_len, clauses, ends_with_comma, has_punct, starts_with_quote })
}

/// Extract visible text of a block element for structure classification,
/// preserving `<br>` as a space separator and skipping hidden / footnote nodes.
fn visible_text(el: &Element, out: &mut String) {
    for n in &el.children {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) => {
                let name = e.name.as_str();
                if SKIP.contains(&name) || e.attr("data-trilium-h1").is_some() || hidden(e) {
                    continue;
                }
                if name == "br" {
                    out.push(' ');
                } else if name == "a" && e.attr("data-note").is_some() {
                    continue;
                } else {
                    visible_text(e, out);
                }
            }
        }
    }
}

struct BlockSlot {
    ptr: *const Element,
    kind: Option<BlockKind>,
    verse: Option<VerseMetrics>,
}

fn collect_blocks(el: &Element, out: &mut Vec<BlockSlot>) {
    for n in &el.children {
        let Node::Element(e) = n else { continue };
        let name = e.name.as_str();
        if SKIP.contains(&name) || e.attr("data-trilium-h1").is_some() || hidden(e) {
            continue;
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                out.push(BlockSlot { ptr: e as *const Element, kind: None, verse: None });
            }
            n if BLOCKS.contains(&n) => {
                let class_attr = e.attr("class").unwrap_or("");
                let explicit_kepan = class_attr.contains("科判");
                let explicit_verse = class_attr.contains("l-js") || class_attr.contains("颂");
                let mut kind = None;
                let mut verse = None;
                if explicit_kepan {
                    let mut txt = String::new();
                    visible_text(e, &mut txt);
                    kind = Some(match detect_kepan(&txt) {
                        Some(BlockKind::KepanSplit) => BlockKind::KepanSplit,
                        _ => BlockKind::KepanHead,
                    });
                } else if explicit_verse {
                    let mut txt = String::new();
                    visible_text(e, &mut txt);
                    kind = Some(if starts_with_open_quote(&txt) {
                        BlockKind::VerseQuote
                    } else {
                        BlockKind::Verse
                    });
                } else if matches!(n, "p" | "blockquote") {
                    let mut txt = String::new();
                    visible_text(e, &mut txt);
                    if let Some(k) = detect_kepan(&txt) {
                        kind = Some(k);
                    } else {
                        verse = verse_metrics(&txt);
                    }
                }
                out.push(BlockSlot { ptr: e as *const Element, kind, verse });
            }
            n if CONTAINERS.contains(&n) => {
                let has_text = e.children.iter().any(|c| matches!(c, Node::Text(t) if !t.trim().is_empty()));
                if has_text {
                    out.push(BlockSlot { ptr: e as *const Element, kind: None, verse: None });
                } else {
                    collect_blocks(e, out);
                }
            }
            "ul" | "ol" | "table" | "tr" => {
                collect_blocks(e, out);
            }
            _ => collect_blocks(e, out),
        }
    }
}

fn classify_blocks(body: &Element) -> std::collections::HashMap<*const Element, BlockKind> {
    let mut slots = Vec::new();
    collect_blocks(body, &mut slots);

    // Resolve verse candidates in contiguous runs sharing the same clause length.
    let mut i = 0;
    while i < slots.len() {
        let Some(first) = slots[i].verse else {
            i += 1;
            continue;
        };
        let len = first.clause_len;
        let has_punct = first.has_punct;
        let mut j = i + 1;
        while j < slots.len()
            && slots[j].verse.is_some_and(|v| v.clause_len == len && v.has_punct == has_punct)
        {
            j += 1;
        }
        let run_len = j - i;
        let total_clauses: usize = slots[i..j].iter().filter_map(|s| s.verse).map(|v| v.clauses).sum();
        let max_clauses: usize = slots[i..j].iter().filter_map(|s| s.verse).map(|v| v.clauses).max().unwrap_or(0);
        let prev_is_kepan = i > 0
            && matches!(slots[i - 1].kind, Some(BlockKind::KepanHead | BlockKind::KepanSplit));
        let confirm = total_clauses >= 4
            || (run_len >= 2 && (max_clauses >= 2 || len >= 5))
            || (run_len == 1
                && first.clauses >= 2
                && (prev_is_kepan || first.ends_with_comma || !first.has_punct));
        if confirm {
            for s in &mut slots[i..j] {
                s.kind = Some(if s.verse.is_some_and(|v| v.starts_with_quote) {
                    BlockKind::VerseQuote
                } else {
                    BlockKind::Verse
                });
            }
        }
        i = j;
    }

    let mut map = std::collections::HashMap::new();
    for s in slots {
        if let Some(k) = s.kind {
            map.insert(s.ptr, k);
        }
    }
    map
}

fn parse_series_num(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse::<u32>().ok();
    }
    let mut total: u32 = 0;
    let mut cur: u32 = 0;
    for c in s.chars() {
        match c {
            '零' | '〇' => cur = 0,
            '一' => cur = 1,
            '二' => cur = 2,
            '三' => cur = 3,
            '四' => cur = 4,
            '五' => cur = 5,
            '六' => cur = 6,
            '七' => cur = 7,
            '八' => cur = 8,
            '九' => cur = 9,
            '十' => {
                total += (if cur == 0 { 1 } else { cur }) * 10;
                cur = 0;
            }
            '廿' => {
                total += 20;
                cur = 0;
            }
            '卅' => {
                total += 30;
                cur = 0;
            }
            '百' => {
                total += (if cur == 0 { 1 } else { cur }) * 100;
                cur = 0;
            }
            '千' => {
                total += (if cur == 0 { 1 } else { cur }) * 1000;
                cur = 0;
            }
            _ => return None,
        }
    }
    Some(total + cur)
}

/// Extract `(prefix, unit, seq)` from titles like `第5课`, `赞戒论第12课`, `第3修法`.
fn series_key(title: &str) -> Option<(String, &'static str, u32)> {
    const UNITS: &[&str] = &["修法", "课", "卷", "章", "讲", "品", "回", "节", "篇", "部", "集", "辑", "册"];
    let mut offset = 0;
    while let Some(pos) = title[offset..].find('第') {
        let at = offset + pos;
        let after_di = &title[at + '第'.len_utf8()..];
        let num_bytes: usize = after_di.chars().take_while(|&c| is_kepan_num(c)).map(char::len_utf8).sum();
        if num_bytes > 0 {
            let num_str = &after_di[..num_bytes];
            let after_num = &after_di[num_bytes..];
            if let Some(&unit) = UNITS.iter().find(|&&u| after_num.starts_with(u)) {
                if let Some(seq) = parse_series_num(num_str) {
                    let prefix = title[..at]
                        .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '_' | '-' | '·' | '—' | '：' | ':'))
                        .trim()
                        .to_string();
                    return Some((prefix, unit, seq));
                }
            }
        }
        offset = at + '第'.len_utf8();
    }
    None
}

/// Sibling files to exclude from cross-document kepan linking:
/// JSON-based formats (`.md.json`, `.map.json`, `.wb.json`, `.json`) and
/// documents whose name/title contains `科判`.
pub fn is_ignored_kepan_sibling(path: &str, title: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".json") {
        return true;
    }
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    title.contains("科判") || file_name.contains("科判")
}

/// Filter and order sibling `(path, title)` pairs for cross-document kepan linking:
/// 1. Always ignore `.json` (`.md.json`, `.map.json`, etc.) and names containing `科判`.
/// 2. If `cur_title` follows a numbered series (`第n课`, `第n修法`, …), keep only
///    siblings of the same `(prefix, unit)` series sorted by `n`.
/// 3. Otherwise (e.g. translations without `第n`), keep all non-ignored siblings in folder order.
pub fn select_kepan_siblings(
    siblings: &[(String, String)],
    cur_path: &str,
    cur_title: &str,
) -> Vec<(String, String)> {
    if is_ignored_kepan_sibling(cur_path, cur_title) {
        return Vec::new();
    }
    let filtered: Vec<(usize, &(String, String))> = siblings
        .iter()
        .enumerate()
        .filter(|(_, (p, t))| !is_ignored_kepan_sibling(p, t))
        .collect();
    if let Some((prefix, unit, _)) = series_key(cur_title) {
        let mut matched: Vec<(u32, usize, (String, String))> = filtered
            .into_iter()
            .filter_map(|(idx, (p, t))| {
                let (sp, su, seq) = series_key(t)?;
                (sp == prefix && su == unit).then(|| (seq, idx, (p.clone(), t.clone())))
            })
            .collect();
        matched.sort_by_key(|(seq, idx, _)| (*seq, *idx));
        return matched.into_iter().map(|(_, _, item)| item).collect();
    }
    filtered.into_iter().map(|(_, item)| item.clone()).collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KepanRef {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_title: Option<String>,
    pub lb: String,
    pub label: String,
    pub title: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KepanChildLink {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkedKepan {
    pub lb: String,
    pub kind: &'static str,
    pub level: u8,
    pub label: String,
    pub title: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<KepanRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<KepanChildLink>,
}

fn ganzhi_level(c: char) -> Option<u8> {
    "甲乙丙丁戊己庚辛壬癸子丑寅卯辰巳午未申酉戌亥"
        .chars()
        .position(|g| g == c)
        .map(|i| (i + 1) as u8)
}

fn normalize_kepan_title(s: &str) -> String {
    let stripped = strip_inline_parens(s).unwrap_or_else(|| s.to_string());
    stripped
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '，' | '。' | '！' | '？' | '；' | '：' | '、' | ',' | '.' | '!' | '?' | ';' | ':'
                        | '“' | '”' | '‘' | '’' | '「' | '」' | '『' | '』' | '—' | '-'
                )
        })
        .collect()
}

fn kepan_titles_compatible(a: &str, b: &str) -> bool {
    let na = normalize_kepan_title(a);
    let nb = normalize_kepan_title(b);
    !na.is_empty() && !nb.is_empty() && (na == nb || na.contains(&nb) || nb.contains(&na))
}

struct ParsedSubItem {
    text: String,
    child_num: u32,
    child_title: String,
    target_path: Option<String>,
    target_doc_title: Option<String>,
    target_lb: Option<String>,
    target_label: Option<String>,
}

struct ParsedKepanNode {
    path: String,
    doc_title: String,
    lb: String,
    is_split: bool,
    level: u8,
    num: u32,
    label: String,
    title: String,
    text: String,
    items: Vec<ParsedSubItem>,
    parent: Option<KepanRef>,
}

fn parse_kepan_node(path: &str, doc_title: &str, k: &KepanEntry) -> ParsedKepanNode {
    let t = k.text.trim();
    let mut level = 0u8;
    let mut num = 0u32;
    let mut label = String::new();
    let mut title = String::new();

    if let Some(first) = t.chars().next() {
        if let Some(lv) = ganzhi_level(first) {
            let rest = &t[first.len_utf8()..];
            let num_bytes: usize = rest.chars().take_while(|&c| is_kepan_num(c)).map(char::len_utf8).sum();
            if num_bytes > 0 {
                let num_str = &rest[..num_bytes];
                level = lv;
                num = parse_series_num(num_str).unwrap_or(0);
                label = format!("{}{}", first, num_str);
                let after = &rest[num_bytes..];
                if k.is_split {
                    if let Some(inner) = after.strip_prefix('（').or_else(|| after.strip_prefix('(')) {
                        if let Some(end) = inner.find(['）', ')']) {
                            title = inner[..end].trim().to_string();
                        }
                    }
                    if title.is_empty() {
                        let before = find_fen_colon(after).map(|(b, _)| b).unwrap_or(after);
                        title = before.trim_start_matches(|c: char| c == '、' || c.is_whitespace()).trim().to_string();
                    }
                } else {
                    title = after
                        .trim_start_matches(|c: char| c == '、' || c.is_whitespace())
                        .trim_end_matches(|c: char| matches!(c, '：' | ':') || c.is_whitespace())
                        .to_string();
                }
            }
        }
    }
    if label.is_empty() && k.is_split {
        if let Some((before, _)) = find_fen_colon(t) {
            label = before.trim().to_string();
            title = label.clone();
        }
    }

    let mut items = Vec::new();
    if k.is_split {
        if let Some((before, after_colon)) = find_fen_colon(t) {
            let fen_num_str: String = t[before.len() + '分'.len_utf8()..]
                .chars()
                .take_while(|&c| is_kepan_num(c))
                .collect();
            let expected = parse_series_num(&fen_num_str).unwrap_or(0) as usize;
            let body = after_colon.trim().trim_end_matches(|c: char| matches!(c, '。' | '；' | ';') || c.is_whitespace());
            let mut raw_parts: Vec<&str> = body
                .split(['；', ';'])
                .map(|s| s.trim().trim_end_matches(['。', '；', ';']))
                .filter(|s| !s.is_empty())
                .collect();
            if raw_parts.len() == 1 && expected >= 2 && strip_ganzhi_num(raw_parts[0]).is_none() {
                let by_dun: Vec<&str> = body.split('、').map(str::trim).filter(|s| !s.is_empty()).collect();
                if by_dun.len() == expected {
                    raw_parts = by_dun;
                }
            }
            for (idx, part) in raw_parts.into_iter().enumerate() {
                let (child_num, child_title) = if let Some(first) = part.chars().next() {
                    if ganzhi_level(first).is_some() {
                        let rest = &part[first.len_utf8()..];
                        let nb: usize = rest.chars().take_while(|&c| is_kepan_num(c)).map(char::len_utf8).sum();
                        if nb > 0 {
                            let n = parse_series_num(&rest[..nb]).unwrap_or((idx + 1) as u32);
                            let ct = rest[nb..].trim_start_matches(|c: char| c == '、' || c.is_whitespace()).trim().to_string();
                            (n, ct)
                        } else {
                            ((idx + 1) as u32, part.to_string())
                        }
                    } else {
                        let nb: usize = part.chars().take_while(|&c| is_kepan_num(c)).map(char::len_utf8).sum();
                        if nb > 0 && part[nb..].starts_with('、') {
                            let n = parse_series_num(&part[..nb]).unwrap_or((idx + 1) as u32);
                            let ct = part[nb + '、'.len_utf8()..].trim().to_string();
                            (n, ct)
                        } else {
                            ((idx + 1) as u32, part.to_string())
                        }
                    }
                } else {
                    ((idx + 1) as u32, String::new())
                };
                items.push(ParsedSubItem {
                    text: part.to_string(),
                    child_num,
                    child_title,
                    target_path: None,
                    target_doc_title: None,
                    target_lb: None,
                    target_label: None,
                });
            }
        }
    }

    ParsedKepanNode {
        path: path.to_string(),
        doc_title: doc_title.to_string(),
        lb: k.lb.clone(),
        is_split: k.is_split,
        level,
        num,
        label,
        title,
        text: k.text.clone(),
        items,
        parent: None,
    }
}

/// Build bidirectional parent/child kepan links across one or more ordered documents,
/// returning the linked kepan entries belonging to `cur_path`.
/// Repeated identical `kepan-split` recaps at the start of later lessons share the same
/// logical stack frame so children in later lessons link back to earlier splits too.
pub fn build_kepan_links(docs: &[(&str, &str, &[KepanEntry])], cur_path: &str) -> Vec<LinkedKepan> {
    let mut nodes: Vec<ParsedKepanNode> = Vec::new();
    for &(path, doc_title, kepans) in docs {
        for k in kepans {
            nodes.push(parse_kepan_node(path, doc_title, k));
        }
    }

    struct Frame {
        level: u8,
        num: u32,
        title: String,
        owners: Vec<usize>,
    }

    let mut stack: Vec<Frame> = Vec::new();
    for i in 0..nodes.len() {
        let cur_level = nodes[i].level;
        let cur_num = nodes[i].num;
        let cur_title = nodes[i].title.clone();
        let cur_is_split = nodes[i].is_split;

        while let Some(top) = stack.last() {
            if top.level > cur_level {
                stack.pop();
            } else if top.level == cur_level {
                let is_recap = cur_is_split
                    && top.num == cur_num
                    && (kepan_titles_compatible(&top.title, &cur_title)
                        || (normalize_kepan_title(&top.title).is_empty() && normalize_kepan_title(&cur_title).is_empty()));
                if is_recap {
                    break;
                }
                stack.pop();
            } else {
                break;
            }
        }

        // If `cur` is a recap of the same `kepan-split` on top of the stack, join that frame
        // and inherit already-resolved child links & parent from its primary owner.
        if cur_is_split
            && stack
                .last()
                .is_some_and(|top| top.level == cur_level && top.num == cur_num)
        {
            let first_owner = stack.last().unwrap().owners[0];
            if nodes[i].parent.is_none() {
                if let Some(ref p) = nodes[first_owner].parent {
                    let mut inherited = p.clone();
                    if inherited.path.is_none() && nodes[first_owner].path != nodes[i].path {
                        inherited.path = Some(nodes[first_owner].path.clone());
                        inherited.doc_title = Some(nodes[first_owner].doc_title.clone());
                    }
                    nodes[i].parent = Some(inherited);
                }
            }
            let copied: Vec<(Option<String>, Option<String>, Option<String>, Option<String>)> = nodes[first_owner]
                .items
                .iter()
                .map(|it| {
                    let tp = it.target_path.clone().or_else(|| {
                        (it.target_lb.is_some() && nodes[first_owner].path != nodes[i].path)
                            .then(|| nodes[first_owner].path.clone())
                    });
                    let td = it.target_doc_title.clone().or_else(|| {
                        (it.target_lb.is_some() && nodes[first_owner].path != nodes[i].path)
                            .then(|| nodes[first_owner].doc_title.clone())
                    });
                    (tp, td, it.target_lb.clone(), it.target_label.clone())
                })
                .collect();
            let cur_node_path = nodes[i].path.clone();
            for (idx, (tp, td, tlb, tlab)) in copied.into_iter().enumerate() {
                if let Some(dst) = nodes[i].items.get_mut(idx) {
                    if dst.target_lb.is_none() && tlb.is_some() {
                        dst.target_path = tp.filter(|p| p != &cur_node_path);
                        dst.target_doc_title = if dst.target_path.is_some() { td } else { None };
                        dst.target_lb = tlb;
                        dst.target_label = tlab;
                    }
                }
            }
            stack.last_mut().unwrap().owners.push(i);
            continue;
        }

        if let Some(frame) = stack.last() {
            let pi = *frame.owners.last().unwrap();
            if nodes[pi].is_split && !nodes[pi].items.is_empty() && cur_level > 0 && cur_level == frame.level + 1 {
                let same_doc = nodes[pi].path == nodes[i].path;
                let cur_path_str = nodes[i].path.clone();
                let cur_doc_title = nodes[i].doc_title.clone();
                let cur_lb = nodes[i].lb.clone();
                let cur_label = nodes[i].label.clone();

                let matched_idx = nodes[pi]
                    .items
                    .iter()
                    .position(|it| it.target_lb.is_none() && it.child_num == cur_num && kepan_titles_compatible(&it.child_title, &cur_title))
                    .or_else(|| {
                        same_doc.then(|| {
                            nodes[pi].items.iter().position(|it| it.target_lb.is_none() && it.child_num == cur_num)
                        }).flatten()
                    })
                    .or_else(|| {
                        nodes[pi].items.iter().position(|it| it.target_lb.is_none() && kepan_titles_compatible(&it.child_title, &cur_title))
                    })
                    .or_else(|| {
                        (!same_doc).then(|| {
                            nodes[pi].items.iter().position(|it| {
                                it.target_lb.is_none() && normalize_kepan_title(&it.child_title).is_empty() && it.child_num == cur_num
                            })
                        }).flatten()
                    });

                if let Some(mi) = matched_idx {
                    let parent_ref = KepanRef {
                        path: (!same_doc).then(|| nodes[pi].path.clone()),
                        doc_title: (!same_doc).then(|| nodes[pi].doc_title.clone()),
                        lb: nodes[pi].lb.clone(),
                        label: nodes[pi].label.clone(),
                        title: nodes[pi].title.clone(),
                    };
                    let owners: Vec<usize> = frame.owners.clone();
                    for owner_idx in owners {
                        let owner_same_doc = nodes[owner_idx].path == cur_path_str;
                        if let Some(item) = nodes[owner_idx].items.get_mut(mi) {
                            if item.target_lb.is_none() {
                                item.target_path = (!owner_same_doc).then(|| cur_path_str.clone());
                                item.target_doc_title = (!owner_same_doc).then(|| cur_doc_title.clone());
                                item.target_lb = Some(cur_lb.clone());
                                item.target_label = Some(cur_label.clone());
                            }
                        }
                    }
                    nodes[i].parent = Some(parent_ref);
                }
            }
        }
        if cur_is_split {
            stack.push(Frame {
                level: cur_level,
                num: cur_num,
                title: cur_title,
                owners: vec![i],
            });
        }
    }

    nodes
        .into_iter()
        .filter(|n| n.path == cur_path)
        .map(|n| LinkedKepan {
            lb: n.lb,
            kind: if n.is_split { "split" } else { "head" },
            level: n.level,
            label: n.label,
            title: n.title,
            text: n.text,
            parent: n.parent,
            children: n
                .items
                .into_iter()
                .map(|it| KepanChildLink {
                    text: it.text,
                    path: it.target_path,
                    doc_title: it.target_doc_title,
                    lb: it.target_lb,
                    label: it.target_label,
                })
                .collect(),
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BookRefOccurrence {
    pub title: String,
    pub count: usize,
    pub lbs: Vec<String>,
}

/// Extract unique `《…》` book citations in order of appearance from `(lb, text)`
/// pairs, matching the same rule as the frontend reader (`1..=60` chars, no `《》\n。！？；：`).
pub fn extract_book_refs<'a>(lines: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<BookRefOccurrence> {
    let mut order: Vec<String> = Vec::new();
    let mut by_title: std::collections::HashMap<String, (usize, Vec<String>)> = std::collections::HashMap::new();
    for (lb, text) in lines {
        let mut rest = text;
        while let Some(open) = rest.find('《') {
            let after = &rest[open + '《'.len_utf8()..];
            let Some(close) = after.find('》') else { break };
            let inner = &after[..close];
            let valid = !inner.is_empty()
                && inner.chars().count() <= 60
                && !inner.chars().any(|c| matches!(c, '《' | '》' | '\n' | '\r' | '。' | '！' | '？' | '；' | '：'));
            if valid {
                let entry = by_title.entry(inner.to_string()).or_insert_with(|| {
                    order.push(inner.to_string());
                    (0, Vec::new())
                });
                entry.0 += 1;
                if !lb.is_empty() && entry.1.last().map(String::as_str) != Some(lb) && entry.1.len() < 20 {
                    entry.1.push(lb.to_string());
                }
                rest = &after[close + '》'.len_utf8()..];
            } else {
                rest = after;
            }
        }
    }
    order
        .into_iter()
        .filter_map(|title| {
            let (count, lbs) = by_title.remove(&title)?;
            Some(BookRefOccurrence { title, count, lbs })
        })
        .collect()
}

struct Walker {
    pieces: Vec<(String, Option<usize>)>,
    lbs: Vec<String>,
    cur_lb: Option<usize>,
    html: Option<HtmlOut>,
    dir: String,
    headings: Vec<Heading>,
    kepans: Vec<KepanEntry>,
    verses: Vec<VerseEntry>,
    rules: Rules,
    anchors: std::collections::HashMap<String, String>,
    pending_anchors: Vec<String>,
    plain_len: usize,
    search_exclusions: Vec<(usize, usize)>,
    block_kinds: std::collections::HashMap<*const Element, BlockKind>,
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
        self.plain_len += s.chars().filter(|&c| !crate::text::is_dropped(c)).count();
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

    fn block(&mut self, el: &Element, tag: &str, class: &str) -> String {
        self.open(tag, class);
        let lb = self.start_block();
        self.walk_children(el);
        self.close(tag);
        // text after the block (outside any block) starts a new paragraph
        self.cur_lb = None;
        lb
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
                let detected = self.block_kinds.get(&(el as *const Element)).copied();
                let verse_class = match detected {
                    Some(BlockKind::VerseQuote) => Some("p verse verse-quote"),
                    Some(BlockKind::Verse) => Some("p verse"),
                    _ if class_attr.contains("l-js") || class_attr.contains("颂") => Some("p verse"),
                    _ => None,
                };
                let kepan = match detected {
                    Some(BlockKind::KepanSplit) => Some("p kepan kepan-split"),
                    Some(BlockKind::KepanHead) => Some("p kepan kepan-head"),
                    _ if class_attr.contains("科判") => Some("p kepan kepan-head"),
                    _ => None,
                };
                let (tag, class) = match n {
                    "li" => ("li", "item"),
                    "td" | "th" => ("td", ""),
                    "pre" => ("p", "p t-pre"),
                    _ if kepan.is_some() => ("p", kepan.unwrap()),
                    _ if verse_class.is_some() => ("p", verse_class.unwrap()),
                    "blockquote" => ("p", "p quote"),
                    _ => ("p", "p"),
                };
                let lb = self.block(el, tag, class);
                if let Some(kc) = kepan {
                    let mut text = String::new();
                    visible_text(el, &mut text);
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        self.kepans.push(KepanEntry {
                            lb,
                            is_split: kc.contains("kepan-split"),
                            text,
                        });
                    }
                } else if verse_class.is_some() {
                    let mut text = String::new();
                    visible_text(el, &mut text);
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        self.verses.push(VerseEntry { lb, text });
                    }
                }
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
            "a" if self.rules.resolved_links && el.attr("href").is_some_and(|href| crate::zim_links::is_internal_href(href) || crate::zim_links::external_url(href).is_some()) => {
                // `#/File:…` is a ZIM path to a file (the link around an
                // image), not an anchor in this article: keep the image, but
                // no link — clicking it would look for a chapter by that name
                if el.attr("href").is_some_and(|href| href.trim_start().starts_with("#/")) {
                    self.walk_children(el);
                    return;
                }
                if let Some(h) = self.html.as_mut() {
                    let href = el.attr("href").unwrap();
                    let kind = if crate::zim_links::external_url(href).is_some() { "external" } else { "internal" };
                    h.open("a", &[("class", "zim-link"), ("href", "#"), ("data-zim-href", href), ("data-link-kind", kind)]);
                }
                self.walk_children(el);
                self.close("a");
            }
            "a" if el.attr("href").is_some_and(|href| crate::zim_links::external_url(href).is_some()) => {
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

    #[test]
    fn detects_kepan_and_verse_in_markdown_and_blockquotes() {
        let md = "\
此论分三：甲一、题目及造论分支——初善首义；甲二、着重赞颂二种福德与劝勉守戒——中善论义；甲三、祝愿祈请造论者暂时与究竟一切所愿称心如意——尾善末义。

甲一（初善首义）分二：乙一、宣说题目；乙二、造论分支。

> 乙一、宣说题目：

> 如来密意持明表示传，补特伽罗人中成善缘，
> 循胜士迹究竟二利者，三大传承上师前敬礼。

丙二、思维利害颠倒、愚昧无知之众生而断除傲慢：

具相师前已受利乐源，如同珍宝璎珞美饰戒，

然换一碗豆粉许骤乐，如此严重愚昧诚可笑。

这是普通的正文讲解段落，前面讲了甲一的内容，现在继续说明持戒、忍辱、精进、禅定的道理。

> 这是一段普通的引用文字，不应被识别为颂词或科判。

人之初，性本善。性相近，习相远。

尤浊时衰雨云滚滚趋蔽佛教日尊容，

不共成就士夫殊胜密宗自宗临匿迹，

极乐世界怙主智悲本智舍（ཧྲཱི༔）字相，

明点悉地海岛莲苞中出相好身，";
        let html = crate::markdown::to_html(md);
        let a = read_article(&html, "notes/test.md", Some(&RenderOptions::default()));
        let out = a.html.unwrap();
        assert!(out.contains(r#"<p class="p kepan kepan-split"><span class="lb" data-lb="¶1"></span>此论分三："#), "{out}");
        assert!(out.contains(r#"<p class="p kepan kepan-split"><span class="lb" data-lb="¶2"></span>甲一（初善首义）分二："#), "{out}");
        assert!(out.contains(r#"<p class="p kepan kepan-head"><span class="lb" data-lb="¶3"></span>乙一、宣说题目："#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶4"></span>如来密意持明表示传，"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶5"></span>循胜士迹究竟二利者，"#), "{out}");
        assert!(out.contains(r#"<p class="p kepan kepan-head"><span class="lb" data-lb="¶6"></span>丙二、思维利害颠倒"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶7"></span>具相师前已受利乐源，"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶8"></span>然换一碗豆粉许骤乐，"#), "{out}");
        assert!(out.contains(r#"<p class="p"><span class="lb" data-lb="¶9"></span>这是普通的正文讲解段落"#), "{out}");
        assert!(out.contains(r#"<p class="p quote"><span class="lb" data-lb="¶10"></span>这是一段普通的引用文字"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶11"></span>人之初，性本善。"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶12"></span>尤浊时衰雨云滚滚趋蔽佛教日尊容，"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶13"></span>不共成就士夫殊胜密宗自宗临匿迹，"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶14"></span>极乐世界怙主智悲本智舍（ཧྲཱི༔）字相，"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶15"></span>明点悉地海岛莲苞中出相好身，"#), "{out}");
    }

    #[test]
    fn explicit_kepan_and_verse_classes_work_and_zim_skips_heuristics() {
        let a = read_article(ZB, "a/ZB/ZBkc006/ZBkc006_006.html", Some(&RenderOptions::default()));
        let out = a.html.unwrap();
        assert!(out.contains(r#"class="p kepan kepan-head""#), "{out}");
        assert!(out.contains(r#"class="p verse""#), "{out}");

        let zim = read_zim_html(
            "<body><p>乙一、宣说题目：</p><p>人之初，性本善。性相近，习相远。</p></body>",
            "A/条目",
            Some(&RenderOptions::default()),
        );
        let zout = zim.html.unwrap();
        assert!(!zout.contains("kepan") && !zout.contains("verse"), "{zout}");
    }

    #[test]
    fn separates_space_and_punct_styles_so_prose_between_verses_is_not_merged() {
        let md = "\
无有障碍　　如焰如化　　是故我今

稽首佛月

尔时世尊。以偈答曰。

此金光明　　诸经之王　　甚深最胜

金刚藏菩萨，　　闻说是事已，

欲令大众悦，　　即时说颂言：

“诸菩萨所行，　　第一难思议，";
        let html = crate::markdown::to_html(md);
        let a = read_article(&html, "notes/test.md", Some(&RenderOptions::default()));
        let out = a.html.unwrap();
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶1"></span>无有障碍"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶2"></span>稽首佛月"#), "{out}");
        assert!(out.contains(r#"<p class="p"><span class="lb" data-lb="¶3"></span>尔时世尊。以偈答曰。"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶4"></span>此金光明"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶5"></span>金刚藏菩萨，"#), "{out}");
        assert!(out.contains(r#"<p class="p verse"><span class="lb" data-lb="¶6"></span>欲令大众悦，"#), "{out}");
        assert!(out.contains(r#"<p class="p verse verse-quote"><span class="lb" data-lb="¶7"></span>“诸菩萨所行，"#), "{out}");
    }

    #[test]
    fn selects_kepan_siblings_by_series_or_folder_and_ignores_kepan_and_json() {
        let sibs = vec![
            ("dir/第10课.md".into(), "第10课".into()),
            ("dir/科判.md.json".into(), "科判".into()),
            ("dir/总科判.md".into(), "总科判".into()),
            ("dir/第2课.md".into(), "第2课".into()),
            ("dir/第1课.md".into(), "第1课".into()),
            ("dir/附记.md".into(), "附记".into()),
        ];
        let picked = select_kepan_siblings(&sibs, "dir/第2课.md", "第2课");
        let titles: Vec<&str> = picked.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(titles, vec!["第1课", "第2课", "第10课"]);

        // Non-series folder (e.g. translations): keep folder order, still ignore 科判 / .json
        let trans = vec![
            ("dir/颂词译文.md".into(), "颂词译文".into()),
            ("dir/科判.map.json".into(), "科判".into()),
            ("dir/注释上.md".into(), "注释上".into()),
            ("dir/注释下.md".into(), "注释下".into()),
        ];
        let picked2 = select_kepan_siblings(&trans, "dir/注释上.md", "注释上");
        let titles2: Vec<&str> = picked2.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(titles2, vec!["颂词译文", "注释上", "注释下"]);
    }

    #[test]
    fn builds_in_doc_and_cross_doc_kepan_links_and_extracts_book_refs() {
        let md1 = "壬二（宣说彼等显现依于心）分二：真实宣说；运用其比喻。\n\n癸一、真实宣说：\n\n心行以心造诸业，现于心中心假立，\n\n为调迷心当精勤。\n\n如《入行论》与《赞戒论》所说，再次引用《入行论》。";
        let md2 = "癸二、运用其比喻：\n\n如为莨菪碱迷醉，呈现种种不同相，";
        let a1 = read_article(&crate::markdown::to_html(md1), "d/第1课.md", Some(&RenderOptions::default()));
        let a2 = read_article(&crate::markdown::to_html(md2), "d/第2课.md", Some(&RenderOptions::default()));
        assert_eq!(a1.verses.len(), 2);

        let docs = vec![
            ("d/第1课.md", "第1课", a1.kepans.as_slice()),
            ("d/第2课.md", "第2课", a2.kepans.as_slice()),
        ];
        let links1 = build_kepan_links(&docs, "d/第1课.md");
        assert_eq!(links1.len(), 2);
        assert_eq!(links1[0].children.len(), 2);
        assert_eq!(links1[0].children[0].path, None); // same doc
        assert_eq!(links1[0].children[0].lb.as_deref(), Some("¶2"));
        assert_eq!(links1[0].children[1].path.as_deref(), Some("d/第2课.md")); // cross doc
        assert_eq!(links1[0].children[1].lb.as_deref(), Some("¶1"));

        let links2 = build_kepan_links(&docs, "d/第2课.md");
        assert_eq!(links2.len(), 1);
        let p = links2[0].parent.as_ref().unwrap();
        assert_eq!(p.path.as_deref(), Some("d/第1课.md"));
        assert_eq!(p.label, "壬二");

        let refs = extract_book_refs([
            ("¶1", "壬二（宣说彼等显现依于心）分二：真实宣说；运用其比喻。"),
            ("¶5", "如《入行论》与《赞戒论》所说，再次引用《入行论》。"),
        ]);
        assert_eq!(refs.len(), 2);
        assert_eq!((refs[0].title.as_str(), refs[0].count), ("入行论", 2));
        assert_eq!((refs[1].title.as_str(), refs[1].count), ("赞戒论", 1));
    }

    #[test]
    fn repeated_kepan_split_recap_in_later_lesson_preserves_cross_doc_links() {
        let md2 = "乙一（广说善恶取舍之分类）分三：丙一、以取舍各自分类而真实赞戒；丙二、别分持戒之方法；丙三、与之相关语结文。\n\n丙一、以取舍各自分类而真实赞戒：";
        let md14 = "乙一（广说善恶取舍之分类）分三：丙一、以取舍各自分类而真实赞戒；丙二、别分持戒之方法；丙三、与之相关语结文。\n\n丙三（与之相关语结文）分四：丁一、无有修持；丁二、教诫。";
        let a2 = read_article(&crate::markdown::to_html(md2), "d/第2课.md", Some(&RenderOptions::default()));
        let a14 = read_article(&crate::markdown::to_html(md14), "d/第14课.md", Some(&RenderOptions::default()));
        let docs = vec![
            ("d/第2课.md", "第2课", a2.kepans.as_slice()),
            ("d/第14课.md", "第14课", a14.kepans.as_slice()),
        ];
        let links2 = build_kepan_links(&docs, "d/第2课.md");
        // 第2课's 乙一 -> 丙三 must link across the recap to 第14课's 丙三!
        assert_eq!(links2[0].children[2].path.as_deref(), Some("d/第14课.md"));
        assert_eq!(links2[0].children[2].lb.as_deref(), Some("¶2"));

        let links14 = build_kepan_links(&docs, "d/第14课.md");
        // 第14课's recap 乙一 -> 丙一 inherits the link back to 第2课's 丙一!
        assert_eq!(links14[0].children[0].path.as_deref(), Some("d/第2课.md"));
        assert_eq!(links14[0].children[0].lb.as_deref(), Some("¶2"));
    }
}
