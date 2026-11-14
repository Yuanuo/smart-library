//! Mind map notes: the note's content is the data of Mind-Elixir
//! (`{"nodeData": {"id", "topic", "children": […], "hyperLink"?, "tags"?}, …}`),
//! its `content_type` is `mindmap`. This module reads that JSON for the
//! things that need text: the search index and the export.

use serde_json::{json, Value};

/// The `content_type` of a mind map note.
pub const TYPE: &str = "mindmap";

/// The data of a new map with only its root.
pub fn new_data(title: &str) -> String {
    json!({ "nodeData": { "id": "root", "topic": title, "root": true, "children": [] } }).to_string()
}

/// `(depth, topic, hyperLink)` of every node, depth first, the root at depth 0.
pub fn nodes(content: &str) -> Vec<(usize, String, Option<String>)> {
    fn walk(n: &Value, depth: usize, out: &mut Vec<(usize, String, Option<String>)>) {
        let topic = n["topic"].as_str().unwrap_or("").replace(['\r', '\n'], " ");
        out.push((depth, topic, n["hyperLink"].as_str().map(str::to_string)));
        if let Some(kids) = n["children"].as_array() {
            for k in kids {
                walk(k, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(content) {
        if v["nodeData"].is_object() {
            walk(&v["nodeData"], 0, &mut out);
        }
    }
    out
}

/// All the topics as text, one per line (what the index gets).
pub fn text(content: &str) -> String {
    nodes(content).into_iter().map(|(_, t, _)| t).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// The map as an indented Markdown list; links to places in a document
/// (`smartlib://`) stay links.
pub fn to_markdown(content: &str) -> String {
    let mut s = String::new();
    for (depth, topic, link) in nodes(content) {
        let t = match link.filter(|l| l.starts_with("smartlib://")) {
            Some(l) => format!("[{}]({})", topic.replace(['[', ']'], ""), l),
            None => topic,
        };
        s.push_str(&format!("{}- {}\n", "  ".repeat(depth), t));
    }
    s
}

fn new_id(seed: &str) -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in seed.bytes().chain(t.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

/// A new child of the root (a clipping): its topic, the address of its
/// source as the link, and a tag with where it is from. The new data.
pub fn add_clip(content: &str, topic: &str, link: &str, tag: &str) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(content).map_err(|e| format!("脑图数据不对：{}", e))?;
    let root = v.get_mut("nodeData").filter(|n| n.is_object()).ok_or("脑图数据不对：没有 nodeData")?;
    let mut node = json!({ "id": new_id(topic), "topic": topic });
    if !link.is_empty() {
        node["hyperLink"] = json!(link);
    }
    if !tag.is_empty() {
        node["tags"] = json!([tag]);
    }
    match root.get_mut("children").and_then(|c| c.as_array_mut()) {
        Some(kids) => kids.push(node),
        None => root["children"] = json!([node]),
    }
    Ok(v.to_string())
}

fn strip_fences(raw: &str) -> &str {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix("```") {
        if let Some(nl) = rest.find('\n') {
            let body = &rest[nl + 1..];
            if let Some(end) = body.rfind("```") {
                return body[..end].trim();
            }
        }
    }
    s
}

fn ensure_node_ids(n: &mut Value, seq: &mut usize, is_root: bool) {
    *seq += 1;
    let topic = n.get("topic").and_then(Value::as_str).unwrap_or("节点").to_string();
    if n.get("id").and_then(Value::as_str).map_or(true, str::is_empty) {
        n["id"] = json!(if is_root { "root".to_string() } else { new_id(&format!("{}-{}", topic, *seq)) });
    }
    if is_root {
        n["root"] = json!(true);
    }
    if let Some(kids) = n.get_mut("children").and_then(Value::as_array_mut) {
        for k in kids {
            ensure_node_ids(k, seq, false);
        }
    } else if is_root {
        n["children"] = json!([]);
    }
}

fn parse_link_topic(raw: &str) -> (String, Option<String>) {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix('[') {
        if let Some((label, tail)) = rest.split_once("](") {
            if let Some(url) = tail.strip_suffix(')') {
                if !label.trim().is_empty() && !url.trim().is_empty() {
                    return (label.trim().to_string(), Some(url.trim().to_string()));
                }
            }
        }
    }
    (s.to_string(), None)
}

/// Convert a Markdown indented list / heading outline OR a Mind-Elixir JSON
/// string into valid Mind-Elixir JSON (`{"nodeData": ...}`).
pub fn from_outline_or_json(input: &str, fallback_title: &str) -> Result<String, String> {
    let clean = strip_fences(input);
    if clean.starts_with('{') {
        let mut v: Value = serde_json::from_str(clean).map_err(|e| format!("脑图 JSON 解析失败：{}", e))?;
        let mut seq = 0usize;
        if v.get("nodeData").map_or(false, Value::is_object) {
            ensure_node_ids(&mut v["nodeData"], &mut seq, true);
            return Ok(v.to_string());
        } else if v.get("topic").is_some() {
            ensure_node_ids(&mut v, &mut seq, true);
            return Ok(json!({ "nodeData": v }).to_string());
        }
    }

    // Parse Markdown headings (`# ...`) and indented list items (`- ...` / `* ...` / `1. ...`)
    let mut items: Vec<(usize, String, Option<String>)> = Vec::new();
    let mut base_heading_depth = 0usize;
    for line in clean.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.trim().is_empty() {
            continue;
        }
        let trimmed = trimmed_end.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with('#') {
            let hashes = trimmed.chars().take_while(|&c| c == '#').count();
            let text = trimmed[hashes..].trim();
            if !text.is_empty() {
                base_heading_depth = hashes;
                let (t, l) = parse_link_topic(text);
                items.push((hashes.saturating_sub(1), t, l));
                continue;
            }
        }
        let bullet_rest = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
            .or_else(|| {
                let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 {
                    trimmed[digits..].strip_prefix(". ").or_else(|| trimmed[digits..].strip_prefix("、"))
                } else {
                    None
                }
            });
        if let Some(rest) = bullet_rest {
            let text = rest.trim();
            if !text.is_empty() {
                let list_depth = indent / 2;
                let depth = if base_heading_depth > 0 { base_heading_depth + list_depth } else { list_depth };
                let (t, l) = parse_link_topic(text);
                items.push((depth, t, l));
            }
        } else if items.is_empty() {
            let (t, l) = parse_link_topic(trimmed);
            items.push((0, t, l));
        }
    }

    if items.is_empty() {
        return Err("没有可转换成脑图的内容".into());
    }

    let min_depth = items.iter().map(|(d, _, _)| *d).min().unwrap_or(0);
    for (d, _, _) in &mut items {
        *d -= min_depth;
    }

    let top_count = items.iter().filter(|(d, _, _)| *d == 0).count();
    if top_count > 1 {
        for (d, _, _) in &mut items {
            *d += 1;
        }
        let title = if fallback_title.trim().is_empty() { "主题" } else { fallback_title.trim() };
        items.insert(0, (0, title.to_string(), None));
    }

    let mut stack: Vec<(usize, Value)> = Vec::new();
    for (idx, (depth, topic, link)) in items.into_iter().enumerate() {
        let mut node = if idx == 0 {
            json!({ "id": "root", "topic": topic, "root": true, "children": [] })
        } else {
            json!({ "id": new_id(&format!("{}-{}", topic, idx)), "topic": topic, "children": [] })
        };
        if let Some(l) = link {
            node["hyperLink"] = json!(l);
        }
        while stack.len() > 1 && stack.last().unwrap().0 >= depth {
            let (_, child) = stack.pop().unwrap();
            let parent = &mut stack.last_mut().unwrap().1;
            parent["children"].as_array_mut().unwrap().push(child);
        }
        stack.push((depth, node));
    }
    while stack.len() > 1 {
        let (_, child) = stack.pop().unwrap();
        let parent = &mut stack.last_mut().unwrap().1;
        parent["children"].as_array_mut().unwrap().push(child);
    }
    let root = stack.pop().unwrap().1;
    Ok(json!({ "nodeData": root }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_markdown_outline_to_mindmap() {
        let md = "- 核心概念\n  - 分支甲\n    - 细节1\n  - [分支乙](smartlib://open?lib=cbeta&path=x)\n";
        let json_str = from_outline_or_json(md, "备用标题").unwrap();
        let parsed = nodes(&json_str);
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0], (0, "核心概念".into(), None));
        assert_eq!(parsed[1], (1, "分支甲".into(), None));
        assert_eq!(parsed[2], (2, "细节1".into(), None));
        assert_eq!(parsed[3], (1, "分支乙".into(), Some("smartlib://open?lib=cbeta&path=x".into())));
    }

    #[test]
    fn reads_text_and_outline() {
        let data = r#"{"nodeData":{"id":"r","topic":"专题","children":[
            {"id":"a","topic":"如是我聞","hyperLink":"smartlib://open?lib=cbeta&path=x","children":[{"id":"b","topic":"子\n节点"}]},
            {"id":"c","topic":"乙"}]}}"#;
        assert_eq!(text(data), "专题\n如是我聞\n子 节点\n乙");
        assert_eq!(
            to_markdown(data),
            "- 专题\n  - [如是我聞](smartlib://open?lib=cbeta&path=x)\n    - 子 节点\n  - 乙\n"
        );
        assert_eq!(text("not json"), "");
    }

    #[test]
    fn adds_a_clipping_under_the_root() {
        let d = add_clip(&new_data("题"), "摘录的文字", "smartlib://open?lib=a", "《经》 p.1").unwrap();
        let n = nodes(&d);
        assert_eq!(n.len(), 2);
        assert_eq!(n[1], (1, "摘录的文字".to_string(), Some("smartlib://open?lib=a".to_string())));
        assert!(d.contains("《经》 p.1"));
        assert!(add_clip("{}", "x", "", "").is_err());
    }
}
