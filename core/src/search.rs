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
//! * `facet` `/bulei/部類/子類`, `/canon/大正新脩大藏經` (the canon's name, not
//!   its code), `/kind/论/经释`, `/period/唐`, `/person/玄奘`, `/book/T0262`
//!
//! Hit positions are **not** stored: for the hits being shown, the volume is
//! extracted again (`Bookcase::volume_text`, ~1.5 ms) and `locate` finds every
//! occurrence with its line and position in the line.

use crate::analyzer::{self, TermKind};
use crate::bookcase::Bookcase;
use crate::library::{cbeta_docs, HtmlTree, IndexDoc};
use crate::props::Overlay;
use crate::volume::VolumeText;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use tantivy::collector::{Count, FacetCollector, FacetCounts, TopDocs};
use tantivy::query::{AllQuery, BooleanQuery, Occur, PhraseQuery, Query, TermQuery, TermSetQuery};
use tantivy::schema::{
    Facet, FacetOptions, Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED,
    STRING,
};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexReader, ReloadPolicy, TantivyDocument, Term};

/// Bumped whenever the schema or the analyzer changes: an index with another
/// version must be rebuilt.
pub const INDEX_VERSION: u32 = 3;
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
    ("/period", "时域"),
    ("/person", "人物"),
    ("/canon", "藏经"),
    ("/kind", "典类"),
];

/// Dimensions whose two levels are listed together (`部類` and `部類/子類`).
const TWO_LEVEL: [&str; 2] = ["/bulei", "/kind"];

pub fn is_two_level(root: &str) -> bool {
    TWO_LEVEL.contains(&root)
}

fn root_name(root: &str) -> &str {
    root.trim_start_matches('/')
}

/// The facet of a filter value: `大集部類` or (two levels) `大集部類/大集部`.
fn filter_facet(root: &str, value: &str) -> Facet {
    let mut path = vec![root_name(root)];
    if is_two_level(root) {
        path.extend(value.split('/').filter(|s| !s.is_empty()));
    } else {
        path.push(value);
    }
    Facet::from_path(path)
}

/// `bulei/阿含部類` -> (`/bulei`, `阿含部類`): the command line form of a filter.
pub fn parse_filter(text: &str) -> Option<Filter> {
    let t = text.trim_start_matches('/');
    let (root, value) = t.split_once('/')?;
    let root = format!("/{}", root);
    FACET_DIMENSIONS.iter().any(|(r, _)| *r == root).then(|| Filter { root, values: vec![value.to_string()] })
}

// ------------------------------------------------------------ index

pub struct SearchIndex {
    index: Index,
    reader: IndexReader,
    f: Fields,
    /// first-level names of the two-level dimensions (`/bulei` -> 部類 names),
    /// recorded when the index was built
    tops: HashMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
pub struct BuildStats {
    pub volumes: usize,
    /// listed volumes that could not be read or parsed: skipped (design-spec A.4)
    pub skipped: Vec<String>,
    pub seconds: f64,
}

impl SearchIndex {
    /// Build a new index for the CBETA data of a bookcase into `dir` (which
    /// must be empty or absent). `progress(done, total)` is called now and then.
    pub fn build(
        bc: &Bookcase,
        dir: &Path,
        overlay: &Overlay,
        progress: impl FnMut(usize, usize),
    ) -> Result<BuildStats, String> {
        let (total, docs) = cbeta_docs(bc, None);
        let meta = serde_json::json!({
            "kind": "cbeta",
            "data_version": bc.data_version(),
            "bookcase_version": bc.version,
            "bookcase_date": bc.date,
        });
        build_index(dir, meta, total, docs, overlay, progress)
    }

