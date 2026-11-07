//! M0b: the reading text of one volume XML (CBETA TEI P5, one juan per file).
//!
//! Port of `tools/ref_extract.py`; the rules are in design-spec appendix A.6:
//!
//! * body text = reading text only: `<lem>` yes, `<rdg>` and non-inline `<note>` no
//! * `<note place="inline">` (夹注) is kept, wrapped in full-width parentheses
//! * `<g ref="#CBxxxxx"/>` (missing glyph) becomes exactly **one char**, so that
//!   positions in the line stay right and the index sees no `[木*翕]` strings:
//!   single-char normalized form > `mapping type="unicode"` > `normal_unicode` >
//!   PUA code point; composition or charName only if none of these exists,
//!   `@Declare@CBxxxxx` when not declared. ("Character in the Siddham font" is
//!   a font code such as `一`, never text.)
//! * `<space quantity="n"/>` -> n spaces; `pb`, `milestone`, `unclear` are skipped
//! * `<cb:tt>` (with or without `type="app"`): only the `zh-Hant` `<cb:t>`, else the first
//! * `<cb:mulu>` is an editorial outline label, not printed text: skipped
//! * line breaks and tabs of the source file are layout and are dropped
//! * `<lb ed="T" n="0159b05"/>` marks the original line; only `lb` whose `ed`
//!   equals the canon code (`idno type="canon"`) counts
//!
//! Output:
//! * `text`: punctuated reading text
//! * `plain`: `text` without punctuation and whitespace (what the index sees)
//! * `lines`: where each original line starts, as a char offset into `plain`
//!
//! Offsets are counted in **chars** (Unicode scalar values), not bytes, so they
//! match the golden files and the "position inside the line" of CBETA, which is
//! counted without punctuation.
//!
//! The XML is first read into a small tree, because some rules look at
//! siblings (the first `lem` of an `app`, the `zh-Hant` `t` of a `tt`). A
//! volume file is at most a few MB, so this is cheap.

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, XmlVersion};
use crate::render::{safe_style, HtmlOut, RenderOptions};
use std::collections::HashMap;

const TEI_NS: &str = "http://www.tei-c.org/ns/1.0";
const CB_NS: &str = "http://www.cbeta.org/ns/1.0";

/// Punctuation that CBETA added on top of the original text (the printed
/// books have none). Same set as `PUNCT` in `tools/ref_extract.py`.
/// `【】〖〗︵︶｀＇` were added after checking the `p` anchors of all volumes
/// (positions in the line do not count them); `○ △ □ 〃 ※ ～ 〇`, box-drawing
/// characters, digits and Latin letters *are* counted and stay.
const PUNCT: &str = "，。、；：？！「」『』（）〔〕［］《》〈〉…—·．﹑﹔﹕﹖﹗“”‘’,.;:?!()[]<>【】〖〗︵︶｀＇";

pub fn is_punct(c: char) -> bool {
    PUNCT.contains(c)
}

/// Characters dropped from `plain`: punctuation and whitespace (as Python's
/// `str.isspace`, which also counts U+001C..U+001F).
pub(crate) fn is_dropped(c: char) -> bool {
    is_punct(c) || c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}


/// Start of an original line inside `plain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineStart {
    /// char offset into `plain`
    pub offset: usize,
    /// `lb n`, e.g. `0159b05`; `None` for text before the first `lb`
    pub lb: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct VolumeText {
    /// canon code from `idno type="canon"`, e.g. `T`
    pub canon: String,
    pub text: String,
    pub plain: String,
    pub lines: Vec<LineStart>,
    /// `<p xml:id>` anchors: (id, char offset into `plain` where the paragraph starts)
    pub anchors: Vec<(String, usize)>,
    /// for each char of `plain`, the char index of the same char in `text`
    /// (to show hits with punctuation)
    pub plain_to_text: Vec<u32>,
}

/// Opening marks start the next line; every other char between two lines
/// (closing punctuation, spaces) stays at the end of the previous line.
fn is_opening(c: char) -> bool {
    "「『（〔［《〈“‘([<【〖".contains(c)
}

impl VolumeText {
    /// The punctuated text cut into original lines: (lb, from, to) as char
    /// ranges of `text`. The ranges are contiguous and cover all of `text`.
    pub fn line_spans(&self) -> Vec<(Option<&str>, usize, usize)> {
        let text: Vec<char> = self.text.chars().collect();
        let mut starts: Vec<usize> = Vec::with_capacity(self.lines.len());
        for (i, line) in self.lines.iter().enumerate() {
            if i == 0 {
                starts.push(0);
                continue;
            }
            let first = self.plain_to_text[line.offset] as usize;
            let mut b = self.plain_to_text[line.offset - 1] as usize + 1;
            while b < first && !is_opening(text[b]) {
                b += 1;
            }
            starts.push(b);
        }
        self.lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let to = starts.get(i + 1).copied().unwrap_or(text.len());
                (l.lb.as_deref(), starts[i], to)
            })
            .collect()
    }

    /// The line containing the given char offset of `plain`, and the
    /// 1-based position inside that line (punctuation not counted).
    pub fn locate(&self, offset: usize) -> Option<(&LineStart, usize)> {
        let i = self.lines.partition_point(|l| l.offset <= offset).checked_sub(1)?;
        let line = &self.lines[i];
        Some((line, offset - line.offset + 1))
    }
}

