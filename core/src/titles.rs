//! Title quick search (design-spec 8.7, M2): find a book, a chapter, a juan
//! or an article by its title, as you type.
//!
//! Independent of the full-text index. Each knowledge library gets a list of
//! [`TitleEntry`]s, built once from its tree and kept in memory:
//!
//! * CBETA: every book of the bound tree (in tree order; a part of a big book
//!   such as `T0220a` is a book entry of its own), then the chapters of its
//!   toc (序, 品, 分, 經 …) and, for books with more than one, its juan;
//! * HTML libraries: every article, with its folders as context.
//!
//! What entries share (a book's id, title and author; the folders of some
//! articles) is a [`TitleGroup`], stored once.
//!
//! Matching works on folded keys (the analyzer's `match_key`: NFKC, lower
//! case, traditional -> simplified, punctuation and spaces dropped), so the
//! search is insensitive to 简繁, full / half width and punctuation. The
//! query is split on whitespace; every keyword must occur in the entry's
//! title or in its group. A linear scan over the ~320 000 entries of the
//! CBETA 部類 tree takes about 4 ms, so no prefix structure is needed.

use crate::analyzer::match_key;
use crate::bookcase::Bookcase;
use crate::library::HtmlTree;
use crate::pinyin::pinyin_key;
use crate::toc::TocItem;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TitleKind {
    Book,
    Article,
    /// a folder of a local folder library
    Folder,
    Chapter,
    Juan,
}

impl TitleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TitleKind::Book => "book",
            TitleKind::Article => "article",
            TitleKind::Folder => "folder",
            TitleKind::Chapter => "chapter",
            TitleKind::Juan => "juan",
        }
    }

    /// Order inside the same match quality: whole documents first.
    fn order(self) -> u8 {
        match self {
            TitleKind::Book | TitleKind::Article | TitleKind::Folder => 0,
            TitleKind::Chapter => 1,
            TitleKind::Juan => 2,
        }
    }
}

/// What entries share: a CBETA book, or the folders of some articles.
#[derive(Debug, Clone)]
pub struct TitleGroup {
    /// `T0262 妙法蓮華經（7卷）`, or the folders `著作 / 悲惨世界`
    pub line: String,
    pub author: String,
    pub book_id: Option<String>,
    /// folded id, title and author, or the folders
    key: String,
}

#[derive(Debug, Clone)]
pub struct TitleEntry {
    pub kind: TitleKind,
    /// shown title: `T0262 妙法蓮華經（7卷）`, `1 序品`, `卷三`, an article title
    pub title: String,
    /// index into [`TitleIndex::groups`]
    pub group: u32,
    /// volume or article to open (`None`: the book's first readable volume)
    pub path: Option<String>,
    /// line (`0001c18`) to jump to
    pub lb: Option<String>,
}

struct PinyinTitleIndex {
    names: Vec<(String, String)>,
    extras: Vec<(String, String)>,
    groups: Vec<(String, String)>,
    aliases: HashMap<usize, Vec<(String, String)>>,
}

pub struct TitleIndex {
    pub entries: Vec<TitleEntry>,
    pub groups: Vec<TitleGroup>,
    /// folded title, for the match quality
    names: Vec<String>,
    /// more words of the entry alone, folded (usually empty)
    extras: Vec<String>,
    /// other names the user gave an entry (folded): entry index -> names
    aliases: HashMap<usize, Vec<String>>,
    /// lazily built pinyin keys (`full`, `initials`) when pinyin search is enabled
    pinyin: RwLock<Option<Arc<PinyinTitleIndex>>>,
}

/// Quality of a match, smaller is better; ties keep the tree order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rank {
    /// 0 direct text match, 1 matched via pinyin
    pub pinyin: u8,
    /// 0 the title starts with the first keyword, 1 every keyword is in the
    /// title, 2 some keywords only match the context (book, folders, author)
    pub tier: u8,
    /// books and articles before chapters, chapters before juan
    pub kind: u8,
    /// 0 when the title is exactly the query
    pub inexact: u8,
    /// title length (tiers 0 and 1), shorter first
    pub len: u32,
}

/// Folded keywords of a query (empty: list everything).
pub fn query_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(key)
        .filter(|w| !w.is_empty())
        .collect()
}

fn key(s: &str) -> String {
    match_key(s).into_iter().collect()
}