    /// Build the index of an HTML library (a user-made tree of a bookcase).
    pub fn build_html(
        bc: &Bookcase,
        lib: &HtmlTree,
        dir: &Path,
        overlay: &Overlay,
        progress: impl FnMut(usize, usize),
    ) -> Result<BuildStats, String> {
        let (total, docs) = lib.docs(bc, None);
        let meta = serde_json::json!({
            "kind": "html",
            "tree": lib.tree_id,
            "data_version": bc.data_version(),
            "bookcase_version": bc.version,
            "bookcase_date": bc.date,
        });
        build_index(dir, meta, total, docs, overlay, progress)
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
        let tops = meta["facet_tops"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| {
                        let names = v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                        (k.clone(), names)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(SearchIndex { index, reader, f, tops })
    }

    /// Data version recorded when the index was built.
    pub fn data_version(dir: &Path) -> Option<String> {
        let meta = std::fs::read_to_string(dir.join(META_FILE)).ok()?;
        let meta: serde_json::Value = serde_json::from_str(&meta).ok()?;
        meta["data_version"].as_str().map(|s| s.to_string())
    }

    /// Only the number of hit documents (to show counts per library).
    pub fn count(&self, req: &SearchRequest) -> Result<usize, String> {
        let query = self.query_for(req, None)?;
        self.reader.searcher().search(&query, &Count).map_err(|e| e.to_string())
    }

    /// The text query (everything when it is empty), limited to `req.within`
    /// and to the filters: one of the values of every filtered dimension
    /// (`skip`: leave that dimension out, to count its alternatives).
    fn query_for(&self, req: &SearchRequest, skip: Option<&str>) -> Result<Box<dyn Query>, String> {
        let text: Box<dyn Query> = if req.query.trim().is_empty() {
            Box::new(AllQuery)
        } else {
            build_query(&self.f, &req.query, req.mode)?
        };
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, self.restrict(text, req))];
        for flt in &req.filters {
            if flt.values.is_empty() || Some(flt.root.as_str()) == skip {
                continue;
            }
            let any: Vec<(Occur, Box<dyn Query>)> = flt
                .values
                .iter()
                .map(|v| {
                    let term = Term::from_facet(self.f.facet, &filter_facet(&flt.root, v));
                    (Occur::Should, Box::new(TermQuery::new(term, IndexRecordOption::Basic)) as Box<dyn Query>)
                })
                .collect();
            clauses.push((Occur::Must, Box::new(BooleanQuery::new(any))));
        }
        Ok(if clauses.len() == 1 { clauses.pop().unwrap().1 } else { Box::new(BooleanQuery::new(clauses)) })
    }

    /// Collectors for the counts of some dimensions: the first level of each,
    /// and the second level of the two-level ones (a collector cannot hold a
    /// facet and one below it, so those go to a collector of their own).
    fn collectors(&self, roots: &[&str]) -> (FacetCollector, FacetCollector) {
        let mut first = FacetCollector::for_field("facet");
        let mut second = FacetCollector::for_field("facet");
        for root in roots {
            first.add_facet(*root);
            if is_two_level(root) {
                for top in self.tops.get(*root).into_iter().flatten() {
                    second.add_facet(Facet::from_path([root_name(root), top.as_str()]));
                }
            }
        }
        (first, second)
    }

    /// The values of one dimension, largest first.
    fn values(&self, root: &str, first: &FacetCounts, second: &FacetCounts) -> FacetCount {
        let mut values: Vec<(String, u64)> = first
            .get(root)
            .map(|(f, c)| (f.to_path().last().unwrap_or(&"").to_string(), c))
            .collect();
        if is_two_level(root) {
            for top in self.tops.get(root).into_iter().flatten() {
                for (f, c) in second.get(Facet::from_path([root_name(root), top.as_str()])) {
                    values.push((format!("{}/{}", top, f.to_path().last().unwrap_or(&"")), c));
                }
            }
        }
        values.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        FacetCount { root: root.to_string(), values }
    }