// ---------------------------------------------------------------- tree

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ns {
    Tei,
    Cb,
    Other,
}

#[derive(Debug)]
enum Node {
    Elem(Elem),
    Text(String),
}

#[derive(Debug)]
struct Elem {
    ns: Ns,
    /// local name, e.g. `lb`, `tt`
    name: String,
    /// raw attribute names, e.g. `n`, `xml:id`, `cb:type`
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Elem {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn is(&self, ns: Ns, name: &str) -> bool {
        self.ns == ns && self.name == name
    }

    fn elems(&self) -> impl Iterator<Item = &Elem> {
        self.children.iter().filter_map(|n| match n {
            Node::Elem(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// Text before the first child element (lxml's `.text`).
    fn head_text(&self) -> &str {
        match self.children.first() {
            Some(Node::Text(t)) => t,
            _ => "",
        }
    }

    /// `findtext(child)`: head text of the first direct child with that name.
    fn child_text(&self, ns: Ns, name: &str) -> &str {
        self.elems().find(|e| e.is(ns, name)).map(|e| e.head_text()).unwrap_or("")
    }

    /// First descendant (depth first, document order), self excluded.
    fn find(&self, pred: &dyn Fn(&Elem) -> bool) -> Option<&Elem> {
        for e in self.elems() {
            if pred(e) {
                return Some(e);
            }
            if let Some(f) = e.find(pred) {
                return Some(f);
            }
        }
        None
    }

    fn for_each(&self, f: &mut dyn FnMut(&Elem)) {
        for e in self.elems() {
            f(e);
            e.for_each(f);
        }
    }
}

fn ns_of(r: &ResolveResult) -> Ns {
    match r {
        ResolveResult::Bound(ns) if ns.as_ref() == TEI_NS => Ns::Tei,
        ResolveResult::Bound(ns) if ns.as_ref() == CB_NS => Ns::Cb,
        _ => Ns::Other,
    }
}

fn new_elem(ns: Ns, e: &BytesStart) -> Result<Elem, String> {
    let name = e.local_name().as_ref().to_string();
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|err| format!("attribute of <{}>: {}", name, err))?;
        let key = a.key.as_ref().to_string();
        let value = a
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|err| format!("attribute {} of <{}>: {}", key, name, err))?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Elem {
        ns,
        name,
        attrs,
        children: Vec::new(),
    })
}

fn push_text(parent: &mut Elem, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Node::Text(t)) = parent.children.last_mut() {
        t.push_str(s);
    } else {
        parent.children.push(Node::Text(s.to_string()));
    }
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

/// Parse the whole document into a tree; returns the root element.
fn parse_tree(xml: &str) -> Result<Elem, String> {
    let mut reader = NsReader::from_str(xml);
    let mut stack: Vec<Elem> = vec![Elem {
        ns: Ns::Other,
        name: String::new(),
        attrs: Vec::new(),
        children: Vec::new(),
    }];
    loop {
        let pos = reader.buffer_position();
        let (res, ev) = reader
            .read_resolved_event()
            .map_err(|e| format!("XML error at byte {}: {}", pos, e))?;
        match ev {
            Event::Start(e) => {
                let el = new_elem(ns_of(&res), &e)?;
                stack.push(el);
            }
            Event::Empty(e) => {
                let el = new_elem(ns_of(&res), &e)?;
                stack.last_mut().unwrap().children.push(Node::Elem(el));
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err(format!("unbalanced end tag at byte {}", pos));
                }
                let el = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(Node::Elem(el));
            }
            Event::Text(t) => push_text(stack.last_mut().unwrap(), &t.xml10_content()),
            Event::CData(t) => {
                let s = t.as_ref().to_string();
                push_text(stack.last_mut().unwrap(), &s);
            }
            Event::GeneralRef(r) => {
                let ch = if r.is_char_ref() {
                    r.resolve_char_ref().map_err(|e| format!("bad char ref at byte {}: {}", pos, e))?
                } else {
                    named_entity(&r.xml10_content())
                };
                match ch {
                    Some(c) => push_text(stack.last_mut().unwrap(), c.encode_utf8(&mut [0; 4])),
                    None => return Err(format!("unknown entity &{}; at byte {}", r.xml10_content(), pos)),
                }
            }
            Event::Eof => break,
            _ => {} // declaration, PI, comment, doctype
        }
    }
    if stack.len() != 1 {
        return Err("unclosed elements at end of document".to_string());
    }
    let doc = stack.pop().unwrap();
    doc.children
        .into_iter()
        .find_map(|n| match n {
            Node::Elem(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| "no root element".to_string())
}

// ---------------------------------------------------------------- extraction

/// `U+3BD3` -> '㯓'
fn parse_code_point(s: &str) -> Option<char> {
    let s = s.trim();
    let hex = s.strip_prefix("U+").or_else(|| s.strip_prefix("u+"))?;
    char::from_u32(u32::from_str_radix(hex, 16).ok()?)
}

fn is_private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0x10FFFF)
}

/// A missing glyph (`charDecl/char`).
#[derive(Debug, Clone)]
pub(crate) struct Glyph {
    /// what the text and the index see: one char when possible
    text: String,
    /// shown instead of `text` when that is a private-use char that no
    /// normal font has, e.g. the composition `[木*翕]`
    display: Option<String>,
    /// composition and name, for a tooltip
    title: String,
}

/// Missing-glyph declarations: `CB01979` -> glyph (one char when possible).
fn load_char_decl(root: &Elem) -> HashMap<String, Glyph> {
    let mut decl = HashMap::new();
    root.for_each(&mut |ch| {
        if !ch.is(Ns::Tei, "char") {
            return;
        }
        let id = match ch.attr("xml:id") {
            Some(id) => id.to_string(),
            None => return,
        };
        let mut props: HashMap<String, String> = HashMap::new();
        for p in ch.elems().filter(|e| e.is(Ns::Tei, "charProp")) {
            let name = p.child_text(Ns::Tei, "localName").trim().to_lowercase();
            let value = p.child_text(Ns::Tei, "value").trim().to_string();
            props.insert(name, value);
        }
        let mapping = |kind: &str| {
            ch.elems()
                .find(|e| e.is(Ns::Tei, "mapping") && e.attr("type") == Some(kind))
                .and_then(|e| parse_code_point(e.head_text()))
                .map(|c| c.to_string())
        };
        let char_name = ch.child_text(Ns::Tei, "charName").trim().to_string();
        let normalized = props.get("normalized form").filter(|v| v.chars().count() == 1).cloned();
        let text = normalized
            .or_else(|| mapping("unicode"))
            .or_else(|| mapping("normal_unicode"))
            .or_else(|| mapping("PUA"))
            .or_else(|| props.get("normalized form").filter(|v| !v.is_empty()).cloned())
            .or_else(|| props.get("composition").filter(|v| !v.is_empty()).cloned())
            .unwrap_or_else(|| char_name.clone());
        let composition = props.get("composition").cloned().unwrap_or_default();
        let roman = props
            .get("romanized form in unicode transcription")
            .or_else(|| props.get("romanized form in cbeta transcription"))
            .cloned()
            .unwrap_or_default();
        let display = match text.chars().next() {
            Some(c) if text.chars().count() == 1 && is_private_use(c) => {
                if !composition.is_empty() {
                    Some(composition.clone())
                } else if !roman.is_empty() {
                    Some(roman.clone())
                } else {
                    Some("□".to_string())
                }
            }
            _ => None,
        };
        let mut title = id.clone();
        for extra in [&composition, &roman] {
            if !extra.is_empty() {
                title.push(' ');
                title.push_str(extra);
            }
        }
        decl.insert(id, Glyph { text, display, title });
    });
    decl
}

struct Extractor<'a> {
    canon: &'a str,
    decl: HashMap<String, Glyph>,
    /// (text, index into `lbs` of the current line)
    pieces: Vec<(String, Option<usize>)>,
    lbs: Vec<String>,
    cur_lb: Option<usize>,
    /// (xml:id of a `p`, index into `pieces` where it starts)
    anchors: Vec<(String, usize)>,
    skip_mulu: bool,
    /// structured HTML, written by the same walk (see `render.rs`)
    html: Option<HtmlOut>,
}