fn key_of(parts: &[&str]) -> String {
    let mut k = String::new();
    for p in parts {
        if !k.is_empty() {
            k.push(' ');
        }
        k.push_str(&key(p));
    }
    k
}

impl TitleIndex {
    fn new() -> Self {
        TitleIndex {
            entries: Vec::new(),
            groups: Vec::new(),
            names: Vec::new(),
            extras: Vec::new(),
            aliases: HashMap::new(),
            pinyin: RwLock::new(None),
        }
    }

    /// Give entries the other names the user set on their nodes: `(book id, path)` -> names
    /// (a CBETA book: `(id, "")`; an article, file, folder or note: `("", path)`). A name counts
    /// like the title itself.
    pub fn add_aliases(&mut self, map: &HashMap<(String, String), Vec<String>>) {
        if map.is_empty() {
            return;
        }
        for (i, e) in self.entries.iter().enumerate() {
            let k = match (e.kind, &e.path) {
                (TitleKind::Book, None) => (self.groups[e.group as usize].book_id.clone().unwrap_or_default(), String::new()),
                (TitleKind::Article | TitleKind::Folder, Some(p)) => (String::new(), p.clone()),
                _ => continue,
            };
            if let Some(names) = map.get(&k) {
                let keys: Vec<String> = names.iter().map(|n| key(n)).filter(|n| !n.is_empty()).collect();
                if !keys.is_empty() {
                    self.aliases.insert(i, keys);
                }
            }
        }
    }

    fn add_group(&mut self, line: String, author: &str, book_id: Option<&str>, key_parts: &[&str]) -> u32 {
        self.groups.push(TitleGroup {
            line,
            author: author.to_string(),
            book_id: book_id.map(|s| s.to_string()),
            key: key_of(key_parts),
        });
        (self.groups.len() - 1) as u32
    }

