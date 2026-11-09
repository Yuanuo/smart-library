//! Knowledge libraries inside a bookcase (design-spec 9a).
//!
//! A bookcase holds one CBETA data set, shown by three official trees
//! (簡易, 進階, 部類), plus any user-made trees whose leaves point to HTML
//! articles under `a/<library>/`. Which trees share data is decided by where
//! their leaves point, not by file names:
//!
//! * leaves into `XML/` -> the CBETA data: only **one** of these trees can be
//!   bound (same data, same index);
//! * leaves into `a/<code>/` -> an HTML library of its own: each can be bound
//!   next to the others, with its own index.
//!
//! This module produces plain [`IndexDoc`]s; Tantivy stays in `search.rs`.

use crate::article::read_article;
use crate::bookcase::Bookcase;
use crate::bookinfo::bookinfo;
use crate::nav::{NavDoc, NavNode};
use crate::text::decode_auto;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeGroup {
    /// the CBETA volumes (`XML/…`)
    Cbeta,
    /// HTML articles under `a/<code>/`
    Html(String),
    /// nothing readable (empty tree, only help pages …)
    Other,
}

#[derive(Debug, Clone)]
pub struct TreeChoice {
    pub id: String,
    pub title: String,
    pub group: TreeGroup,
    /// number of links in the tree
    pub leaves: usize,
}

/// The trees of a bookcase with the data each one shows.
pub fn classify_trees(bc: &Bookcase) -> Vec<TreeChoice> {
    bc.trees
        .iter()
        .map(|t| {
            let (group, leaves) = match bc.nav(&t.id) {
                Ok(doc) => {
                    let leaves = doc.leaves();
                    let mut count: BTreeMap<String, usize> = BTreeMap::new();
                    for (href, _) in &leaves {
                        let key = if href.starts_with("XML/") {
                            "XML".to_string()
                        } else if let Some(rest) = href.strip_prefix("a/") {
                            format!("a/{}", rest.split('/').next().unwrap_or(""))
                        } else {
                            continue; // help/ pages and the like
                        };
                        *count.entry(key).or_default() += 1;
                    }
                    let group = match count.into_iter().max_by_key(|(_, n)| *n) {
                        Some((k, _)) if k == "XML" => TreeGroup::Cbeta,
                        Some((k, _)) => TreeGroup::Html(k.trim_start_matches("a/").to_string()),
                        None => TreeGroup::Other,
                    };
                    (group, leaves.len())
                }
                Err(_) => (TreeGroup::Other, 0),
            };
            TreeChoice {
                id: t.id.clone(),
                title: t.title.clone(),
                group,
                leaves,
            }
        })
        .collect()
}

/// One document of an index, before it goes to Tantivy.
#[derive(Debug, Clone)]
pub struct IndexDoc {
    /// node id inside the library: the volume path or the article path
    pub path: String,
    /// book id (CBETA) or the article's book / folder
    pub book: String,
    /// juan number (CBETA), 0 for articles
    pub juan: u16,
    pub title: String,
    /// reading text (punctuation is transparent to the analyzer)
    pub text: String,
    /// facet paths, e.g. `["bulei", "阿含部類", "阿含部"]`
    pub facets: Vec<Vec<String>>,
    /// the node and the nodes above it that can carry properties, as
    /// (book id, path), the document itself last (`props::Overlay`)
    pub keys: Vec<(String, String)>,
}

/// Every existing volume of every registered CBETA book (LC, Y are not
/// registered). Unreadable volumes come out as `Err` and are skipped.
pub fn cbeta_docs<'a>(bc: &'a Bookcase, only: Option<&'a HashSet<String>>) -> (usize, impl Iterator<Item = Result<IndexDoc, String>> + 'a) {
    let mut jobs = Vec::new();
    for id in bc.catalog.order.iter().filter(|id| only.is_none_or(|o| o.contains(*id))) {
        for v in bc.volumes(id) {
            if v.exists {
                jobs.push((id.clone(), v));
            }
        }
    }
    let total = jobs.len();
    let iter = jobs.into_iter().filter_map(move |(book_id, vol)| {
        let book = bc.book(&book_id)?;
        let vt = match bc.volume_text(&vol.path) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let facets = cbeta_facets(bc, &book_id)?;
        Some(Ok(IndexDoc {
            path: vol.path.clone(),
            book: book_id.clone(),
            juan: vol.idx,
            title: book.title.clone(),
            text: vt.text,
            facets,
            keys: vec![(book_id.clone(), String::new())],
        }))
    });
    (total, iter)
}

