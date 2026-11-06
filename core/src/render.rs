//! Structured reading view of a volume (design-spec A.6, "渲染的结构映射").
//!
//! The HTML is produced by the **same walk** as the text extraction
//! (`volume.rs`): every char that the extraction emits is written here too,
//! and counted with the same rule as `plain`. So a hit found in `plain`
//! (`search::locate`) can be highlighted in the HTML without any second
//! mapping, and `data-lb` markers sit exactly where `lines` says.
//!
//! What is shown but not counted (it is not reading text): collation notes
//! (a numbered marker with the note in a hidden bubble), variant readings
//! (in the `title` of the reading), verse caesuras, illegible-char marks,
//! the other language of a Sanskrit/Chinese pair, images.

use crate::volume::{extract_with_html, is_dropped};

/// Options for one rendering.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// char ranges of `plain` to highlight (`search::Occurrence` from/to), sorted
    pub highlights: Vec<(usize, usize)>,
    /// prefix for image URLs; the bookcase path (`figures/B/B06p0447_01.gif`)
    /// is appended, e.g. `http://sl.localhost/`
    pub asset_prefix: String,
}

#[derive(Debug, Clone)]
pub struct RenderedVolume {
    pub html: String,
    /// number of collation notes shown as markers
    pub notes: usize,
    /// number of `plain` chars written (must equal the extraction's `plain`)
    pub plain_chars: usize,
}

/// Render a volume XML to an HTML fragment (no `<html>`/`<body>`).
pub fn render_volume(xml: &str, opts: &RenderOptions) -> Result<RenderedVolume, String> {
    let (_vt, out) = extract_with_html(xml, opts)?;
    Ok(RenderedVolume {
        html: out.buf,
        notes: out.notes,
        plain_chars: out.plain,
    })
}

pub(crate) fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

/// Keep only `margin-left`, `margin-right`, `text-indent` with `em` values.
pub(crate) fn safe_style(style: &str) -> String {
    let mut out = Vec::new();
    for decl in style.split(';') {
        let mut kv = decl.splitn(2, ':');
        let (k, v) = match (kv.next(), kv.next()) {
            (Some(k), Some(v)) => (k.trim(), v.trim()),
            _ => continue,
        };
        if !matches!(k, "margin-left" | "margin-right" | "text-indent") {
            continue;
        }
        let num = v.strip_suffix("em").unwrap_or("");
        if !num.is_empty() && num.trim_start_matches('-').chars().all(|c| c.is_ascii_digit() || c == '.') {
            out.push(format!("{}:{}", k, v));
        }
    }
    out.join(";")
}

/// `../figures/B/B06p0447_01.gif` -> `figures/B/B06p0447_01.gif` (the
/// figures folder sits at the bookcase root, whatever the `../` say).
pub fn figure_path(url: &str) -> Option<String> {
    let i = url.find("figures/")?;
    let p = &url[i..];
    if p.contains("..") || p.contains('\\') {
        return None;
    }
    Some(p.to_string())
}

/// HTML sink used by the extraction walk.
pub(crate) struct HtmlOut {
    pub buf: String,
    pub notes: usize,
    /// plain chars written so far
    pub plain: usize,
    hl: Vec<(usize, usize)>,
    hl_i: usize,
    in_mark: bool,
    pub asset_prefix: String,
    /// depth of `p cb:type="pre"` (科判 diagrams): line breaks are kept
    pub in_pre: usize,
}

impl HtmlOut {
    pub fn new(opts: &RenderOptions) -> Self {
        let mut hl = opts.highlights.clone();
        hl.sort();
        HtmlOut {
            buf: String::with_capacity(64 * 1024),
            notes: 0,
            plain: 0,
            hl,
            hl_i: 0,
            in_mark: false,
            asset_prefix: opts.asset_prefix.clone(),
            in_pre: 0,
        }
    }

    /// Is plain char `i` inside a highlight? Ranges are visited in order.
    fn hit(&mut self, i: usize) -> bool {
        while self.hl_i < self.hl.len() && self.hl[self.hl_i].1 <= i {
            self.hl_i += 1;
        }
        self.hl_i < self.hl.len() && self.hl[self.hl_i].0 <= i
    }

