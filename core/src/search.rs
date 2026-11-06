//! M0c: full-text index and search for a CBETA bookcase (design-spec §7, §8).
//!
//! One Tantivy document per volume file (卷). The index lives in its own
//! directory and can be deleted and rebuilt at any time.
//!
//! Fields:
//! * `path`  volume path, e.g. `XML/T/T09/T09n0262_001.xml` (stored, id)
//! * `book`, `juan`, `title`                                 (stored)
//! * `body`  `aio` terms: bigrams, words, single-char runs, with positions
//! * `body1` CJK unigrams, frequencies only (one-char queries need no positions)
//! * `facet` `/bulei/部類/子類`, `/canon/T`, `/kind/论/经释`, `/period/唐`,
//!   `/person/玄奘`, `/book/T0262`
//!
//! Hit positions are **not** stored: for the hits being shown, the volume is
//! extracted again (`Bookcase::volume_text`, ~1.5 ms) and `locate` finds every
//! occurrence with its line and position in the line.

use crate::analyzer::{self, TermKind};
use crate::bookcase::Bookcase;
use crate::bookinfo::bookinfo;
use crate::volume::VolumeText;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use tantivy::collector::{Count, FacetCollector, TopDocs};
use tantivy::query::{BooleanQuery, Occur, PhraseQuery, Query, TermQuery};
use tantivy::schema::{
    Facet, FacetOptions, Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED,
    STRING,
};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexReader, ReloadPolicy, TantivyDocument, Term};

/// Bumped whenever the schema or the analyzer changes: an index with another
/// version must be rebuilt.
pub const INDEX_VERSION: u32 = 2;
const META_FILE: &str = "smart-library-index.json";

// ------------------------------------------------------------ tokenizers

#[derive(Clone, Default)]
struct AioTokenizer;

#[derive(Clone, Default)]
struct AioUnigramTokenizer;

struct VecTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for VecTokenStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }
    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}

impl Tokenizer for AioTokenizer {
    type TokenStream<'a> = VecTokenStream;
    fn token_stream<'a>(&'a mut self, text: &'a str) -> VecTokenStream {
        let tokens = analyzer::terms(text)
            .into_iter()
            .map(|t| Token {
                offset_from: t.from,
                offset_to: t.to,
                position: t.position,
                text: t.text,
                position_length: 1,
            })
            .collect();
        VecTokenStream { tokens, index: 0 }
    }
}

impl Tokenizer for AioUnigramTokenizer {
    type TokenStream<'a> = VecTokenStream;
    fn token_stream<'a>(&'a mut self, text: &'a str) -> VecTokenStream {
        let tokens = analyzer::unigrams(text)
            .into_iter()
            .enumerate()
            .map(|(i, u)| Token {
                offset_from: u.from,
                offset_to: u.to,
                position: i,
                text: u.text,
                position_length: 1,
            })
            .collect();
        VecTokenStream { tokens, index: 0 }
    }
}

// ------------------------------------------------------------ schema

#[derive(Clone, Copy)]
struct Fields {
    path: Field,
    book: Field,
    juan: Field,
    title: Field,
    body: Field,
    body1: Field,
    facet: Field,
}

fn schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let body_opts = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer("aio")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let body1_opts = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer("aio1")
            .set_index_option(IndexRecordOption::WithFreqs),
    );
    let f = Fields {
        path: b.add_text_field("path", STRING | STORED),
        book: b.add_text_field("book", STRING | STORED),
        juan: b.add_u64_field("juan", STORED),
        title: b.add_text_field("title", STORED),
        body: b.add_text_field("body", body_opts),
        body1: b.add_text_field("body1", body1_opts),
        facet: b.add_facet_field("facet", FacetOptions::default()),
    };
    (b.build(), f)
}

fn register_tokenizers(index: &Index) {
    index.tokenizers().register("aio", AioTokenizer);
    index.tokenizers().register("aio1", AioUnigramTokenizer);
}

