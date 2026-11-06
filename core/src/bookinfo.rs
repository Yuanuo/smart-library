//! Hand-made book metadata (design-spec A.5): 典类, 朝代, 作译者 for ~4800
//! books, from the old implementation, exported to `data/bookinfo.json`.
//! Embedded in the binary so that facets do not depend on the working directory.

use std::collections::HashMap;
use std::sync::OnceLock;

const JSON: &str = include_str!("../../data/bookinfo.json");

#[derive(Debug, Clone, Default)]
pub struct BookInfo {
    /// e.g. `经`, `论：经释`
    pub kind_label: String,
    /// e.g. `["唐"]`
    pub periods: Vec<String>,
    /// e.g. `["玄奘"]`
    pub authors: Vec<String>,
}

/// Book id -> info. Parsed once.
pub fn bookinfo() -> &'static HashMap<String, BookInfo> {
    static MAP: OnceLock<HashMap<String, BookInfo>> = OnceLock::new();
    MAP.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(JSON).unwrap_or(serde_json::Value::Null);
        let mut map = HashMap::new();
        if let Some(obj) = v.as_object() {
            for (id, e) in obj {
                let strings = |key: &str| -> Vec<String> {
                    e.get(key)
                        .and_then(|a| a.as_array())
                        .map(|a| a.iter().filter_map(|s| s.as_str()).map(|s| s.to_string()).collect())
                        .unwrap_or_default()
                };
                map.insert(
                    id.clone(),
                    BookInfo {
                        kind_label: e.get("kind_label").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                        periods: strings("periods"),
                        authors: strings("authors"),
                    },
                );
            }
        }
        map
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_embedded_bookinfo() {
        let m = bookinfo();
        assert_eq!(m.len(), 4826);
        let a = &m["A1057"];
        assert_eq!(a.kind_label, "音译");
        assert_eq!(a.periods, vec!["唐"]);
        assert_eq!(a.authors, vec!["慧菀"]);
    }
}