    /// A dropped char (punctuation) is highlighted when the hit goes on after it.
    fn hit_continues(&self) -> bool {
        self.plain > 0
            && self.hl_i < self.hl.len()
            && self.hl[self.hl_i].0 < self.plain
            && self.plain < self.hl[self.hl_i].1
    }

    fn set_mark(&mut self, on: bool) {
        if on && !self.in_mark {
            self.buf.push_str("<mark>");
        } else if !on && self.in_mark {
            self.buf.push_str("</mark>");
        }
        self.in_mark = on;
    }

    /// Reading text: written and counted.
    pub fn text(&mut self, s: &str) {
        for c in s.chars() {
            let on = if is_dropped(c) {
                self.in_mark && self.hit_continues()
            } else {
                let on = self.hit(self.plain);
                self.plain += 1;
                on
            };
            self.set_mark(on);
            let mut tmp = String::new();
            escape(&c.to_string(), &mut tmp);
            self.buf.push_str(&tmp);
        }
        self.set_mark(false);
    }

    /// A missing glyph: counted as `text` (one char), shown as `display`.
    pub fn glyph(&mut self, text: &str, display: Option<&str>, title: &str) {
        match display {
            None if title.is_empty() => self.text(text),
            _ => {
                self.buf.push_str("<span class=\"g\" title=\"");
                escape(title, &mut self.buf);
                self.buf.push_str("\">");
                match display {
                    Some(d) => {
                        // count the glyph, show its description
                        let mut on = false;
                        for c in text.chars() {
                            if !is_dropped(c) {
                                on |= self.hit(self.plain);
                                self.plain += 1;
                            }
                        }
                        self.set_mark(on);
                        escape(d, &mut self.buf);
                        self.set_mark(false);
                    }
                    None => self.text(text),
                }
                self.buf.push_str("</span>");
            }
        }
    }

    /// Markup that is not reading text.
    pub fn raw(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    /// `<tag class="..." attr="...">`; empty values are left out.
    pub fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.buf.push('<');
        self.buf.push_str(tag);
        for (k, v) in attrs {
            if v.is_empty() {
                continue;
            }
            self.buf.push(' ');
            self.buf.push_str(k);
            self.buf.push_str("=\"");
            escape(v, &mut self.buf);
            self.buf.push('"');
        }
        self.buf.push('>');
    }

    pub fn close(&mut self, tag: &str) {
        self.buf.push_str("</");
        self.buf.push_str(tag);
        self.buf.push('>');
    }

    pub fn image(&mut self, url: &str) {
        match figure_path(url) {
            Some(p) => {
                let src = format!("{}{}", self.asset_prefix, p);
                self.open("img", &[("class", "figure"), ("src", &src), ("loading", "lazy"), ("alt", &p)]);
            }
            None => self.raw("<span class=\"figure-missing\">[圖]</span>"),
        }
    }

    /// An image given by its path inside the bookcase (articles under `a/`).
    pub fn img(&mut self, path: &str) {
        let src = format!("{}{}", self.asset_prefix, path);
        self.open("img", &[("class", "figure"), ("src", &src), ("loading", "lazy"), ("alt", "")]);
    }