    /// `name`: the part of the title that counts for the match quality;
    /// `extra`: more words of this entry alone (the full title, an id).
    fn push(&mut self, e: TitleEntry, name: &str, extra: &[&str]) {
        self.names.push(key(name));
        self.extras.push(key_of(extra));
        self.entries.push(e);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The book or folders an entry belongs to (empty for a book itself).
    pub fn context(&self, e: &TitleEntry) -> &str {
        if e.kind == TitleKind::Book {
            ""
        } else {
            &self.groups[e.group as usize].line
        }
    }

    pub fn author(&self, e: &TitleEntry) -> &str {
        &self.groups[e.group as usize].author
    }

    pub fn book_id(&self, e: &TitleEntry) -> Option<&str> {
        self.groups[e.group as usize].book_id.as_deref()
    }

    /// The books (and their chapters and juan) of a CBETA tree.
    pub fn cbeta(bc: &Bookcase, tree_id: &str) -> Result<TitleIndex, String> {
        let nav = bc.nav(tree_id)?;
        let mut t = TitleIndex::new();
        let mut group_of: HashMap<String, u32> = HashMap::new();
        for (href, text) in nav.leaves() {
            let entry = match bc.resolve_leaf(href, text) {
                Some(e) => e,
                None => continue, // kept in the menu, nothing to find (A.4)
            };
            let book = match bc.book(&entry.book_id) {
                Some(b) => b,
                None => continue,
            };
            // the first time a book is met: its group, chapters and juan
            let mut first = None;
            let g = match group_of.get(&book.id) {
                Some(&g) => g,
                None => {
                    let toc = bc.outline(&book.id);
                    let vols = bc.volumes_with_toc(&book.id, toc.as_ref());
                    let juans = vols.iter().filter(|v| v.exists).map(|v| v.idx).collect::<HashSet<_>>().len();
                    let line = if juans > 0 {
                        format!("{} {}（{}卷）", book.id, book.title, juans)
                    } else {
                        format!("{} {}", book.id, book.title)
                    };
                    let g = t.add_group(line, &book.author_info, Some(&book.id), &[&book.id, &book.title, &book.author_info]);
                    group_of.insert(book.id.clone(), g);
                    first = Some((toc, vols, juans));
                    g
                }
            };

            if entry.cloned {
                // a part of a big book: `T0220a 大般若經第一分`
                t.push(
                    TitleEntry {
                        kind: TitleKind::Book,
                        title: entry.title.clone(),
                        group: g,
                        path: entry.start.clone(),
                        lb: None,
                    },
                    text.split([' ', '　']).nth(1).unwrap_or(text),
                    &[text],
                );
            } else {
                let line = t.groups[g as usize].line.clone();
                t.push(
                    TitleEntry {
                        kind: TitleKind::Book,
                        title: line,
                        group: g,
                        path: None,
                        lb: None,
                    },
                    &book.title,
                    &[],
                );
            }

            let (toc, vols, juans) = match first {
                Some(f) => f,
                None => continue,
            };
            let existing: HashSet<&str> = vols.iter().filter(|v| v.exists).map(|v| v.path.as_str()).collect();
            if let Some(toc) = &toc {
                fn walk(t: &mut TitleIndex, items: &[TocItem], existing: &HashSet<&str>, g: u32) {
                    for it in items {
                        let text = it.link.text.trim();
                        if existing.contains(it.link.file.as_str()) && !text.is_empty() {
                            // `1 序品`: the number does not count for the match quality
                            let name = text.trim_start_matches(|c: char| c.is_ascii_digit()).trim();
                            let extra: &[&str] = if name.len() == text.len() { &[] } else { &[text] };
                            t.push(
                                TitleEntry {
                                    kind: TitleKind::Chapter,
                                    title: text.to_string(),
                                    group: g,
                                    path: Some(it.link.file.clone()),
                                    lb: it.link.anchor.as_ref().map(|a| a.trim_start_matches('p').to_string()),
                                },
                                name,
                                extra,
                            );
                        }
                        walk(t, &it.children, existing, g);
                    }
                }
                walk(&mut t, &toc.chapters, &existing, g);
            }
            if juans > 1 {
                for v in vols.iter().filter(|v| v.exists) {
                    t.push(
                        TitleEntry {
                            kind: TitleKind::Juan,
                            title: v.label.clone(),
                            group: g,
                            path: Some(v.path.clone()),
                            lb: None,
                        },
                        &v.label,
                        &[],
                    );
                }
            }
        }
        Ok(t)
    }

    /// The articles of an HTML library.
    pub fn html(tree: &HtmlTree) -> TitleIndex {
        let mut t = TitleIndex::new();
        let mut group_of: HashMap<Vec<String>, u32> = HashMap::new();
        for leaf in &tree.leaves {
            let g = match group_of.get(&leaf.folders) {
                Some(&g) => g,
                None => {
                    let parts: Vec<&str> = leaf.folders.iter().map(|s| s.as_str()).collect();
                    let g = t.add_group(leaf.folders.join(" / "), "", None, &parts);
                    group_of.insert(leaf.folders.clone(), g);
                    g
                }
            };
            t.push(
                TitleEntry {
                    kind: TitleKind::Article,
                    title: leaf.title.clone(),
                    group: g,
                    path: Some(leaf.path.clone()),
                    lb: None,
                },
                &leaf.title,
                &[],
            );
        }
        t
    }

    /// The notes of an internal library (`path` is the note id).
    pub fn notes(rows: &[crate::notes::NoteRow], root_name: &str) -> TitleIndex {
        let mut t = TitleIndex::new();
        let mut group_of: HashMap<Vec<String>, u32> = HashMap::new();
        for r in rows {
            let g = match group_of.get(&r.folders) {
                Some(&g) => g,
                None => {
                    let parts: Vec<&str> = r.folders.iter().map(|s| s.as_str()).collect();
                    let line = if r.folders.is_empty() { root_name.to_string() } else { r.folders.join(" / ") };
                    let g = t.add_group(line, "", None, &parts);
                    group_of.insert(r.folders.clone(), g);
                    g
                }
            };
            t.push(
                TitleEntry { kind: TitleKind::Article, title: r.title.clone(), group: g, path: Some(r.id.to_string()), lb: None },
                &r.title,
                &[],
            );
        }
        t
    }

    /// Article titles from a ZIM archive, with stable archive paths.
    pub fn zim(zim: &crate::zim::Zim, root_name: &str) -> TitleIndex {
        let mut t = TitleIndex::new();
        let g = t.add_group(root_name.to_string(), "", None, &[]);
        for i in zim.article_indices() {
            let e = &zim.entries[i];
            let path = format!("{}/{}", e.namespace as char, e.path);
            t.push(TitleEntry { kind: TitleKind::Article, title: e.title.clone(), group: g, path: Some(path), lb: None }, &e.title, &[]);
        }
        t
    }

    /// The files of a folder library, grouped by their folder.
    pub fn folder(files: &[crate::folder::FolderFile], dirs: &[String], root_name: &str) -> TitleIndex {
        let mut t = TitleIndex::new();
        let mut group_of: HashMap<String, u32> = HashMap::new();
        // the folders themselves come first, each under the folder that holds it
        for d in dirs {
            let (up, name) = d.rsplit_once('/').unwrap_or(("", d));
            let g = match group_of.get(up) {
                Some(&g) => g,
                None => {
                    let line = if up.is_empty() { root_name.to_string() } else { up.replace('/', " / ") };
                    let parts: Vec<&str> = up.split('/').filter(|s| !s.is_empty()).collect();
                    let g = t.add_group(line, "", None, &parts);
                    group_of.insert(up.to_string(), g);
                    g
                }
            };
            t.push(TitleEntry { kind: TitleKind::Folder, title: name.to_string(), group: g, path: Some(d.clone()), lb: None }, name, &[]);
        }
        for f in files {
            let dir = f.rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("").to_string();
            let g = match group_of.get(&dir) {
                Some(&g) => g,
                None => {
                    let line = if dir.is_empty() { root_name.to_string() } else { dir.replace('/', " / ") };
                    let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
                    let g = t.add_group(line, "", None, &parts);
                    group_of.insert(dir, g);
                    g
                }
            };
            let title = crate::folder::title_of(&f.name);
            t.push(
                TitleEntry { kind: TitleKind::Article, title: title.to_string(), group: g, path: Some(f.rel.clone()), lb: None },
                title,
                &[&f.name],
            );
        }
        t
    }

    fn pinyin_index(&self) -> Arc<PinyinTitleIndex> {
        if let Some(p) = self.pinyin.read().unwrap().as_ref() {
            return p.clone();
        }
        let built = Arc::new(PinyinTitleIndex {
            names: self.names.iter().map(|s| pinyin_key(s)).collect(),
            extras: self.extras.iter().map(|s| pinyin_key(s)).collect(),
            groups: self.groups.iter().map(|g| pinyin_key(&g.key)).collect(),
            aliases: self
                .aliases
                .iter()
                .map(|(&k, v)| (k, v.iter().map(|s| pinyin_key(s)).collect()))
                .collect(),
        });
        *self.pinyin.write().unwrap() = Some(built.clone());
        built
    }

    /// Drop the cached pinyin keys so the next pinyin search rebuilds them
    /// (called after the user updates their custom pinyin dictionary).
    pub fn invalidate_pinyin(&self) {
        *self.pinyin.write().unwrap() = None;
    }

    /// Every entry matching all `words`, with its rank, in tree order (the
    /// caller sorts). No words: every entry, all with the same rank.
    pub fn search(&self, words: &[String]) -> Vec<(Rank, usize)> {
        self.search_with_pinyin(words, false)
    }

    /// Like [`TitleIndex::search`], optionally matching ASCII alphabetic keywords
    /// against full pinyin and pinyin initials.
    pub fn search_with_pinyin(&self, words: &[String], use_pinyin: bool) -> Vec<(Rank, usize)> {
        if words.is_empty() {
            let r = Rank { pinyin: 0, tier: 0, kind: 0, inexact: 0, len: 0 };
            return (0..self.entries.len()).map(|i| (r, i)).collect();
        }
        let py_arc = if use_pinyin { Some(self.pinyin_index()) } else { None };
        let py = py_arc.as_deref();
        let is_py_word = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphabetic());
        let py_contains = |pair: &(String, String), w: &str| pair.0.contains(w) || pair.1.contains(w);

        let joined: String = words.concat();
        let mut out = Vec::new();
        for (i, name) in self.names.iter().enumerate() {
            let e = &self.entries[i];
            let mut in_name = true;
            let mut used_py = false;
            let all = words.iter().all(|w| {
                let ws = w.as_str();
                if name.contains(ws) {
                    return true;
                }
                if let Some(py) = py {
                    if is_py_word(ws) && py_contains(&py.names[i], ws) {
                        used_py = true;
                        return true;
                    }
                }
                in_name = false;
                if self.extras[i].contains(ws) || self.groups[e.group as usize].key.contains(ws) {
                    return true;
                }
                if let Some(py) = py {
                    if is_py_word(ws)
                        && (py_contains(&py.extras[i], ws)
                            || py_contains(&py.groups[e.group as usize], ws))
                    {
                        used_py = true;
                        return true;
                    }
                }
                false
            });
            // another name of the entry may match better than the title
            let by_alias = self.aliases.get(&i).and_then(|list| {
                let py_list = py.and_then(|p| p.aliases.get(&i));
                list.iter()
                    .enumerate()
                    .filter_map(|(ai, a)| {
                        let mut a_py = false;
                        let ok = words.iter().all(|w| {
                            let ws = w.as_str();
                            if a.contains(ws) {
                                return true;
                            }
                            if let Some(pl) = py_list {
                                if is_py_word(ws) && py_contains(&pl[ai], ws) {
                                    a_py = true;
                                    return true;
                                }
                            }
                            false
                        });
                        if !ok {
                            return None;
                        }
                        let w0 = words[0].as_str();
                        let starts = a.starts_with(w0)
                            || (a_py
                                && py_list.is_some_and(|pl| {
                                    pl[ai].0.starts_with(w0) || pl[ai].1.starts_with(w0)
                                }));
                        let exact = *a == joined
                            || (a_py
                                && py_list
                                    .is_some_and(|pl| pl[ai].0 == joined || pl[ai].1 == joined));
                        Some(Rank {
                            pinyin: a_py as u8,
                            tier: (!starts) as u8,
                            kind: e.kind.order(),
                            inexact: (!exact) as u8,
                            len: a.chars().count() as u32,
                        })
                    })
                    .min()
            });
            if !all {
                if let Some(r) = by_alias {
                    out.push((r, i));
                }
                continue;
            }
            let w0 = words[0].as_str();
            let tier = if !in_name {
                2
            } else if name.starts_with(w0)
                || (used_py
                    && py.is_some_and(|p| p.names[i].0.starts_with(w0) || p.names[i].1.starts_with(w0)))
            {
                0
            } else {
                1
            };
            let exact = *name == joined
                || (used_py
                    && py.is_some_and(|p| p.names[i].0 == joined || p.names[i].1 == joined));
            let own = Rank {
                pinyin: used_py as u8,
                tier,
                kind: e.kind.order(),
                inexact: (!exact) as u8,
                len: if tier < 2 { name.chars().count() as u32 } else { 0 },
            };
            out.push((by_alias.map_or(own, |a| a.min(own)), i));
        }
        out
    }

