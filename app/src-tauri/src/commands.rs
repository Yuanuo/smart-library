//! Tauri commands. Each one converts arguments, calls `smart-library-core`
//! and returns plain serializable structs (DTOs).
//!
//! Knowledge libraries (design-spec 9a): the user binds data sources from the
//! tree panel. A bookcase gives one CBETA library (one of its official trees)
//! and any number of HTML libraries (its user-made trees). Each library has
//! its own index under `<app data>/repos/<id>/index`. The registry is
//! `<app config>/libraries.json`.
//!
//! Long work runs in `async` commands, off the window's main thread;
//! building an index runs on its own thread and reports progress with events.

use serde::{Deserialize, Serialize};
use smart_library_core::bookcase::Bookcase;
use smart_library_core::library::{classify_trees, HtmlTree, TreeGroup};
use smart_library_core::nav::NavNode;
use smart_library_core::render::RenderOptions;
use smart_library_core::search::{facet_label, locate, snippets, MatchMode, SearchIndex, SearchRequest};
use smart_library_core::source::{DirSource, Source, ZipSource};
use smart_library_core::cite::{parse_citations, parse_command};
use smart_library_core::titles::{query_words, TitleIndex};
use smart_library_core::toc::TocItem;
use smart_library_core::volume::VolumeText;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager, State};

/// URL prefix of the `sl` protocol (images of a library): `<prefix><lib>/<path>`.
/// WebView2 on Windows reaches custom protocols as `http://<scheme>.localhost/`.
#[cfg(windows)]
pub const ASSET_PREFIX: &str = "http://sl.localhost/";
#[cfg(not(windows))]
pub const ASSET_PREFIX: &str = "sl://localhost/";

// ------------------------------------------------------------ registry

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    /// `cbeta`, `lz`, `zb` … (also the index folder name)
    pub id: String,
    /// `cbeta` or `html`
    pub kind: String,
    pub title: String,
    /// path of the bookcase (zip or folder)
    pub source: String,
    /// tree shown for this library (`bulei`, `longzang` …)
    pub tree: String,
}

pub struct AppState {
    config_dir: PathBuf,
    data_dir: PathBuf,
    workspace_file: PathBuf,
    libraries: RwLock<Vec<Library>>,
    sources: RwLock<HashMap<String, Arc<Bookcase>>>,
    html: RwLock<HashMap<String, Arc<HtmlTree>>>,
    indexes: RwLock<HashMap<String, Arc<SearchIndex>>>,
    /// title quick search, per library (built on first use)
    titles: RwLock<HashMap<String, Arc<TitleIndex>>>,
    building: Mutex<HashSet<String>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        let libraries = load_registry(&config_dir);
        AppState {
            workspace_file: data_dir.join("workspace.json"),
            config_dir,
            data_dir,
            libraries: RwLock::new(libraries),
            sources: RwLock::new(HashMap::new()),
            html: RwLock::new(HashMap::new()),
            indexes: RwLock::new(HashMap::new()),
            titles: RwLock::new(HashMap::new()),
            building: Mutex::new(HashSet::new()),
        }
    }

    fn library(&self, id: &str) -> Result<Library, String> {
        self.libraries
            .read()
            .unwrap()
            .iter()
            .find(|l| l.id == id)
            .cloned()
            .ok_or_else(|| format!("没有这个知识库：{}", id))
    }

    fn source(&self, path: &str) -> Result<Arc<Bookcase>, String> {
        if let Some(bc) = self.sources.read().unwrap().get(path) {
            return Ok(bc.clone());
        }
        let bc = Arc::new(open_bookcase_at(path)?);
        self.sources.write().unwrap().insert(path.to_string(), bc.clone());
        Ok(bc)
    }

    fn bookcase_of(&self, lib: &Library) -> Result<Arc<Bookcase>, String> {
        self.source(&lib.source)
    }

    fn html_tree(&self, lib: &Library) -> Result<Arc<HtmlTree>, String> {
        if let Some(t) = self.html.read().unwrap().get(&lib.id) {
            return Ok(t.clone());
        }
        let bc = self.bookcase_of(lib)?;
        let t = Arc::new(HtmlTree::open(&bc, &lib.tree)?);
        self.html.write().unwrap().insert(lib.id.clone(), t.clone());
        Ok(t)
    }

    fn titles(&self, lib: &Library) -> Result<Arc<TitleIndex>, String> {
        if let Some(t) = self.titles.read().unwrap().get(&lib.id) {
            return Ok(t.clone());
        }
        let t = Arc::new(if lib.kind == "html" {
            TitleIndex::html(&*self.html_tree(lib)?)
        } else {
            TitleIndex::cbeta(&*self.bookcase_of(lib)?, &lib.tree)?
        });
        self.titles.write().unwrap().insert(lib.id.clone(), t.clone());
        Ok(t)
    }

    fn index_dir(&self, id: &str) -> PathBuf {
        self.data_dir.join("repos").join(id).join("index")
    }

    fn index(&self, id: &str) -> Option<Arc<SearchIndex>> {
        if let Some(i) = self.indexes.read().unwrap().get(id) {
            return Some(i.clone());
        }
        let idx = Arc::new(SearchIndex::open(&self.index_dir(id)).ok()?);
        self.indexes.write().unwrap().insert(id.to_string(), idx.clone());
        Some(idx)
    }

    fn save_registry(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.config_dir).map_err(|e| e.to_string())?;
        let libs = self.libraries.read().unwrap().clone();
        let json = serde_json::json!({ "libraries": libs });
        std::fs::write(
            self.config_dir.join("libraries.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .map_err(|e| format!("保存知识库设置失败：{}", e))
    }
}

