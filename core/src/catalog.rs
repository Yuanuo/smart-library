//! `bookdata.txt` (canon list) and `catalog.txt` (book list).

use std::collections::{BTreeMap, HashMap};

/// One line of `bookdata.txt`: `T,2,【大】,大正新脩大藏經,Taishō Tripiṭaka`.
/// The meaning of the number (2nd column) is unknown.
#[derive(Debug, Clone, PartialEq)]
pub struct Tripitaka {
    pub id: String,
    pub num: i32,
    pub tag: String,
    pub name: String,
    pub name_en: String,
}

pub fn parse_bookdata(text: &str) -> Vec<Tripitaka> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.splitn(5, ',').map(|s| s.trim()).collect();
        out.push(Tripitaka {
            id: f[0].to_string(),
            num: f.get(1).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0),
            tag: f.get(2).copied().unwrap_or("").to_string(),
            name: f.get(3).copied().unwrap_or("").to_string(),
            name_en: f.get(4).copied().unwrap_or("").to_string(),
        });
    }
    out
}

/// A book (经) as described by `catalog.txt`. One book may span several lines
/// (several physical volumes of the canon), each line covering a range of juan.
#[derive(Debug, Clone, Default)]
pub struct Book {
    /// canon code + number, e.g. `T0004`, `A1267`
    pub id: String,
    /// canon code, e.g. `T`
    pub canon: String,
    /// number inside the canon, e.g. `0004`
    pub number: String,
    pub title: String,
    /// raw author string, e.g. `唐 慧菀述`
    pub author_info: String,
    /// `部類` or `部類/子類`
    pub catalog: String,
    /// juan number -> physical volumes (册), e.g. 1 -> ["01"].
    /// Usually one 册 per juan, but a juan can be split across two 册: the
    /// L1557 lines `130 ... (第1卷-第17卷)` and `131 ... (第17卷-第34卷)` give
    /// juan 17 -> ["130", "131"] (files L130n1557_017 and L131n1557_017).
    /// NOTE: a range given by the catalog is not a guarantee that the files exist.
    pub volumes: BTreeMap<u16, Vec<String>>,
}

impl Book {
    /// Path of this book's table-of-contents file inside the bookcase.
    pub fn toc_path(&self) -> String {
        format!("toc/{}/{}.xml", self.canon, self.id)
    }
}

/// `XML/T/T01/T01n0004_001.xml`
pub fn volume_path(canon: &str, serial: &str, number: &str, idx: u16) -> String {
    format!(
        "XML/{c}/{c}{s}/{c}{s}n{n}_{i:03}.xml",
        c = canon,
        s = serial,
        n = number,
        i = idx
    )
}

#[derive(Debug, Default)]
pub struct Catalog {
    pub books: HashMap<String, Book>,
    /// book ids in file order
    pub order: Vec<String>,
    /// lines that could not be parsed
    pub skipped: usize,
}

/// Columns (separated by ", "):
/// canon, 部類, 子類 (often empty), 册號, 經號, 卷數, 經名, 作譯者
pub fn parse_catalog(text: &str) -> Catalog {
    let mut books: HashMap<String, Book> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut skipped = 0usize;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.splitn(8, ", ").map(|s| s.trim()).collect();
        if f.len() < 8 {
            skipped += 1;
            continue;
        }
        let canon = f[0];
        let mut catalog = f[1].to_string();
        if !f[2].is_empty() {
            catalog = format!("{}/{}", catalog, f[2]);
        }
        let serial = f[3];
        let number = f[4];
        let vols_num: u16 = match f[5].parse() {
            Ok(v) => v,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let (name, start, end) = match split_vol_range(f[6]) {
            Some((n, s, e)) => (n, s, e),
            None => (f[6].to_string(), 1, vols_num),
        };
        let author = f[7];
        let id = format!("{}{}", canon, number);

        let book = books.entry(id.clone()).or_insert_with(|| {
            order.push(id.clone());
            Book {
                id: id.clone(),
                canon: canon.to_string(),
                number: number.to_string(),
                title: name.clone(),
                author_info: author.to_string(),
                catalog: catalog.clone(),
                volumes: BTreeMap::new(),
            }
        });
        for i in start..=end {
            let serials = book.volumes.entry(i).or_default();
            if !serials.iter().any(|s| s == serial) {
                serials.push(serial.to_string());
            }
        }
    }
    Catalog {
        books,
        order,
        skipped,
    }
}

