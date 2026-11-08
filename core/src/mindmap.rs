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

#[cfg(test)]
mod tests {
    use super::*;

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
