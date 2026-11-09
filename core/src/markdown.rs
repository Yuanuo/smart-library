//! A small Markdown subset to the HTML whitelist of `article.rs`: headings,
//! paragraphs, bullet and numbered lists, block quotes, fenced code,
//! tables (every row is a paragraph of its own, the cells apart by ` | `),
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
    (!id.is_empty() && !id.contains(['[', ']', ' '])).then(|| (id.to_string(), restore(&unbackslash(rest.trim()))))
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

/// `![alt](attach:name)` of an already escaped line -> `<img src="attach:name">`
/// (only names made by `attach::store`).
fn images(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find("![") {
        let after = &rest[at + 2..];
        if let Some(close) = after.find("](attach:") {
            let name_at = close + 9;
            if let Some(end) = after[name_at..].find(')') {
                // `![alt](attach:name "title")`: the title (after a space) is left out
                let name = after[name_at..name_at + end].split(' ').next().unwrap_or("");
                if crate::attach::is_name(name) && !after[..close].contains('[') {
                    out.push_str(&rest[..at]);
                    out.push_str(&format!("<img src=\"attach:{}\">", name));
                    rest = &after[name_at + end + 1..];
                    continue;
                }
            }
        }
        out.push_str(&rest[..at + 2]);
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Backslash escapes (`\*`, `\[`, `\&`, `\_` … as editors write them): the
/// backslash goes; `*`, `[` and `]` are kept apart (`\u{1}`–`\u{3}`) so that
/// they are not read as marks, see [`restore`].
fn unbackslash(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(&n) = it.peek() {
                if n.is_ascii_punctuation() {
                    it.next();
                    out.push(match n {
                        '*' => '\u{1}',
                        '[' => '\u{2}',
                        ']' => '\u{3}',
                        o => o,
                    });
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

fn restore(s: &str) -> String {
    s.replace('\u{1}', "*").replace('\u{2}', "[").replace('\u{3}', "]")
}

/// One line of text: images, footnotes, links, bold and italic.
fn line(s: &str, defs: &Defs) -> String {
    restore(&inline(&links(&refs(&images(&esc(&unbackslash(s))), defs))))
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
        // a backslash at the end is a hard break of the visual editor
        let line = match line.strip_suffix('\\') {
            Some(l) if !l.ends_with('\\') => l.trim_end(),
            _ => line,
        };
        if line.trim_start().starts_with("```") {
            code = !code;
            continue;
        }
        // `$$` around a formula block is only a mark; the formula's lines are text
        if line.trim() == "$$" {
            continue;
        }
        if code {
            if !line.trim().is_empty() {
                close_list(&mut out, &mut list);
                out.push_str(&format!("<p>{}</p>", esc(line.trim())));
            }
            continue;
        }
        // `<br>` is a line break of HTML in Markdown (the visual editor writes it for an empty line): a space here
        let no_br = without_br(line);
        let t = no_br.trim_start();
        if t.is_empty() {
            close_list(&mut out, &mut list);
            continue;
        }
        // a table row `| a | b |`; the line under the header (`| - | - |`) says nothing
        if t.len() > 1 && t.starts_with('|') && t.ends_with('|') {
            close_list(&mut out, &mut list);
            if !t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                let cells: Vec<String> = t[1..t.len() - 1].split('|').map(|c| self::line(c.trim(), &defs)).collect();
                out.push_str(&format!("<p>{}</p>", cells.join(" | ")));
            }
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

/// The line without `<br>`, `<br/>` and `<br />` (any case): each one is a space.
fn without_br(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find('<') {
        let tail = &rest[at..];
        let lower: String = tail.chars().take(6).collect::<String>().to_ascii_lowercase();
        let len = ["<br />", "<br/>", "<br>"].iter().find(|p| lower.starts_with(*p)).map(|p| p.len());
        match len {
            Some(n) => {
                out.push_str(&rest[..at]);
                out.push(' ');
                rest = &tail[n..];
            }
            None => {
                out.push_str(&rest[..at + 1]);
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
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
    fn an_image_with_a_title_is_still_an_image() {
        let n = "5c990784e96425a5f842aef07429950c.png";
        let h = to_html(&format!("![1.00](attach:{} \"aa\")
![](attach:{}12345678.png)", n, &n[..32]));
        assert!(h.contains(&format!("<img src=\"attach:{}\">", n)), "{h}");
        assert!(h.contains(&format!("attach:{}12345678.png", &n[..32])), "{h}");
        assert!(!h.contains("![") && !h.contains("\"aa\""), "{h}");
    }

    #[test]
    fn br_is_a_line_break_not_text() {
        let h = to_html("text is ok\n\n<br />\n\n* 1\n* 2\n\n<br>\n甲<BR/>乙 a<b");
        assert!(!h.contains("&lt;br") && !h.contains("&lt;BR"), "{h}");
        assert!(h.contains("<p>text is ok</p>") && h.contains("<ul><li>1</li><li>2</li></ul>"), "{h}");
        assert!(h.contains("<p>甲 乙 a&lt;b</p>"), "{h}");
    }

    #[test]
    fn table_rows_are_paragraphs() {
        let h = to_html("| 名 | 义 |
| - | - |
| **甲** | 一 |

```js
let a = 1;
```");
        assert!(h.contains("<p>名 | 义</p><p><b>甲</b> | 一</p>"), "{h}");
        assert!(!h.contains("- | -") && h.contains("<p>let a = 1;</p>"), "{h}");
    }

    #[test]
    fn reads_what_the_visual_editor_writes() {
        // `\&` in an address, escaped marks, a hard break as a backslash
        let h = to_html("> [出处](smartlib://open?lib=a\\&book=b)\\\n下一行 \\*不是粗体\\* \\[方括号\\]\n\n* 项\n\n[^1]: 注 \\*x\\*\n文[^1]");
        assert!(h.contains("<a data-src=\"smartlib://open?lib=a&amp;book=b\">出处</a>"), "{h}");
        assert!(h.contains("<p>下一行 *不是粗体* [方括号]</p>"), "{h}");
        assert!(h.contains("<li>项</li>"), "{h}");
        assert!(h.contains("data-note=\"%E6%B3%A8%20%2Ax%2A\""), "{h}");
    }

    #[test]
    fn pasted_pictures_become_images() {
        let n = "0123456789abcdef0123456789abcdef.png";
        let h = to_html(&format!("见图\n![截图](attach:{})\n![x](attach:../a.png)", n));
        assert!(h.contains(&format!("<p><img src=\"attach:{}\"></p>", n)), "{h}");
        assert!(h.contains("![x](attach:../a.png)"), "{h}");
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