/// Facet dimensions, in display order: (root, label).
pub const FACET_DIMENSIONS: [(&str, &str); 5] = [
    ("/bulei", "部类"),
    ("/canon", "藏经"),
    ("/kind", "典类"),
    ("/period", "时域"),
    ("/person", "人物"),
];

// ------------------------------------------------------------ index

pub struct SearchIndex {
    index: Index,
    reader: IndexReader,
    f: Fields,
}

#[derive(Debug, Default)]
pub struct BuildStats {
    pub volumes: usize,
    /// listed volumes that could not be read or parsed: skipped (design-spec A.4)
    pub skipped: Vec<String>,
    pub seconds: f64,
}

impl SearchIndex {
    /// Build a new index for the whole bookcase into `dir` (which must be
    /// empty or absent). `progress(done, total)` is called now and then.
    pub fn build(
        bc: &Bookcase,
        dir: &Path,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<BuildStats, String> {
        let start = std::time::Instant::now();
        if dir.exists() && std::fs::read_dir(dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
            return Err(format!("{} is not empty; delete it to rebuild the index", dir.display()));
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        let (schema, f) = schema();
        let index = Index::create_in_dir(dir, schema).map_err(|e| e.to_string())?;
        register_tokenizers(&index);
        let mut writer = index.writer::<TantivyDocument>(1_000_000_000).map_err(|e| e.to_string())?;

        // every existing volume of every registered book (LC, Y are not registered)
        let mut jobs = Vec::new();
        for id in &bc.catalog.order {
            for v in bc.volumes(id) {
                if v.exists {
                    jobs.push((id.clone(), v));
                }
            }
        }
        let total = jobs.len();
        let mut stats = BuildStats::default();
        let info = bookinfo();
        for (n, (book_id, vol)) in jobs.iter().enumerate() {
            if n % 500 == 0 {
                progress(n, total);
            }
            let vt = match bc.volume_text(&vol.path) {
                Ok(v) => v,
                Err(e) => {
                    stats.skipped.push(e);
                    continue;
                }
            };
            let book = match bc.book(book_id) {
                Some(b) => b,
                None => continue,
            };
            let mut doc = TantivyDocument::default();
            doc.add_text(f.path, &vol.path);
            doc.add_text(f.book, book_id);
            doc.add_u64(f.juan, vol.idx as u64);
            doc.add_text(f.title, &book.title);
            // index the punctuated text: punctuation is transparent to `aio`,
            // and whitespace still separates Latin words
            doc.add_text(f.body, &vt.text);
            doc.add_text(f.body1, &vt.text);
            doc.add_facet(f.facet, Facet::from_path(["canon", book.canon.as_str()]));
            doc.add_facet(f.facet, Facet::from_path(["book", book_id.as_str()]));
            let mut bulei = vec!["bulei"];
            bulei.extend(book.catalog.split('/').filter(|s| !s.is_empty()));
            if bulei.len() > 1 {
                doc.add_facet(f.facet, Facet::from_path(bulei));
            }
            if let Some(bi) = info.get(book_id) {
                if !bi.kind_label.is_empty() {
                    let mut kind = vec!["kind"];
                    kind.extend(bi.kind_label.split('：'));
                    doc.add_facet(f.facet, Facet::from_path(kind));
                }
                for p in &bi.periods {
                    doc.add_facet(f.facet, Facet::from_path(["period", p.as_str()]));
                }
                for a in &bi.authors {
                    doc.add_facet(f.facet, Facet::from_path(["person", a.as_str()]));
                }
            }
            writer.add_document(doc).map_err(|e| e.to_string())?;
            stats.volumes += 1;
        }
        progress(total, total);
        writer.commit().map_err(|e| e.to_string())?;
        writer.wait_merging_threads().map_err(|e| e.to_string())?;

        let meta = serde_json::json!({
            "index_version": INDEX_VERSION,
            "data_version": bc.data_version(),
            "bookcase_version": bc.version,
            "bookcase_date": bc.date,
            "volumes": stats.volumes,
        });
        std::fs::write(dir.join(META_FILE), serde_json::to_string_pretty(&meta).unwrap())
            .map_err(|e| e.to_string())?;
        stats.seconds = start.elapsed().as_secs_f64();
        Ok(stats)
    }

    pub fn open(dir: &Path) -> Result<SearchIndex, String> {
        let meta = std::fs::read_to_string(dir.join(META_FILE))
            .map_err(|_| format!("no index in {} (run `index` first)", dir.display()))?;
        let meta: serde_json::Value = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
        if meta["index_version"].as_u64() != Some(INDEX_VERSION as u64) {
            return Err(format!(
                "index in {} has version {}, expected {}: delete it and rebuild",
                dir.display(),
                meta["index_version"],
                INDEX_VERSION
            ));
        }
        let index = Index::open_in_dir(dir).map_err(|e| e.to_string())?;
        register_tokenizers(&index);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e: tantivy::TantivyError| e.to_string())?;
        let (_, f) = schema();
        Ok(SearchIndex { index, reader, f })
    }

    /// Data version recorded when the index was built.
    pub fn data_version(dir: &Path) -> Option<String> {
        let meta = std::fs::read_to_string(dir.join(META_FILE)).ok()?;
        let meta: serde_json::Value = serde_json::from_str(&meta).ok()?;
        meta["data_version"].as_str().map(|s| s.to_string())
    }

    pub fn search(&self, req: &SearchRequest) -> Result<SearchResult, String> {
        let _ = &self.index;
        let query = build_query(&self.f, &req.query, req.mode)?;
        let query: Box<dyn Query> = match &req.scope {
            Some(scope) => {
                // `bulei/阿含部類` or `/bulei/阿含部類` (shells such as Git Bash
                // rewrite arguments that start with `/`)
                let text = if scope.starts_with('/') { scope.clone() } else { format!("/{}", scope) };
                let facet = Facet::from_text(&text).map_err(|e| format!("bad scope {}: {}", scope, e))?;
                Box::new(BooleanQuery::new(vec![
                    (Occur::Must, query),
                    (
                        Occur::Must,
                        Box::new(TermQuery::new(Term::from_facet(self.f.facet, &facet), IndexRecordOption::Basic)),
                    ),
                ]))
            }
            None => query,
        };
        let searcher = self.reader.searcher();
        let mut fc = FacetCollector::for_field("facet");
        fc.add_facet("/book");
        for (root, _) in FACET_DIMENSIONS {
            fc.add_facet(root);
        }
        let drill: Vec<String> = req
            .facet_drill
            .iter()
            .map(|d| if d.starts_with('/') { d.clone() } else { format!("/{}", d) })
            .collect();
        // drill-down roots lie under the roots above, and one collector may not
        // hold a facet and its descendant: they get a collector of their own
        // (nested drill-downs are dropped for the same reason)
        let drill: Vec<String> = drill
            .iter()
            .filter(|d| !drill.iter().any(|o| o != *d && d.starts_with(&format!("{}/", o))))
            .cloned()
            .collect();
        let mut fc_drill = FacetCollector::for_field("facet");
        for sub in &drill {
            fc_drill.add_facet(sub.as_str());
        }
        let top = TopDocs::with_limit(req.limit.max(1)).and_offset(req.offset).order_by_score();
        let (top, total, counts, drill_counts) = searcher
            .search(&query, &(top, Count, fc, fc_drill))
            .map_err(|e| e.to_string())?;

        let mut hits = Vec::new();
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr).map_err(|e| e.to_string())?;
            let s = |field: Field| doc.get_first(field).and_then(|v| v.as_str()).unwrap_or("").to_string();
            hits.push(Hit {
                score,
                path: s(self.f.path),
                book: s(self.f.book),
                title: s(self.f.title),
                juan: doc.get_first(self.f.juan).and_then(|v| v.as_u64()).unwrap_or(0) as u16,
            });
        }
        let mut facets = Vec::new();
        let roots: Vec<String> = FACET_DIMENSIONS
            .iter()
            .map(|(r, _)| r.to_string())
            .chain(drill.iter().cloned())
            .collect();
        for root in roots {
            let source = if drill.contains(&root) { &drill_counts } else { &counts };
            let mut values: Vec<(String, u64)> = source
                .get(root.as_str())
                .map(|(f, c)| (f.to_path().last().unwrap_or(&"").to_string(), c))
                .collect();
            values.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            facets.push(FacetCount { root, values });
        }
        let books = counts.get("/book").count();
        Ok(SearchResult {
            total_volumes: total,
            total_books: books,
            hits,
            facets,
        })
    }
}

