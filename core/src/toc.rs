//! Per-book table of contents: `toc/{canon}/{id}.xml`.
//!
//! Two optional `<nav>` blocks:
//! - `type="catalog"`: chapters (品, 章 ...), nested `ol/li`, links like
//!   `XML/T/T09/T09n0262_001.xml#p0001c18`
//! - `type="juan"`: the juan (卷) of the book
//!
//! The anchor is `p` + the line-break id (`0001c18` = page 1, column c, line 18).
//! The order in the file is meaningful (e.g. an appendix chapter located on
//! page 198 sits between page 55 and page 56), so it must not be re-sorted.

use crate::markup::{self, Element};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct TocLink {
    /// path inside the bookcase, e.g. `XML/T/T09/T09n0262_001.xml`
    pub file: String,
    /// without the leading `#`, e.g. `p0001c18`
    pub anchor: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TocItem {
    pub link: TocLink,
    pub children: Vec<TocItem>,
}

#[derive(Debug, Clone, Default)]
pub struct Toc {
    pub chapters: Vec<TocItem>,
    pub juan: Vec<TocLink>,
}

impl Toc {
    /// Every volume file referenced by the chapters or the juan list.
    pub fn volume_files(&self) -> BTreeSet<String> {
        fn walk(items: &[TocItem], set: &mut BTreeSet<String>) {
            for it in items {
                if !it.link.file.is_empty() {
                    set.insert(it.link.file.clone());
                }
                walk(&it.children, set);
            }
        }
        let mut set = BTreeSet::new();
        walk(&self.chapters, &mut set);
        for l in &self.juan {
            if !l.file.is_empty() {
                set.insert(l.file.clone());
            }
        }
        set
    }
}

pub fn parse_toc(xml: &str) -> Toc {
    let root = markup::parse(xml);
    let mut navs: Vec<&Element> = Vec::new();
    root.descendants("nav", &mut navs);
    let mut toc = Toc::default();
    for nav in navs {
        match nav.attr("type") {
            Some("catalog") => {
                if let Some(ol) = nav.first("ol") {
                    toc.chapters = parse_items(ol);
                }
            }
            Some("juan") => {
                if let Some(ol) = nav.first("ol") {
                    for li in ol.all("li") {
                        if let Some(link) = li.first_with_attr("href") {
                            toc.juan.push(link_of(link));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    toc
}

fn parse_items(ol: &Element) -> Vec<TocItem> {
    ol.all("li")
        .filter_map(|li| {
            let link = li.first_with_attr("href")?;
            let children = li.first("ol").map(parse_items).unwrap_or_default();
            Some(TocItem {
                link: link_of(link),
                children,
            })
        })
        .collect()
}

fn link_of(el: &Element) -> TocLink {
    let href = el.attr("href").unwrap_or("");
    let (file, anchor) = match href.split_once('#') {
        Some((f, a)) => (f.to_string(), Some(a.to_string())),
        None => (href.to_string(), None),
    };
    TocLink {
        file,
        anchor,
        text: el.text(),
    }
}

/// Parts of a volume file name such as `XML/T/T01/T01n0004_001.xml`.
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeName {
    pub canon: String,
    /// physical volume, e.g. `01`
    pub serial: String,
    /// e.g. `0004`
    pub number: String,
    /// juan number, e.g. 1
    pub idx: u16,
}

pub fn parse_volume_path(path: &str) -> Option<VolumeName> {
    let file = path.rsplit('/').next()?;
    let stem = file.strip_suffix(".xml")?;
    let (head, idx) = stem.rsplit_once('_')?;
    let idx: u16 = idx.parse().ok()?;
    let canon_len = head.chars().take_while(|c| c.is_ascii_uppercase()).count();
    if canon_len == 0 {
        return None;
    }
    let (canon, rest) = head.split_at(canon_len);
    let (serial, number) = rest.split_once('n')?;
    if serial.is_empty() || number.is_empty() {
        return None;
    }
    Some(VolumeName {
        canon: canon.to_string(),
        serial: serial.to_string(),
        number: number.to_string(),
        idx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0004: &str = include_str!("../../samples/cbeta/toc_T0004.xml");
    const T0262: &str = include_str!("../../samples/cbeta/toc_T0262.xml");

    #[test]
    fn parses_toc_with_only_juan() {
        let toc = parse_toc(T0004);
        assert!(toc.chapters.is_empty());
        assert_eq!(toc.juan.len(), 1);
        assert_eq!(toc.juan[0].file, "XML/T/T01/T01n0004_001.xml");
        assert_eq!(toc.juan[0].anchor.as_deref(), Some("p0159a24"));
        assert_eq!(toc.juan[0].text, "第一");
    }

    #[test]
    fn parses_toc_with_chapters_and_nesting() {
        let toc = parse_toc(T0262);
        assert_eq!(toc.chapters.len(), 32);
        assert_eq!(toc.juan.len(), 7);

        assert_eq!(toc.chapters[2].link.text, "1 序品");
        assert_eq!(toc.chapters[2].link.anchor.as_deref(), Some("p0001c18"));

        let appendix = &toc.chapters[26];
        assert_eq!(appendix.link.text, "附文");
        assert_eq!(appendix.link.anchor.as_deref(), Some("p0198a10"));
        assert_eq!(appendix.children.len(), 1);
        assert_eq!(appendix.children[0].link.text, "御製觀世音普門品經序");

        // document order is kept: 附文 (p0198) sits before 25 觀世音菩薩普門品 (p0056)
        assert_eq!(toc.chapters[27].link.text, "25 觀世音菩薩普門品");

        let files = toc.volume_files();
        assert_eq!(files.len(), 7);
        assert!(files.contains("XML/T/T09/T09n0262_007.xml"));
    }

    #[test]
    fn parses_volume_paths() {
        let v = parse_volume_path("XML/T/T01/T01n0004_001.xml").unwrap();
        assert_eq!(
            v,
            VolumeName {
                canon: "T".into(),
                serial: "01".into(),
                number: "0004".into(),
                idx: 1
            }
        );
        let v = parse_volume_path("XML/A/A097/A097n1267_012.xml").unwrap();
        assert_eq!((v.canon.as_str(), v.serial.as_str(), v.idx), ("A", "097", 12));
        assert!(parse_volume_path("toc/T/T0004.xml").is_none());
    }
}
