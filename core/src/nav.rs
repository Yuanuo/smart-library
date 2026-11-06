//! Navigation trees: `menu_nav.xhtml` (list of trees) and `*_nav.xhtml` (the trees).
//!
//! The files are only *almost* well formed: the first node of a `<nav>` is not
//! wrapped in `<li>`. Like the original Java parser we therefore treat the
//! `<nav>` element itself as the first item, followed by its direct `<li>` children.

use crate::markup::{self, Element};

#[derive(Debug, Clone)]
pub enum NavNode {
    /// A node with a `<span>` title and an optional `<ol>` of children.
    Folder { title: String, children: Vec<NavNode> },
    /// A node with a link (`cblink`, `navlink`, `a` ...).
    Leaf {
        href: String,
        text: String,
        /// value of the `i` attribute, if any (an id used by some entries)
        id: Option<String>,
        /// Usually empty. A link can also have an `<ol>` of children
        /// (`<li><a href=..>菩萨戒本</a><ol>..</ol></li>` in a user-made tree):
        /// a node that has content *and* children.
        children: Vec<NavNode>,
    },
}

#[derive(Debug, Clone)]
pub struct NavDoc {
    pub nav_type: String,
    pub han_lang: String,
    pub items: Vec<NavNode>,
}

impl NavDoc {
    /// All leaves as (href, text), in document order.
    pub fn leaves(&self) -> Vec<(&str, &str)> {
        fn walk<'a>(n: &'a NavNode, out: &mut Vec<(&'a str, &'a str)>) {
            match n {
                NavNode::Folder { children, .. } => {
                    for c in children {
                        walk(c, out);
                    }
                }
                NavNode::Leaf { href, text, children, .. } => {
                    out.push((href.as_str(), text.as_str()));
                    for c in children {
                        walk(c, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        for n in &self.items {
            walk(n, &mut out);
        }
        out
    }
}

pub fn parse_nav(xml: &str) -> Option<NavDoc> {
    let root = markup::parse(xml);
    let nav = root.find_descendant("nav")?;
    let mut items = Vec::new();
    if let Some(n) = item_to_node(nav) {
        items.push(n);
    }
    for li in nav.all("li") {
        if let Some(n) = item_to_node(li) {
            items.push(n);
        }
    }
    Some(NavDoc {
        nav_type: nav.attr("type").unwrap_or("").to_string(),
        han_lang: nav.attr("han-lang").unwrap_or("zh-tw").to_string(),
        items,
    })
}

fn item_to_node(el: &Element) -> Option<NavNode> {
    if let Some(span) = el.first("span") {
        let title = match span.attr("t") {
            Some(t) => t.to_string(),
            None => clean_nav_title(&span.text()),
        };
        let children: Vec<NavNode> = match el.first("ol") {
            Some(ol) => ol.all("li").filter_map(item_to_node).collect(),
            None => Vec::new(),
        };
        return Some(NavNode::Folder { title, children });
    }
    let link = el.first_with_attr("href")?;
    let children: Vec<NavNode> = match el.first("ol") {
        Some(ol) => ol.all("li").filter_map(item_to_node).collect(),
        None => Vec::new(),
    };
    Some(NavNode::Leaf {
        href: link.attr("href").unwrap_or("").to_string(),
        text: link.text(),
        id: link.attr("i").map(|s| s.to_string()),
        children,
    })
}

/// Port of `BookHelper.parseNavCatalogInfo`: folder titles carry machine
/// tokens such as `01 阿含部類 T01-02,25,33 etc.`; keep only the readable part.
/// (Splitting on parentheses is an assumption, the Java helper is not available.)
pub fn clean_nav_title(text: &str) -> String {
    if !text.contains(' ') {
        return text.to_string();
    }
    let replaced = text
        .replace(", ", ",,")
        .replace("etc.", " ")
        .replace("  ", " ");
    let mut result = String::new();
    for group in replaced.split('／') {
        let mut buf = String::new();
        for tok in group.split(|c: char| c == ' ' || c == '(' || c == ')') {
            let first = match tok.chars().next() {
                Some(c) => c,
                None => continue,
            };
            if first.is_ascii_alphabetic() || first == '=' {
                continue;
            }
            if buf.is_empty() && first.is_ascii_digit() {
                continue;
            }
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(tok);
        }
        if !buf.is_empty() {
            if !result.is_empty() {
                result.push('／');
            }
            result.push_str(&buf);
        }
    }
    if result.is_empty() {
        text.to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html><head><meta charset="UTF-8" /><title>目次</title></head><body>
<nav type="catalog">
	<span>大正新脩大藏經</span>
	<ol>
		<li><span>01 阿含部類 T01-02,25,33 etc.</span>
		<ol>
			<li><span>T0001-25 長阿含經 etc. T01</span>
			<ol>
				<li><cblink href="XML/T/T01/T01n0001_001.xml">T0001 長阿含經</cblink></li>
				<li><span>T0002-25 長阿含經單本</span>
				<ol>
					<li><cblink href="XML/T/T01/T01n0004_001.xml">T0004 七佛父母姓字經</cblink></li>
				</ol></li>
			</ol></li>
		</ol></li>
	</ol>
</nav></body></html>"#;

    const MENU: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html><head><meta charset="UTF-8" /><title>目次</title></head><body>
<nav type="catalog" default="zhibei">
		<navlink href="simple_nav.xhtml">大藏经 · 簡易目錄</navlink>
		<li><navlink href="advance_nav.xhtml">大藏经 · 進階目錄</navlink></li>
		<li><navlink href="bulei_nav.xhtml">大藏经 · 部類目錄</navlink></li>
</nav></body></html>"#;

    #[test]
    fn cleans_nav_titles() {
        assert_eq!(clean_nav_title("01 阿含部類 T01-02,25,33 etc."), "阿含部類");
        assert_eq!(clean_nav_title("T0001-25 長阿含經 etc. T01"), "長阿含經");
        assert_eq!(clean_nav_title("T0002-25 長阿含經單本"), "長阿含經單本");
        assert_eq!(clean_nav_title("大正新脩大藏經"), "大正新脩大藏經");
    }

    #[test]
    fn parses_tree_with_unwrapped_first_node() {
        let doc = parse_nav(SIMPLE).unwrap();
        assert_eq!(doc.nav_type, "catalog");
        assert_eq!(doc.items.len(), 1);
        match &doc.items[0] {
            NavNode::Folder { title, children } => {
                assert_eq!(title.as_str(), "大正新脩大藏經");
                assert_eq!(children.len(), 1);
                match &children[0] {
                    NavNode::Folder { title, .. } => assert_eq!(title.as_str(), "阿含部類"),
                    _ => panic!("folder expected"),
                }
            }
            _ => panic!("folder expected"),
        }
        let leaves = doc.leaves();
        assert_eq!(leaves.len(), 2);
        assert_eq!(leaves[0].0, "XML/T/T01/T01n0001_001.xml");
        assert_eq!(leaves[1].1, "T0004 七佛父母姓字經");
    }

    #[test]
    fn keeps_children_of_a_link_node() {
        let doc = parse_nav(include_str!("../../samples/cbeta/longzang_nav_fragment.xhtml")).unwrap();
        assert_eq!(doc.han_lang, "hans");
        let leaves = doc.leaves();
        let texts: Vec<&str> = leaves.iter().map(|l| l.1).collect();
        assert_eq!(texts, vec!["菩萨戒本", "菩萨戒本", "菩萨戒羯磨序", "菩萨戒羯磨文"]);
        match &doc.items[0] {
            NavNode::Folder { children, .. } => match &children[0] {
                NavNode::Leaf { href, id, children, .. } => {
                    assert_eq!(href.as_str(), "a/LZ/大乘律/菩萨戒本.html");
                    assert_eq!(id.as_deref(), Some("LZa_3895"));
                    assert_eq!(children.len(), 2);
                }
                _ => panic!("link node expected"),
            },
            _ => panic!("folder expected"),
        }
    }

    #[test]
    fn parses_menu_with_unwrapped_first_link() {
        let doc = parse_nav(MENU).unwrap();
        let leaves = doc.leaves();
        assert_eq!(leaves.len(), 3);
        assert_eq!(leaves[0].0, "simple_nav.xhtml");
        assert_eq!(leaves[1].0, "advance_nav.xhtml");
        assert_eq!(leaves[2].0, "bulei_nav.xhtml");
    }
}
