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
fn is_dropped(c: char) -> bool {
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
}

impl VolumeText {
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

/// Missing-glyph declarations: `CB01979` -> text to use (one char when possible).
fn load_char_decl(root: &Elem) -> HashMap<String, String> {
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
        let normalized = props.get("normalized form").filter(|v| v.chars().count() == 1).cloned();
        let pick = normalized
            .or_else(|| mapping("unicode"))
            .or_else(|| mapping("normal_unicode"))
            .or_else(|| mapping("PUA"))
            .or_else(|| props.get("normalized form").filter(|v| !v.is_empty()).cloned())
            .or_else(|| props.get("composition").filter(|v| !v.is_empty()).cloned())
            .unwrap_or_else(|| ch.child_text(Ns::Tei, "charName").trim().to_string());
        decl.insert(id, pick);
    });
    decl
}

struct Extractor<'a> {
    canon: &'a str,
    decl: HashMap<String, String>,
    /// (text, index into `lbs` of the current line)
    pieces: Vec<(String, Option<usize>)>,
    lbs: Vec<String>,
    cur_lb: Option<usize>,
    /// (xml:id of a `p`, index into `pieces` where it starts)
    anchors: Vec<(String, usize)>,
    skip_mulu: bool,
}

impl<'a> Extractor<'a> {
    fn emit(&mut self, s: &str) {
        if !s.is_empty() {
            self.pieces.push((s.to_string(), self.cur_lb));
        }
    }

    fn children(&mut self, el: &Elem) {
        for n in &el.children {
            match n {
                Node::Text(t) => {
                    let s: String = t.chars().filter(|c| !matches!(c, '\r' | '\n' | '\t')).collect();
                    self.emit(&s);
                }
                Node::Elem(e) => self.walk(e),
            }
        }
    }

    fn walk(&mut self, el: &Elem) {
        match el.name.as_str() {
            "teiHeader" => {}
            "lb" => {
                if el.attr("ed") == Some(self.canon) {
                    self.lbs.push(el.attr("n").unwrap_or("").to_string());
                    self.cur_lb = Some(self.lbs.len() - 1);
                }
            }
            "pb" | "milestone" | "unclear" => {}
            "note" => {
                if el.attr("place") == Some("inline") {
                    self.emit("（");
                    self.children(el);
                    self.emit("）");
                }
            }
            "app" => {
                let pick = el
                    .elems()
                    .find(|e| e.is(Ns::Tei, "lem"))
                    .or_else(|| el.elems().find(|e| e.is(Ns::Tei, "rdg")));
                if let Some(lem) = pick {
                    self.children(lem);
                }
            }
            "rdg" => {}
            "mulu" if el.ns == Ns::Cb && self.skip_mulu => {}
            "p" => {
                if let Some(id) = el.attr("xml:id") {
                    self.anchors.push((id.to_string(), self.pieces.len()));
                }
                self.children(el)
            }
            "g" => {
                let id = el.attr("ref").unwrap_or("").trim_start_matches('#');
                let s = match self.decl.get(id) {
                    Some(s) => s.clone(),
                    None => format!("@Declare@{}", id),
                };
                self.emit(&s);
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
                    .or_else(|| ts.first());
                if let Some(t) = pick {
                    self.children(t);
                }
            }
            _ => self.children(el),
        }
    }
}

/// Extract the reading text of one volume XML file.
pub fn extract_volume(xml: &str) -> Result<VolumeText, String> {
    extract_volume_with(xml, true)
}

/// `skip_mulu = false` keeps the text of `cb:mulu` (old behaviour, only for comparison).
pub fn extract_volume_with(xml: &str, skip_mulu: bool) -> Result<VolumeText, String> {
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
    };
    ex.children(body);

    let mut text = String::new();
    let mut plain = String::new();
    let mut plain_len = 0usize;
    let mut lines: Vec<LineStart> = Vec::new();
    let mut last: Option<Option<&str>> = None; // None = no line yet
    let mut anchors: Vec<(String, usize)> = Vec::new();
    let mut next_anchor = 0;
    for (pi, (s, lb)) in ex.pieces.iter().enumerate() {
        while next_anchor < ex.anchors.len() && ex.anchors[next_anchor].1 <= pi {
            anchors.push((ex.anchors[next_anchor].0.clone(), plain_len));
            next_anchor += 1;
        }
        text.push_str(s);
        let lb_str = lb.map(|i| ex.lbs[i].as_str());
        for c in s.chars() {
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
            plain_len += 1;
        }
    }
    for (id, _) in &ex.anchors[next_anchor..] {
        anchors.push((id.clone(), plain_len));
    }
    Ok(VolumeText {
        canon,
        text,
        plain,
        lines,
        anchors,
    })
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
    fn reports_broken_xml() {
        assert!(extract_volume("<TEI><text><body><p>甲</body></TEI>").is_err());
        assert!(extract_volume("<TEI><teiHeader/></TEI>").is_err());
    }
}
