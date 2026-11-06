//! Assembles a CBETA bookcase: canon list, catalog, navigation trees, and the
//! volumes / outline of every book.
//!
//! How a book finds its volumes (see appendix B of the spec):
//! 1. `toc/{canon}/{id}.xml` lists the volume files (juan list and chapter links);
//! 2. `catalog.txt` gives a juan -> physical volume table, used to fill gaps;
//! 3. the source is asked whether each file really exists.

use crate::catalog::{parse_bookdata, parse_catalog, volume_path, Book, Catalog, Tripitaka};
use crate::nav::{parse_nav, NavDoc};
use crate::source::Source;
use crate::text::decode_auto;
use crate::toc::{parse_toc, parse_volume_path, Toc};
use crate::volume::{extract_volume, VolumeText};
use crate::article::{read_article, Article};
use crate::render::{render_volume, RenderOptions, RenderedVolume};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// Files that must exist for a directory/zip to count as a bookcase.
pub const REQUIRED_FILES: [&str; 7] = [
    "advance_nav.xhtml",
    "bookdata.txt",
    "bulei_nav.xhtml",
    "catalog.txt",
    "menu_nav.xhtml",
    "simple_nav.xhtml",
    "spine.txt",
];

#[derive(Debug, Clone)]
pub struct TreeInfo {
    /// `simple`, `advance`, `bulei`
    pub id: String,
    pub title: String,
    /// `simple_nav.xhtml`
    pub file: String,
}

/// A navigation leaf resolved to a book of the catalog.
#[derive(Debug, Clone)]
pub struct Entry {
    pub book_id: String,
    pub title: String,
    /// for cloned entries (`T0220a`): the link of the first file of this part
    pub start: Option<String>,
    pub cloned: bool,
}

#[derive(Debug, Clone)]
pub struct VolumeRef {
    pub idx: u16,
    /// physical volume (册), e.g. `01`. A juan split across two 册 gives two
    /// `VolumeRef`s with the same `idx` and different `serial`.
    pub serial: String,
    /// `卷一`
    pub label: String,
    pub path: String,
    /// does the file exist in the source?
    pub exists: bool,
    /// listed by the book's toc file (otherwise taken from catalog.txt)
    pub from_toc: bool,
}

pub struct Bookcase {
    pub source: Box<dyn Source>,
    pub version: String,
    pub date: String,
    pub tripitakas: Vec<Tripitaka>,
    pub catalog: Catalog,
    pub trees: Vec<TreeInfo>,
    /// volume file -> its first line, from `spine.txt` (read when first needed)
    spine: OnceLock<HashMap<String, String>>,
}

impl Bookcase {
    pub fn open(source: Box<dyn Source>) -> Result<Self, String> {
        for name in REQUIRED_FILES {
            if !source.exists(name) {
                return Err(format!("not a valid bookcase, missing: {}", name));
            }
        }

        let index = source.read("index.xml").ok_or("cannot read index.xml")?;
        let root = crate::markup::parse(&decode_auto(&index));
        let meta = root.find_descendant("metadata");
        let version = meta
            .and_then(|m| m.first("version"))
            .map(|e| e.text())
            .unwrap_or_default();
        let date = meta
            .and_then(|m| m.first("date"))
            .map(|e| e.text())
            .unwrap_or_default();

        let bookdata = source.read("bookdata.txt").ok_or("cannot read bookdata.txt")?;
        let tripitakas = parse_bookdata(&decode_auto(&bookdata));

        let catalog_bytes = source.read("catalog.txt").ok_or("cannot read catalog.txt")?;
        let catalog = parse_catalog(&decode_auto(&catalog_bytes));

        let menu_bytes = source.read("menu_nav.xhtml").ok_or("cannot read menu_nav.xhtml")?;
        let menu = parse_nav(&decode_auto(&menu_bytes)).ok_or("no <nav> in menu_nav.xhtml")?;
        let trees: Vec<TreeInfo> = menu
            .leaves()
            .into_iter()
            .map(|(href, text)| TreeInfo {
                id: href
                    .strip_suffix("_nav.xhtml")
                    .unwrap_or(href)
                    .to_string(),
                title: text.to_string(),
                file: href.to_string(),
            })
            .collect();

        Ok(Bookcase {
            source,
            version,
            date,
            tripitakas,
            catalog,
            trees,
            spine: OnceLock::new(),
        })
    }