/// The facets of a CBETA book as the data gives them (no user properties).
pub fn cbeta_facets(bc: &Bookcase, book_id: &str) -> Option<Vec<Vec<String>>> {
    let book = bc.book(book_id)?;
    // facets carry the canon's name (大正新脩大藏經), not its code (T)
    let canon = bc
        .tripitakas
        .iter()
        .find(|t| t.id == book.canon)
        .map(|t| t.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| book.canon.clone());
    let mut facets = vec![vec!["canon".to_string(), canon], vec!["book".to_string(), book_id.to_string()]];
    let mut bulei = vec!["bulei".to_string()];
    bulei.extend(book.catalog.split('/').filter(|s| !s.is_empty()).map(|s| s.to_string()));
    if bulei.len() > 1 {
        facets.push(bulei);
    }
    if let Some(bi) = bookinfo().get(book_id) {
        if !bi.kind_label.is_empty() {
            let mut kind = vec!["kind".to_string()];
            kind.extend(bi.kind_label.split('：').map(|s| s.to_string()));
            facets.push(kind);
        }
        for p in &bi.periods {
            facets.push(vec!["period".to_string(), p.clone()]);
        }
        for a in &bi.authors {
            facets.push(vec!["person".to_string(), a.clone()]);
        }
    }
    Some(facets)
}

/// An article of an HTML library, as placed by its tree.
#[derive(Debug, Clone)]
pub struct HtmlLeaf {
    pub path: String,
    pub title: String,
    /// titles of the folders above it, top first
    pub folders: Vec<String>,
    /// leaves with the same parent share a group (prev / next navigation)
    pub group: usize,
}

/// An HTML library: one user-made tree of a bookcase and its articles.
pub struct HtmlTree {
    pub tree_id: String,
    pub code: String,
    pub nav: NavDoc,
    pub leaves: Vec<HtmlLeaf>,
    by_path: HashMap<String, usize>,
}

impl HtmlTree {
    pub fn open(bc: &Bookcase, tree_id: &str) -> Result<HtmlTree, String> {
        let nav = bc.nav(tree_id)?;
        let mut leaves = Vec::new();
        let mut by_path = HashMap::new();
        let mut groups = 0usize;
        fn walk(
            bc: &Bookcase,
            nodes: &[NavNode],
            folders: &mut Vec<String>,
            group: usize,
            groups: &mut usize,
            leaves: &mut Vec<HtmlLeaf>,
            by_path: &mut HashMap<String, usize>,
        ) {
            for n in nodes {
                match n {
                    NavNode::Folder { title, children } => {
                        *groups += 1;
                        let g = *groups;
                        folders.push(title.clone());
                        walk(bc, children, folders, g, groups, leaves, by_path);
                        folders.pop();
                    }
                    NavNode::Leaf { href, text, children, .. } => {
                        let path = href.split('#').next().unwrap_or("").to_string();
                        if path.starts_with("a/") && bc.source.exists(&path) && !by_path.contains_key(&path) {
                            by_path.insert(path.clone(), leaves.len());
                            leaves.push(HtmlLeaf {
                                path,
                                title: text.clone(),
                                folders: folders.clone(),
                                group,
                            });
                        }
                        if !children.is_empty() {
                            *groups += 1;
                            let g = *groups;
                            folders.push(text.clone());
                            walk(bc, children, folders, g, groups, leaves, by_path);
                            folders.pop();
                        }
                    }
                }
            }
        }
        walk(bc, &nav.items, &mut Vec::new(), 0, &mut groups, &mut leaves, &mut by_path);
        let code = leaves
            .first()
            .and_then(|l| l.path.split('/').nth(1))
            .unwrap_or("")
            .to_string();
        Ok(HtmlTree {
            tree_id: tree_id.to_string(),
            code,
            nav,
            leaves,
            by_path,
        })
    }

    pub fn leaf(&self, path: &str) -> Option<&HtmlLeaf> {
        self.by_path.get(path).map(|&i| &self.leaves[i])
    }

    /// Articles under the same parent, in tree order.
    pub fn siblings(&self, path: &str) -> Vec<&HtmlLeaf> {
        match self.leaf(path) {
            Some(l) => self.leaves.iter().filter(|x| x.group == l.group).collect(),
            None => Vec::new(),
        }
    }