    /// The query limited to `req.within`.
    fn restrict(&self, query: Box<dyn Query>, req: &SearchRequest) -> Box<dyn Query> {
        let Some(w) = &req.within else { return query };
        let mut any: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        if !w.books.is_empty() {
            let terms = w.books.iter().map(|b| Term::from_field_text(self.f.book, b));
            any.push((Occur::Should, Box::new(TermSetQuery::new(terms))));
        }
        if !w.paths.is_empty() {
            let terms = w.paths.iter().map(|p| Term::from_field_text(self.f.path, p));
            any.push((Occur::Should, Box::new(TermSetQuery::new(terms))));
        }
        // nothing in the set: nothing matches
        let allowed: Box<dyn Query> = if any.is_empty() { Box::new(tantivy::query::EmptyQuery) } else { Box::new(BooleanQuery::new(any)) };
        Box::new(BooleanQuery::new(vec![(Occur::Must, query), (Occur::Must, allowed)]))
    }

    pub fn search(&self, req: &SearchRequest) -> Result<SearchResult, String> {
        let _ = &self.index;
        let searcher = self.reader.searcher();
        // dimensions without a filter are counted under the whole query; one
        // with a filter is counted without that filter, so that its other
        // values stay visible and can be added
        let filtered: Vec<&str> = req.filters.iter().filter(|f| !f.values.is_empty()).map(|f| f.root.as_str()).collect();
        let shared: Vec<&str> = FACET_DIMENSIONS.iter().map(|d| d.0).filter(|r| !filtered.contains(r)).collect();
        let (mut first, second) = self.collectors(&shared);
        first.add_facet("/book");
        let top = TopDocs::with_limit(req.limit.max(1)).and_offset(req.offset).order_by_score();
        let query = self.query_for(req, None)?;
        let (top, total, counts, counts2) = searcher.search(&query, &(top, Count, first, second)).map_err(|e| e.to_string())?;

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
        for (root, _) in FACET_DIMENSIONS {
            if shared.contains(&root) {
                facets.push(self.values(root, &counts, &counts2));
            } else {
                let q = self.query_for(req, Some(root))?;
                let (c1, c2) = self.collectors(&[root]);
                let (c1, c2) = searcher.search(&q, &(c1, c2)).map_err(|e| e.to_string())?;
                facets.push(self.values(root, &c1, &c2));
            }
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

/// The Tantivy document of an index document; `tops` collects the first
/// levels of the two-level facets.
fn make_doc(f: &Fields, d: &IndexDoc, tops: &mut BTreeMap<String, BTreeSet<String>>) -> TantivyDocument {
    let mut doc = TantivyDocument::default();
    doc.add_text(f.path, &d.path);
    doc.add_text(f.book, &d.book);
    doc.add_u64(f.juan, d.juan as u64);
    doc.add_text(f.title, &d.title);
    // index the punctuated text: punctuation is transparent to `aio`,
    // and whitespace still separates Latin words
    doc.add_text(f.body, &d.text);
    doc.add_text(f.body1, &d.text);
    for facet in &d.facets {
        if facet.len() > 1 && facet.iter().all(|c| !c.is_empty()) {
            let root = format!("/{}", facet[0]);
            if facet.len() > 2 && is_two_level(&root) {
                tops.entry(root).or_default().insert(facet[1].clone());
            }
            doc.add_facet(f.facet, Facet::from_path(facet.iter().map(|s| s.as_str())));
        }
    }
    doc
}

/// Write documents into a new index in `dir` (which must be empty or
/// absent). `meta` is stored next to the index with `index_version` and the
/// number of documents. Documents that come out as `Err` are skipped
/// (design-spec A.4: entries without real content are ignored). The user's
/// properties (`overlay`) are put into the facets of each document.
pub fn build_index(
    dir: &Path,
    mut meta: serde_json::Value,
    total: usize,
    docs: impl Iterator<Item = Result<IndexDoc, String>>,
    overlay: &Overlay,
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
    let mut stats = BuildStats::default();
    let mut tops: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (n, d) in docs.enumerate() {
        if n % 500 == 0 {
            progress(n, total);
        }
        let mut d = match d {
            Ok(d) => d,
            Err(e) => {
                stats.skipped.push(e);
                continue;
            }
        };
        overlay.apply(&mut d);
        writer.add_document(make_doc(&f, &d, &mut tops)).map_err(|e| e.to_string())?;
        stats.volumes += 1;
    }
    progress(total, total);
    writer.commit().map_err(|e| e.to_string())?;
    writer.wait_merging_threads().map_err(|e| e.to_string())?;
    meta["index_version"] = serde_json::json!(INDEX_VERSION);
    meta["documents"] = serde_json::json!(stats.volumes);
    meta["facet_tops"] = serde_json::json!(tops);
    std::fs::write(dir.join(META_FILE), serde_json::to_string_pretty(&meta).unwrap()).map_err(|e| e.to_string())?;
    stats.seconds = start.elapsed().as_secs_f64();
    Ok(stats)
}

/// Replace some documents of an existing index (same `path`, new facets: the
/// properties of the user changed). The index must be opened again afterwards.
/// Returns the number of documents written.
pub fn update_docs(
    dir: &Path,
    docs: impl Iterator<Item = Result<IndexDoc, String>>,
    overlay: &Overlay,
) -> Result<usize, String> {
    sync_docs(dir, docs, &[], None, overlay)
}

/// Bring an existing index up to date with edited documents: `docs` replace
/// (or add) the documents of the same `path`, the documents at `removed`
/// paths are dropped, and `data_version` (when given) is recorded as the
/// version of the data the index now matches. The index must be opened again
/// afterwards. Returns the number of documents written.
pub fn sync_docs(
    dir: &Path,
    docs: impl Iterator<Item = Result<IndexDoc, String>>,
    removed: &[String],
    data_version: Option<&str>,
    overlay: &Overlay,
) -> Result<usize, String> {
    let meta_text = std::fs::read_to_string(dir.join(META_FILE)).map_err(|_| format!("no index in {}", dir.display()))?;
    let mut meta: serde_json::Value = serde_json::from_str(&meta_text).map_err(|e| e.to_string())?;
    if meta["index_version"].as_u64() != Some(INDEX_VERSION as u64) {
        return Err("索引版本过旧，请重建索引".into());
    }
    let index = Index::open_in_dir(dir).map_err(|e| e.to_string())?;
    register_tokenizers(&index);
    let (_, f) = schema();
    let mut writer = index.writer::<TantivyDocument>(200_000_000).map_err(|e| e.to_string())?;
    let mut tops: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut n = 0;
    for p in removed {
        writer.delete_term(Term::from_field_text(f.path, p));
    }
    for d in docs {
        let Ok(mut d) = d else { continue };
        overlay.apply(&mut d);
        writer.delete_term(Term::from_field_text(f.path, &d.path));
        writer.add_document(make_doc(&f, &d, &mut tops)).map_err(|e| e.to_string())?;
        n += 1;
    }
    writer.commit().map_err(|e| e.to_string())?;
    writer.wait_merging_threads().map_err(|e| e.to_string())?;
    // new first-level names join the recorded ones
    let mut all: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if let Some(o) = meta["facet_tops"].as_object() {
        for (k, v) in o {
            all.insert(k.clone(), v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default());
        }
    }
    for (k, v) in tops {
        all.entry(k).or_default().extend(v);
    }
    meta["facet_tops"] = serde_json::json!(all);
    if let Some(v) = data_version {
        meta["data_version"] = serde_json::json!(v);
    }
    std::fs::write(dir.join(META_FILE), serde_json::to_string_pretty(&meta).unwrap()).map_err(|e| e.to_string())?;
    Ok(n)
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
    /// empty: every document (to browse)
    pub query: String,
    pub mode: MatchMode,
    pub offset: usize,
    pub limit: usize,
    /// facet filters: a document needs one of the values of every filter
    pub filters: Vec<Filter>,
    /// only these books and / or documents (a node of the tree and what is below it)
    pub within: Option<Within>,
}

/// Values of one facet dimension a document may have.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// e.g. `/bulei`
    pub root: String,
    /// `阿含部類`, or `阿含部類/阿含部` in the two-level dimensions
    pub values: Vec<String>,
}

/// A set of books (CBETA book ids) and documents (paths) to search in.
#[derive(Debug, Clone, Default)]
pub struct Within {
    pub books: Vec<String>,
    pub paths: Vec<String>,
}

impl SearchRequest {
    pub fn new(query: &str) -> Self {
        SearchRequest {
            query: query.to_string(),
            mode: MatchMode::Loose,
            offset: 0,
            limit: 10,
            filters: Vec::new(),
            within: None,
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
        let stats = SearchIndex::build(&fixture(), dir.path(), &Overlay::default(), |_, _| {}).unwrap();
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
    fn searches_within_books_and_paths() {
        let d = tempdir::Dir::new();
        let docs = vec![
            ("T0001", "p1", "如是我聞"),
            ("T0002", "p2", "如是我聞"),
            ("zz", "a/x.html", "如是我聞"),
        ]
        .into_iter()
        .map(|(book, path, text)| {
            Ok(IndexDoc { path: path.into(), book: book.into(), juan: 1, title: book.into(), text: text.into(), facets: vec![], keys: vec![] })
        });
        build_index(d.path(), serde_json::json!({}), 3, docs, &Overlay::default(), |_, _| {}).unwrap();
        let idx = SearchIndex::open(d.path()).unwrap();
        let mut req = SearchRequest::new("如是我聞");
        assert_eq!(idx.search(&req).unwrap().total_volumes, 3);
        req.within = Some(Within { books: vec!["T0002".into()], paths: vec![] });
        assert_eq!(idx.search(&req).unwrap().total_volumes, 1);
        req.within = Some(Within { books: vec!["T0001".into()], paths: vec!["a/x.html".into()] });
        assert_eq!(idx.search(&req).unwrap().total_volumes, 2);
        assert_eq!(idx.count(&req).unwrap(), 2);
        req.within = Some(Within::default());
        assert_eq!(idx.search(&req).unwrap().total_volumes, 0);
    }

    fn filter(root: &str, values: &[&str]) -> Filter {
        Filter { root: root.to_string(), values: values.iter().map(|v| v.to_string()).collect() }
    }

    fn values<'a>(r: &'a SearchResult, root: &str) -> &'a Vec<(String, u64)> {
        &r.facets.iter().find(|f| f.root == root).unwrap().values
    }

    #[test]
    fn facets_count_volumes_with_both_levels_and_real_canon_names() {
        let (_d, idx) = build();
        let r = idx.search(&SearchRequest::new("佛")).unwrap();
        // two levels side by side; the canon is its name, not `T`
        assert_eq!(
            values(&r, "/bulei"),
            &vec![
                ("法華部類".to_string(), 1),
                ("法華部類/法華部".to_string(), 1),
                ("阿含部類".to_string(), 1),
                ("阿含部類/阿含部".to_string(), 1),
            ]
        );
        assert_eq!(values(&r, "/canon"), &vec![("大正新脩大藏經".to_string(), 2)]);
    }

    #[test]
    fn filters_or_within_a_dimension_and_across_dimensions() {
        let (_d, idx) = build();
        let mut req = SearchRequest::new("佛");
        req.filters = vec![filter("/bulei", &["法華部類"])];
        let r = idx.search(&req).unwrap();
        assert_eq!(books(&r), vec!["T0262_1"]);
        // the filtered dimension still lists its alternatives
        assert_eq!(values(&r, "/bulei").len(), 4);
        // two values of one dimension: either
        req.filters = vec![filter("/bulei", &["法華部類", "阿含部類/阿含部"])];
        assert_eq!(idx.search(&req).unwrap().total_volumes, 2);
        // a second dimension narrows down; the first one is counted without its own filter
        req.filters = vec![filter("/bulei", &["法華部類"]), filter("/canon", &["大正新脩大藏經"])];
        let r = idx.search(&req).unwrap();
        assert_eq!(books(&r), vec!["T0262_1"]);
        req.filters = vec![filter("/bulei", &["法華部類"]), filter("/canon", &["卍續"])];
        let r = idx.search(&req).unwrap();
        assert_eq!(r.total_volumes, 0);
        assert_eq!(values(&r, "/bulei").iter().filter(|v| !v.0.contains('/')).count(), 0);
    }

    #[test]
    fn an_empty_query_lists_everything_to_browse() {
        let (_d, idx) = build();
        let r = idx.search(&SearchRequest::new("")).unwrap();
        assert_eq!(r.total_volumes, 3);
        assert_eq!(r.total_books, 2);
        let mut req = SearchRequest::new("  ");
        req.filters = vec![filter("/bulei", &["阿含部類"])];
        assert_eq!(books(&idx.search(&req).unwrap()), vec!["T0001_1"]);
        assert_eq!(idx.count(&req).unwrap(), 1);
    }

    #[test]
    fn user_properties_change_the_facets_and_can_be_updated_later() {
        use crate::props::{Dims, Props};
        let d = tempdir::Dir::new();
        let bc = fixture();
        let props = Props::memory().unwrap();
        let set = |p: &Props, dim: &str, v: &[&str]| {
            let mut m = Dims::new();
            m.insert(dim.to_string(), v.iter().map(|s| s.to_string()).collect());
            p.set("cbeta", Some("T0262"), "", &m).unwrap();
        };
        set(&props, "period", &["姚秦"]);
        SearchIndex::build(&bc, d.path(), &props.overlay("cbeta").unwrap(), |_, _| {}).unwrap();
        let idx = SearchIndex::open(d.path()).unwrap();
        let with = |idx: &SearchIndex, dim: &str, v: &str| {
            let mut req = SearchRequest::new("");
            req.filters = vec![filter(dim, &[v])];
            idx.search(&req).unwrap().total_volumes
        };
        assert_eq!(with(&idx, "/period", "姚秦"), 2);
        // change it later: only the book's volumes are written again
        let mut m = Dims::new();
        m.insert("period".to_string(), vec!["後秦".to_string()]);
        m.insert("bulei".to_string(), vec!["新部/子部".to_string()]);
        props.set("cbeta", Some("T0262"), "", &m).unwrap();
        let only: HashSet<String> = ["T0262".to_string()].into_iter().collect();
        let (_, docs) = cbeta_docs(&bc, Some(&only));
        let n = update_docs(d.path(), docs, &props.overlay("cbeta").unwrap()).unwrap();
        assert_eq!(n, 2);
        let idx = SearchIndex::open(d.path()).unwrap();
        assert_eq!(idx.search(&SearchRequest::new("")).unwrap().total_volumes, 3); // no document twice
        assert_eq!(with(&idx, "/period", "姚秦"), 0);
        assert_eq!(with(&idx, "/bulei", "新部/子部"), 2);
        assert_eq!(with(&idx, "/bulei", "新部"), 2);
        assert_eq!(with(&idx, "/bulei", "法華部類"), 0);
        assert_eq!(with(&idx, "/bulei", "阿含部類"), 1); // the other book keeps its own
    }

    #[test]
    fn sync_drops_documents_and_records_the_version() {
        let bc = fixture();
        let d = tempdir::Dir::new();
        SearchIndex::build(&bc, d.path(), &Overlay::default(), |_, _| {}).unwrap();
        let before = SearchIndex::open(d.path()).unwrap().search(&SearchRequest::new("")).unwrap().total_volumes;
        let gone = "XML/T/T01/T01n0001_001.xml".to_string();
        let n = sync_docs(d.path(), std::iter::empty(), &[gone], Some("v2"), &Overlay::default()).unwrap();
        assert_eq!(n, 0);
        let after = SearchIndex::open(d.path()).unwrap().search(&SearchRequest::new("")).unwrap().total_volumes;
        assert_eq!(after, before - 1);
        assert_eq!(SearchIndex::data_version(d.path()).as_deref(), Some("v2"));
    }

    #[test]
    fn parses_command_line_filters() {
        let f = parse_filter("bulei/阿含部類").unwrap();
        assert_eq!((f.root.as_str(), f.values.as_slice()), ("/bulei", &["阿含部類".to_string()][..]));
        assert!(parse_filter("/nothing/x").is_none());
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
