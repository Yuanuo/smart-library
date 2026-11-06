//! CBETA citations and `#` commands: go straight to a book, juan or line.
//!
//! CBETA's "copy as citation" gives text such as
//!
//! ```text
//! 《坐禪三昧經》卷1：「第五法門治等分行……」(CBETA 2025.Q1, T15, no. 614, p. 269c29)
//! ```
//!
//! The part in parentheses names the canon (`T`), the physical volume (册,
//! `15`), the book number (`614`) and the line (`p. 269c29`: page 269,
//! column c, line 29).
//!
//! In the quick search, a query starting with `#` is a command (the same as
//! in cbeta-explorer):
//!
//! | command | goes to |
//! | --- | --- |
//! | `#T01n0001_p0001a01` | line 0001a01 of T0001 (册 01) |
//! | `#T0001_p0001a01` | line 0001a01 of T0001 |
//! | `#T01, no. 1, p. 1a1` | line 0001a01 of T0001 (also `p. 1, a1`, `pp. 1a1-2b3`) |
//! | `#T01n0001_1`, `#T01n0001_001` | juan 1 of T0001 |
//! | `#T0001_1`, `#T0001_001` | juan 1 of T0001 |
//! | `#T0001` | T0001 |
//!
//! Pasted citation text is found by [`parse_citations`] (the citation forms
//! only, `T15, no. 614, p. 269c29` and `T15n0614_p0269c29`) and turned into
//! the command [`Citation::command`].
//!
//! Books are matched with leading zeros ignored (`T1` = `T0001`, `JB480` =
//! `J` + `B480`). Without a juan, the volume is the one whose first line
//! (`spine.txt`) is the last not after the cited line, inside the cited 册
//! when given (pages restart in every 册).
//!
//! The cited line is not always where the quoted passage is (a citation can
//! give the first line of the juan). When the text before the citation has a
//! quote 「…」, it is looked for in that juan as a phrase (punctuation
//! ignored, 简繁 folded); found, its line is the target, the nearest at or
//! after the cited line.

use crate::bookcase::{Bookcase, VolumeRef};
use crate::search::{locate, MatchMode};

/// (page, column, line): `269c29` -> (269, 'c', 29)
pub type LinePos = (u32, char, u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// `T`, `X`, `GA` … (for a book id like `JB480` it may hold `JB`)
    pub canon: String,
    /// physical volume (册), digits as cited: `15`
    pub serial: Option<String>,
    /// book number as cited: `614`, `B480`, `220a`
    pub number: String,
    pub juan: Option<u16>,
    pub line: Option<LinePos>,
    /// the citation as found, e.g. `T15, no. 614, p. 269c29`
    pub text: String,
    /// the quoted passage before it (between 「 and 」), if any
    pub quote: Option<String>,
}

impl Citation {
    /// The `#` command for it: `#T15, no. 614, p. 269c29`.
    pub fn command(&self) -> String {
        format!("#{}", self.text)
    }
}

/// Where a citation leads.
#[derive(Debug, Clone)]
pub struct CiteTarget {
    pub book_id: String,
    pub title: String,
    /// volume file (`None`: the book's first readable volume)
    pub path: Option<String>,
    /// `卷一`
    pub label: String,
    /// line in the volume's format, e.g. `0269c29`; the quote's line when
    /// it was found
    pub lb: Option<String>,
    /// the quote was found in the volume
    pub quote_found: bool,
}

