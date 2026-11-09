//! The table of periods (时域): name, years, other names (design-spec A.5).
//!
//! A preset table (`data/dynasties.txt`) is embedded; the user's own table (same text format,
//! kept in the settings folder) adds to it and replaces rows of the same name. A period named
//! by one of its other names is stored under the first name, so 姚秦 and 後秦 are one facet;
//! facets are ordered by the first year.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const PRESET: &str = include_str!("../../data/dynasties.txt");

#[derive(Debug, Clone, PartialEq)]
pub struct Dynasty {
    pub name: String,
    pub start: i32,
    pub end: i32,
    pub aliases: Vec<String>,
}

/// Rows of the text format: `name<Tab>start<Tab>end<Tab>alias,alias`; `#` starts a comment.
/// Lines that cannot be read are skipped.
pub fn parse(text: &str) -> Vec<Dynasty> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let name = f[0].trim();
        let Some(start) = f.get(1).and_then(|s| s.trim().parse::<i32>().ok()) else { continue };
        if name.is_empty() {
            continue;
        }
        let end = f.get(2).and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(start);
        let aliases = f
            .get(3)
            .map(|a| a.split([',', '，', '、']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();
        out.push(Dynasty { name: name.to_string(), start, end, aliases });
    }
    out
}

pub fn preset_text() -> &'static str {
    PRESET
}

/// The preset rows with the user's rows on top (same name: the user's wins).
pub fn merged(user: &str) -> Vec<Dynasty> {
    let mut rows = parse(PRESET);
    for u in parse(user) {
        match rows.iter_mut().find(|r| r.name == u.name) {
            Some(r) => *r = u,
            None => rows.push(u),
        }
    }
    rows
}

struct Table {
    rows: Vec<Dynasty>,
    by_name: HashMap<String, usize>,
}

impl Table {
    fn new(rows: Vec<Dynasty>) -> Table {
        let mut by_name = HashMap::new();
        for (i, r) in rows.iter().enumerate() {
            by_name.entry(r.name.clone()).or_insert(i);
        }
        for (i, r) in rows.iter().enumerate() {
            for a in &r.aliases {
                by_name.entry(a.clone()).or_insert(i);
            }
        }
        Table { rows, by_name }
    }
}

fn table() -> &'static RwLock<Table> {
    static T: OnceLock<RwLock<Table>> = OnceLock::new();
    T.get_or_init(|| RwLock::new(Table::new(merged(""))))
}

/// Use the user's table (text) from now on. Facets already in an index change when it is built again.
pub fn set_user(text: &str) {
    *table().write().unwrap() = Table::new(merged(text));
}

/// The first name of a period (an unknown name stays as it is).
pub fn canonical(name: &str) -> String {
    let t = table().read().unwrap();
    match t.by_name.get(name.trim()) {
        Some(&i) => t.rows[i].name.clone(),
        None => name.trim().to_string(),
    }
}

/// The first year of a period (`None`: not in the table), for ordering.
pub fn start_of(name: &str) -> Option<i32> {
    let t = table().read().unwrap();
    t.by_name.get(name.trim()).map(|&i| t.rows[i].start)
}

/// Whether `word` is a period name (or another name of one).
pub fn is_period(word: &str) -> bool {
    table().read().unwrap().by_name.contains_key(word.trim())
}

/// The period and the people of a raw author string such as `唐 慧菀述` or `後秦 鳩摩羅什譯`
/// (design-spec A.5). Used for books that the hand-made table does not know.
pub fn parse_author(raw: &str) -> (Option<String>, Vec<String>) {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let mut period = None;
    let mut rest: &[&str] = &words;
    if let Some(first) = words.first() {
        if is_period(first) || (first.chars().count() < 3 && words.len() > 1) {
            period = Some(canonical(first));
            rest = &words[1..];
        }
    }
    let mut people = Vec::new();
    for w in rest {
        for part in w.split(['、', '，', ',', '．', '·', '等']) {
            let name = strip_duty(part);
            if !name.is_empty() && !DUTIES.contains(&name.as_str()) && !is_period(&name) && !["失譯", "失名", "佚名", "未詳", "不詳"].contains(&name.as_str()) && !people.contains(&name) {
                people.push(name);
            }
        }
    }
    (period, people)
}