/// `大唐開元釋教廣品歷章(第3卷-第4卷)` -> (`大唐開元釋教廣品歷章`, 3, 4)
fn split_vol_range(name: &str) -> Option<(String, u16, u16)> {
    let mut search_end = name.len();
    while let Some(pos) = name[..search_end].rfind("(第") {
        let rest = &name[pos + "(第".len()..];
        if let Some((a, rest2)) = take_digits(rest) {
            if let Some(rest3) = rest2.strip_prefix("卷-第") {
                if let Some((b, rest4)) = take_digits(rest3) {
                    if rest4.starts_with('卷') {
                        return Some((name[..pos].to_string(), a, b));
                    }
                }
            }
        }
        search_end = pos;
    }
    None
}

fn take_digits(s: &str) -> Option<(u16, &str)> {
    let n = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    let (digits, rest) = s.split_at(n);
    Some((digits.parse().ok()?, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = "A , 事彙部類 , , 091 , 1057 , 2 , 新譯大方廣佛華嚴經音義 , 唐 慧菀述
A , 事彙部類 , , 097 , 1267 , 2 , 大唐開元釋教廣品歷章(第3卷-第4卷) , 唐 玄逸撰
A , 事彙部類 , , 098 , 1267 , 15 , 大唐開元釋教廣品歷章(第5卷-第20卷) , 唐 玄逸撰
A , 事彙部類 , , 110 , 1490 , 2 , 天聖釋教總錄 , 宋 惟淨等編修
A , 事彙部類 , , 111 , 1493 , 9 , 大中祥符法寶錄(第3卷-第12卷) , 宋 楊億等編修
";

    #[test]
    fn parses_bookdata() {
        let v = parse_bookdata(
            "T,2,【大】,大正新脩大藏經,Taishō Tripiṭaka\nX,2,【卍續】,卍新纂大日本續藏經,Manji Shinsan Dainihon Zokuzōkyō\n",
        );
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "T");
        assert_eq!(v[0].num, 2);
        assert_eq!(v[0].tag, "【大】");
        assert_eq!(v[1].name_en, "Manji Shinsan Dainihon Zokuzōkyō");
    }

    #[test]
    fn parses_catalog_and_merges_books_across_lines() {
        let cat = parse_catalog(CATALOG);
        assert_eq!(cat.skipped, 0);
        assert_eq!(cat.books.len(), 4);
        assert_eq!(cat.order, vec!["A1057", "A1267", "A1490", "A1493"]);

        let b = &cat.books["A1267"];
        assert_eq!(b.title, "大唐開元釋教廣品歷章");
        assert_eq!(b.catalog, "事彙部類");
        assert_eq!(b.author_info, "唐 玄逸撰");
        assert_eq!(b.volumes[&3], vec!["097"]);
        assert_eq!(b.volumes[&5], vec!["098"]);
        assert_eq!(b.volumes.get(&1), None);
        assert_eq!(b.volumes.len(), 18); // 3..=4 and 5..=20

        // a plain line: juan 1..=卷數 in one physical volume
        let a = &cat.books["A1057"];
        assert_eq!(a.volumes.len(), 2);
        assert_eq!(a.volumes[&1], vec!["091"]);
        assert_eq!(a.toc_path(), "toc/A/A1057.xml");

        // the range in the title (3..=12) says 10 juan although 卷數 is 9:
        // catalog ranges are hints, not proof that every file exists.
        assert_eq!(cat.books["A1493"].volumes.len(), 10);
    }

    #[test]
    fn keeps_both_physical_volumes_of_a_split_juan() {
        // real lines (bookcase 2025.Q1): juan 3 starts in X39 and continues in X40
        let cat = parse_catalog(
            "X , 律部類 , , 39 , 0714 , 3 , 四分律含注戒本疏行宗記(第1卷-第3卷) , 唐 道宣撰
             X , 律部類 , , 40 , 0714 , 2 , 四分律含注戒本疏行宗記(第3卷-第4卷) , 唐 道宣撰
",
        );
        let b = &cat.books["X0714"];
        assert_eq!(b.volumes.len(), 4);
        assert_eq!(b.volumes[&2], vec!["39"]);
        assert_eq!(b.volumes[&3], vec!["39", "40"]);
        assert_eq!(b.volumes[&4], vec!["40"]);
    }

    #[test]
    fn joins_catalog_and_group() {
        let cat = parse_catalog("T , 般若部類 , 大般若經關法 , 05 , 0220 , 1 , 某經 , 唐 某某譯\n");
        assert_eq!(cat.books["T0220"].catalog, "般若部類/大般若經關法");
    }

    #[test]
    fn skips_broken_lines() {
        let cat = parse_catalog("garbage\nT , x , , 01 , 0001 , notnum , t , a\n");
        assert_eq!(cat.skipped, 2);
        assert!(cat.books.is_empty());
    }

    #[test]
    fn builds_volume_paths() {
        assert_eq!(volume_path("T", "01", "0004", 1), "XML/T/T01/T01n0004_001.xml");
        assert_eq!(volume_path("A", "097", "1267", 12), "XML/A/A097/A097n1267_012.xml");
    }
}