struct Cursor<'a> {
    s: &'a [char],
    i: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn peek_at(&self, n: usize) -> Option<char> {
        self.s.get(self.i + n).copied()
    }
    fn spaces(&mut self) {
        while matches!(self.peek(), Some(c) if c == ' ' || c == '\u{3000}') {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn eat_str(&mut self, s: &str) -> bool {
        let start = self.i;
        for c in s.chars() {
            if !self.eat(c) {
                self.i = start;
                return false;
            }
        }
        true
    }
    fn take_while(&mut self, f: impl Fn(char) -> bool) -> String {
        let start = self.i;
        while matches!(self.peek(), Some(c) if f(c)) {
            self.i += 1;
        }
        self.s[start..self.i].iter().collect()
    }
    /// `,` or `，` with spaces around
    fn comma(&mut self) -> bool {
        self.spaces();
        let ok = self.eat(',') || self.eat('，');
        self.spaces();
        ok
    }
}

fn digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// `269c29`, `0269c29` or `269, c29`
fn page_line(c: &mut Cursor) -> Option<LinePos> {
    let page = c.take_while(digit).parse().ok()?;
    if !matches!(c.peek(), Some(x) if x.is_ascii_lowercase()) {
        let save = c.i;
        if !c.comma() || !matches!(c.peek(), Some(x) if x.is_ascii_lowercase()) {
            c.i = save;
            return None;
        }
    }
    let column = c.peek()?;
    c.i += 1;
    let line = c.take_while(digit).parse().ok()?;
    Some((page, column, line))
}

type Parsed = (String, Option<String>, String, Option<u16>, Option<LinePos>);

/// `T15, no. 614, p. 269c29`
fn modern(c: &mut Cursor) -> Option<Parsed> {
    let canon = c.take_while(|x| x.is_ascii_uppercase());
    let serial = c.take_while(digit);
    if canon.is_empty() || canon.len() > 3 || serial.is_empty() || !c.comma() {
        return None;
    }
    if !(c.eat_str("no.") || c.eat_str("No.") || c.eat_str("no")) {
        return None;
    }
    c.spaces();
    let number = c.take_while(|x| x.is_ascii_alphanumeric());
    if !number.chars().any(digit) || !c.comma() || !(c.eat_str("pp.") || c.eat_str("p.")) {
        return None;
    }
    c.spaces();
    let line = page_line(c)?;
    Some((canon, Some(serial), number, None, Some(line)))
}

/// `T15n0614_p0269c29`, `T0614_p0269c29`, `T15n0614_1`, `T0614_001`,
/// `T0614` (`book_only`: also the last two forms)
fn compact(c: &mut Cursor, book_only: bool) -> Option<Parsed> {
    let canon = c.take_while(|x| x.is_ascii_uppercase());
    let digits = c.take_while(digit);
    if canon.is_empty() || canon.len() > 3 || digits.is_empty() {
        return None;
    }
    let (serial, number) = if c.peek() == Some('n') && c.peek_at(1).is_some_and(|x| x.is_ascii_alphanumeric()) {
        c.i += 1;
        (Some(digits), c.take_while(|x| x.is_ascii_alphanumeric() && x != '_'))
    } else {
        // `T0220a`: a part of a big book
        let mut n = digits;
        if c.peek().is_some_and(|x| x.is_ascii_lowercase()) && c.peek_at(1).is_none_or(|x| !x.is_ascii_alphanumeric()) {
            n.push(c.peek().unwrap());
            c.i += 1;
        }
        (None, n)
    };
    if !number.chars().any(digit) {
        return None;
    }
    if c.eat('_') {
        if c.eat('p') {
            let line = page_line(c)?;
            return Some((canon, serial, number, None, Some(line)));
        }
        let juan: u16 = c.take_while(digit).parse().ok()?;
        return book_only.then_some((canon, serial, number, Some(juan), None));
    }
    book_only.then_some((canon, serial, number, None, None))
}

fn citation(p: Parsed, text: String, quote: Option<String>) -> Citation {
    let (canon, serial, number, juan, line) = p;
    Citation {
        canon,
        serial,
        number,
        juan,
        line,
        text,
        quote,
    }
}

/// Every citation in a text (usually one, pasted from CBETA): the forms
/// `T15, no. 614, p. 269c29` and `T15n0614_p0269c29`.
pub fn parse_citations(text: &str) -> Vec<Citation> {
    let s: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let starts = s[i].is_ascii_uppercase() && (i == 0 || !s[i - 1].is_ascii_alphanumeric());
        if !starts {
            i += 1;
            continue;
        }
        let mut c = Cursor { s: &s, i };
        let found = modern(&mut c).or_else(|| {
            c.i = i;
            compact(&mut c, false)
        });
        match found {
            Some(p) => {
                out.push(citation(p, s[i..c.i].iter().collect(), quote_before(&s[..i])));
                i = c.i;
            }
            None => i += 1,
        }
    }
    out
}