    /// First line of every volume file (`spine.txt`: `XML/T/T01/T01n0001_001.xml , 0001a01`).
    pub fn spine(&self) -> &HashMap<String, String> {
        self.spine.get_or_init(|| {
            let text = self.source.read("spine.txt").map(|b| decode_auto(&b)).unwrap_or_default();
            text.lines()
                .filter_map(|l| l.split_once(','))
                .map(|(p, lb)| (p.trim().to_string(), lb.trim().to_string()))
                .collect()
        })
    }

    /// Quarterly data version derived from `index.xml`'s date, like the old
    /// implementation: Jan/Feb -> previous year Q4, Mar-May Q1, Jun-Aug Q2,
    /// Sep-Nov Q3, Dec Q4. Falls back to the plain version string.
    pub fn data_version(&self) -> String {
        let d = self.date.as_bytes();
        let ok = d.len() == 10
            && d[4] == b'-'
            && d[7] == b'-'
            && self.date.chars().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
        if !ok {
            return self.version.clone();
        }
        let year: i32 = self.date[0..4].parse().unwrap_or(2099);
        let month: i32 = self.date[5..7].parse().unwrap_or(1);
        if month < 3 {
            format!("{}.Q4", year - 1)
        } else if month < 6 {
            format!("{}.Q1", year)
        } else if month < 9 {
            format!("{}.Q2", year)
        } else if month < 12 {
            format!("{}.Q3", year)
        } else {
            format!("{}.Q4", year)
        }
    }

    pub fn nav(&self, tree_id: &str) -> Result<NavDoc, String> {
        let t = self
            .trees
            .iter()
            .find(|t| t.id == tree_id)
            .ok_or_else(|| format!("unknown tree: {}", tree_id))?;
        let bytes = self
            .source
            .read(&t.file)
            .ok_or_else(|| format!("cannot read {}", t.file))?;
        parse_nav(&decode_auto(&bytes)).ok_or_else(|| format!("no <nav> in {}", t.file))
    }

    pub fn book(&self, id: &str) -> Option<&Book> {
        self.catalog.books.get(id)
    }

    /// Resolve a leaf of a navigation tree to a book.
    ///
    /// Links look like `XML/T/T01/T01n0004_001.xml` with the text `T0004 七佛父母姓字經`;
    /// the first word of the text is the book id. A text such as `T0220a ...`
    /// (id with a lower-case letter) denotes a part of book `T0220`.
    pub fn resolve_leaf(&self, href: &str, text: &str) -> Option<Entry> {
        if !href.starts_with("XML/") {
            return None;
        }
        let token = text.split(|c: char| c == ' ' || c == '　').next().unwrap_or("");
        if let Some(book) = self.catalog.books.get(token) {
            return Some(Entry {
                book_id: book.id.clone(),
                title: book.title.clone(),
                start: None,
                cloned: false,
            });
        }
        if token.ends_with(|c: char| c.is_ascii_lowercase()) {
            let base = &token[..token.len() - 1];
            if let Some(book) = self.catalog.books.get(base) {
                return Some(Entry {
                    book_id: book.id.clone(),
                    title: text.to_string(),
                    start: Some(href.to_string()),
                    cloned: true,
                });
            }
        }
        None
    }

    /// The book's table of contents (chapters + juan list), if its toc file exists.
    pub fn outline(&self, book_id: &str) -> Option<Toc> {
        let book = self.catalog.books.get(book_id)?;
        let bytes = self.source.read(&book.toc_path())?;
        Some(parse_toc(&decode_auto(&bytes)))
    }

    /// Reading text of one volume file (M0b). `Err` when the file is missing
    /// or cannot be parsed; callers that build trees or indexes skip such a
    /// volume instead of failing (design-spec A.4: entries without real
    /// content are kept in the menu and ignored).
    pub fn volume_text(&self, path: &str) -> Result<VolumeText, String> {
        let bytes = self.source.read(path).ok_or_else(|| format!("{}: not in the bookcase", path))?;
        extract_volume(&decode_auto(&bytes)).map_err(|e| format!("{}: {}", path, e))
    }

