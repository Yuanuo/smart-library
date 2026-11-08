//! A small Markdown subset to the HTML whitelist of `article.rs`: headings,
//! paragraphs, bullet and numbered lists, block quotes, fenced code,
//! `**bold**`, `*italic*`, and a link to a place in a document,
//! `[text](smartlib://open?lib=…&path=…&lb=…)` (how a clipping points back to
//! its source), and footnotes `[^1]` with `[^1]: text` lines (shown as hover
//! notes). Anything else stays plain text. Not a full
//! CommonMark implementation: notes are read through the paragraph view, so
//! only the structure that view shows matters.

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `**bold**` and `*italic*` of an already escaped line.
fn inline(s: &str) -> String {
    fn pair(s: &str, mark: &str, tag: &str) -> String {
        let parts: Vec<&str> = s.split(mark).collect();
        // an odd number of marks leaves the last one as text
        let pairs = (parts.len() - 1) / 2 * 2;
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                if i <= pairs {
                    out.push_str(&if i % 2 == 1 { format!("<{}>", tag) } else { format!("</{}>", tag) });
                } else {
                    // an unpaired mark stays text; hidden from the next pass
                    out.push_str(&mark.replace('*', "\u{1}"));
                }
            }
            out.push_str(p);
        }
        out
    }
    pair(&pair(s, "**", "b"), "*", "i").replace('\u{1}', "*")
}

/// `[text](smartlib://…)` of an already escaped line -> `<a data-src="…">text</a>`.
/// Only that scheme is linked; `*` in the address is kept as `%2A` so that the
/// bold / italic pass leaves it alone.
fn links(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find("](smartlib://") {
            let text = &after[..close];
            let url_at = close + 2;
            if !text.contains('[') {
                if let Some(end) = after[url_at..].find(')') {
                    let url = &after[url_at..url_at + end];
                    if !url.contains(['"', ' ', '<']) {
                        out.push_str(&rest[..open]);
                        out.push_str(&format!("<a data-src=\"{}\">{}</a>", url.replace('*', "%2A"), text));
                        rest = &after[url_at + end + 1..];
                        continue;
                    }
                }
            }
        }
        out.push_str(&rest[..=open]);
        rest = &rest[open + 1..];
    }
    out.push_str(rest);
    out
}

type Defs = std::collections::BTreeMap<String, String>;

/// Percent-encoding of everything but `A-Za-z0-9-_.~` (the form the HTML
/// footnotes of the `a/` trees use: `data-note`).
fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// `[^id: text` -> (id, text) for a definition line `[^id]: text`.
fn definition(line: &str) -> Option<(String, String)> {
    let t = line.trim_start().strip_prefix("[^")?;
    let (id, rest) = t.split_once("]:")?;
    (!id.is_empty() && !id.contains(['[', ']', ' '])).then(|| (id.to_string(), rest.trim().to_string()))
}

/// `[^id]` of an already escaped line -> a footnote anchor (when it is defined).
fn refs(s: &str, defs: &Defs) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find("[^") {
        let after = &rest[at + 2..];
        if let Some(end) = after.find(']') {
            if let Some(text) = defs.get(&after[..end]).filter(|t| !t.is_empty()) {
                out.push_str(&rest[..at]);
                out.push_str(&format!("<a data-note=\"{}\"></a>", pct(&unesc(text))));
                rest = &after[end + 1..];
                continue;
            }
        }
        out.push_str(&rest[..at + 2]);
        rest = after;
    }
    out.push_str(rest);
    out
}

fn unesc(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// One line of text: footnotes, links, bold and italic.
fn line(s: &str, defs: &Defs) -> String {
    inline(&links(&refs(&esc(s), defs)))
}

pub fn to_html(md: &str) -> String {
    let defs: Defs = md.lines().filter_map(definition).collect();
    let mut out = String::from("<html><body>");
    let mut list: Option<&str> = None;
    let mut code = false;
    let close_list = |out: &mut String, list: &mut Option<&str>| {
        if let Some(t) = list.take() {
            out.push_str(&format!("</{}>", t));
        }
    };
    for raw in md.lines() {
        if definition(raw).is_some() {
            continue;
        }
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            code = !code;
            continue;
        }
        if code {
            if !line.trim().is_empty() {
                close_list(&mut out, &mut list);
                out.push_str(&format!("<p>{}</p>", esc(line.trim())));
            }
            continue;
        }
        let t = line.trim_start();
        if t.is_empty() {
            close_list(&mut out, &mut list);
            continue;
        }
        let hashes = t.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
            close_list(&mut out, &mut list);
            out.push_str(&format!("<h{0}>{1}</h{0}>", hashes, self::line(t[hashes..].trim(), &defs)));
        } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            if list != Some("ul") {
                close_list(&mut out, &mut list);
                out.push_str("<ul>");
                list = Some("ul");
            }
            out.push_str(&format!("<li>{}</li>", self::line(rest.trim(), &defs)));
        } else if let Some(rest) = numbered(t) {
            if list != Some("ol") {
                close_list(&mut out, &mut list);
                out.push_str("<ol>");
                list = Some("ol");
            }
            out.push_str(&format!("<li>{}</li>", self::line(rest.trim(), &defs)));
        } else {
            close_list(&mut out, &mut list);
            if let Some(q) = t.strip_prefix('>') {
                // a quoted line is a paragraph of its own; an empty one only separates
                if !q.trim().is_empty() {
                    out.push_str(&format!("<blockquote>{}</blockquote>", self::line(q.trim(), &defs)));
                }
            } else {
                out.push_str(&format!("<p>{}</p>", self::line(t, &defs)));
            }
        }
    }
    close_list(&mut out, &mut list);
    out.push_str("</body></html>");
    out
}

/// `12. text` -> `text`
fn numbered(t: &str) -> Option<&str> {
    let n = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    t[n..].strip_prefix(". ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_blocks_and_inline() {
        let h = to_html("# 标题\n正文 **粗** 和 *斜* 和 a<b\n\n- 一\n- 二\n1. 甲\n> 引用\n```\ncode\n```");
        assert!(h.contains("<h1>标题</h1>"));
        assert!(h.contains("<p>正文 <b>粗</b> 和 <i>斜</i> 和 a&lt;b</p>"));
        assert!(h.contains("<ul><li>一</li><li>二</li></ul>"));
        assert!(h.contains("<ol><li>甲</li></ol>"));
        assert!(h.contains("<blockquote>引用</blockquote>") && h.contains("<p>code</p>"));
    }

    #[test]
    fn links_back_to_a_source() {
        let h = to_html("> 引文
>
> —— [《经》 · 0001a05](smartlib://open?lib=cbeta&path=XML%2Fa.xml&q=a*b) 好");
        assert!(h.contains("<blockquote>引文</blockquote>"), "{h}");
        assert!(h.contains("<a data-src=\"smartlib://open?lib=cbeta&amp;path=XML%2Fa.xml&amp;q=a%2Ab\">《经》 · 0001a05</a> 好"), "{h}");
        // other schemes are not linked
        assert!(to_html("[x](http://a.b)").contains("[x](http://a.b)"));
    }

    #[test]
    fn lone_marks_stay_text() {
        assert!(to_html("a * b").contains("<p>a * b</p>"));
        assert!(to_html("**x").contains("<p>**x</p>"));
    }
}