/// Block-level HTML for TEI/CBETA elements: (local name, html tag, class).
fn html_tag(el: &Elem) -> (&'static str, String) {
    let attr_class = |prefix: &str, v: Option<&str>| match v {
        Some(v) if !v.is_empty() => format!(" {}-{}", prefix, v.replace(|c: char| !c.is_alphanumeric(), "-")),
        _ => String::new(),
    };
    match (el.ns, el.name.as_str()) {
        (Ns::Cb, "div") => ("div", format!("div{}", attr_class("t", el.attr("type")))),
        (_, "head") => ("div", "head".to_string()),
        (_, "byline") => ("p", format!("byline{}", attr_class("t", el.attr("cb:type")))),
        (Ns::Cb, "juan") => ("p", format!("juan{}", attr_class("f", el.attr("fun")))),
        (Ns::Cb, "jhead") => ("span", "jhead".to_string()),
        (Ns::Cb, "docNumber") => ("p", "docnumber".to_string()),
        (_, "lg") => ("div", "lg".to_string()),
        (_, "l") => ("div", "l".to_string()),
        (_, "list") => ("ul", "list".to_string()),
        (_, "item") => ("li", "item".to_string()),
        (_, "table") => ("table", "table".to_string()),
        (_, "row") => ("tr", String::new()),
        (_, "cell") => ("td", String::new()),
        (_, "trailer") => ("p", "trailer".to_string()),
        (_, "entry") => ("div", "entry".to_string()),
        (_, "form") => ("span", "form".to_string()),
        (Ns::Cb, "def") => ("div", "def".to_string()),
        (Ns::Cb, "dialog") => ("div", "dialog".to_string()),
        (_, "sp") => ("div", format!("sp{}", attr_class("t", el.attr("cb:type")))),
        (_, "hi") | (_, "seg") => ("span", format!("hi{}", attr_class("r", el.attr("rend")))),
        (_, "figure") => ("span", "figure-box".to_string()),
        (_, name) => ("span", format!("x-{}", name)),
    }
}