// ------------------------------------------------------------ queries

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// every term of the query in the same volume, in any order (default)
    Loose,
    /// the query as a contiguous phrase (punctuation ignored)
    Phrase,
}

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub mode: MatchMode,
    pub offset: usize,
    pub limit: usize,
    /// facet filter, e.g. `/bulei/阿含部類` or `/canon/T`
    pub scope: Option<String>,
    /// extra facet roots to count, e.g. `/bulei/阿含部類` for its sub-categories
    pub facet_drill: Vec<String>,
}

impl SearchRequest {
    pub fn new(query: &str) -> Self {
        SearchRequest {
            query: query.to_string(),
            mode: MatchMode::Loose,
            offset: 0,
            limit: 10,
            scope: None,
            facet_drill: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub score: f32,
    pub path: String,
    pub book: String,
    pub title: String,
    pub juan: u16,
}

#[derive(Debug, Clone)]
pub struct FacetCount {
    /// e.g. `/bulei`
    pub root: String,
    /// (child name, number of hit volumes), largest first
    pub values: Vec<(String, u64)>,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    /// number of hit volumes (documents)
    pub total_volumes: usize,
    /// number of distinct books among the hits
    pub total_books: usize,
    pub hits: Vec<Hit>,
    /// counts are numbers of hit **volumes**
    pub facets: Vec<FacetCount>,
}

/// Terms of a query. `Loose`: whitespace separates keywords, each analysed
/// on its own (no bigram across the space); `Phrase`: the whole query.
pub fn query_terms(q: &str, mode: MatchMode) -> Vec<analyzer::Term> {
    match mode {
        MatchMode::Phrase => analyzer::terms(q),
        MatchMode::Loose => q.split_whitespace().flat_map(analyzer::terms).collect(),
    }
}

fn build_query(f: &Fields, q: &str, mode: MatchMode) -> Result<Box<dyn Query>, String> {
    let terms = query_terms(q, mode);
    if terms.is_empty() {
        return Err("empty query".to_string());
    }
    let term_of = |t: &analyzer::Term| match t.kind {
        TermKind::Single => Term::from_field_text(f.body1, &t.text),
        _ => Term::from_field_text(f.body, &t.text),
    };
    let tq = |t: &analyzer::Term| -> Box<dyn Query> {
        Box::new(TermQuery::new(term_of(t), IndexRecordOption::WithFreqs))
    };
    if terms.len() == 1 {
        return Ok(tq(&terms[0]));
    }
    match mode {
        MatchMode::Loose => {
            let mut seen = HashSet::new();
            let clauses: Vec<(Occur, Box<dyn Query>)> = terms
                .iter()
                .filter(|t| seen.insert((t.kind == TermKind::Single, t.text.clone())))
                .map(|t| (Occur::Must, tq(t)))
                .collect();
            Ok(Box::new(BooleanQuery::new(clauses)))
        }
        MatchMode::Phrase => {
            // consecutive non-single terms form phrases; a single-char run
            // (only next to a symbol or a Latin word) is required on its own
            let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
            let mut group: Vec<Term> = Vec::new();
            let flush = |group: &mut Vec<Term>, clauses: &mut Vec<(Occur, Box<dyn Query>)>| {
                match group.len() {
                    0 => {}
                    1 => clauses.push((
                        Occur::Must,
                        Box::new(TermQuery::new(group.pop().unwrap(), IndexRecordOption::WithFreqs)),
                    )),
                    _ => clauses.push((Occur::Must, Box::new(PhraseQuery::new(std::mem::take(group))))),
                }
            };
            for t in &terms {
                if t.kind == TermKind::Single {
                    flush(&mut group, &mut clauses);
                    clauses.push((Occur::Must, tq(t)));
                } else {
                    group.push(term_of(t));
                }
            }
            flush(&mut group, &mut clauses);
            if clauses.len() == 1 {
                Ok(clauses.pop().unwrap().1)
            } else {
                Ok(Box::new(BooleanQuery::new(clauses)))
            }
        }
    }
}

// ------------------------------------------------------------ locate

/// One occurrence inside a volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// char range in `plain`
    pub from: usize,
    pub to: usize,
    /// `lb n` of the line where it starts, e.g. `0010b28`
    pub lb: String,
    /// 1-based position in that line (punctuation not counted)
    pub pos: usize,
}

/// Every occurrence of the query in one volume, in text order.
/// `Phrase`: the whole query (punctuation ignored); `Loose`: each term on its
/// own (bigrams, words, single chars).
pub fn locate(vt: &VolumeText, query: &str, mode: MatchMode) -> Vec<Occurrence> {
    // folded plain text; a char that expands under NFKC keeps its index
    let mut hay: Vec<char> = Vec::with_capacity(vt.plain.len() / 3);
    let mut hay_idx: Vec<usize> = Vec::with_capacity(vt.plain.len() / 3);
    let mut buf = String::new();
    for (i, c) in vt.plain.chars().enumerate() {
        buf.clear();
        analyzer::fold(c, &mut buf);
        for f in buf.chars() {
            hay.push(f);
            hay_idx.push(i);
        }
    }
    let needles: Vec<Vec<char>> = match mode {
        MatchMode::Phrase => vec![analyzer::match_key(query)],
        MatchMode::Loose => {
            let mut seen = HashSet::new();
            query_terms(query, mode)
                .into_iter()
                .map(|t| t.text.chars().collect::<Vec<char>>())
                .filter(|n| seen.insert(n.clone()))
                .collect()
        }
    };
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for n in needles.iter().filter(|n| !n.is_empty()) {
        if n.len() > hay.len() {
            continue;
        }
        for start in 0..=hay.len() - n.len() {
            if hay[start..start + n.len()] == n[..] {
                spans.push((hay_idx[start], hay_idx[start + n.len() - 1] + 1));
            }
        }
    }
    spans.sort();
    spans.dedup();
    spans
        .into_iter()
        .map(|(from, to)| {
            let (lb, pos) = vt
                .locate(from)
                .map(|(l, p)| (l.lb.clone().unwrap_or_default(), p))
                .unwrap_or_default();
            Occurrence { from, to, lb, pos }
        })
        .collect()
}

/// A piece of the punctuated text around one or more nearby occurrences.
#[derive(Debug, Clone)]
pub struct Snippet {
    /// line of the first occurrence
    pub lb: String,
    pub pos: usize,
    /// (text, highlighted?)
    pub parts: Vec<(String, bool)>,
}

/// Group occurrences that are close together and cut snippets from the
/// punctuated text, with `context` chars on each side.
pub fn snippets(vt: &VolumeText, occ: &[Occurrence], context: usize, max: usize) -> Vec<Snippet> {
    let text: Vec<char> = vt.text.chars().collect();
    let t = |plain_idx: usize| vt.plain_to_text.get(plain_idx).copied().unwrap_or(0) as usize;
    let mut groups: Vec<Vec<&Occurrence>> = Vec::new();
    for o in occ {
        match groups.last_mut() {
            Some(g) if o.from <= g.last().unwrap().to + context => g.push(o),
            _ => groups.push(vec![o]),
        }
    }
    groups
        .into_iter()
        .take(max)
        .map(|g| {
            let first = g[0];
            let last_to = g.iter().map(|o| o.to).max().unwrap();
            let start = t(first.from).saturating_sub(context);
            let end = (t(last_to - 1) + 1 + context).min(text.len());
            let mut marks = vec![false; end - start];
            for o in &g {
                for i in t(o.from)..=t(o.to - 1) {
                    if i >= start && i < end {
                        marks[i - start] = true;
                    }
                }
            }
            let mut parts: Vec<(String, bool)> = Vec::new();
            for (i, &c) in text[start..end].iter().enumerate() {
                let m = marks[i];
                match parts.last_mut() {
                    Some((s, pm)) if *pm == m => s.push(c),
                    _ => parts.push((c.to_string(), m)),
                }
            }
            Snippet {
                lb: first.lb.clone(),
                pos: first.pos,
                parts,
            }
        })
        .collect()
}

/// Facet counts as a readable tree path, e.g. `/bulei/阿含部類` -> `阿含部類`.
pub fn facet_label(root: &str) -> &str {
    FACET_DIMENSIONS
        .iter()
        .find(|(r, _)| *r == root)
        .map(|(_, l)| *l)
        .unwrap_or(root)
}

/// Facet counts of one search, grouped for display: label -> values.
pub fn facets_by_label(r: &SearchResult) -> BTreeMap<String, Vec<(String, u64)>> {
    r.facets
        .iter()
        .map(|f| (facet_label(&f.root).to_string(), f.values.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::MemSource;
    use crate::text::encode_utf16le_bom;
    use crate::volume::extract_volume;

    fn vol(body: &str) -> String {
        format!(
            r#"<TEI xmlns="http://www.tei-c.org/ns/1.0" xmlns:cb="http://www.cbeta.org/ns/1.0"><teiHeader><idno type="canon">T</idno></teiHeader><text><body>{}</body></text></TEI>"#,
            body
        )
    }

    fn fixture() -> Bookcase {
        let mut src = MemSource::new();
        src.add("index.xml", "<metadata><version>0.9.5.0</version><date>2025-04-26</date></metadata>");
        src.add("bookdata.txt", encode_utf16le_bom("T,2,【大】,大正新脩大藏經,Taisho\n"));
        src.add(
            "catalog.txt",
            "T , 阿含部類 , 阿含部 , 01 , 0001 , 1 , 長阿含經 , 後秦 佛陀耶舍共竺佛念譯\n\
             T , 法華部類 , 法華部 , 09 , 0262 , 2 , 妙法蓮華經 , 姚秦 鳩摩羅什譯\n",
        );
        let nav = r#"<html><body><nav type="catalog"><span>x</span></nav></body></html>"#;
        src.add("menu_nav.xhtml", r#"<html><body><nav type="catalog"><navlink href="simple_nav.xhtml">s</navlink></nav></body></html>"#);
        for n in ["simple_nav.xhtml", "advance_nav.xhtml", "bulei_nav.xhtml"] {
            src.add(n, nav);
        }
        src.add("spine.txt", "");
        src.add(
            "XML/T/T01/T01n0001_001.xml",
            vol(r#"<p><lb n="0001a01" ed="T"/>佛告比丘：「汝等當念，<lb n="0001a02" ed="T"/>他念攝之令常在。」</p>"#),
        );
        src.add(
            "XML/T/T09/T09n0262_001.xml",
            vol(r#"<p><lb n="0001a01" ed="T"/>如是我聞：一時佛住王舍城，<lb n="0001a02" ed="T"/>常念佛法。</p>"#),
        );
        src.add(
            "XML/T/T09/T09n0262_002.xml",
            vol(r#"<p><lb n="0010a01" ed="T"/>爾時世尊從三昧安詳而起。</p>"#),
        );
        Bookcase::open(Box::new(src)).unwrap()
    }

    fn build() -> (tempdir::Dir, SearchIndex) {
        let dir = tempdir::Dir::new();
        let stats = SearchIndex::build(&fixture(), dir.path(), |_, _| {}).unwrap();
        assert_eq!(stats.volumes, 3);
        let idx = SearchIndex::open(dir.path()).unwrap();
        (dir, idx)
    }

    /// minimal temp dir helper (no extra dependency)
    mod tempdir {
        pub struct Dir(std::path::PathBuf);
        impl Dir {
            pub fn new() -> Dir {
                use std::sync::atomic::{AtomicUsize, Ordering};
                static N: AtomicUsize = AtomicUsize::new(0);
                let p = std::env::temp_dir().join(format!(
                    "smart-library-test-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::SeqCst)
                ));
                let _ = std::fs::remove_dir_all(&p);
                Dir(p)
            }
            pub fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn books(r: &SearchResult) -> Vec<String> {
        let mut v: Vec<String> = r.hits.iter().map(|h| format!("{}_{}", h.book, h.juan)).collect();
        v.sort();
        v
    }

    #[test]
    fn single_char_queries_hit() {
        let (_d, idx) = build();
        let r = idx.search(&SearchRequest::new("念")).unwrap();
        assert_eq!(books(&r), vec!["T0001_1", "T0262_1"]);
        assert_eq!(r.total_books, 2);
    }

    #[test]
    fn loose_match_needs_all_bigrams_in_one_volume_in_any_order() {
        let (_d, idx) = build();
        // every bigram of the query must be there: 丘他 is not
        assert_eq!(idx.search(&SearchRequest::new("比丘他念")).unwrap().total_volumes, 0);
        // a space separates keywords: 比丘 and 他念 anywhere in the volume
        let r = idx.search(&SearchRequest::new("他念 比丘")).unwrap();
        assert_eq!(books(&r), vec!["T0001_1"]);
        let r = idx.search(&SearchRequest::new("比丘 世尊")).unwrap();
        assert_eq!(r.total_volumes, 0);
    }

    #[test]
    fn phrase_crosses_punctuation_and_line_breaks_but_not_gaps() {
        let (_d, idx) = build();
        let mut req = SearchRequest::new("當念他念");
        req.mode = MatchMode::Phrase;
        assert_eq!(books(&idx.search(&req).unwrap()), vec!["T0001_1"]);
        req.query = "比丘他念".to_string();
        assert_eq!(idx.search(&req).unwrap().total_volumes, 0);
    }

    #[test]
    fn facets_count_volumes_and_scope_filters() {
        let (_d, idx) = build();
        let r = idx.search(&SearchRequest::new("佛")).unwrap();
        let bulei = r.facets.iter().find(|f| f.root == "/bulei").unwrap();
        assert_eq!(bulei.values, vec![("法華部類".to_string(), 1), ("阿含部類".to_string(), 1)]);
        let mut req = SearchRequest::new("佛");
        req.scope = Some("bulei/法華部類".to_string());
        assert_eq!(books(&idx.search(&req).unwrap()), vec!["T0262_1"]);
        // drill-down into a sub-category, counted next to the top level
        let mut req = SearchRequest::new("佛");
        req.facet_drill = vec!["bulei/阿含部類".to_string()];
        let r = idx.search(&req).unwrap();
        let sub = r.facets.iter().find(|f| f.root == "/bulei/阿含部類").unwrap();
        assert_eq!(sub.values, vec![("阿含部".to_string(), 1)]);
        assert_eq!(r.facets.iter().find(|f| f.root == "/bulei").unwrap().values.len(), 2);
    }

    #[test]
    fn locates_hits_with_line_and_position() {
        let bc = fixture();
        let vt = bc.volume_text("XML/T/T01/T01n0001_001.xml").unwrap();
        let occ = locate(&vt, "當念他念", MatchMode::Phrase);
        assert_eq!(occ.len(), 1);
        assert_eq!((occ[0].lb.as_str(), occ[0].pos), ("0001a01", 7));
        let occ = locate(&vt, "念", MatchMode::Loose);
        assert_eq!(occ.iter().map(|o| (o.lb.as_str(), o.pos)).collect::<Vec<_>>(), vec![("0001a01", 8), ("0001a02", 2)]);
        let sn = snippets(&vt, &locate(&vt, "當念他念", MatchMode::Phrase), 2, 3);
        let marked: String = sn[0].parts.iter().filter(|p| p.1).map(|p| p.0.as_str()).collect();
        assert_eq!(marked, "當念，他念");
        let _ = extract_volume;
    }
}