    /// Structured HTML of one volume (`render.rs`), with optional highlights.
    pub fn volume_html(&self, path: &str, opts: &RenderOptions) -> Result<RenderedVolume, String> {
        let bytes = self.source.read(path).ok_or_else(|| format!("{}: not in the bookcase", path))?;
        render_volume(&decode_auto(&bytes), opts).map_err(|e| format!("{}: {}", path, e))
    }

    /// A file of the bookcase that the reader may show: `figures/…` of the
    /// volumes, and images next to the HTML articles under `a/`.
    pub fn asset(&self, path: &str) -> Option<Vec<u8>> {
        if path.contains("..") {
            return None;
        }
        let lower = path.to_ascii_lowercase();
        let image = [".gif", ".png", ".jpg", ".jpeg", ".svg", ".webp"].iter().any(|e| lower.ends_with(e));
        if path.starts_with("figures/") || (path.starts_with("a/") && image) {
            self.source.read(path)
        } else {
            None
        }
    }

    /// An HTML article under `a/` (user-made trees), optionally rendered.
    pub fn article(&self, path: &str, render: Option<&RenderOptions>) -> Result<Article, String> {
        let bytes = self.source.read(path).ok_or_else(|| format!("{}: not in the bookcase", path))?;
        Ok(read_article(&decode_auto(&bytes), path, render))
    }

    /// All volumes (juan) of a book, ordered by juan number.
    pub fn volumes(&self, book_id: &str) -> Vec<VolumeRef> {
        self.volumes_with_toc(book_id, self.outline(book_id).as_ref())
    }

    /// [`Bookcase::volumes`] with the book's toc already read (or `None` when
    /// it has no toc file), so that callers needing both parse it once.
    pub fn volumes_with_toc(&self, book_id: &str, toc: Option<&Toc>) -> Vec<VolumeRef> {
        let book = match self.catalog.books.get(book_id) {
            Some(b) => b,
            None => return Vec::new(),
        };
        // keyed by (juan, path): the path differs only in the 册, so a split
        // juan sorts in physical order
        let mut map: BTreeMap<(u16, String), VolumeRef> = BTreeMap::new();

        if let Some(toc) = toc {
            for file in toc.volume_files() {
                if let Some(vn) = parse_volume_path(&file) {
                    let exists = self.source.exists(&file);
                    map.entry((vn.idx, file.clone())).or_insert(VolumeRef {
                        idx: vn.idx,
                        serial: vn.serial.clone(),
                        label: format!("卷{}", chinese_number(vn.idx as u32)),
                        path: file.clone(),
                        exists,
                        from_toc: true,
                    });
                }
            }
        }
        let toc_juan: std::collections::BTreeSet<u16> = map.keys().map(|(i, _)| *i).collect();
        for (idx, serials) in &book.volumes {
            // the toc is authoritative for every juan it mentions
            if toc_juan.contains(idx) {
                continue;
            }
            for serial in serials {
                let path = volume_path(&book.canon, serial, &book.number, *idx);
                if self.source.exists(&path) {
                    map.insert(
                        (*idx, path.clone()),
                        VolumeRef {
                            idx: *idx,
                            serial: serial.clone(),
                            label: format!("卷{}", chinese_number(*idx as u32)),
                            path,
                            exists: true,
                            from_toc: false,
                        },
                    );
                }
            }
        }
        map.into_values().collect()
    }

    /// Books whose id, title or author string contains `kw`.
    pub fn find_books(&self, kw: &str) -> Vec<&Book> {
        let kw_lower = kw.to_lowercase();
        self.catalog
            .order
            .iter()
            .filter_map(|id| self.catalog.books.get(id))
            .filter(|b| {
                b.title.contains(kw)
                    || b.author_info.contains(kw)
                    || b.id.to_lowercase().contains(&kw_lower)
            })
            .collect()
    }
}