/// A collation note at the foot of the page (not an inline note).
fn is_foot_note(e: &Elem) -> bool {
    e.name == "note" && e.attr("place") != Some("inline") && e.attr("place") != Some("interlinear")
}

impl<'a> Extractor<'a> {
    fn emit(&mut self, s: &str) {
        if !s.is_empty() {
            self.pieces.push((s.to_string(), self.cur_lb));
            if let Some(h) = self.html.as_mut() {
                h.text(s);
            }
        }
    }

    fn children(&mut self, el: &Elem) {
        let mut skip_to = 0;
        for (i, n) in el.children.iter().enumerate() {
            if i < skip_to {
                continue;
            }
            match n {
                Node::Text(t) => {
                    let s: String = t.chars().filter(|c| !matches!(c, '\r' | '\n' | '\t')).collect();
                    self.emit(&s);
                }
                Node::Elem(e) => {
                    // the notes of one note point: the Taisho note, CBETA's
                    // revision … (same `n`, next to each other) give one marker
                    if self.html.is_some() && is_foot_note(e) && e.attr("n").is_some() {
                        let mut group = vec![e];
                        let mut j = i + 1;
                        while let Some(next) = el.children.get(j) {
                            match next {
                                Node::Text(t) if t.trim().is_empty() => {}
                                Node::Elem(x) if is_foot_note(x) && x.attr("n") == e.attr("n") => {
                                    group.push(x);
                                    skip_to = j + 1;
                                }
                                _ => break,
                            }
                            j += 1;
                        }
                        self.note_group(&group);
                        continue;
                    }
                    self.walk(e)
                }
            }
        }
    }

    /// One marker for the notes of a note point (HTML only): the original
    /// note (`type="orig"`, Taisho) and CBETA's note (`mod`, or `add` for a
    /// note of CBETA's own), both kept so that the reader can choose what to
    /// show. Other types (`rest` …) only when neither is there.
    fn note_group(&mut self, notes: &[&Elem]) {
        let text_of = |this: &Self, t: &str| -> Option<String> {
            notes.iter().find(|e| e.attr("type") == Some(t)).map(|e| {
                let mut s = String::new();
                this.display_text(e, &mut s);
                s.trim().to_string()
            })
        };
        let orig = text_of(self, "orig").filter(|s| !s.is_empty());
        let mut cb = text_of(self, "mod").or_else(|| text_of(self, "add")).filter(|s| !s.is_empty());
        if orig.is_none() && cb.is_none() {
            for e in notes {
                let kind = e.attr("type").unwrap_or("");
                if !kind.is_empty() && !kind.starts_with("cf") {
                    let mut s = String::new();
                    self.display_text(e, &mut s);
                    if !s.trim().is_empty() {
                        cb = Some(s.trim().to_string());
                        break;
                    }
                }
            }
        }
        if orig.is_none() && cb.is_none() {
            return;
        }
        let n = notes[0].attr("n").unwrap_or("");
        if let Some(h) = self.html.as_mut() {
            h.note(n, orig.as_deref(), cb.as_deref());
        }
    }

    /// Display text of an element that is not reading text (notes, variant
    /// readings): glyphs resolved, inline notes in parentheses.
    fn display_text(&self, el: &Elem, out: &mut String) {
        for n in &el.children {
            match n {
                Node::Text(t) => out.extend(t.chars().filter(|c| !matches!(c, '\r' | '\n' | '\t'))),
                Node::Elem(e) => match e.name.as_str() {
                    "g" => {
                        let id = e.attr("ref").unwrap_or("").trim_start_matches('#');
                        match self.decl.get(id) {
                            Some(g) => out.push_str(g.display.as_deref().unwrap_or(&g.text)),
                            None => out.push('□'),
                        }
                    }
                    "note" if e.attr("place") == Some("inline") || e.attr("place") == Some("interlinear") => {
                        out.push('（');
                        self.display_text(e, out);
                        out.push('）');
                    }
                    "note" | "lb" | "pb" | "milestone" => {}
                    "app" => {
                        if let Some(lem) = e.elems().find(|x| x.name == "lem") {
                            self.display_text(lem, out);
                        }
                    }
                    "space" => out.push(' '),
                    _ => self.display_text(e, out),
                },
            }
        }
    }