/// A `#` command (the whole query, see the module doc). `None` when the
/// query is not a command or not a valid one.
pub fn parse_command(query: &str) -> Option<Citation> {
    let rest = query.trim().strip_prefix('#')?.trim();
    let s: Vec<char> = rest.chars().collect();
    let mut c = Cursor { s: &s, i: 0 };
    let p = modern(&mut c).or_else(|| {
        c.i = 0;
        compact(&mut c, true)
    })?;
    // a range (`pp. 1a1-2b3`, `p. 420, a4-5`) may follow
    if c.i < s.len() && !matches!(s[c.i], '-' | '～' | '~' | ' ' | ')' | '）') {
        return None;
    }
    Some(citation(p, s[..c.i].iter().collect(), None))
}

/// The text between the last 「 and its 」 before a citation.
fn quote_before(s: &[char]) -> Option<String> {
    let close = s.iter().rposition(|&c| c == '」')?;
    let open = s[..close].iter().rposition(|&c| c == '「')?;
    let q: String = s[open + 1..close].iter().collect();
    let q = q.trim().to_string();
    (!q.is_empty()).then_some(q)
}

/// Leading zeros of numbers dropped, upper case: `T0001` -> `T1`,
/// `JB0480` -> `JB480`, `0614` -> `614`.
fn norm(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let after_digit = out.chars().last().is_some_and(|p| p.is_ascii_digit());
        let next_digit = chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
        if c == '0' && !after_digit && next_digit {
            continue;
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// `0269c29` -> ((269, 'c', 29), 4: the width of the page digits)
fn parse_lb(lb: &str) -> Option<(LinePos, usize)> {
    let width = lb.chars().take_while(|c| c.is_ascii_digit()).count();
    let page = lb[..width].parse().ok()?;
    let mut rest = lb[width..].chars();
    let column = rest.next()?;
    let line = rest.as_str().parse().ok()?;
    Some(((page, column, line), width))
}

impl Bookcase {
    /// The book, volume and line a citation points to.
    pub fn resolve_citation(&self, cite: &Citation) -> Option<CiteTarget> {
        let find = |number: &str| {
            let want = norm(&format!("{}{}", cite.canon, number));
            self.book(&format!("{}{}", cite.canon, number)).or_else(|| {
                self.catalog
                    .order
                    .iter()
                    .filter_map(|id| self.book(id))
                    .find(|b| norm(&format!("{}{}", b.canon, b.number)) == want)
            })
        };
        // `T0220a` is a part of T0220 in the trees, not a book of the catalog
        let book = find(&cite.number)
            .or_else(|| find(cite.number.trim_end_matches(|c: char| c.is_ascii_lowercase())))?;
        let mut vols: Vec<VolumeRef> = self.volumes(&book.id).into_iter().filter(|v| v.exists).collect();
        if let Some(serial) = &cite.serial {
            let same: Vec<VolumeRef> = vols.iter().filter(|v| norm(&v.serial) == norm(serial)).cloned().collect();
            if !same.is_empty() {
                vols = same;
            }
        }
        let mut target = CiteTarget {
            book_id: book.id.clone(),
            title: book.title.clone(),
            path: None,
            label: String::new(),
            lb: None,
            quote_found: false,
        };
        if let Some(j) = cite.juan {
            let v = vols.iter().find(|v| v.idx == j)?;
            target.path = Some(v.path.clone());
            target.label = v.label.clone();
            return Some(target);
        }
        let line = match cite.line {
            Some(l) => l,
            None => return Some(target),
        };
        // the volume holding the line: its start is not after the line, and
        // the next one starts after it (or in a new 册, pages starting again)
        let starts: Vec<Option<(LinePos, usize)>> =
            vols.iter().map(|v| self.spine().get(&v.path).and_then(|lb| parse_lb(lb))).collect();
        let mut chosen = 0;
        for (i, s) in starts.iter().enumerate() {
            if let Some((start, _)) = s {
                if *start <= line {
                    chosen = i;
                    let next = starts.get(i + 1).copied().flatten().map(|(n, _)| n);
                    if next.is_none_or(|n| n > line || n < *start) {
                        break;
                    }
                }
            }
        }
        let v = vols.get(chosen)?;
        let width = starts[chosen].map(|(_, w)| w).unwrap_or(4);
        let mut lb = format!("{:0w$}{}{:02}", line.0, line.1, line.2, w = width);
        if let (Some(q), Ok(vt)) = (&cite.quote, self.volume_text(&v.path)) {
            let occ = locate(&vt, q, MatchMode::Phrase);
            let after = occ.iter().find(|o| parse_lb(&o.lb).is_some_and(|(p, _)| p >= line));
            if let Some(o) = after.or(occ.first()) {
                lb = o.lb.clone();
                target.quote_found = true;
            }
        }
        target.path = Some(v.path.clone());
        target.label = v.label.clone();
        target.lb = Some(lb);
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::MemSource;
    use crate::text::encode_utf16le_bom;

    fn parts(c: &Citation) -> (&str, Option<&str>, &str, Option<u16>, Option<LinePos>) {
        (c.canon.as_str(), c.serial.as_deref(), c.number.as_str(), c.juan, c.line)
    }

    #[test]
    fn parses_pasted_citations() {
        let c = parse_citations(
            "《坐禪三昧經》卷1：「第五法門治等分行，及重罪人求索佛，如是人等當教一心念佛三昧。」(CBETA 2025.Q1, T15, no. 614, p. 269c29)",
        );
        assert_eq!(c.len(), 1);
        assert_eq!(parts(&c[0]), ("T", Some("15"), "614", None, Some((269, 'c', 29))));
        assert_eq!(c[0].command(), "#T15, no. 614, p. 269c29");
        assert_eq!(c[0].quote.as_deref(), Some("第五法門治等分行，及重罪人求索佛，如是人等當教一心念佛三昧。"));

        let c = parse_citations("(CBETA, X78, no. 1553, p. 420, a4-5 // Z 2B:8, p. 298, a1-2 // R135, p. 595, a1-2)");
        assert_eq!(c.len(), 1);
        assert_eq!(parts(&c[0]), ("X", Some("78"), "1553", None, Some((420, 'a', 4))));

        let c = &parse_citations("CBETA 2025.Q1, J40, no. B480, pp. 163a9-164b2")[0];
        assert_eq!(parts(c), ("J", Some("40"), "B480", None, Some((163, 'a', 9))));

        let c = &parse_citations("見 T15n0614_p0269c29。")[0];
        assert_eq!(parts(c), ("T", Some("15"), "0614", None, Some((269, 'c', 29))));

        // plain titles and ids are not citations
        assert!(parse_citations("如是我聞 T0262 法華 T0001_001").is_empty());
    }

    #[test]
    fn parses_the_hash_commands() {
        let cmd = |q: &str| parse_command(q).map(|c| (c.canon, c.serial, c.number, c.juan, c.line));
        let line = Some((1, 'a', 1));
        let s = |x: &str| Some(x.to_string());
        assert_eq!(cmd("#T01n0001_p0001a01"), Some(("T".into(), s("01"), "0001".into(), None, line)));
        assert_eq!(cmd("#T0001_p0001a01"), Some(("T".into(), None, "0001".into(), None, line)));
        assert_eq!(cmd("#T01, no. 1, p. 1a1"), Some(("T".into(), s("01"), "1".into(), None, line)));
        assert_eq!(cmd("#T01n0001_1"), Some(("T".into(), s("01"), "0001".into(), Some(1), None)));
        assert_eq!(cmd("#T01n0001_001"), Some(("T".into(), s("01"), "0001".into(), Some(1), None)));
        assert_eq!(cmd(" # T0001_1 "), Some(("T".into(), None, "0001".into(), Some(1), None)));
        assert_eq!(cmd("#T0001_001"), Some(("T".into(), None, "0001".into(), Some(1), None)));
        assert_eq!(cmd("#T0001"), Some(("T".into(), None, "0001".into(), None, None)));
        assert_eq!(cmd("#T0220a"), Some(("T".into(), None, "0220a".into(), None, None)));
        assert_eq!(cmd("#JB480_2"), Some(("JB".into(), None, "480".into(), Some(2), None)));
        assert_eq!(cmd("#X78, no. 1553, p. 420, a4-5"), Some(("X".into(), s("78"), "1553".into(), None, Some((420, 'a', 4)))));
        assert!(cmd("#").is_none());
        assert!(cmd("#法華").is_none());
        assert!(cmd("#T0001_x").is_none());
        assert!(cmd("T0001").is_none());
    }

    fn bookcase() -> Bookcase {
        let mut src = MemSource::new();
        src.add("index.xml", "<metadata><version>1</version><date>2025-04-26</date></metadata>");
        src.add("bookdata.txt", encode_utf16le_bom("T,2,【大】,大正新脩大藏經,Taisho\n"));
        src.add(
            "catalog.txt",
            "T , 經集部類 , , 15 , 0614 , 2 , 坐禪三昧經 , 姚秦 鳩摩羅什譯\n\
             L , 華嚴部類 , , 130 , 1557 , 2 , 華嚴經疏鈔(第1卷-第2卷) , 唐 澄觀\n\
             L , 華嚴部類 , , 131 , 1557 , 2 , 華嚴經疏鈔(第2卷-第3卷) , 唐 澄觀\n",
        );
        src.add("menu_nav.xhtml", r#"<nav><navlink href="bulei_nav.xhtml">部類</navlink></nav>"#);
        for n in ["simple_nav.xhtml", "advance_nav.xhtml", "bulei_nav.xhtml"] {
            src.add(n, "<nav></nav>");
        }
        src.add(
            "spine.txt",
            "XML/T/T15/T15n0614_001.xml , 0269c05\nXML/T/T15/T15n0614_002.xml , 0277a17\n\
             XML/L/L130/L130n1557_001.xml , 0001a01\nXML/L/L130/L130n1557_002.xml , 0400a01\n\
             XML/L/L131/L131n1557_002.xml , 0001a01\nXML/L/L131/L131n1557_003.xml , 0300a01\n",
        );
        for p in [
            "T/T15/T15n0614_001",
            "T/T15/T15n0614_002",
            "L/L130/L130n1557_001",
            "L/L130/L130n1557_002",
            "L/L131/L131n1557_002",
            "L/L131/L131n1557_003",
        ] {
            src.add(&format!("XML/{}.xml", p), "x");
        }
        Bookcase::open(Box::new(src)).unwrap()
    }

    #[test]
    fn finds_the_book_juan_and_line() {
        let bc = bookcase();
        let go = |q: &str| bc.resolve_citation(&parse_command(q).unwrap());

        let t = go("#T15, no. 614, p. 269c29").unwrap();
        assert_eq!(t.book_id, "T0614");
        assert_eq!(t.path.as_deref(), Some("XML/T/T15/T15n0614_001.xml"));
        assert_eq!(t.lb.as_deref(), Some("0269c29"));
        let t = go("#T0614_p280a4").unwrap();
        assert_eq!((t.label.as_str(), t.lb.as_deref()), ("卷二", Some("0280a04")));
        let t = go("#T0614_2").unwrap();
        assert_eq!((t.path.as_deref(), t.lb), (Some("XML/T/T15/T15n0614_002.xml"), None));
        let t = go("#T614").unwrap();
        assert_eq!((t.book_id.as_str(), t.path), ("T0614", None));
        assert!(go("#T0614_9").is_none());
        assert!(go("#T0999").is_none());

        // pages restart in every 册: the 册 decides, or the first fitting volume
        let t = go("#L131n1557_p0350a01").unwrap();
        assert_eq!(t.path.as_deref(), Some("XML/L/L131/L131n1557_003.xml"));
        let t = go("#L130n1557_p0350a01").unwrap();
        assert_eq!(t.path.as_deref(), Some("XML/L/L130/L130n1557_001.xml"));
        let t = go("#L1557_p0350a01").unwrap();
        assert_eq!(t.path.as_deref(), Some("XML/L/L130/L130n1557_001.xml"));
    }
}