    /// Index documents for every article of the tree.
    pub fn docs<'a>(&'a self, bc: &'a Bookcase, only: Option<&'a HashSet<String>>) -> (usize, impl Iterator<Item = Result<IndexDoc, String>> + 'a) {
        let leaves: Vec<&HtmlLeaf> = self.leaves.iter().filter(|l| only.is_none_or(|o| o.contains(&l.path))).collect();
        let total = leaves.len();
        let iter = leaves.into_iter().map(move |leaf| {
            let bytes = bc.source.read(&leaf.path).ok_or_else(|| format!("{}: not in the bookcase", leaf.path))?;
            let a = read_article(&decode_auto(&bytes), &leaf.path, None);
            let mut facets = Vec::new();
            let mut bulei = vec!["bulei".to_string()];
            bulei.extend(leaf.folders.iter().take(2).cloned());
            if bulei.len() > 1 {
                facets.push(bulei);
            }
            let book = if !a.meta.book.is_empty() {
                a.meta.book.clone()
            } else {
                leaf.folders.last().cloned().unwrap_or_else(|| leaf.title.clone())
            };
            facets.push(vec!["book".to_string(), book.clone()]);
            if !a.meta.catalog.is_empty() {
                facets.push(vec!["kind".to_string(), a.meta.catalog.clone()]);
            }
            if !a.meta.period.is_empty() {
                facets.push(vec!["period".to_string(), a.meta.period.clone()]);
            }
            for p in &a.meta.authors {
                facets.push(vec!["person".to_string(), p.clone()]);
            }
            Ok(IndexDoc {
                path: leaf.path.clone(),
                book,
                juan: 0,
                title: leaf.title.clone(),
                text: a.text.text,
                facets,
                keys: vec![(String::new(), leaf.path.clone())],
            })
        });
        (total, iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::MemSource;
    use crate::text::encode_utf16le_bom;

    const MENU: &str = r#"<html><body><nav type="catalog" default="zhibei">
<navlink href="zhibei_nav.xhtml">索达吉堪布全集</navlink>
<li><navlink href="simple_nav.xhtml">簡易</navlink></li>
<li><navlink href="advance_nav.xhtml">進階</navlink></li>
<li><navlink href="bulei_nav.xhtml">部類</navlink></li></nav></body></html>"#;

    fn fixture() -> Bookcase {
        let mut src = MemSource::new();
        src.add("index.xml", "<metadata><version>1</version><date>2025-04-26</date></metadata>");
        src.add("bookdata.txt", encode_utf16le_bom("T,2,【大】,大正新脩大藏經,Taisho\n"));
        src.add("catalog.txt", "T , 阿含部類 , , 01 , 0001 , 1 , 長阿含經 , 後秦 佛陀耶舍共竺佛念譯\n");
        src.add("menu_nav.xhtml", MENU);
        let cbeta = r#"<html><body><nav type="catalog"><span>大正</span><ol>
<li><cblink href="XML/T/T01/T01n0001_001.xml">T0001 長阿含經</cblink></li>
<li><a href="help/other/N/x.htm">說明</a></li></ol></nav></body></html>"#;
        for n in ["simple_nav.xhtml", "advance_nav.xhtml", "bulei_nav.xhtml"] {
            src.add(n, cbeta);
        }
        src.add(
            "zhibei_nav.xhtml",
            r#"<html><body><nav type="catalog" han-lang="hans"><li><span>著作</span><ol>
<li><span>悲惨世界</span><ol>
<li><a i="ZB1" href="a/ZB/z/1.html">童年的梦</a></li>
<li><a i="ZB2" href="a/ZB/z/2.html">狗王之死</a></li></ol></li>
<li><a i="ZB3" href="a/ZB/z/missing.html">不存在</a></li></ol></li></nav></body></html>"#,
        );
        src.add("spine.txt", "");
        src.add("a/ZB/z/1.html", "<html><head><meta author=\"索达吉堪布\"></head><body><p>童年的梦。</p></body></html>");
        src.add("a/ZB/z/2.html", "<html><body><h2>狗王</h2><p>狗王之死。</p></body></html>");
        Bookcase::open(Box::new(src)).unwrap()
    }

    #[test]
    fn tells_cbeta_trees_from_html_libraries() {
        let bc = fixture();
        let t = classify_trees(&bc);
        let groups: Vec<(&str, &TreeGroup)> = t.iter().map(|c| (c.id.as_str(), &c.group)).collect();
        assert_eq!(
            groups,
            vec![
                ("zhibei", &TreeGroup::Html("ZB".into())),
                ("simple", &TreeGroup::Cbeta),
                ("advance", &TreeGroup::Cbeta),
                ("bulei", &TreeGroup::Cbeta),
            ]
        );
    }

    #[test]
    fn reads_an_html_library_and_its_documents() {
        let bc = fixture();
        let lib = HtmlTree::open(&bc, "zhibei").unwrap();
        assert_eq!(lib.code, "ZB");
        // the missing file is ignored (kept in the menu, no content)
        assert_eq!(lib.leaves.len(), 2);
        assert_eq!(lib.leaf("a/ZB/z/2.html").unwrap().folders, vec!["著作", "悲惨世界"]);
        assert_eq!(lib.siblings("a/ZB/z/1.html").len(), 2);
        let (total, docs) = lib.docs(&bc, None);
        let docs: Vec<IndexDoc> = docs.map(|d| d.unwrap()).collect();
        assert_eq!(total, 2);
        assert_eq!(docs[0].book, "悲惨世界");
        assert!(docs[0].facets.contains(&vec!["person".to_string(), "索达吉堪布".to_string()]));
        assert!(docs[1].text.contains("狗王之死"));
    }
}