    fn walk(&mut self, el: &Elem) {
        match el.name.as_str() {
            "teiHeader" => {}
            "lb" => {
                if el.attr("ed") == Some(self.canon) {
                    let n = el.attr("n").unwrap_or("").to_string();
                    if let Some(h) = self.html.as_mut() {
                        if h.in_pre > 0 && !self.lbs.is_empty() {
                            h.raw("<br>");
                        }
                        h.open("span", &[("class", "lb"), ("data-lb", &n)]);
                        h.close("span");
                    }
                    self.lbs.push(n);
                    self.cur_lb = Some(self.lbs.len() - 1);
                }
            }
            "pb" => {
                if el.attr("ed") == Some(self.canon) {
                    if let Some(h) = self.html.as_mut() {
                        h.open("span", &[("class", "pb"), ("data-pb", el.attr("n").unwrap_or(""))]);
                        h.close("span");
                    }
                }
            }
            "milestone" => {}
            "unclear" => {
                if let Some(h) = self.html.as_mut() {
                    h.raw("<span class=\"unclear\" title=\"字跡不清\">▆</span>");
                }
            }
            "caesura" => {
                if let Some(h) = self.html.as_mut() {
                    h.raw("<span class=\"caesura\">　</span>");
                }
            }
            "note" => {
                if el.attr("place") == Some("inline") {
                    if let Some(h) = self.html.as_mut() {
                        h.open("span", &[("class", "inline-note")]);
                    }
                    self.emit("（");
                    self.children(el);
                    self.emit("）");
                    if let Some(h) = self.html.as_mut() {
                        h.close("span");
                    }
                } else if self.html.is_some() {
                    self.note_group(&[el]);
                }
            }
            "app" => {
                let pick = el
                    .elems()
                    .find(|e| e.is(Ns::Tei, "lem"))
                    .or_else(|| el.elems().find(|e| e.is(Ns::Tei, "rdg")));
                let variants = el.elems().filter(|e| e.name == "rdg").count();
                let title = if self.html.is_some() {
                    let mut t = String::new();
                    for r in el.elems().filter(|e| e.name == "lem" || e.name == "rdg") {
                        let mut s = String::new();
                        self.display_text(r, &mut s);
                        if !t.is_empty() {
                            t.push('；');
                        }
                        t.push_str(r.attr("wit").unwrap_or("").trim());
                        t.push_str(if s.trim().is_empty() { "（無）" } else { s.trim() });
                    }
                    t
                } else {
                    String::new()
                };
                if let Some(h) = self.html.as_mut() {
                    // `data-n` links the reading to its note point
                    let class = if variants > 0 { "app v" } else { "app" };
                    h.open("span", &[("class", class), ("data-n", el.attr("n").unwrap_or("")), ("title", &title)]);
                }
                if let Some(lem) = pick {
                    self.children(lem);
                }
                if let Some(h) = self.html.as_mut() {
                    h.close("span");
                }
            }
            "rdg" => {}
            "mulu" if el.ns == Ns::Cb && self.skip_mulu => {}
            "p" => {
                if let Some(id) = el.attr("xml:id") {
                    self.anchors.push((id.to_string(), self.pieces.len()));
                }
                let pre = el.attr("cb:type") == Some("pre");
                if let Some(h) = self.html.as_mut() {
                    let mut class = "p".to_string();
                    if let Some(t) = el.attr("cb:type") {
                        class.push_str(&format!(" t-{}", t));
                    }
                    if let Some(r) = el.attr("rend") {
                        class.push_str(&format!(" r-{}", r));
                    }
                    let style = safe_style(el.attr("style").unwrap_or(""));
                    h.open("p", &[("class", &class), ("id", el.attr("xml:id").unwrap_or("")), ("style", &style)]);
                    if pre {
                        h.in_pre += 1;
                    }
                }
                self.children(el);
                if let Some(h) = self.html.as_mut() {
                    if pre {
                        h.in_pre -= 1;
                    }
                    h.close("p");
                }
            }
            "g" => {
                let id = el.attr("ref").unwrap_or("").trim_start_matches('#');
                let g = match self.decl.get(id) {
                    Some(g) => g.clone(),
                    None => Glyph {
                        text: format!("@Declare@{}", id),
                        display: None,
                        title: String::new(),
                    },
                };
                self.pieces.push((g.text.clone(), self.cur_lb));
                if let Some(h) = self.html.as_mut() {
                    h.glyph(&g.text, g.display.as_deref(), &g.title);
                }
            }
            "space" => {
                let n: usize = el.attr("quantity").and_then(|q| q.trim().parse().ok()).unwrap_or(0);
                self.emit(&" ".repeat(n));
            }
            "tt" if el.ns == Ns::Cb => {
                let ts: Vec<&Elem> = el.elems().filter(|e| e.is(Ns::Cb, "t")).collect();
                let pick = ts
                    .iter()
                    .find(|t| t.attr("xml:lang") == Some("zh-Hant"))
                    .or_else(|| ts.first())
                    .copied();
                if self.html.is_some() {
                    let mut title = String::new();
                    for t in ts.iter().filter(|t| pick.map_or(true, |p| !std::ptr::eq(**t, p))) {
                        let mut s = String::new();
                        self.display_text(t, &mut s);
                        if !s.trim().is_empty() {
                            if !title.is_empty() {
                                title.push('；');
                            }
                            title.push_str(&format!("{}: {}", t.attr("xml:lang").unwrap_or(""), s.trim()));
                        }
                    }
                    self.html.as_mut().unwrap().open("span", &[("class", "tt"), ("title", &title)]);
                }
                if let Some(t) = pick {
                    self.children(t);
                }
                if let Some(h) = self.html.as_mut() {
                    h.close("span");
                }
            }
            "graphic" => {
                if let Some(h) = self.html.as_mut() {
                    h.image(el.attr("url").unwrap_or(""));
                }
            }
            _ => {
                if self.html.is_none() {
                    self.children(el);
                    return;
                }
                let (tag, class) = html_tag(el);
                let style = safe_style(el.attr("style").unwrap_or(""));
                let colspan = if tag == "td" { el.attr("cols").unwrap_or("") } else { "" };
                let rowspan = if tag == "td" { el.attr("rows").unwrap_or("") } else { "" };
                self.html.as_mut().unwrap().open(
                    tag,
                    &[("class", &class), ("style", &style), ("colspan", colspan), ("rowspan", rowspan)],
                );
                self.children(el);
                self.html.as_mut().unwrap().close(tag);
            }
        }
    }
}