const DUTIES: [&str; 36] = [
    "譯藏", "重修", "重刊", "重", "續", "修", "重譯", "別譯", "共譯", "同譯", "譯", "述", "撰", "編修", "編", "集", "著", "造", "作", "說", "集註", "註", "注", "解", "疏", "釋", "科", "錄", "抄", "輯", "校", "訂", "刻", "纂", "記", "傳",
];

/// Remove brackets and a trailing duty word (譯、述、撰、編修…) from a name.
fn strip_duty(part: &str) -> String {
    let mut s = String::new();
    let mut depth = 0;
    for c in part.chars() {
        match c {
            '(' | '（' | '[' | '〔' => depth += 1,
            ')' | '）' | ']' | '〕' => depth = (depth - 1).max(0),
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    let mut s = s.trim().to_string();
    loop {
        let mut cut = false;
        for d in DUTIES {
            if s.chars().count() > d.chars().count() && s.ends_with(d) {
                s.truncate(s.len() - d.len());
                cut = true;
                break;
            }
        }
        if !cut {
            return s.trim().to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_names_and_orders() {
        assert_eq!(canonical("後秦"), "姚秦");
        assert_eq!(canonical("元魏"), "北魏");
        assert_eq!(canonical("某朝"), "某朝");
        assert!(start_of("東漢").unwrap() < start_of("唐").unwrap());
        let rows = merged("唐\t600\t900\t大唐\n新朝\t10\t20\t");
        assert_eq!(rows.iter().find(|r| r.name == "唐").unwrap().aliases, vec!["大唐"]);
        assert!(rows.iter().any(|r| r.name == "新朝"));
        assert!(parse("坏行\nabc\tx\n").is_empty());
    }

    #[test]
    fn reads_author_strings() {
        assert_eq!(parse_author("唐 慧菀述"), (Some("唐".into()), vec!["慧菀".into()]));
        assert_eq!(parse_author("後秦 鳩摩羅什譯"), (Some("姚秦".into()), vec!["鳩摩羅什".into()]));
        assert_eq!(parse_author("宋 法天等譯").1, vec!["法天"]);
        assert_eq!(parse_author("失譯").0, None);
        assert_eq!(parse_author("唐 玄奘、窺基 撰").1, vec!["玄奘", "窺基"]);
        assert_eq!(parse_author("隋 智顗說 灌頂記").1, vec!["智顗", "灌頂"]);
    }

    /// Needs the test data: `cargo test -p smart-library-core real_authors -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_authors() {
        let p = std::path::Path::new("C:/Devx/smartLib.d/bookcase.zip");
        let bc = crate::bookcase::Bookcase::open(Box::new(crate::source::ZipSource::open(p).unwrap())).unwrap();
        let mut seen = std::collections::BTreeMap::<String, usize>::new();
        let mut people = std::collections::BTreeMap::<String, usize>::new();
        let mut n = 0;
        let ids: Vec<String> = bc.catalog.books.keys().cloned().collect();
        for id in ids {
            if crate::bookinfo::bookinfo().contains_key(&id) {
                continue;
            }
            let Some(b) = bc.book(&id) else { continue };
            let (p, a) = parse_author(&b.author_info);
            n += 1;
            *seen.entry(p.unwrap_or("（无）".into())).or_default() += 1;
            for x in a {
                *people.entry(x).or_default() += 1;
            }
        }
        println!("{} books without bookinfo
periods {:?}
people {:?}", n, seen, people.iter().take(60).collect::<Vec<_>>());
    }
}