    /// Best matches first, at most `limit`; also the number of matches.
    pub fn top(&self, query: &str, limit: usize) -> (usize, Vec<&TitleEntry>) {
        self.top_with_pinyin(query, limit, false)
    }

    /// Like [`TitleIndex::top`], optionally matching pinyin.
    pub fn top_with_pinyin(&self, query: &str, limit: usize, use_pinyin: bool) -> (usize, Vec<&TitleEntry>) {
        let mut m = self.search_with_pinyin(&query_words(query), use_pinyin);
        let total = m.len();
        m.sort();
        (total, m.into_iter().take(limit).map(|(_, i)| &self.entries[i]).collect())
    }
}

// ------------------------------------------------------------ on-disk cache

/// A built title list is kept on disk, so that a restart does not read the
/// whole source again: the CBETA 部類 tree has 320 000 entries and takes
/// ~0.6 s to build, and a large ZIM has to be parsed whole (over a second)
/// before its titles exist. The image is a compact binary — as JSON the same
/// list is 40 MB, which would take as long to read as to build.
///
/// `source` is whatever identifies the current data (a ZIM's size and
/// modification time, a bookcase's data version and tree); an image built for
/// another `source` is ignored.
const CACHE_MAGIC: &[u8; 4] = b"SLT1";

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.b.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_le_bytes(s.try_into().ok()?))
    }
    fn string(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        String::from_utf8(s.to_vec()).ok()
    }
    fn opt_string(&mut self) -> Option<Option<String>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.string()?)),
            _ => None,
        }
    }
}