/// Extract the reading text of one volume XML file.
pub fn extract_volume(xml: &str) -> Result<VolumeText, String> {
    extract_volume_with(xml, true)
}

/// `skip_mulu = false` keeps the text of `cb:mulu` (old behaviour, only for comparison).
pub fn extract_volume_with(xml: &str, skip_mulu: bool) -> Result<VolumeText, String> {
    run(xml, skip_mulu, None).map(|(vt, _)| vt)
}

/// Extraction plus the structured HTML of `render.rs`, from one walk.
pub(crate) fn extract_with_html(xml: &str, opts: &RenderOptions) -> Result<(VolumeText, HtmlOut), String> {
    let (vt, html) = run(xml, true, Some(HtmlOut::new(opts)))?;
    Ok((vt, html.expect("html requested")))
}

fn run(xml: &str, skip_mulu: bool, html: Option<HtmlOut>) -> Result<(VolumeText, Option<HtmlOut>), String> {
    let root = parse_tree(xml)?;
    let canon = root
        .find(&|e| e.is(Ns::Tei, "idno") && e.attr("type") == Some("canon"))
        .map(|e| e.head_text().trim().to_string())
        .unwrap_or_default();
    let body = root
        .find(&|e| e.is(Ns::Tei, "body"))
        .ok_or_else(|| "no <body> element".to_string())?;

    let mut ex = Extractor {
        canon: &canon,
        decl: load_char_decl(&root),
        pieces: Vec::new(),
        lbs: Vec::new(),
        cur_lb: None,
        anchors: Vec::new(),
        skip_mulu,
        html,
    };
    ex.children(body);

    let vt = assemble(canon.clone(), &ex.pieces, &ex.lbs, &ex.anchors);
    Ok((vt, ex.html))
}

