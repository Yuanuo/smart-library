//! Shared HTML output for volume and article extraction.

use crate::text::is_dropped;

/// Options for one rendering.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// char ranges of `plain` to highlight (`search::Occurrence` from/to), sorted
    pub highlights: Vec<(usize, usize)>,
    /// prefix for image URLs; the bookcase path (`figures/B/B06p0447_01.gif`)
    /// is appended, e.g. `http://sl.localhost/`
    pub asset_prefix: String,
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
        if !num.is_empty()
            && num
                .trim_start_matches('-')
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.')
        {
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
                self.open(
                    "img",
                    &[
                        ("class", "figure"),
                        ("src", &src),
                        ("loading", "lazy"),
                        ("alt", &p),
                    ],
                );
            }
            None => self.raw("<span class=\"figure-missing\">[圖]</span>"),
        }
    }

    /// An image given by its path inside the bookcase (articles under `a/`).
    pub fn img(&mut self, path: &str) {
        let src = format!("{}{}", self.asset_prefix, path);
        self.open(
            "img",
            &[
                ("class", "figure"),
                ("src", &src),
                ("loading", "lazy"),
                ("alt", ""),
            ],
        );
    }

    /// A collation note shown as a numbered marker with a hidden bubble.
    /// A note point: a numbered marker holding the original note and
    /// CBETA's note; the reader shows them by the chosen note mode. `n` is
    /// the note id shared with the variant reading (`app data-n`). The
    /// number itself is CSS content drawn from `data-note` (Reader.svelte),
    /// not text, so selecting the text does not copy it.
    pub fn note(&mut self, n: &str, orig: Option<&str>, cb: Option<&str>) {
        self.notes += 1;
        let no = self.notes.to_string();
        self.open(
            "span",
            &[
                ("class", "note"),
                ("tabindex", "0"),
                ("data-note", &no),
                ("data-n", n),
            ],
        );
        for (class, text) in [("nb-orig", orig), ("nb-cb", cb)] {
            if let Some(t) = text {
                self.buf.push_str("<span class=\"");
                self.buf.push_str(class);
                self.buf.push_str("\">");
                escape(t, &mut self.buf);
                self.buf.push_str("</span>");
            }
        }
        self.buf.push_str("</span>");
    }
}
