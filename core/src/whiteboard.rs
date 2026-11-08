//! Whiteboard notes: the note's content is the scene of Excalidraw
//! (`{"type":"excalidraw","version":2,"elements":[…],"appState":{…},"files":{…}}`),
//! its `content_type` is `whiteboard`. This module reads that JSON for the
//! things that need text: the search index and the export.

use serde_json::{json, Value};

/// The `content_type` of a whiteboard note.
pub const TYPE: &str = "whiteboard";

/// The scene of a new, empty board.
pub fn new_data() -> String {
    json!({ "type": "excalidraw", "version": 2, "source": "smart-library", "elements": [], "appState": {}, "files": {} }).to_string()
}

/// The texts on the board (text elements that are not deleted), top to bottom.
pub fn texts(content: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(content) else {
        return Vec::new();
    };
    let mut found: Vec<(f64, f64, String)> = v["elements"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter(|e| e["type"] == "text" && e["isDeleted"] != true)
        .filter_map(|e| {
            let t = e["text"].as_str()?.trim();
            (!t.is_empty()).then(|| (e["y"].as_f64().unwrap_or(0.0), e["x"].as_f64().unwrap_or(0.0), t.to_string()))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    found.into_iter().map(|(_, _, t)| t).collect()
}

/// All the texts, one per line (what the index gets).
pub fn text(content: &str) -> String {
    texts(content).join("\n")
}

/// The texts as a Markdown list (the readable part of an export).
pub fn to_markdown(content: &str) -> String {
    texts(content).iter().map(|t| format!("- {}\n", t.replace('\n', " "))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_texts_in_reading_order() {
        let data = r#"{"type":"excalidraw","elements":[
            {"type":"text","x":10,"y":50,"text":"下"},
            {"type":"rectangle","x":0,"y":0},
            {"type":"text","x":0,"y":5,"text":"上\n行"},
            {"type":"text","x":0,"y":9,"text":"已删","isDeleted":true},
            {"type":"text","x":99,"y":50,"text":"右"}]}"#;
        assert_eq!(texts(data), vec!["上\n行", "下", "右"]);
        assert_eq!(to_markdown(data), "- 上 行\n- 下\n- 右\n");
        assert_eq!(text("not json"), "");
        assert_eq!(text(&new_data()), "");
    }
}