/// `libraries.json`; or, the first time, the bookcase of the old single
/// setting (`config.json` / `SMART_LIBRARY_BOOKCASE`) bound with its 部類
/// tree as library `cbeta` (which keeps the existing index).
fn load_registry(config_dir: &Path) -> Vec<Library> {
    if let Ok(s) = std::fs::read_to_string(config_dir.join("libraries.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Ok(libs) = serde_json::from_value::<Vec<Library>>(v["libraries"].clone()) {
                return libs;
            }
        }
    }
    let old = std::fs::read_to_string(config_dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["bookcase"].as_str().map(|s| s.to_string()))
        .or_else(|| std::env::var("SMART_LIBRARY_BOOKCASE").ok());
    match old {
        Some(source) => vec![Library {
            id: "cbeta".into(),
            kind: "cbeta".into(),
            title: "大藏经 · 部類目錄".into(),
            source,
            tree: "bulei".into(),
        }],
        None => Vec::new(),
    }
}

fn open_bookcase_at(path: &str) -> Result<Bookcase, String> {
    let p = Path::new(path);
    let source: Box<dyn Source> = if p.is_dir() {
        Box::new(DirSource::open(p)?)
    } else {
        Box::new(ZipSource::open(p)?)
    };
    Bookcase::open(source)
}

fn same_path(a: &str, b: &str) -> bool {
    a.replace('/', "\\").eq_ignore_ascii_case(&b.replace('/', "\\"))
}

/// `lz`, then `lz-2` … if taken.
fn new_id(base: &str, libs: &[Library]) -> String {
    let base = base.to_ascii_lowercase();
    if !libs.iter().any(|l| l.id == base) {
        return base;
    }
    (2..).map(|n| format!("{}-{}", base, n)).find(|id| !libs.iter().any(|l| &l.id == id)).unwrap()
}

