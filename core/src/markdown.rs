//! A small Markdown subset to the HTML whitelist of `article.rs`: headings,
//! paragraphs, bullet and numbered lists, block quotes, fenced code,
//! `**bold**`, `*italic*`. Anything else stays plain text. Not a full
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

pub fn to_html(md: &str) -> String {
    let mut out = String::from("<html><body>");
    let mut list: Option<&str> = None;
    let mut code = false;
    let close_list = |out: &mut String, list: &mut Option<&str>| {
        if let Some(t) = list.take() {
            out.push_str(&format!("</{}>", t));
        }
    };
    for raw in md.lines() {
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
            out.push_str(&format!("<h{0}>{1}</h{0}>", hashes, inline(&esc(t[hashes..].trim()))));
        } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            if list != Some("ul") {
                close_list(&mut out, &mut list);
                out.push_str("<ul>");
                list = Some("ul");
            }
            out.push_str(&format!("<li>{}</li>", inline(&esc(rest.trim()))));
        } else if let Some(rest) = numbered(t) {
            if list != Some("ol") {
                close_list(&mut out, &mut list);
                out.push_str("<ol>");
                list = Some("ol");
            }
            out.push_str(&format!("<li>{}</li>", inline(&esc(rest.trim()))));
        } else {
            close_list(&mut out, &mut list);
            let body = t.strip_prefix('>').map(|r| r.trim()).unwrap_or(t);
            out.push_str(&format!("<p>{}</p>", inline(&esc(body))));
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
        assert!(h.contains("<p>引用</p>") && h.contains("<p>code</p>"));
    }

    #[test]
    fn lone_marks_stay_text() {
        assert!(to_html("a * b").contains("<p>a * b</p>"));
        assert!(to_html("**x").contains("<p>**x</p>"));
    }
}