    /// A collation note shown as a numbered marker with a hidden bubble.
    pub fn note(&mut self, label: &str, text: &str) {
        self.notes += 1;
        let n = self.notes.to_string();
        self.open("span", &[("class", "note"), ("tabindex", "0"), ("data-note", &n)]);
        self.buf.push_str("<sup>");
        self.buf.push_str(&n);
        self.buf.push_str("</sup><span class=\"note-body\">");
        if !label.is_empty() {
            self.buf.push_str("<b>");
            escape(label, &mut self.buf);
            self.buf.push_str("</b> ");
        }
        escape(text, &mut self.buf);
        self.buf.push_str("</span></span>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::volume::extract_volume;

    const T0004: &str = include_str!("../../samples/cbeta/T01n0004_001.xml");

    fn wrap(body: &str) -> String {
        format!(
            r#"<TEI xmlns="http://www.tei-c.org/ns/1.0" xmlns:cb="http://www.cbeta.org/ns/1.0"><teiHeader><idno type="canon">T</idno>
<charDecl><char xml:id="CB01197"><charName>CBETA CHARACTER CB01197</charName>
<charProp><localName>composition</localName><value>[臊-品+巳]</value></charProp>
<mapping type="PUA">U+F04AD</mapping></char></charDecl></teiHeader><text><body>{}</body></text></TEI>"#,
            body
        )
    }

    fn marked(html: &str) -> Vec<String> {
        html.split("<mark>").skip(1).map(|s| s.split("</mark>").next().unwrap().to_string()).collect()
    }

    #[test]
    fn counts_exactly_the_plain_text_of_the_extraction() {
        let r = render_volume(T0004, &RenderOptions::default()).unwrap();
        let v = extract_volume(T0004).unwrap();
        assert_eq!(r.plain_chars, v.plain.chars().count());
        // every line of the extraction has its marker
        for l in &v.lines {
            assert!(r.html.contains(&format!("data-lb=\"{}\"", l.lb.as_deref().unwrap())));
        }
        // T0004 has Taisho/CBETA note pairs: only one marker per pair
        assert!(r.notes > 0);
        assert!(r.html.contains("class=\"note\""));
        assert!(r.html.contains("<p class=\"p\" id=\"pT01p0159a2701\""));
    }

    #[test]
    fn highlights_plain_ranges_with_inner_punctuation() {
        let xml = wrap(r#"<p><lb n="0001a01" ed="T"/>甲當念，<lb n="0001a02" ed="T"/>他念乙</p>"#);
        let r = render_volume(&xml, &RenderOptions { highlights: vec![(1, 5)], ..Default::default() }).unwrap();
        // the line marker splits the mark; the comma inside the hit is marked too
        assert_eq!(marked(&r.html).concat(), "當念，他念");
        assert!(!r.html.contains("<mark>甲"));
    }

    #[test]
    fn shows_notes_variants_glyphs_and_diagrams() {
        let xml = wrap(concat!(
            r#"<p><lb n="0001a01" ed="T"/>生<note n="1" type="orig" place="foot text">〔死〕－【三】</note>"#,
            r#"<note n="1" type="mod">〔－〕【CB】，死【大】</note><app><lem wit="【CB】">無</lem><rdg wit="【大】">死</rdg></app>"#,
            r##"<g ref="#CB01197"/></p><p cb:type="pre"><lb n="0001a02" ed="T"/>┌初<lb n="0001a03" ed="T"/>└二</p>"##,
            r#"<lg><l>佛昔修習<caesura/>於諸境界</l></lg>"#,
        ));
        let r = render_volume(&xml, &RenderOptions::default()).unwrap();
        assert_eq!(r.notes, 1, "orig + mod with the same n -> one marker");
        assert!(r.html.contains("CBETA 校"));
        assert!(r.html.contains("title=\"【CB】無；【大】死\""));
        assert!(r.html.contains("[臊-品+巳]</span>"));
        assert!(r.html.contains("┌初<br><span class=\"lb\" data-lb=\"0001a03\"></span>└二"));
        assert!(r.html.contains("<span class=\"caesura\">　</span>"));
        let v = extract_volume(&xml).unwrap();
        assert_eq!(r.plain_chars, v.plain.chars().count());
    }

    #[test]
    fn keeps_only_safe_styles() {
        assert_eq!(safe_style("margin-left:1em;text-indent:-1em;"), "margin-left:1em;text-indent:-1em");
        assert_eq!(safe_style("color:red;margin-left:expression(x)"), "");
    }

    #[test]
    fn finds_figures_at_the_bookcase_root() {
        assert_eq!(figure_path("../figures/B/B06p0447_01.gif").as_deref(), Some("figures/B/B06p0447_01.gif"));
        assert_eq!(figure_path("../../etc/passwd"), None);
        assert_eq!(figure_path("figures/../../x"), None);
    }
}