/// Bytes of an image for the `sl` protocol: `<lib>/<path in the bookcase>`.
pub fn asset(state: &AppState, url_path: &str) -> Option<(Vec<u8>, &'static str)> {
    let decoded = percent_decode(url_path);
    let (lib, path) = decoded.split_once('/')?;
    let lib = state.library(lib).ok()?;
    let bytes = state.bookcase_of(&lib).ok()?.asset(path)?;
    let mime = match path.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("gif") => "image/gif",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    Some((bytes, mime))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------ status

#[derive(Serialize)]
pub struct LibraryDto {
    id: String,
    kind: String,
    title: String,
    source: String,
    tree: String,
    /// the source cannot be opened (moved, deleted …)
    error: Option<String>,
    data_version: Option<String>,
    index_ready: bool,
    /// data version the index was built for (may differ from the source)
    index_data_version: Option<String>,
    building: bool,
}

#[derive(Serialize)]
pub struct StatusDto {
    libraries: Vec<LibraryDto>,
}

fn status_of(state: &AppState) -> StatusDto {
    let libs = state.libraries.read().unwrap().clone();
    let building = state.building.lock().unwrap().clone();
    let libraries = libs
        .into_iter()
        .map(|l| {
            let bc = state.bookcase_of(&l);
            let dir = state.index_dir(&l.id);
            LibraryDto {
                error: bc.as_ref().err().cloned(),
                data_version: bc.as_ref().ok().map(|b| b.data_version()),
                index_ready: state.index(&l.id).is_some(),
                index_data_version: SearchIndex::data_version(&dir),
                building: building.contains(&l.id),
                id: l.id,
                kind: l.kind,
                title: l.title,
                source: l.source,
                tree: l.tree,
            }
        })
        .collect();
    StatusDto { libraries }
}

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> Result<StatusDto, String> {
    Ok(status_of(&state))
}

// ------------------------------------------------------------ binding

#[derive(Serialize)]
pub struct TreeChoiceDto {
    id: String,
    title: String,
    /// `cbeta`, `html` or `other`
    group: String,
    /// `LZ`, `ZB` for html trees
    code: String,
    leaves: usize,
    /// id of the library that shows this tree now, if any
    bound_as: Option<String>,
}

#[derive(Serialize)]
pub struct SourceDto {
    path: String,
    version: String,
    data_version: String,
    trees: Vec<TreeChoiceDto>,
    /// the CBETA tree to preselect: the bound one, else 部類
    default_cbeta_tree: Option<String>,
}

/// Open a data source and list what can be bound from it.
#[tauri::command]
pub async fn inspect_source(path: String, state: State<'_, AppState>) -> Result<SourceDto, String> {
    let path = path.trim().trim_matches('"').to_string();
    let bc = state.source(&path)?;
    let libs = state.libraries.read().unwrap().clone();
    let mut trees = Vec::new();
    let mut cbeta_bound_tree = None;
    for t in classify_trees(&bc) {
        let (group, code) = match &t.group {
            TreeGroup::Cbeta => ("cbeta", String::new()),
            TreeGroup::Html(c) => ("html", c.clone()),
            TreeGroup::Other => ("other", String::new()),
        };
        let bound_as = libs
            .iter()
            .find(|l| same_path(&l.source, &path) && l.kind == group && l.tree == t.id)
            .map(|l| l.id.clone());
        if group == "cbeta" && bound_as.is_some() {
            cbeta_bound_tree = Some(t.id.clone());
        }
        trees.push(TreeChoiceDto {
            id: t.id,
            title: t.title,
            group: group.to_string(),
            code,
            leaves: t.leaves,
            bound_as,
        });
    }
    let default_cbeta_tree = cbeta_bound_tree.or_else(|| {
        trees
            .iter()
            .find(|t| t.group == "cbeta" && t.id == "bulei")
            .or_else(|| trees.iter().find(|t| t.group == "cbeta"))
            .map(|t| t.id.clone())
    });
    Ok(SourceDto {
        path,
        version: bc.version.clone(),
        data_version: bc.data_version(),
        trees,
        default_cbeta_tree,
    })
}

/// Bind a source: the CBETA data with one tree (`cbeta_tree`, or none), and
/// the chosen HTML trees. Rebinding CBETA with another tree only changes the
/// tree: the data, and so the index, stay the same.
#[tauri::command]
pub async fn bind_libraries(
    path: String,
    cbeta_tree: Option<String>,
    html_trees: Vec<String>,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    let path = path.trim().trim_matches('"').to_string();
    let bc = state.source(&path)?;
    let choices = classify_trees(&bc);
    {
        let mut libs = state.libraries.write().unwrap();
        if let Some(tree) = cbeta_tree {
            let t = choices
                .iter()
                .find(|c| c.id == tree && c.group == TreeGroup::Cbeta)
                .ok_or_else(|| format!("{} 不是 CBETA 的目录树", tree))?;
            match libs.iter_mut().find(|l| same_path(&l.source, &path) && l.kind == "cbeta") {
                Some(l) => {
                    l.tree = t.id.clone();
                    l.title = t.title.clone();
                }
                None => {
                    let id = new_id("cbeta", &libs);
                    libs.push(Library {
                        id,
                        kind: "cbeta".into(),
                        title: t.title.clone(),
                        source: path.clone(),
                        tree: t.id.clone(),
                    });
                }
            }
        }
        for tree in html_trees {
            let t = choices
                .iter()
                .find(|c| c.id == tree)
                .ok_or_else(|| format!("没有这个目录树：{}", tree))?;
            let code = match &t.group {
                TreeGroup::Html(c) => c.clone(),
                _ => return Err(format!("{} 不是 HTML 知识库", t.title)),
            };
            if libs.iter().any(|l| same_path(&l.source, &path) && l.kind == "html" && l.tree == t.id) {
                continue;
            }
            let id = new_id(&code, &libs);
            libs.push(Library {
                id,
                kind: "html".into(),
                title: t.title.clone(),
                source: path.clone(),
                tree: t.id.clone(),
            });
        }
    }
    state.save_registry()?;
    // titles follow the tree
    let ids: Vec<String> = state
        .libraries
        .read()
        .unwrap()
        .iter()
        .filter(|l| same_path(&l.source, &path))
        .map(|l| l.id.clone())
        .collect();
    let mut titles = state.titles.write().unwrap();
    for id in ids {
        titles.remove(&id);
    }
    drop(titles);
    Ok(status_of(&state))
}

/// Remove a library; its index is derived data and is deleted.
#[tauri::command]
pub async fn unbind_library(id: String, state: State<'_, AppState>) -> Result<StatusDto, String> {
    if state.building.lock().unwrap().contains(&id) {
        return Err("正在建立这个知识库的索引，请稍后再解除绑定".into());
    }
    state.libraries.write().unwrap().retain(|l| l.id != id);
    state.indexes.write().unwrap().remove(&id);
    state.html.write().unwrap().remove(&id);
    state.titles.write().unwrap().remove(&id);
    state.save_registry()?;
    let dir = state.data_dir.join("repos").join(&id);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(status_of(&state))
}

// ------------------------------------------------------------ tree

#[derive(Serialize)]
pub struct NodeDto {
    title: String,
    /// CBETA: the book the leaf points to
    book_id: Option<String>,
    /// CBETA: first volume of a part of a big book (`T0220a`)
    start: Option<String>,
    /// HTML libraries: the article (only when it can be read)
    path: Option<String>,
    /// original link
    href: Option<String>,
    children: Vec<NodeDto>,
}

fn node_dto(bc: &Bookcase, html: Option<&HtmlTree>, n: &NavNode) -> NodeDto {
    match n {
        NavNode::Folder { title, children } => NodeDto {
            title: title.clone(),
            book_id: None,
            start: None,
            path: None,
            href: None,
            children: children.iter().map(|c| node_dto(bc, html, c)).collect(),
        },
        NavNode::Leaf { href, text, children, .. } => {
            let children = children.iter().map(|c| node_dto(bc, html, c)).collect();
            if let Some(t) = html {
                let p = href.split('#').next().unwrap_or("");
                return NodeDto {
                    title: text.clone(),
                    book_id: None,
                    start: None,
                    path: t.leaf(p).map(|l| l.path.clone()),
                    href: Some(href.clone()),
                    children,
                };
            }
            let entry = bc.resolve_leaf(href, text);
            NodeDto {
                title: match &entry {
                    Some(e) if !e.cloned => format!("{} {}", e.book_id, e.title),
                    Some(e) => e.title.clone(),
                    None => text.clone(),
                },
                book_id: entry.as_ref().map(|e| e.book_id.clone()),
                start: entry.as_ref().and_then(|e| e.start.clone()),
                path: None,
                href: Some(href.clone()),
                children,
            }
        }
    }
}

#[tauri::command]
pub async fn tree(lib: String, state: State<'_, AppState>) -> Result<Vec<NodeDto>, String> {
    let l = state.library(&lib)?;
    let bc = state.bookcase_of(&l)?;
    if l.kind == "html" {
        let t = state.html_tree(&l)?;
        Ok(t.nav.items.iter().map(|n| node_dto(&bc, Some(&t), n)).collect())
    } else {
        let doc = bc.nav(&l.tree)?;
        Ok(doc.items.iter().map(|n| node_dto(&bc, None, n)).collect())
    }
}

// ------------------------------------------------------------ reader

#[derive(Serialize)]
pub struct PartDto {
    t: String,
    /// highlighted (a hit of the current query)
    hl: bool,
}

#[derive(Serialize)]
pub struct LineDto {
    lb: String,
    parts: Vec<PartDto>,
}

#[derive(Serialize)]
pub struct VolumeItemDto {
    idx: u16,
    label: String,
    path: String,
    exists: bool,
}

#[derive(Serialize)]
pub struct ChapterDto {
    title: String,
    path: String,
    /// line (CBETA `0001c18`) or paragraph (`¶3`) to jump to
    lb: Option<String>,
    depth: usize,
}

#[derive(Serialize)]
pub struct ReaderDto {
    lib: String,
    kind: String,
    book_id: Option<String>,
    title: String,
    author: String,
    path: String,
    juan: u16,
    label: String,
    /// CBETA: the volumes of the book; HTML: the articles of the same folder
    volumes: Vec<VolumeItemDto>,
    chapters: Vec<ChapterDto>,
    lines: Vec<LineDto>,
    /// structured view, hits wrapped in <mark>
    html: String,
    /// number of hits of the query in this document
    hits: usize,
    /// line of the first hit
    first_hit_lb: Option<String>,
}

fn flatten_chapters(items: &[TocItem], depth: usize, out: &mut Vec<ChapterDto>) {
    for it in items {
        out.push(ChapterDto {
            title: it.link.text.clone(),
            path: it.link.file.clone(),
            lb: it.link.anchor.as_ref().map(|a| a.trim_start_matches('p').to_string()),
            depth,
        });
        flatten_chapters(&it.children, depth + 1, out);
    }
}

/// Hits of the query in a text: plain ranges, count, first line.
fn hits_of(vt: &VolumeText, query: Option<&str>, phrase: Option<bool>) -> (Vec<(usize, usize)>, usize, Option<String>) {
    match query.filter(|q| !q.trim().is_empty()) {
        Some(q) => {
            let mode = if phrase.unwrap_or(false) { MatchMode::Phrase } else { MatchMode::Loose };
            let occ = locate(vt, q, mode);
            let first = occ.first().map(|o| o.lb.clone());
            (occ.iter().map(|o| (o.from, o.to)).collect(), occ.len(), first)
        }
        None => (Vec::new(), 0, None),
    }
}

/// The line view: the punctuated text cut into lines, hits marked.
fn line_view(vt: &VolumeText, ranges: &[(usize, usize)]) -> Vec<LineDto> {
    let text: Vec<char> = vt.text.chars().collect();
    let mut marks = vec![false; text.len()];
    for &(from, to) in ranges {
        let a = vt.plain_to_text[from] as usize;
        let b = vt.plain_to_text[to - 1] as usize;
        for m in &mut marks[a..=b] {
            *m = true;
        }
    }
    vt.line_spans()
        .into_iter()
        .map(|(lb, a, b)| {
            let mut parts: Vec<PartDto> = Vec::new();
            for i in a..b {
                match parts.last_mut() {
                    Some(p) if p.hl == marks[i] => p.t.push(text[i]),
                    _ => parts.push(PartDto {
                        t: text[i].to_string(),
                        hl: marks[i],
                    }),
                }
            }
            LineDto {
                lb: lb.unwrap_or("").to_string(),
                parts,
            }
        })
        .collect()
}

#[tauri::command]
pub async fn open_node(
    lib: String,
    book_id: Option<String>,
    path: Option<String>,
    query: Option<String>,
    phrase: Option<bool>,
    state: State<'_, AppState>,
) -> Result<ReaderDto, String> {
    let l = state.library(&lib)?;
    let bc = state.bookcase_of(&l)?;
    let asset_prefix = format!("{}{}/", ASSET_PREFIX, l.id);

    if l.kind == "html" {
        let t = state.html_tree(&l)?;
        let path = path.ok_or("没有指定文章")?;
        let leaf = t.leaf(&path).ok_or_else(|| format!("目录里没有这篇文章：{}", path))?;
        let plain = bc.article(&path, None)?;
        let (ranges, hits, first_hit_lb) = hits_of(&plain.text, query.as_deref(), phrase);
        let a = bc.article(&path, Some(&RenderOptions { highlights: ranges.clone(), asset_prefix }))?;
        let siblings = t.siblings(&path);
        return Ok(ReaderDto {
            lib: l.id.clone(),
            kind: l.kind.clone(),
            book_id: None,
            title: leaf.title.clone(),
            author: if a.meta.authors.is_empty() {
                leaf.folders.join(" / ")
            } else {
                a.meta.authors.join("、")
            },
            path: path.clone(),
            juan: 0,
            label: String::new(),
            volumes: siblings
                .iter()
                .enumerate()
                .map(|(i, s)| VolumeItemDto {
                    idx: i as u16 + 1,
                    label: s.title.clone(),
                    path: s.path.clone(),
                    exists: true,
                })
                .collect(),
            chapters: a
                .headings
                .iter()
                .map(|h| ChapterDto {
                    title: h.text.clone(),
                    path: path.clone(),
                    lb: Some(h.lb.clone()),
                    depth: h.level.saturating_sub(2) as usize,
                })
                .collect(),
            lines: line_view(&a.text, &ranges),
            html: a.html.unwrap_or_default(),
            hits,
            first_hit_lb,
        });
    }

    let book_id = book_id.ok_or("没有指定经号")?;
    let book = bc.book(&book_id).ok_or_else(|| format!("没有这部经：{}", book_id))?;
    let vols = bc.volumes(&book_id);
    let vol = match &path {
        Some(p) => vols.iter().find(|v| &v.path == p),
        None => vols.iter().find(|v| v.exists),
    }
    .ok_or_else(|| format!("{} 没有可读的卷", book_id))?;
    let vt = bc.volume_text(&vol.path)?;
    let (ranges, hits, first_hit_lb) = hits_of(&vt, query.as_deref(), phrase);
    let lines = line_view(&vt, &ranges);
    let html = bc
        .volume_html(&vol.path, &RenderOptions { highlights: ranges, asset_prefix })?
        .html;
    let mut chapters = Vec::new();
    if let Some(toc) = bc.outline(&book_id) {
        flatten_chapters(&toc.chapters, 0, &mut chapters);
    }
    Ok(ReaderDto {
        lib: l.id.clone(),
        kind: l.kind.clone(),
        book_id: Some(book_id.clone()),
        title: book.title.clone(),
        author: book.author_info.clone(),
        path: vol.path.clone(),
        juan: vol.idx,
        label: vol.label.clone(),
        volumes: vols
            .iter()
            .map(|v| VolumeItemDto {
                idx: v.idx,
                label: v.label.clone(),
                path: v.path.clone(),
                exists: v.exists,
            })
            .collect(),
        chapters,
        lines,
        html,
        hits,
        first_hit_lb,
    })
}

#[derive(Serialize)]
pub struct NodeIdDto {
    /// tab id: `<lib>|<path>`
    tab: String,
    lib: String,
    book_id: Option<String>,
    path: String,
    /// tab title, e.g. `妙法蓮華經 卷一`
    title: String,
}

/// Which document a request means (the first readable volume of a book when
/// `path` is absent), without reading it: the tab id is `<lib>|<path>`.
#[tauri::command]
pub async fn resolve_node(
    lib: String,
    book_id: Option<String>,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<NodeIdDto, String> {
    let l = state.library(&lib)?;
    if l.kind == "html" {
        let t = state.html_tree(&l)?;
        let path = path.ok_or("没有指定文章")?;
        let leaf = t.leaf(&path).ok_or_else(|| format!("目录里没有这篇文章：{}", path))?;
        return Ok(NodeIdDto {
            tab: format!("{}|{}", l.id, path),
            lib: l.id.clone(),
            book_id: None,
            path: path.clone(),
            title: leaf.title.clone(),
        });
    }
    let bc = state.bookcase_of(&l)?;
    let book_id = book_id.ok_or("没有指定经号")?;
    let book = bc.book(&book_id).ok_or_else(|| format!("没有这部经：{}", book_id))?;
    let vols = bc.volumes(&book_id);
    let vol = match &path {
        Some(p) => vols.iter().find(|v| &v.path == p),
        None => vols.iter().find(|v| v.exists),
    }
    .ok_or_else(|| format!("{} 没有可读的卷", book_id))?;
    Ok(NodeIdDto {
        tab: format!("{}|{}", l.id, vol.path),
        lib: l.id.clone(),
        book_id: Some(book_id.clone()),
        path: vol.path.clone(),
        title: format!("{} {}", book.title, vol.label),
    })
}

// ------------------------------------------------------------ search

#[derive(Serialize)]
pub struct SnippetDto {
    lb: String,
    pos: usize,
    parts: Vec<PartDto>,
}

#[derive(Serialize)]
pub struct HitDto {
    lib: String,
    book_id: Option<String>,
    title: String,
    /// `卷三` for CBETA, the folder for articles
    subtitle: String,
    path: String,
    score: f32,
    occurrences: usize,
    snippets: Vec<SnippetDto>,
}

#[derive(Serialize)]
pub struct FacetValueDto {
    name: String,
    count: u64,
}

#[derive(Serialize)]
pub struct FacetDto {
    /// e.g. `/bulei`
    root: String,
    label: String,
    values: Vec<FacetValueDto>,
}

#[derive(Serialize)]
pub struct LibCountDto {
    id: String,
    title: String,
    count: usize,
}

#[derive(Serialize)]
pub struct SearchDto {
    /// hit counts of every library with an index (results are grouped by
    /// library, design-spec 8.2)
    libraries: Vec<LibCountDto>,
    /// the library whose results follow
    lib: Option<String>,
    total_volumes: usize,
    total_books: usize,
    page: usize,
    pages: usize,
    millis: f64,
    facets: Vec<FacetDto>,
    hits: Vec<HitDto>,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn search(
    query: String,
    phrase: bool,
    lib: Option<String>,
    scope: Option<String>,
    drill: Option<String>,
    page: Option<usize>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<SearchDto, String> {
    let start = std::time::Instant::now();
    let mut req = SearchRequest::new(&query);
    req.mode = if phrase { MatchMode::Phrase } else { MatchMode::Loose };
    req.limit = limit.unwrap_or(20).clamp(1, 100);

    let libs = state.libraries.read().unwrap().clone();
    let mut counts = Vec::new();
    for l in &libs {
        if let Some(idx) = state.index(&l.id) {
            counts.push(LibCountDto {
                id: l.id.clone(),
                title: l.title.clone(),
                count: idx.count(&req)?,
            });
        }
    }
    if counts.is_empty() {
        return Err("还没有可搜索的知识库：请先为知识库建立索引".into());
    }
    // the asked library, else the first one with hits
    let selected = lib
        .filter(|id| counts.iter().any(|c| &c.id == id))
        .or_else(|| counts.iter().find(|c| c.count > 0).map(|c| c.id.clone()))
        .unwrap_or_else(|| counts[0].id.clone());
    let l = state.library(&selected)?;
    let idx = state.index(&selected).ok_or("索引不可用")?;
    let bc = state.bookcase_of(&l)?;
    let html = if l.kind == "html" { Some(state.html_tree(&l)?) } else { None };

    let page = page.unwrap_or(1).max(1);
    req.offset = (page - 1) * req.limit;
    req.scope = scope.filter(|s| !s.is_empty());
    req.facet_drill = drill.filter(|s| !s.is_empty()).into_iter().collect();
    let r = idx.search(&req)?;

    let mut hits = Vec::new();
    for h in &r.hits {
        let text = if l.kind == "html" {
            bc.article(&h.path, None).map(|a| a.text)
        } else {
            bc.volume_text(&h.path)
        };
        let (occurrences, sn) = match text {
            Ok(vt) => {
                let occ = locate(&vt, &req.query, req.mode);
                let sn = snippets(&vt, &occ, 24, 3)
                    .into_iter()
                    .map(|s| SnippetDto {
                        lb: s.lb,
                        pos: s.pos,
                        parts: s.parts.into_iter().map(|(t, hl)| PartDto { t, hl }).collect(),
                    })
                    .collect();
                (occ.len(), sn)
            }
            Err(_) => (0, Vec::new()), // listed but unreadable: shown without snippets
        };
        let subtitle = match &html {
            Some(t) => t.leaf(&h.path).map(|x| x.folders.join(" / ")).unwrap_or_default(),
            None => format!("卷{}", smart_library_core::bookcase::chinese_number(h.juan as u32)),
        };
        hits.push(HitDto {
            lib: l.id.clone(),
            book_id: if l.kind == "html" { None } else { Some(h.book.clone()) },
            title: h.title.clone(),
            subtitle,
            path: h.path.clone(),
            score: h.score,
            occurrences,
            snippets: sn,
        });
    }
    let facets = r
        .facets
        .iter()
        .map(|f| FacetDto {
            root: f.root.clone(),
            label: facet_label(&f.root).to_string(),
            values: f
                .values
                .iter()
                .map(|(n, c)| FacetValueDto {
                    name: n.clone(),
                    count: *c,
                })
                .collect(),
        })
        .collect();
    Ok(SearchDto {
        libraries: counts,
        lib: Some(selected),
        total_volumes: r.total_volumes,
        total_books: r.total_books,
        page,
        pages: r.total_volumes.div_ceil(req.limit),
        millis: start.elapsed().as_secs_f64() * 1000.0,
        facets,
        hits,
    })
}

// ------------------------------------------------------------ quick search

#[derive(Serialize)]
pub struct QuickItemDto {
    lib: String,
    /// `book`, `chapter`, `juan`, `article`; `cite` for a CBETA citation
    /// found in the query, `cite_missing` when it leads nowhere
    kind: &'static str,
    title: String,
    /// the book of a chapter or juan, the folders of an article
    context: String,
    author: String,
    book_id: Option<String>,
    path: Option<String>,
    lb: Option<String>,
    /// citations: the quoted passage, highlighted when the volume opens
    quote: Option<String>,
}

#[derive(Serialize)]
pub struct QuickDto {
    /// matches in every library (also those not shown)
    libraries: Vec<LibCountDto>,
    total: usize,
    millis: f64,
    items: Vec<QuickItemDto>,
}

/// Title quick search (design-spec 8.7) in one library or, without `lib`, in
/// all of them (merged by match quality, then library order).
#[tauri::command]
pub async fn quick_search(
    query: String,
    lib: Option<String>,
    limit: Option<usize>,
    quote: Option<String>,
    state: State<'_, AppState>,
) -> Result<QuickDto, String> {
    let start = std::time::Instant::now();
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    // `#...`: a command to go to a CBETA book, juan or line, nothing else
    let command = query.trim_start().starts_with('#');
    let words = query_words(&query);
    let libs = state.libraries.read().unwrap().clone();
    let indexes = titles_of(&state, &libs);
    let mut counts = Vec::new();
    let mut all = Vec::new(); // (rank, library order, entry)
    for (n, (l, t)) in indexes.iter().enumerate() {
        let m = if command { Vec::new() } else { t.search(&words) };
        counts.push(LibCountDto {
            id: l.id.clone(),
            title: l.title.clone(),
            count: m.len(),
        });
        if lib.as_deref().is_none_or(|id| id == l.id) {
            all.extend(m.into_iter().map(|(r, i)| (r, n, i)));
        }
    }
    // a `#` command, or a pasted CBETA citation `(CBETA 2025.Q1, T15, no.
    // 614, p. 269c29)`: its passage first, whatever library tab is chosen
    let mut cites = Vec::new();
    let citations = if command {
        let mut c: Vec<_> = parse_command(&query).into_iter().collect();
        if let Some(c) = c.first_mut() {
            c.quote = quote.filter(|q| !q.trim().is_empty());
        }
        c
    } else {
        parse_citations(&query)
    };
    if !citations.is_empty() {
        let cbeta = libs.iter().find(|l| l.kind == "cbeta");
        for c in citations {
            let target = cbeta.and_then(|l| state.bookcase_of(l).ok()).and_then(|bc| bc.resolve_citation(&c));
            cites.push(match (cbeta, target) {
                (Some(l), Some(t)) => QuickItemDto {
                    lib: l.id.clone(),
                    kind: "cite",
                    title: format!("{} {} {}", t.book_id, t.title, t.label).trim_end().to_string(),
                    context: c.text.clone(),
                    author: String::new(),
                    book_id: Some(t.book_id),
                    path: t.path,
                    lb: t.lb,
                    quote: c.quote.clone(),
                },
                _ => QuickItemDto {
                    lib: cbeta.map(|l| l.id.clone()).unwrap_or_default(),
                    kind: "cite_missing",
                    title: if cbeta.is_some() { "没有找到这部经、卷或行".into() } else { "没有绑定 CBETA，无法定位".into() },
                    context: c.text.clone(),
                    author: String::new(),
                    book_id: None,
                    path: None,
                    lb: None,
                    quote: None,
                },
            });
        }
    }
    let total = all.len();
    let items = if limit < total {
        all.select_nth_unstable(limit);
        &mut all[..limit]
    } else {
        &mut all[..]
    };
    items.sort_unstable();
    let items = items
        .iter()
        .map(|&(_, n, i)| {
            let (l, t) = &indexes[n];
            let e = &t.entries[i];
            QuickItemDto {
                lib: l.id.clone(),
                kind: e.kind.as_str(),
                title: e.title.clone(),
                context: t.context(e).to_string(),
                author: t.author(e).to_string(),
                book_id: t.book_id(e).map(|s| s.to_string()),
                path: e.path.clone(),
                lb: e.lb.clone(),
                quote: None,
            }
        })
        .collect::<Vec<_>>();
    let total = total + cites.len();
    let items = cites.into_iter().chain(items).collect();
    Ok(QuickDto {
        libraries: counts,
        total,
        millis: start.elapsed().as_secs_f64() * 1000.0,
        items,
    })
}

#[derive(Serialize)]
pub struct PastedCiteDto {
    /// `#T15, no. 614, p. 269c29`
    command: String,
    quote: Option<String>,
}

/// Pasted text with a CBETA citation: the `#` command it means and its quote.
#[tauri::command]
pub async fn citation_command(text: String) -> Result<Option<PastedCiteDto>, String> {
    Ok(parse_citations(&text).into_iter().next().map(|c| PastedCiteDto {
        command: c.command(),
        quote: c.quote,
    }))
}

/// The title lists of the libraries that can be read, built in parallel
/// the first time (a library that cannot be opened is left out).
fn titles_of(state: &AppState, libs: &[Library]) -> Vec<(Library, Arc<TitleIndex>)> {
    std::thread::scope(|s| {
        let jobs: Vec<_> = libs.iter().map(|l| s.spawn(move || state.titles(l).ok())).collect();
        libs.iter()
            .zip(jobs)
            .filter_map(|(l, j)| j.join().ok().flatten().map(|t| (l.clone(), t)))
            .collect()
    })
}

/// Build the title lists in the background, so that the first quick search
/// does not wait.
#[tauri::command]
pub async fn prepare_titles(app: AppHandle) -> Result<(), String> {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let libs = state.libraries.read().unwrap().clone();
        titles_of(&state, &libs);
    });
    Ok(())
}

// ------------------------------------------------------------ index

#[derive(Serialize, Clone)]
struct ProgressDto {
    lib: String,
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
struct IndexDoneDto {
    lib: String,
    ok: bool,
    message: String,
}

/// Build (or rebuild) the index of a library on a background thread. Emits
/// `index-progress` {lib, done, total} and finally `index-done` {lib, ok, message}.
#[tauri::command]
pub async fn build_index(lib: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let l = state.library(&lib)?;
    let bc = state.bookcase_of(&l)?;
    let html = if l.kind == "html" { Some(state.html_tree(&l)?) } else { None };
    if !state.building.lock().unwrap().insert(lib.clone()) {
        return Err("正在建立这个知识库的索引".to_string());
    }
    let dir = state.index_dir(&lib);
    // drop the open index before deleting its files
    state.indexes.write().unwrap().remove(&lib);
    std::thread::spawn(move || {
        let result = (|| -> Result<String, String> {
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| format!("无法删除旧索引：{}", e))?;
            }
            let app2 = app.clone();
            let lib2 = lib.clone();
            let progress = move |done, total| {
                let _ = app2.emit("index-progress", ProgressDto { lib: lib2.clone(), done, total });
            };
            let stats = match &html {
                Some(t) => SearchIndex::build_html(&bc, t, &dir, progress)?,
                None => SearchIndex::build(&bc, &dir, progress)?,
            };
            Ok(format!(
                "已索引 {} 篇，用时 {:.0} 秒{}",
                stats.volumes,
                stats.seconds,
                if stats.skipped.is_empty() {
                    String::new()
                } else {
                    format!("，跳过 {} 篇", stats.skipped.len())
                }
            ))
        })();
        let state = app.state::<AppState>();
        state.building.lock().unwrap().remove(&lib);
        let done = match result {
            Ok(message) => IndexDoneDto { lib: lib.clone(), ok: true, message },
            Err(message) => IndexDoneDto { lib: lib.clone(), ok: false, message },
        };
        let _ = app.emit("index-done", done);
    });
    Ok(())
}

// ------------------------------------------------------------ workspace

/// The saved workspace (JSON written by the front end), or null.
#[tauri::command]
pub async fn load_workspace(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    match std::fs::read_to_string(&state.workspace_file) {
        Ok(s) => Ok(serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
        Err(_) => Ok(serde_json::Value::Null),
    }
}

/// Save the workspace; written to a temporary file first so that a crash
/// never leaves half a file behind.
#[tauri::command]
pub async fn save_workspace(workspace: serde_json::Value, state: State<'_, AppState>) -> Result<(), String> {
    let file = &state.workspace_file;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&workspace).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())
}