/// Build a `VolumeText` from the pieces of a walk: (text, index of the
/// current line in `lbs`), and anchors (id, index into `pieces`). Shared by
/// the volume XML reader and the HTML article reader (`article.rs`).
pub(crate) fn assemble(
    canon: String,
    pieces: &[(String, Option<usize>)],
    lbs: &[String],
    piece_anchors: &[(String, usize)],
) -> VolumeText {
    let mut text = String::new();
    let mut plain = String::new();
    let mut plain_len = 0usize;
    let mut lines: Vec<LineStart> = Vec::new();
    let mut last: Option<Option<&str>> = None; // None = no line yet
    let mut anchors: Vec<(String, usize)> = Vec::new();
    let mut next_anchor = 0;
    let mut plain_to_text: Vec<u32> = Vec::new();
    let mut text_len = 0u32;
    for (pi, (s, lb)) in pieces.iter().enumerate() {
        while next_anchor < piece_anchors.len() && piece_anchors[next_anchor].1 <= pi {
            anchors.push((piece_anchors[next_anchor].0.clone(), plain_len));
            next_anchor += 1;
        }
        text.push_str(s);
        let lb_str = lb.map(|i| lbs[i].as_str());
        for c in s.chars() {
            text_len += 1;
            if is_dropped(c) {
                continue;
            }
            if last != Some(lb_str) {
                lines.push(LineStart {
                    offset: plain_len,
                    lb: lb_str.map(|s| s.to_string()),
                });
                last = Some(lb_str);
            }
            plain.push(c);
            plain_to_text.push(text_len - 1);
            plain_len += 1;
        }
    }
    for (id, _) in &piece_anchors[next_anchor..] {
        anchors.push((id.clone(), plain_len));
    }
    VolumeText {
        canon,
        text,
        plain,
        lines,
        anchors,
        plain_to_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0004: &str = include_str!("../../samples/cbeta/T01n0004_001.xml");
    const T0004_TEXT: &str = include_str!("../../samples/cbeta/T01n0004_001.text.txt");
    const T0004_PLAIN: &str = include_str!("../../samples/cbeta/T01n0004_001.plain.txt");
    const T0004_LINES: &str = include_str!("../../samples/cbeta/T01n0004_001.linemap.json");

    /// The golden linemap is `[{"offset": 0, "lb": "0159a23"}, ...]`; read it
    /// without a JSON dependency.
    fn golden_lines(json: &str) -> Vec<LineStart> {
        let mut out = Vec::new();
        let mut offset = None;
        for line in json.lines() {
            let line = line.trim().trim_end_matches(',');
            if let Some(v) = line.strip_prefix("\"offset\":") {
                offset = Some(v.trim().parse::<usize>().unwrap());
            } else if let Some(v) = line.strip_prefix("\"lb\":") {
                let v = v.trim();
                let lb = if v == "null" { None } else { Some(v.trim_matches('"').to_string()) };
                out.push(LineStart {
                    offset: offset.take().unwrap(),
                    lb,
                });
            }
        }
        out
    }

    fn wrap(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<TEI xmlns="http://www.tei-c.org/ns/1.0" xmlns:cb="http://www.cbeta.org/ns/1.0">
<teiHeader><fileDesc><publicationStmt><idno type="CBETA"><idno type="canon">T</idno></idno></publicationStmt></fileDesc>
<encodingDesc><charDecl>
<char xml:id="CB00001"><charName>CBETA CHARACTER CB00001</charName>
<charProp><localName>composition</localName><value>[口*尼]</value></charProp>
<charProp><localName>normalized form</localName><value>呢</value></charProp></char>
<char xml:id="CB00002"><charName>CBETA CHARACTER CB00002</charName>
<charProp><localName>composition</localName><value>[王*巳]</value></charProp></char>
<char xml:id="CB00186"><charName>CBETA CHARACTER CB00186</charName>
<charProp><localName>composition</localName><value>[木*翕]</value></charProp>
<mapping type="unicode">U+3BD3</mapping><mapping cb:dec="983226" type="PUA">U+F00BA</mapping></char>
<char xml:id="CB03216"><charName>CBETA CHARACTER CB03216</charName>
<charProp><localName>composition</localName><value>[示*玄]</value></charProp>
<mapping type="normal_unicode">U+25646</mapping><mapping cb:dec="986256" type="PUA">U+F0C90</mapping></char>
<char xml:id="CB01197"><charName>CBETA CHARACTER CB01197</charName>
<charProp><localName>composition</localName><value>[臊-品+巳]</value></charProp>
<mapping cb:dec="984237" type="PUA">U+F04AD</mapping></char>
<char xml:id="SD-A441"><charName>CBETA CHARACTER SD-A441</charName>
<charProp><localName>Romanized form in CBETA transcription</localName><value>kaa</value></charProp>
<charProp><localName>Character in the Siddham font</localName><value>乙</value></charProp>
<mapping cb:dec="1066049" type="PUA">U+104441</mapping></char>
</charDecl></encodingDesc></teiHeader>
<text><body>{}</body></text></TEI>"#,
            body
        )
    }

    #[test]
    fn matches_golden_output_of_t0004() {
        let v = extract_volume(T0004).unwrap();
        assert_eq!(v.canon, "T");
        assert_eq!(v.text, T0004_TEXT.trim_end_matches('\n'));
        assert_eq!(v.plain, T0004_PLAIN.trim_end_matches('\n'));
        let golden = golden_lines(T0004_LINES);
        assert_eq!(v.lines.len(), golden.len());
        assert_eq!(v.lines, golden);
    }

    #[test]
    fn locates_offsets_in_lines() {
        let v = extract_volume(T0004).unwrap();
        // golden: line 0159a24 starts at plain offset 12
        let (line, pos) = v.locate(12).unwrap();
        assert_eq!((line.lb.as_deref(), pos), (Some("0159a24"), 1));
        let (line, pos) = v.locate(14).unwrap();
        assert_eq!((line.lb.as_deref(), pos), (Some("0159a24"), 3));
        let (line, _) = v.locate(v.plain.chars().count() - 1).unwrap();
        assert_eq!(line, v.lines.last().unwrap());
    }

    #[test]
    fn takes_lem_and_skips_rdg_and_notes() {
        let v = extract_volume(&wrap(
            r#"<p><lb n="0001a01" ed="T"/>甲<app><lem wit="【CB】">乙</lem><rdg wit="【大】">丙</rdg></app>丁<note n="0001001" resp="Taisho" type="orig" place="foot text">注</note>戊</p>"#,
        ))
        .unwrap();
        assert_eq!(v.text, "甲乙丁戊");
    }

    #[test]
    fn falls_back_to_rdg_when_app_has_no_lem() {
        let v = extract_volume(&wrap(r#"<p><app><rdg>丙</rdg><rdg>丁</rdg></app></p>"#)).unwrap();
        assert_eq!(v.text, "丙");
    }

    #[test]
    fn keeps_inline_notes_in_full_width_parentheses() {
        let v = extract_volume(&wrap(r#"<p>甲<note place="inline">乙丙</note>丁</p>"#)).unwrap();
        assert_eq!(v.text, "甲（乙丙）丁");
        assert_eq!(v.plain, "甲乙丙丁");
        assert_eq!(v.plain_to_text, vec![0, 2, 3, 5]);
    }

    #[test]
    fn resolves_missing_glyphs_to_one_char() {
        // real declarations (T01n0026_001, T02n0125_014, F28n1071_008 ...)
        let v = extract_volume(&wrap(
            r##"<p><g ref="#CB00001"/><g ref="#CB00186"/><g ref="#CB03216"/><g ref="#CB01197"/><g ref="#SD-A441"/></p>"##,
        ))
        .unwrap();
        assert_eq!(v.text, "呢\u{3BD3}\u{25646}\u{F04AD}\u{104441}");
        assert_eq!(v.plain.chars().count(), 5);
        // nothing but a composition / no declaration at all
        let v = extract_volume(&wrap(r##"<p><g ref="#CB00002"/><g ref="#CB09999"/></p>"##)).unwrap();
        assert_eq!(v.text, "[王*巳]@Declare@CB09999");
    }

    #[test]
    fn takes_zh_hant_of_a_siddham_pair_without_type() {
        // F28n1071_008: <cb:tt place="inline"> has no type="app"
        let v = extract_volume(&wrap(
            r##"<p>唧理<cb:tt place="inline"><cb:t xml:lang="sa-Sidd"><g ref="#SD-A441"/></cb:t><cb:t xml:lang="zh-Hant">劒</cb:t></cb:tt></p>"##,
        ))
        .unwrap();
        assert_eq!(v.text, "唧理劒");
    }

    #[test]
    fn skips_mulu_and_records_paragraph_anchors() {
        // T09n0262_001: the mulu repeats the head; positions in the line do not count it
        let v = extract_volume(&wrap(
            r#"<cb:div><cb:mulu level="1" type="序">御製序</cb:mulu><head><lb n="0001a03" ed="T"/>御製序</head><p xml:id="pT09p0001a0304">昔如來</p></cb:div>"#,
        ))
        .unwrap();
        assert_eq!(v.text, "御製序昔如來");
        assert_eq!(v.anchors, vec![("pT09p0001a0304".to_string(), 3)]);
        let (line, pos) = v.locate(3).unwrap();
        assert_eq!((line.lb.as_deref(), pos), (Some("0001a03"), 4));
        let old = extract_volume_with(&wrap(r#"<cb:mulu>御製序</cb:mulu><head>御製序</head>"#), false).unwrap();
        assert_eq!(old.text, "御製序御製序");
    }

    #[test]
    fn picks_zh_hant_in_cb_tt_and_expands_space() {
        let v = extract_volume(&wrap(
            r#"<p><cb:tt type="app"><cb:t xml:lang="sa-Sidd">x</cb:t><cb:t xml:lang="zh-Hant">般若</cb:t></cb:tt><space quantity="2"/>經</p>"#,
        ))
        .unwrap();
        assert_eq!(v.text, "般若  經");
        assert_eq!(v.plain, "般若經");
    }

    #[test]
    fn uses_only_lb_of_the_own_canon_and_drops_layout() {
        let v = extract_volume(&wrap(
            "<p><lb n=\"0001a01\" ed=\"T\"/>甲乙，\r\n\t<lb n=\"0099a01\" ed=\"R\"/>丙<pb n=\"0001b\" ed=\"T\"/><lb n=\"0001b01\" ed=\"T\"/>&#x4E01;&amp;</p>",
        ))
        .unwrap();
        assert_eq!(v.text, "甲乙，丙丁&");
        assert_eq!(v.plain, "甲乙丙丁&");
        let lbs: Vec<(usize, &str)> = v.lines.iter().map(|l| (l.offset, l.lb.as_deref().unwrap())).collect();
        assert_eq!(lbs, vec![(0, "0001a01"), (3, "0001b01")]);
    }

    #[test]
    fn drops_editorial_marks_but_keeps_printed_symbols() {
        // T39n1788_002 line 0208a24: ｀ and ＇ are not counted by the p anchor
        let v = extract_volume(&wrap(
            r#"<p><lb n="0208a24" ed="T"/>經　「如是｀妙行，汝等勤修，勿為＇放逸。」</p><p xml:id="pT39p0208a2414">贊曰</p>"#,
        ))
        .unwrap();
        assert_eq!(v.plain, "經如是妙行汝等勤修勿為放逸贊曰");
        let (line, pos) = v.locate(v.anchors[0].1).unwrap();
        assert_eq!((line.lb.as_deref(), pos), (Some("0208a24"), 14));
        // 【】 are editorial; ○ 〃 ※ ～ 〇 are counted
        let v = extract_volume(&wrap("<p>【疏】三○初〃※～〇</p>")).unwrap();
        assert_eq!(v.plain, "疏三○初〃※～〇");
    }

    #[test]
    fn cuts_text_into_original_lines() {
        let v = extract_volume(&wrap(
            r#"<p><lb n="0001a01" ed="T"/>甲乙，<lb n="0001a02" ed="T"/>「丙丁。」<lb n="0001a03" ed="T"/>戊</p>"#,
        ))
        .unwrap();
        let text: Vec<char> = v.text.chars().collect();
        let lines: Vec<(&str, String)> = v
            .line_spans()
            .into_iter()
            .map(|(lb, a, b)| (lb.unwrap(), text[a..b].iter().collect()))
            .collect();
        assert_eq!(
            lines,
            vec![("0001a01", "甲乙，".to_string()), ("0001a02", "「丙丁。」".to_string()), ("0001a03", "戊".to_string())]
        );
        // the real sample: lines cover the whole text
        let v = extract_volume(T0004).unwrap();
        let spans = v.line_spans();
        assert_eq!(spans.len(), v.lines.len());
        assert_eq!(spans.last().unwrap().2, v.text.chars().count());
        assert!(spans.windows(2).all(|w| w[0].2 == w[1].1));
    }

    #[test]
    fn reports_broken_xml() {
        assert!(extract_volume("<TEI><text><body><p>甲</body></TEI>").is_err());
        assert!(extract_volume("<TEI><teiHeader/></TEI>").is_err());
    }
}
