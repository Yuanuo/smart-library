//! A tiny, tolerant XML/XHTML reader for the small structural files of a bookcase
//! (index.xml, *_nav.xhtml, toc/*.xml).
//!
//! It is NOT meant for the volume XML files (those need a real XML parser).
//! Whitespace-only text nodes are dropped, element and attribute names are
//! lower-cased, unknown closing tags are ignored.

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|kv| kv.0 == key)
            .map(|kv| kv.1.as_str())
    }

    /// Direct child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> + '_ {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// First direct child element with the given name.
    pub fn first(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// All direct child elements with the given name.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// First direct child element that carries the given attribute.
    pub fn first_with_attr(&self, key: &str) -> Option<&Element> {
        self.elements().find(|e| e.attr(key).is_some())
    }

    /// First descendant (depth first) with the given name.
    pub fn find_descendant(&self, name: &str) -> Option<&Element> {
        for e in self.elements() {
            if e.name == name {
                return Some(e);
            }
            if let Some(found) = e.find_descendant(name) {
                return Some(found);
            }
        }
        None
    }

    /// All descendants with the given name, in document order.
    pub fn descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for e in self.elements() {
            if e.name == name {
                out.push(e);
            }
            e.descendants(name, out);
        }
    }

    /// Concatenated descendant text, ASCII whitespace collapsed (like jsoup's text()).
    pub fn text(&self) -> String {
        let mut buf = String::new();
        collect_text(self, &mut buf);
        buf.split(|c: char| c.is_ascii_whitespace())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn collect_text(e: &Element, buf: &mut String) {
    for c in &e.children {
        match c {
            Node::Text(t) => buf.push_str(t),
            Node::Element(el) => collect_text(el, buf),
        }
    }
}

const VOID: [&str; 6] = ["meta", "br", "img", "link", "hr", "input"];

/// Parse into a synthetic root element named `#root`.
pub fn parse(input: &str) -> Element {
    let mut stack: Vec<Element> = vec![Element {
        name: "#root".to_string(),
        ..Default::default()
    }];
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &input[i..];
            if rest.starts_with("<!--") {
                i = match rest.find("-->") {
                    Some(p) => i + p + 3,
                    None => bytes.len(),
                };
                continue;
            }
            if rest.starts_with("<?") || rest.starts_with("<!") {
                i = match rest.find('>') {
                    Some(p) => i + p + 1,
                    None => bytes.len(),
                };
                continue;
            }
            let end = tag_end(bytes, i);
            let inner = &input[i + 1..end];
            if let Some(closing) = inner.strip_prefix('/') {
                let name = closing.trim().to_ascii_lowercase();
                if let Some(pos) = stack.iter().rposition(|e| e.name == name) {
                    if pos > 0 {
                        while stack.len() > pos {
                            let done = stack.pop().unwrap();
                            stack.last_mut().unwrap().children.push(Node::Element(done));
                        }
                    }
                }
            } else {
                let trimmed = inner.trim_end();
                let self_closing = trimmed.ends_with('/');
                let body = trimmed.trim_end_matches('/');
                let (name, attr_str) = match body.find(|c: char| c.is_whitespace()) {
                    Some(p) => (&body[..p], &body[p..]),
                    None => (body, ""),
                };
                let name = name.to_ascii_lowercase();
                if !name.is_empty() {
                    let el = Element {
                        name: name.clone(),
                        attrs: parse_attrs(attr_str),
                        children: Vec::new(),
                    };
                    if self_closing || VOID.contains(&name.as_str()) {
                        stack.last_mut().unwrap().children.push(Node::Element(el));
                    } else {
                        stack.push(el);
                    }
                }
            }
            i = end + 1;
        } else {
            let next = input[i..].find('<').map(|p| i + p).unwrap_or(bytes.len());
            let raw = &input[i..next];
            if !raw.trim().is_empty() {
                stack.last_mut().unwrap().children.push(Node::Text(unescape(raw)));
            }
            i = next;
        }
    }
    while stack.len() > 1 {
        let done = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(Node::Element(done));
    }
    stack.pop().unwrap()
}

/// Index of the `>` that ends the tag starting at `start` (quotes respected),
/// or `bytes.len()` when the tag is not terminated.
fn tag_end(bytes: &[u8], start: usize) -> usize {
    let mut quote: u8 = 0;
    let mut j = start + 1;
    while j < bytes.len() {
        let b = bytes[j];
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'"' || b == b'\'' {
            quote = b;
        } else if b == b'>' {
            return j;
        }
        j += 1;
    }
    bytes.len()
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let ks = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
            i += 1;
        }
        let key: String = chars[ks..i].iter().collect::<String>().to_ascii_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let q = chars[i];
                i += 1;
                let vs = i;
                while i < chars.len() && chars[i] != q {
                    i += 1;
                }
                val = chars[vs..i].iter().collect::<String>();
                if i < chars.len() {
                    i += 1;
                }
            } else {
                let vs = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                val = chars[vs..i].iter().collect::<String>();
            }
        }
        if !key.is_empty() {
            out.push((key, unescape(&val)));
        }
    }
    out
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let after = &rest[p..];
        if let Some(semi) = after.find(';') {
            let ent = &after[1..semi];
            let rep = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => numeric_entity(ent),
            };
            match rep {
                Some(c) if semi <= 10 => {
                    out.push(c);
                    rest = &after[semi + 1..];
                    continue;
                }
                _ => {}
            }
        }
        out.push('&');
        rest = &after[1..];
    }
    out.push_str(rest);
    out
}

fn numeric_entity(ent: &str) -> Option<char> {
    let num = ent.strip_prefix('#')?;
    let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        num.parse::<u32>().ok()?
    };
    char::from_u32(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nav_like_markup() {
        let root = parse(
            r#"<?xml version="1.0"?><html><head><meta charset="UTF-8" /></head><body><nav type="catalog"><span>A &amp; B</span><ol><li><cblink href="x.xml#p1">T0001 長阿含經</cblink></li></ol></nav></body></html>"#,
        );
        let nav = root.find_descendant("nav").unwrap();
        assert_eq!(nav.attr("type"), Some("catalog"));
        assert_eq!(nav.first("span").unwrap().text(), "A & B");
        let li = nav.first("ol").unwrap().first("li").unwrap();
        let link = li.first_with_attr("href").unwrap();
        assert_eq!(link.attr("href"), Some("x.xml#p1"));
        assert_eq!(link.text(), "T0001 長阿含經");
        // <meta /> must not swallow the rest of the document
        assert!(root.find_descendant("body").is_some());
    }

    #[test]
    fn tolerates_stray_closing_tags_and_crlf() {
        let root = parse("<a>\r\n<b>x</c></b>\r\n</a>");
        let a = root.first("a").unwrap();
        assert_eq!(a.first("b").unwrap().text(), "x");
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(unescape("a &lt; b &#x4E00; &#20108; &foo; &"), "a < b 一 二 &foo; &");
    }
}