/// 1 -> 一, 11 -> 十一, 20 -> 二十, 101 -> 一百零一, 110 -> 一百一十. (n < 1000)
pub fn chinese_number(n: u32) -> String {
    const D: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if n < 10 {
        return D[n as usize].to_string();
    }
    if n < 20 {
        return if n == 10 {
            "十".to_string()
        } else {
            format!("十{}", D[(n % 10) as usize])
        };
    }
    if n < 100 {
        let o = n % 10;
        return format!("{}十{}", D[(n / 10) as usize], if o == 0 { "" } else { D[o as usize] });
    }
    if n < 1000 {
        let mut s = format!("{}百", D[(n / 100) as usize]);
        let rest = n % 100;
        if rest == 0 {
            return s;
        }
        if rest < 10 {
            s.push('零');
        } else if rest < 20 {
            s.push('一');
        }
        s.push_str(&chinese_number(rest));
        return s;
    }
    n.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::MemSource;
    use crate::text::encode_utf16le_bom;

    const T0004: &str = include_str!("../../samples/cbeta/toc_T0004.xml");
    const T0262: &str = include_str!("../../samples/cbeta/toc_T0262.xml");
    const X0714: &str = include_str!("../../samples/cbeta/toc_X0714.xml");

    // fixture rows: the 部類 names for T0001/T0004/T0220/T0262 are invented for the test
    const CATALOG: &str = "T , 阿含部類 , , 01 , 0001 , 22 , 長阿含經 , 後秦 佛陀耶舍共竺佛念譯
T , 阿含部類 , , 01 , 0004 , 1 , 七佛父母姓字經 , 失譯
T , 般若部類 , , 05 , 0220 , 200 , 大般若波羅蜜多經 , 唐 玄奘譯
T , 法華部類 , , 09 , 0262 , 7 , 妙法蓮華經 , 姚秦 鳩摩羅什譯
";

    const MENU: &str = r#"<html><body><nav type="catalog" default="zhibei">
<navlink href="simple_nav.xhtml">大藏经 · 簡易目錄</navlink>
<li><navlink href="advance_nav.xhtml">大藏经 · 進階目錄</navlink></li>
<li><navlink href="bulei_nav.xhtml">大藏经 · 部類目錄</navlink></li>
</nav></body></html>"#;

    const SIMPLE: &str = r#"<html><body><nav type="catalog"><span>大正新脩大藏經</span><ol>
<li><span>01 阿含部類 T01-02,25,33 etc.</span><ol>
<li><cblink href="XML/T/T01/T01n0001_001.xml">T0001 長阿含經</cblink></li>
<li><cblink href="XML/T/T01/T01n0004_001.xml">T0004 七佛父母姓字經</cblink></li>
</ol></li></ol></nav></body></html>"#;

    fn fixture() -> Bookcase {
        let mut src = MemSource::new();
        src.add(
            "index.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><metadata><id>CBETA</id><date>2025-04-26</date><cover src="cover.png"/><version>0.9.5.0</version></metadata>"#,
        );
        src.add(
            "bookdata.txt",
            encode_utf16le_bom("T,2,【大】,大正新脩大藏經,Taishō Tripiṭaka\n"),
        );
        src.add("catalog.txt", CATALOG);
        src.add("menu_nav.xhtml", MENU);
        src.add("simple_nav.xhtml", SIMPLE);
        src.add("advance_nav.xhtml", SIMPLE);
        src.add("bulei_nav.xhtml", SIMPLE);
        src.add("spine.txt", "XML/T/T01/T01n0001_001.xml , 0001a01\n");
        src.add("toc/T/T0004.xml", T0004);
        src.add("toc/T/T0262.xml", T0262);
        src.add("XML/T/T01/T01n0004_001.xml", "x");
        for i in 1..=5 {
            src.add(&format!("XML/T/T09/T09n0262_{:03}.xml", i), "x");
        }
        Bookcase::open(Box::new(src)).unwrap()
    }

    #[test]
    fn opens_and_lists_trees() {
        let bc = fixture();
        assert_eq!(bc.version, "0.9.5.0");
        assert_eq!(bc.date, "2025-04-26");
        assert_eq!(bc.data_version(), "2025.Q1");
        assert_eq!(bc.tripitakas.len(), 1);
        assert_eq!(bc.tripitakas[0].tag, "【大】");
        assert_eq!(bc.catalog.books.len(), 4);
        let ids: Vec<&str> = bc.trees.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["simple", "advance", "bulei"]);
    }

    #[test]
    fn rejects_incomplete_bookcase() {
        let mut src = MemSource::new();
        src.add("index.xml", "<metadata/>");
        assert!(Bookcase::open(Box::new(src)).is_err());
    }

    #[test]
    fn resolves_nav_leaves() {
        let bc = fixture();
        let doc = bc.nav("simple").unwrap();
        let leaves = doc.leaves();
        assert_eq!(leaves.len(), 2);

        let e = bc.resolve_leaf(leaves[1].0, leaves[1].1).unwrap();
        assert_eq!(e.book_id, "T0004");
        assert_eq!(e.title, "七佛父母姓字經");
        assert!(!e.cloned);

        let c = bc
            .resolve_leaf("XML/T/T05/T05n0220_001.xml", "T0220a 大般若經第一分")
            .unwrap();
        assert_eq!(c.book_id, "T0220");
        assert!(c.cloned);
        assert_eq!(c.start.as_deref(), Some("XML/T/T05/T05n0220_001.xml"));

        assert!(bc
            .resolve_leaf("XML/T/T99/T99n9999_001.xml", "T9999 不存在")
            .is_none());
    }

    #[test]
    fn assembles_volumes_from_toc_and_checks_existence() {
        let bc = fixture();

        let v4 = bc.volumes("T0004");
        assert_eq!(v4.len(), 1);
        assert!(v4[0].exists && v4[0].from_toc);
        assert_eq!(v4[0].label, "卷一");

        let v262 = bc.volumes("T0262");
        assert_eq!(v262.len(), 7);
        assert_eq!(v262[0].path, "XML/T/T09/T09n0262_001.xml");
        assert!(v262[4].exists);
        assert!(!v262[5].exists); // listed by the toc, but not in the source
        assert!(!v262[6].exists);

        // no toc file and no volume file present -> nothing
        assert!(bc.volumes("T0001").is_empty());
        assert!(bc.volumes("T9999").is_empty());
    }

    #[test]
    fn keeps_a_juan_split_across_two_physical_volumes() {
        let mut src = MemSource::new();
        src.add("index.xml", "<metadata><version>0.9.5.0</version><date>2025-04-26</date></metadata>");
        src.add("bookdata.txt", encode_utf16le_bom("X,2,【卍續】,卍新纂大日本續藏經,Manji
"));
        src.add(
            "catalog.txt",
            "X , 律部類 , , 39 , 0714 , 3 , 四分律含注戒本疏行宗記(第1卷-第3卷) , 唐 道宣撰
             X , 律部類 , , 40 , 0714 , 2 , 四分律含注戒本疏行宗記(第3卷-第4卷) , 唐 道宣撰
",
        );
        src.add("menu_nav.xhtml", MENU);
        src.add("simple_nav.xhtml", SIMPLE);
        src.add("advance_nav.xhtml", SIMPLE);
        src.add("bulei_nav.xhtml", SIMPLE);
        src.add("spine.txt", "");
        src.add("toc/X/X0714.xml", X0714);
        for p in ["X39/X39n0714_001", "X39/X39n0714_002", "X39/X39n0714_003", "X40/X40n0714_003", "X40/X40n0714_004"] {
            src.add(&format!("XML/X/{}.xml", p), "x");
        }
        let bc = Bookcase::open(Box::new(src)).unwrap();
        let v = bc.volumes("X0714");
        let paths: Vec<&str> = v.iter().map(|v| v.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "XML/X/X39/X39n0714_001.xml",
                "XML/X/X39/X39n0714_002.xml",
                "XML/X/X39/X39n0714_003.xml",
                "XML/X/X40/X40n0714_003.xml",
                "XML/X/X40/X40n0714_004.xml",
            ]
        );
        assert_eq!((v[2].idx, v[2].serial.as_str()), (3, "39"));
        assert_eq!((v[3].idx, v[3].serial.as_str()), (3, "40"));
        assert!(v.iter().all(|v| v.exists && v.from_toc));
    }

    #[test]
    fn finds_books_by_title_author_and_id() {
        let bc = fixture();
        assert_eq!(bc.find_books("法華").len(), 0);
        assert_eq!(bc.find_books("阿含")[0].id, "T0001");
        assert_eq!(bc.find_books("鳩摩羅什")[0].id, "T0262");
        assert_eq!(bc.find_books("t0004")[0].id, "T0004");
    }

    #[test]
    fn writes_chinese_numbers() {
        assert_eq!(chinese_number(1), "一");
        assert_eq!(chinese_number(10), "十");
        assert_eq!(chinese_number(11), "十一");
        assert_eq!(chinese_number(20), "二十");
        assert_eq!(chinese_number(101), "一百零一");
        assert_eq!(chinese_number(110), "一百一十");
        assert_eq!(chinese_number(600), "六百");
    }
}