impl TitleKind {
    fn code(self) -> u8 {
        match self {
            TitleKind::Book => 0,
            TitleKind::Article => 1,
            TitleKind::Folder => 2,
            TitleKind::Chapter => 3,
            TitleKind::Juan => 4,
        }
    }
    fn from_code(c: u8) -> Option<TitleKind> {
        Some(match c {
            0 => TitleKind::Book,
            1 => TitleKind::Article,
            2 => TitleKind::Folder,
            3 => TitleKind::Chapter,
            4 => TitleKind::Juan,
            _ => return None,
        })
    }
}

/// A list larger than this is certainly not one of ours (a damaged file).
const MAX_CACHED_ENTRIES: usize = 20_000_000;

impl TitleIndex {
    /// The bytes a cache file holds for this list, built from `source`.
    pub fn store(&self, source: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.entries.len() * 64 + 1024);
        out.extend_from_slice(CACHE_MAGIC);
        put_str(&mut out, source);
        out.extend_from_slice(&(self.groups.len() as u32).to_le_bytes());
        for g in &self.groups {
            put_str(&mut out, &g.line);
            put_str(&mut out, &g.author);
            match &g.book_id {
                Some(id) => {
                    out.push(1);
                    put_str(&mut out, id);
                }
                None => out.push(0),
            }
            put_str(&mut out, &g.key);
        }
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            out.push(e.kind.code());
            out.extend_from_slice(&e.group.to_le_bytes());
            put_str(&mut out, &e.title);
            match &e.path {
                Some(p) => {
                    out.push(1);
                    put_str(&mut out, p);
                }
                None => out.push(0),
            }
            match &e.lb {
                Some(lb) => {
                    out.push(1);
                    put_str(&mut out, lb);
                }
                None => out.push(0),
            }
        }
        for name in &self.names {
            put_str(&mut out, name);
        }
        for extra in &self.extras {
            put_str(&mut out, extra);
        }
        out
    }

    /// Read back what [`TitleIndex::store`] wrote. `None` when the bytes are
    /// damaged or were built for another `source` (then the list is rebuilt).
    pub fn restore(data: &[u8], source: &str) -> Option<TitleIndex> {
        if data.get(..4)? != CACHE_MAGIC {
            return None;
        }
        let mut r = Reader { b: data, at: 4 };
        if r.string()? != source {
            return None;
        }
        let group_count = r.u32()? as usize;
        if group_count > MAX_CACHED_ENTRIES {
            return None;
        }
        let mut groups = Vec::with_capacity(group_count);
        for _ in 0..group_count {
            groups.push(TitleGroup {
                line: r.string()?,
                author: r.string()?,
                book_id: r.opt_string()?,
                key: r.string()?,
            });
        }
        let entry_count = r.u32()? as usize;
        if entry_count > MAX_CACHED_ENTRIES {
            return None;
        }
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let kind = TitleKind::from_code(r.u8()?)?;
            let group = r.u32()?;
            if group as usize >= groups.len() {
                return None;
            }
            entries.push(TitleEntry { kind, group, title: r.string()?, path: r.opt_string()?, lb: r.opt_string()? });
        }
        let mut names = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            names.push(r.string()?);
        }
        let mut extras = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            extras.push(r.string()?);
        }
        Some(TitleIndex {
            entries,
            groups,
            names,
            extras,
            aliases: HashMap::new(),
            pinyin: RwLock::new(None),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::source::MemSource;
    use crate::text::encode_utf16le_bom;

    const T0262: &str = include_str!("../../samples/cbeta/toc_T0262.xml");

    fn base(src: &mut MemSource, catalog: &str, menu: &str) {
        src.add("index.xml", "<metadata><version>1</version><date>2025-04-26</date></metadata>");
        src.add("bookdata.txt", encode_utf16le_bom("T,2,【大】,大正新脩大藏經,Taisho\n"));
        src.add("catalog.txt", catalog);
        src.add("menu_nav.xhtml", menu);
        src.add("spine.txt", "");
    }

    fn fixture() -> Bookcase {
        let mut src = MemSource::new();
        base(
            &mut src,
            "T , 法華部類 , , 09 , 0262 , 7 , 妙法蓮華經 , 姚秦 鳩摩羅什譯\n\
             T , 般若部類 , , 05 , 0220 , 200 , 大般若波羅蜜多經 , 唐 玄奘譯\n",
            r#"<nav><navlink href="bulei_nav.xhtml">部類</navlink></nav>"#,
        );
        let nav = r#"<nav type="catalog"><span>大正</span><ol>
<li><cblink href="XML/T/T09/T09n0262_001.xml">T0262 妙法蓮華經</cblink></li>
<li><cblink href="XML/T/T05/T05n0220_001.xml">T0220a 大般若經第一分</cblink></li>
<li><cblink href="XML/T/T99/T99n9999_001.xml">T9999 不存在</cblink></li></ol></nav>"#;
        for n in ["simple_nav.xhtml", "advance_nav.xhtml", "bulei_nav.xhtml"] {
            src.add(n, nav);
        }
        src.add("toc/T/T0262.xml", T0262);
        for i in 1..=7 {
            src.add(&format!("XML/T/T09/T09n0262_{:03}.xml", i), "x");
        }
        src.add("XML/T/T05/T05n0220_001.xml", "x");
        Bookcase::open(Box::new(src)).unwrap()
    }

    #[test]
    fn lists_books_chapters_and_juan_of_a_tree() {
        let bc = fixture();
        let t = TitleIndex::cbeta(&bc, "bulei").unwrap();
        let first = &t.entries[0];
        assert_eq!(first.kind, TitleKind::Book);
        assert_eq!(first.title, "T0262 妙法蓮華經（7卷）");
        assert!(first.path.is_none());
        let chapters = t.entries.iter().filter(|e| e.kind == TitleKind::Chapter).count();
        let juan = t.entries.iter().filter(|e| e.kind == TitleKind::Juan).count();
        assert_eq!(juan, 7);
        assert!(chapters >= 30);
        // a part of a big book is a book entry that opens at its own start
        let part = t.entries.iter().find(|e| e.title.starts_with("T0220a")).unwrap();
        assert_eq!(part.path.as_deref(), Some("XML/T/T05/T05n0220_001.xml"));
        // the unresolved leaf gives nothing
        assert!(!t.entries.iter().any(|e| e.title.contains("不存在")));
    }

    #[test]
    fn finds_titles_regardless_of_script_and_ranks_them() {
        let bc = fixture();
        let t = TitleIndex::cbeta(&bc, "bulei").unwrap();
        // simplified query, traditional data
        let (n, top) = t.top("序品", 10);
        assert!(n >= 1);
        assert_eq!(top[0].title, "1 序品");
        assert_eq!(top[0].lb.as_deref(), Some("0001c18"));
        assert_eq!(top[0].path.as_deref(), Some("XML/T/T09/T09n0262_001.xml"));
        assert_eq!(t.context(top[0]), "T0262 妙法蓮華經（7卷）");
        assert_eq!(t.author(top[0]), "姚秦 鳩摩羅什譯");
        // the book first, then chapters that only match by their book
        let (_, top) = t.top("妙法莲华经", 3);
        assert_eq!(top[0].kind, TitleKind::Book);
        assert_eq!(t.context(top[0]), "");
        // keywords: book + chapter
        let (n, top) = t.top("妙法 普门品", 5);
        assert_eq!(n, 2); // the chapter and the preface 御製觀世音普門品經序
        assert!(top.iter().any(|e| e.title == "25 觀世音菩薩普門品"));
        // book id
        let (_, top) = t.top("t0262", 1);
        assert_eq!(t.book_id(top[0]), Some("T0262"));
        assert_eq!(top[0].kind, TitleKind::Book);
        // juan with its book
        let (_, top) = t.top("莲华经 卷三", 1);
        assert_eq!(top[0].path.as_deref(), Some("XML/T/T09/T09n0262_003.xml"));
        // empty query lists everything in tree order
        assert_eq!(t.top("", 5).0, t.len());
        assert_eq!(t.top("没有这个", 5).0, 0);
    }

    #[test]
    fn lists_the_articles_of_an_html_library() {
        let mut src = MemSource::new();
        base(&mut src, "", r#"<nav><navlink href="zhibei_nav.xhtml">索达吉</navlink></nav>"#);
        for n in ["simple_nav.xhtml", "advance_nav.xhtml", "bulei_nav.xhtml"] {
            src.add(n, "<nav></nav>");
        }
        src.add(
            "zhibei_nav.xhtml",
            r#"<nav han-lang="hans"><li><span>著作</span><ol><li><span>悲惨世界</span><ol>
<li><a href="a/ZB/z/1.html">童年的梦</a></li><li><a href="a/ZB/z/2.html">狗王之死</a></li></ol></li></ol></li></nav>"#,
        );
        src.add("a/ZB/z/1.html", "<p>x</p>");
        src.add("a/ZB/z/2.html", "<p>y</p>");
        let bc = Bookcase::open(Box::new(src)).unwrap();
        let t = TitleIndex::html(&HtmlTree::open(&bc, "zhibei").unwrap());
        assert_eq!(t.len(), 2);
        assert_eq!(t.groups.len(), 1);
        // traditional query, simplified data; the folders count as context
        let (n, top) = t.top("童年 悲慘", 5);
        assert_eq!(n, 1);
        assert_eq!(top[0].path.as_deref(), Some("a/ZB/z/1.html"));
        assert_eq!(t.context(top[0]), "著作 / 悲惨世界");
        assert_eq!(t.top("悲惨世界", 5).0, 2);
        // another name of an article finds it, and counts like the title
        let mut t = t;
        assert_eq!(t.top("少年", 5).0, 0);
        let mut map = HashMap::new();
        map.insert((String::new(), "a/ZB/z/1.html".to_string()), vec!["少年时".to_string()]);
        t.add_aliases(&map);
        let (n, top) = t.top("少年", 5);
        assert_eq!((n, top[0].path.as_deref()), (1, Some("a/ZB/z/1.html")));
    }

    #[test]
    fn lists_the_folders_of_a_folder_library() {
        let files = vec![crate::folder::FolderFile { rel: "甲/乙/文.md".into(), name: "文.md".into(), size: 1 }];
        let dirs = vec!["甲".to_string(), "甲/乙".to_string()];
        let t = TitleIndex::folder(&files, &dirs, "库");
        let (n, top) = t.top("乙", 5);
        assert_eq!(n, 2); // the folder, and the file inside it (its folder is context)
        assert_eq!((top[0].kind, top[0].path.as_deref(), t.context(top[0])), (TitleKind::Folder, Some("甲/乙"), "甲"));
    }

    #[test]
    fn caches_and_restores_a_title_list() {
        let bc = fixture();
        let t = TitleIndex::cbeta(&bc, "bulei").unwrap();
        let data = t.store("bookcase-1:bulei");
        let back = TitleIndex::restore(&data, "bookcase-1:bulei").unwrap();
        assert_eq!(back.len(), t.len());
        assert_eq!(back.groups.len(), t.groups.len());
        assert_eq!((back.entries[0].kind, back.entries[0].title.as_str()), (t.entries[0].kind, t.entries[0].title.as_str()));
        let (n1, top1) = t.top("法華", 5);
        let (n2, top2) = back.top("法華", 5);
        assert_eq!(n1, n2);
        assert_eq!(top1.iter().map(|e| &e.title).collect::<Vec<_>>(), top2.iter().map(|e| &e.title).collect::<Vec<_>>());
        // another source, or a damaged file: rebuild instead of trusting it
        assert!(TitleIndex::restore(&data, "bookcase-2:bulei").is_none());
        assert!(TitleIndex::restore(&data[..data.len() / 2], "bookcase-1:bulei").is_none());
        assert!(TitleIndex::restore(b"nope", "bookcase-1:bulei").is_none());
    }

    #[test]
    fn matches_titles_by_pinyin_when_enabled() {
        let bc = fixture();
        let t = TitleIndex::cbeta(&bc, "bulei").unwrap();
        // disabled by default
        assert_eq!(t.top_with_pinyin("miaofa", 5, false).0, 0);
        // full pinyin
        let (n, top) = t.top_with_pinyin("miaofalianhua", 5, true);
        assert!(n >= 1);
        assert_eq!(top[0].kind, TitleKind::Book);
        assert!(top[0].title.contains("妙法蓮華經"));
        // pinyin initials
        let (n, top) = t.top_with_pinyin("xp", 5, true);
        assert!(n >= 1);
        assert_eq!(top[0].title, "1 序品");
        // mixed book pinyin + chapter Chinese, and polyphone 大般若 -> bore
        let (n, top) = t.top_with_pinyin("miaofa 普门品", 5, true);
        assert_eq!(n, 2);
        assert!(top.iter().any(|e| e.title == "25 觀世音菩薩普門品"));
        let (n, top) = t.top_with_pinyin("dabore", 5, true);
        assert!(n >= 1);
        assert!(top[0].title.contains("大般若"));
    }
}
