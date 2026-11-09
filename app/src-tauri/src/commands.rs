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
use smart_library_core::folder::{self, FolderFilter, FolderLibrary};
use smart_library_core::history::History;
use smart_library_core::marks::{parse_tags, MarkFilter, MarkNew, Marks};
use smart_library_core::props::{self, Dims, Props};
use smart_library_core::library::{classify_trees, HtmlTree, TreeGroup};
use smart_library_core::nav::NavNode;
use smart_library_core::notes::{self, NotesStore};
use smart_library_core::render::RenderOptions;
use smart_library_core::search::{facet_label, locate, snippets, Filter, MatchMode, SearchIndex, SearchRequest, Within};
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
    /// folder libraries: suffixes that are indexed, and the size limit in MB (0 = none)
    #[serde(default)]
    pub exts: Vec<String>,
    #[serde(default)]
    pub max_mb: u64,
    /// folder libraries: linked in read-write mode. Kept here, in the
    /// settings of this computer, never in the folder: whoever clones the
    /// folder must accept the rules again (design-spec 6.5).
    #[serde(default)]
    pub writable: bool,
}

pub struct AppState {
    config_dir: PathBuf,
    data_dir: PathBuf,
    workspace_file: PathBuf,
    libraries: RwLock<Vec<Library>>,
    sources: RwLock<HashMap<String, Arc<Bookcase>>>,
    html: RwLock<HashMap<String, Arc<HtmlTree>>>,
    folders: RwLock<HashMap<String, Arc<FolderLibrary>>>,
    notes: RwLock<HashMap<String, Arc<NotesStore>>>,
    /// read-write folder libraries: the stamp of each open file as last read or
    /// saved (`<lib>|<path>`), to notice changes made outside
    stamps: Mutex<HashMap<String, String>>,
    /// documents edited since the index was built or last brought up to date
    /// (folder paths / note ids), per library; the index gets them just
    /// before a full-text search (`flush_dirty`)
    dirty: Mutex<HashMap<String, HashSet<String>>>,
    indexes: RwLock<HashMap<String, Arc<SearchIndex>>>,
    /// title quick search, per library (built on first use)
    titles: RwLock<HashMap<String, Arc<TitleIndex>>>,
    building: Mutex<HashSet<String>>,
    history: History,
    marks: Marks,
    /// user properties of nodes (部类, 时域, 人物, 藏经, 典类)
    props: Props,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        let libraries = load_registry(&config_dir);
        let data_dir_for_history = data_dir.join("history.db");
        AppState {
            workspace_file: data_dir.join("workspace.json"),
            config_dir,
            data_dir,
            libraries: RwLock::new(libraries),
            sources: RwLock::new(HashMap::new()),
            html: RwLock::new(HashMap::new()),
            folders: RwLock::new(HashMap::new()),
            notes: RwLock::new(HashMap::new()),
            stamps: Mutex::new(HashMap::new()),
            dirty: Mutex::new(HashMap::new()),
            indexes: RwLock::new(HashMap::new()),
            titles: RwLock::new(HashMap::new()),
            building: Mutex::new(HashSet::new()),
            history: History::open(&data_dir_for_history).unwrap_or_else(|_| History::memory().unwrap()),
            marks: Marks::open(&data_dir_for_history.with_file_name("marks.db")).unwrap_or_else(|_| Marks::memory().unwrap()),
            props: Props::open(&data_dir_for_history.with_file_name("props.db")).unwrap_or_else(|_| Props::memory().unwrap()),
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

    /// An external folder library (kind `folder`).
    fn folder(&self, lib: &Library) -> Result<Arc<FolderLibrary>, String> {
        if let Some(f) = self.folders.read().unwrap().get(&lib.id) {
            return Ok(f.clone());
        }
        let f = Arc::new(FolderLibrary::open(Path::new(&lib.source), filter_of(lib))?.with_writable(lib.writable));
        if lib.writable {
            let _ = self.sidecar_load(lib); // a bad file leaves what we have
        }
        self.folders.write().unwrap().insert(lib.id.clone(), f.clone());
        Ok(f)
    }

    /// The notes of an internal library (kind `notes`).
    fn notes(&self, lib: &Library) -> Result<Arc<NotesStore>, String> {
        if let Some(n) = self.notes.read().unwrap().get(&lib.id) {
            return Ok(n.clone());
        }
        let n = Arc::new(NotesStore::open(&self.data_dir.join("repos").join(&lib.id).join("notes.db"))?);
        self.notes.write().unwrap().insert(lib.id.clone(), n.clone());
        Ok(n)
    }

    fn titles(&self, lib: &Library) -> Result<Arc<TitleIndex>, String> {
        if let Some(t) = self.titles.read().unwrap().get(&lib.id) {
            return Ok(t.clone());
        }
        let t = Arc::new(if lib.kind == "notes" {
            TitleIndex::notes(&self.notes(lib)?.rows()?, &lib.title)
        } else if lib.kind == "folder" {
            let f = self.folder(lib)?;
            let name = Path::new(&lib.source).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            TitleIndex::folder(&f.files(), &name)
        } else if lib.kind == "html" {
            TitleIndex::html(&*self.html_tree(lib)?)
        } else {
            TitleIndex::cbeta(&*self.bookcase_of(lib)?, &lib.tree)?
        });
        self.titles.write().unwrap().insert(lib.id.clone(), t.clone());
        Ok(t)
    }

    /// Where the pictures pasted into the notes of a library are kept.
    pub fn attachments_dir(&self, lib: &str) -> PathBuf {
        self.data_dir.join("repos").join(lib).join("attachments")
    }

    fn index_dir(&self, id: &str) -> PathBuf {
        self.data_dir.join("repos").join(id).join("index")
    }

    /// A file or folder of a library got another path: what is kept by path
    /// (properties, marks, history, the stamps of open files) follows it, and
    /// the index must see the old documents gone and the new ones there.
    /// `before`: the files it stands for, by their old paths; `title`: the new
    /// title of a renamed file.
    fn after_move(&self, lib: &str, from: &str, to: &str, before: &[String], title: Option<&str>) {
        if from == to {
            return;
        }
        let swap = |p: &str| if p == from { to.to_string() } else { format!("{}{}", to, &p[from.len()..]) };
        let _ = self.props.rekey(lib, from, to);
        let _ = self.marks.rekey(lib, from, to, title);
        let _ = self.history.rekey(lib, from, to, title);
        self.sidecar_sync();
        {
            let mut stamps = self.stamps.lock().unwrap();
            for b in before {
                if let Some(s) = stamps.remove(&format!("{}|{}", lib, b)) {
                    stamps.insert(format!("{}|{}", lib, swap(b)), s);
                }
            }
        }
        self.mark_dirty(lib, before.iter().cloned().chain(before.iter().map(|b| swap(b))));
    }

    /// Write the properties and marks of every read-write folder library into
    /// its `.smart-library/meta.json` (only when they differ from the file).
    /// Called after each change; a failure (a folder on a read-only disk …)
    /// does not stop the change, the data stays in our own files.
    fn sidecar_sync(&self) {
        let libs: Vec<Library> = self.libraries.read().unwrap().iter().filter(|l| l.kind == "folder" && l.writable).cloned().collect();
        for l in libs {
            let _ = self.sidecar_write(&l);
        }
    }

    fn sidecar_write(&self, l: &Library) -> Result<bool, String> {
        let props = self.props.all(&l.id)?.into_iter().filter(|(b, _, _)| b.is_empty()).map(|(_, p, d)| (p, d)).collect();
        let marks = self.marks.export_lib(&l.id)?;
        smart_library_core::sidecar::write(Path::new(&l.source), &smart_library_core::sidecar::Sidecar { props, marks })
    }

    /// The file of a read-write folder replaces what we keep for the library
    /// (it is the copy that travels with the folder).
    fn sidecar_load(&self, l: &Library) -> Result<(), String> {
        let Some(sc) = smart_library_core::sidecar::read(Path::new(&l.source), &l.id)? else { return Ok(()) };
        let props: Vec<(String, String, Dims)> = sc.props.into_iter().map(|(p, d)| (String::new(), p, d)).collect();
        self.props.replace_lib(&l.id, &props)?;
        let marks: Vec<_> = sc.marks.into_iter().map(|mut m| { m.lib_title = l.title.clone(); m }).collect();
        self.marks.replace_lib(&l.id, &marks)
    }

    /// A folder library has just become read-write: what its file has and what
    /// we kept so far are put together, and the file is written.
    fn sidecar_enable(&self, l: &Library) -> Result<(), String> {
        if let Some(sc) = smart_library_core::sidecar::read(Path::new(&l.source), &l.id)? {
            for (p, d) in sc.props {
                if self.props.get(&l.id, None, &p)?.is_empty() {
                    self.props.set(&l.id, None, &p, &d)?;
                }
            }
            let marks: Vec<_> = sc.marks.into_iter().map(|mut m| { m.lib_title = l.title.clone(); m }).collect();
            self.marks.merge_lib(&l.id, &marks)?;
        }
        self.sidecar_write(l).map(|_| ())
    }

    /// Remember that documents of a library were changed (saved, made, renamed, deleted).
    fn mark_dirty(&self, lib: &str, paths: impl IntoIterator<Item = String>) {
        self.dirty.lock().unwrap().entry(lib.to_string()).or_default().extend(paths);
    }

    /// Bring the indexes of `libs` up to date with the documents edited since:
    /// those that exist are written again, those that are gone are dropped.
    /// Done just before a full-text search, so saving a note stays cheap and
    /// the index only matters when it is used. Failures are left for a rebuild.
    fn flush_dirty(&self, libs: &[Library]) {
        for l in libs {
            let paths: HashSet<String> = match self.dirty.lock().unwrap().get_mut(&l.id) {
                Some(s) if !s.is_empty() => std::mem::take(s),
                _ => continue,
            };
            let dir = self.index_dir(&l.id);
            if !dir.join("smart-library-index.json").exists() || self.building.lock().unwrap().contains(&l.id) {
                continue; // no index to update (a build reads everything anyway)
            }
            let result = (|| -> Result<(), String> {
                let overlay = self.props.overlay(&l.id).unwrap_or_default();
                let mut removed = Vec::new();
                let (docs, version): (Vec<_>, Option<String>) = if l.kind == "folder" {
                    let f = self.folder(l)?;
                    let mut files = Vec::new();
                    for p in &paths {
                        match f.abs(p).ok().and_then(|a| std::fs::metadata(a).ok()).filter(|m| m.is_file()) {
                            Some(m) => files.push(folder::FolderFile { rel: p.clone(), name: p.rsplit('/').next().unwrap_or(p).to_string(), size: m.len() }),
                            None => removed.push(p.clone()),
                        }
                    }
                    (f.docs(&files).collect(), None)
                } else if l.kind == "notes" {
                    let store = self.notes(l)?;
                    let rows: Vec<_> = store.rows()?.into_iter().filter(|r| paths.contains(&r.id.to_string())).collect();
                    removed = paths.iter().filter(|p| !rows.iter().any(|r| &r.id.to_string() == *p)).cloned().collect();
                    (notes::docs(&rows, &l.title).into_iter().map(Ok).collect(), Some(store.fingerprint()))
                } else {
                    return Ok(());
                };
                smart_library_core::search::sync_docs(&dir, docs.into_iter(), &removed, version.as_deref(), &overlay)?;
                self.indexes.write().unwrap().remove(&l.id); // opened again with the new documents
                Ok(())
            })();
            if result.is_err() {
                self.mark_dirty(&l.id, paths); // try again at the next search
            }
        }
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
            exts: Vec::new(),
            max_mb: 0,
            writable: false,
        }],
        None => Vec::new(),
    }
}

fn filter_of(l: &Library) -> FolderFilter {
    FolderFilter { exts: l.exts.clone(), max_mb: l.max_mb }
}

/// Libraries whose documents are read as articles (paragraph view).
fn is_article(l: &Library) -> bool {
    l.kind != "cbeta"
}

/// The article of a document of an article-like library.
fn article_of(
    state: &AppState,
    l: &Library,
    path: &str,
    render: Option<&RenderOptions>,
) -> Result<smart_library_core::article::Article, String> {
    if l.kind == "folder" {
        state.folder(l)?.article(path, render)
    } else if l.kind == "notes" {
        let id: i64 = path.parse().map_err(|_| format!("不是笔记：{}", path))?;
        let note = state.notes(l)?.get(id)?.ok_or("笔记不存在")?;
        Ok(notes::article(&note.content, render))
    } else {
        state.bookcase_of(l)?.article(path, render)
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
    if lib.kind == "notes" || lib.kind == "folder" {
        // a picture pasted into a note
        let name = path.strip_prefix("attachments/")?;
        let file = if lib.kind == "folder" {
            state.folder(&lib).ok()?.find_picture(name)?
        } else {
            smart_library_core::attach::path_of(&state.attachments_dir(&lib.id), name)?
        };
        return Some((std::fs::read(file).ok()?, smart_library_core::attach::mime_of(name)));
    }
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
    exts: Vec<String>,
    max_mb: u64,
    /// the library can be edited in the program (internal libraries; folders in read-write mode)
    writable: bool,
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
            let (error, data_version) = if l.kind == "notes" {
                match state.notes(&l) {
                    Ok(n) => (None, Some(n.fingerprint())),
                    Err(e) => (Some(e), None),
                }
            } else if l.kind == "folder" {
                (state.folder(&l).err(), None)
            } else {
                let bc = state.bookcase_of(&l);
                (bc.as_ref().err().cloned(), bc.as_ref().ok().map(|b| b.data_version()))
            };
            let dir = state.index_dir(&l.id);
            let writable = l.writable || l.kind == "notes";
            LibraryDto {
                writable,
                error,
                data_version,
                index_ready: state.index(&l.id).is_some(),
                index_data_version: SearchIndex::data_version(&dir),
                building: building.contains(&l.id),
                id: l.id,
                kind: l.kind,
                title: l.title,
                source: l.source,
                tree: l.tree,
                exts: l.exts,
                max_mb: l.max_mb,
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
/// the chosen HTML trees. The CBETA tree is fixed once bound.
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
            // The tree is chosen once: the index records where each volume sits
            // in it. To use another tree, unbind the library and bind it again.
            match libs.iter().find(|l| same_path(&l.source, &path) && l.kind == "cbeta") {
                Some(l) if l.tree != t.id => {
                    return Err(format!(
                        "这个数据包的 CBETA 已按「{}」绑定，不能更换目录树。如需更换，请先解除绑定再重新绑定。",
                        l.title
                    ))
                }
                Some(_) => {}
                None => {
                    let id = new_id("cbeta", &libs);
                    libs.push(Library {
                        id,
                        kind: "cbeta".into(),
                        title: t.title.clone(),
                        source: path.clone(),
                        tree: t.id.clone(),
                        exts: Vec::new(),
                        max_mb: 0,
                        writable: false,
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
                exts: Vec::new(),
                max_mb: 0,
                writable: false,
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
    let own_lib = state.library(&id).map(|l| l.kind == "notes").unwrap_or(false);
    state.libraries.write().unwrap().retain(|l| l.id != id);
    state.indexes.write().unwrap().remove(&id);
    state.html.write().unwrap().remove(&id);
    state.folders.write().unwrap().remove(&id);
    state.notes.write().unwrap().remove(&id);
    let _ = state.history.remove_lib(&id);
    if own_lib {
        let _ = state.props.remove_lib(&id);
    }
    state.titles.write().unwrap().remove(&id);
    state.save_registry()?;
    let dir = state.data_dir.join("repos").join(&id);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(status_of(&state))
}

/// Check a folder before it is linked.
#[derive(Serialize)]
pub struct FolderDto {
    path: String,
    name: String,
    /// why it cannot be linked, if so
    conflict: Option<String>,
}

#[tauri::command]
pub async fn inspect_folder(path: String, state: State<'_, AppState>) -> Result<FolderDto, String> {
    let canon = folder::canonical(&path)?;
    let libs = state.libraries.read().unwrap().clone();
    let existing: Vec<&str> = libs.iter().filter(|l| l.kind == "folder").map(|l| l.source.as_str()).collect();
    Ok(FolderDto {
        name: Path::new(&canon).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| canon.clone()),
        conflict: folder::conflict(&existing, &canon),
        path: canon,
    })
}

/// Link a folder as a library. `exts`: suffixes separated by commas or
/// spaces; `max_mb`: size limit, 0 for none; `writable`: read-write mode (the
/// window has asked the user to accept the rules first).
#[tauri::command]
pub async fn add_folder(
    path: String,
    title: Option<String>,
    exts: String,
    max_mb: u64,
    writable: Option<bool>,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    let canon = folder::canonical(&path)?;
    let exts = FolderFilter::parse_exts(&exts);
    if exts.is_empty() {
        return Err("请至少填写一种文件后缀".into());
    }
    {
        let mut libs = state.libraries.write().unwrap();
        let existing: Vec<&str> = libs.iter().filter(|l| l.kind == "folder").map(|l| l.source.as_str()).collect();
        if let Some(c) = folder::conflict(&existing, &canon) {
            return Err(c);
        }
        let name = Path::new(&canon).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "folder".into());
        let id = new_id("folder", &libs);
        libs.push(Library {
            id,
            kind: "folder".into(),
            title: title.filter(|t| !t.trim().is_empty()).unwrap_or(name),
            source: canon,
            tree: String::new(),
            exts,
            max_mb,
            writable: writable.unwrap_or(false),
        });
    }
    state.save_registry()?;
    Ok(status_of(&state))
}

/// Switch a folder library between read-only and read-write mode (the window
/// has asked the user to accept the rules before going to read-write).
#[tauri::command]
pub async fn set_folder_writable(id: String, writable: bool, state: State<'_, AppState>) -> Result<StatusDto, String> {
    {
        let mut libs = state.libraries.write().unwrap();
        let l = libs.iter_mut().find(|l| l.id == id).ok_or("找不到这个知识库")?;
        if l.kind != "folder" {
            return Err("只有外部文件夹可以切换读写模式".into());
        }
        l.writable = writable;
    }
    if writable {
        // what the folder's own file has and what we kept are put together
        let l = state.library(&id)?;
        state.sidecar_enable(&l)?;
    }
    state.folders.write().unwrap().remove(&id);
    state.titles.write().unwrap().remove(&id);
    state.stamps.lock().unwrap().retain(|k, _| !k.starts_with(&format!("{}|", id)));
    state.save_registry()?;
    Ok(status_of(&state))
}

// ------------------------------------------------------------ internal libraries

/// Add an internal (read-write) library; `Err` if the name is taken or empty.
fn add_notes_library(state: &AppState, title: &str) -> Result<Library, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("请填写知识库名称".into());
    }
    let lib = {
        let mut libs = state.libraries.write().unwrap();
        if libs.iter().any(|l| l.title == title) {
            return Err("已经有同名的知识库".into());
        }
        let id = new_id("notes", &libs);
        let lib = Library { id, kind: "notes".into(), title, source: String::new(), tree: String::new(), exts: Vec::new(), max_mb: 0, writable: true };
        libs.push(lib.clone());
        lib
    };
    if let Err(e) = state.notes(&lib) {
        state.libraries.write().unwrap().retain(|l| l.id != lib.id);
        return Err(e);
    }
    state.save_registry()?;
    Ok(lib)
}

/// Create an internal (read-write) library.
#[tauri::command]
pub async fn create_notes_library(title: String, state: State<'_, AppState>) -> Result<StatusDto, String> {
    add_notes_library(&state, &title)?;
    Ok(status_of(&state))
}

/// Show a library under another name. Only the registry changes: a bound
/// source (a data package, a folder) is not touched.
#[tauri::command]
pub async fn rename_library(id: String, title: String, state: State<'_, AppState>) -> Result<StatusDto, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("请填写知识库名称".into());
    }
    {
        let mut libs = state.libraries.write().unwrap();
        if libs.iter().any(|l| l.id != id && l.title == title) {
            return Err("已经有同名的知识库".into());
        }
        let l = libs.iter_mut().find(|l| l.id == id).ok_or("找不到这个知识库")?;
        l.title = title;
    }
    state.titles.write().unwrap().remove(&id);
    state.save_registry()?;
    Ok(status_of(&state))
}

/// A place where a note can be added: a library's top or one of its notes.
#[derive(Serialize)]
pub struct NoteTargetDto {
    lib: String,
    /// `""` for the top of the library
    id: String,
    /// `library / folder / note`, as typed in the add dialog
    path: String,
    /// the note is a mind map
    mind: bool,
    /// the note is a whiteboard
    board: bool,
}

/// Internal-library places matching what the user types (every word must
/// appear in the path), for the add-note dialog.
#[tauri::command]
pub async fn note_targets(query: String, state: State<'_, AppState>) -> Result<Vec<NoteTargetDto>, String> {
    let words: Vec<String> = query
        .split(|c: char| c == '/' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    let libs: Vec<Library> = state.libraries.read().unwrap().iter().filter(|l| l.kind == "notes").cloned().collect();
    let mut out = Vec::new();
    for l in libs {
        let hit = |p: &str| {
            let p = p.to_lowercase();
            words.iter().all(|w| p.contains(w.as_str()))
        };
        if hit(&l.title) {
            out.push(NoteTargetDto { lib: l.id.clone(), id: String::new(), path: l.title.clone(), mind: false, board: false });
        }
        for r in state.notes(&l)?.rows()? {
            let mut parts = vec![l.title.clone()];
            parts.extend(r.folders.iter().cloned());
            parts.push(r.title.clone());
            let path = parts.join("/");
            if hit(&path) {
                out.push(NoteTargetDto { lib: l.id.clone(), id: r.id.to_string(), path, mind: r.content_type == smart_library_core::mindmap::TYPE, board: r.content_type == smart_library_core::whiteboard::TYPE });
            }
            if out.len() >= 30 {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

#[derive(Serialize)]
pub struct NewNoteDto {
    lib: String,
    id: String,
    /// ids from the top of the library down to the new note
    chain: Vec<String>,
}

/// Add a note at `position` (`library/folder/folder`, or just folders for
/// `default_lib`); folders that do not exist yet are made. `kind` is `md`,
/// `mindmap` or `whiteboard`. Without a title the note is named `笔记 n`
/// (`脑图 n`, `白板 n`) after the first number no sibling uses.
#[tauri::command]
pub async fn create_note_at(
    default_lib: Option<String>,
    position: String,
    title: Option<String>,
    kind: Option<String>,
    state: State<'_, AppState>,
) -> Result<NewNoteDto, String> {
    let mut libs: Vec<Library> = state.libraries.read().unwrap().iter().filter(|l| l.kind == "notes").cloned().collect();
    if libs.is_empty() {
        // the first note needs a place: make a library for it
        libs.push(add_notes_library(&state, "我的笔记")?);
    }
    let mut parts: Vec<String> = position.split('/').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    // the first part names a library, else the default one (or the first that is not the diary)
    let lib = match parts.first().and_then(|p| libs.iter().find(|l| l.title.to_lowercase() == p.to_lowercase())) {
        Some(l) => {
            parts.remove(0);
            l.clone()
        }
        None => default_lib
            .and_then(|d| libs.iter().find(|l| l.id == d))
            .or_else(|| libs.iter().find(|l| l.title != smart_library_core::diary::LIBRARY))
            .unwrap_or(&libs[0])
            .clone(),
    };
    state.titles.write().unwrap().remove(&lib.id);
    let store = state.notes(&lib)?;
    let parent = store.ensure_path(&parts)?;
    let kind = kind.as_deref().unwrap_or("md");
    let title = match title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t.to_string(),
        None => store.free_title(parent, match kind { "mindmap" => "脑图", "whiteboard" => "白板", _ => "笔记" })?,
    };
    let id = match kind {
        "mindmap" => store.create_typed(parent, &title, &smart_library_core::mindmap::new_data(&title), smart_library_core::mindmap::TYPE)?,
        "whiteboard" => store.create_typed(parent, &title, &smart_library_core::whiteboard::new_data(), smart_library_core::whiteboard::TYPE)?,
        _ => store.create(parent, &title, "")?,
    };
    let chain: Vec<String> = store.chain(id)?.iter().map(|i| i.to_string()).collect();
    state.mark_dirty(&lib.id, chain.iter().cloned()); // the folders made on the way too
    Ok(NewNoteDto { lib: lib.id, chain, id: id.to_string() })
}

#[derive(Serialize)]
pub struct DiaryDto {
    /// the library list (the diary library may have just been made)
    status: StatusDto,
    note: NewNoteDto,
}

/// The note of `date` (`YYYY-MM-DD`) in the diary library, made with its year
/// and month (and the library itself) when it is missing.
fn diary_note(state: &AppState, date: &str) -> Result<(Arc<NotesStore>, NewNoteDto), String> {
    use smart_library_core::diary;
    let titles = diary::path_of(date)?;
    let existing = state.libraries.read().unwrap().iter().find(|l| l.kind == "notes" && l.title == diary::LIBRARY).cloned();
    let lib = match existing {
        Some(l) => l,
        None => add_notes_library(state, diary::LIBRARY)?,
    };
    state.titles.write().unwrap().remove(&lib.id);
    let store = state.notes(&lib)?;
    let parent = store.ensure_path(&titles[..2])?;
    let id = match store.find_child(parent, &titles[2])? {
        Some(id) => id,
        None => store.create(parent, &titles[2], "")?,
    };
    let dto = NewNoteDto { lib: lib.id, chain: store.chain(id)?.iter().map(|i| i.to_string()).collect(), id: id.to_string() };
    state.mark_dirty(&dto.lib, dto.chain.iter().cloned());
    Ok((store, dto))
}

/// Today's diary (`date` is the user's local date): made when missing.
#[tauri::command]
pub async fn diary_today(date: String, state: State<'_, AppState>) -> Result<DiaryDto, String> {
    let (_, note) = diary_note(&state, &date)?;
    Ok(DiaryDto { status: status_of(&state), note })
}

/// The ids from the top of a library down to a note (itself last), to find it in the tree.
#[tauri::command]
pub async fn note_chain(lib: String, id: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let l = state.library(&lib)?;
    if l.kind != "notes" {
        return Err("这个知识库不是自建的".into());
    }
    let store = state.notes(&l)?;
    let id = note_id(&id)?;
    store.get(id)?.ok_or("笔记不存在")?;
    Ok(store.chain(id)?.iter().map(|i| i.to_string()).collect())
}

fn notes_of(state: &AppState, lib: &str) -> Result<Arc<NotesStore>, String> {
    let l = state.library(lib)?;
    if l.kind != "notes" {
        return Err("这个知识库是只读的".into());
    }
    // every call here edits the notes: the title list is out of date
    state.titles.write().unwrap().remove(lib);
    state.notes(&l)
}

fn note_id(s: &str) -> Result<i64, String> {
    s.parse().map_err(|_| format!("笔记编号不对：{}", s))
}

/// Add a note below `parent` (`None`: top of the library); returns its id.
#[tauri::command]
pub async fn create_note(lib: String, parent: Option<String>, title: String, state: State<'_, AppState>) -> Result<String, String> {
    let parent = match parent.filter(|p| !p.is_empty()) {
        Some(p) => Some(note_id(&p)?),
        None => None,
    };
    let id = notes_of(&state, &lib)?.create(parent, &title, "")?.to_string();
    state.mark_dirty(&lib, [id.clone()]);
    Ok(id)
}

/// The files a path stands for: itself for a file, the files below it for a folder.
fn files_under(f: &FolderLibrary, rel: &str) -> Vec<String> {
    if f.abs(rel).map(|p| p.is_dir()).unwrap_or(false) {
        let prefix = format!("{}/", rel);
        f.files().into_iter().filter(|x| x.rel.starts_with(&prefix)).map(|x| x.rel).collect()
    } else {
        vec![rel.to_string()]
    }
}

/// Move a file or a folder of a read-write folder library into the folder
/// `parent` ("" for the top). Returns the new path.
#[tauri::command]
pub async fn move_folder_node(lib: String, rel: String, parent: String, state: State<'_, AppState>) -> Result<String, String> {
    let f = rw_folder(&state, &lib)?.ok_or("只有读写模式的外部文件夹可以这样移动")?;
    let before = files_under(&f, &rel);
    let new_rel = f.move_to(&rel, &parent)?;
    state.after_move(&lib, &rel, &new_rel, &before, None);
    Ok(new_rel)
}

/// Delete a folder of a read-write folder library with everything in it: it goes to
/// `.smart-library/trash`. Returns the number of documents that were in it.
#[tauri::command]
pub async fn delete_folder_dir(lib: String, rel: String, state: State<'_, AppState>) -> Result<usize, String> {
    let f = rw_folder(&state, &lib)?.ok_or("只有读写模式的外部文件夹可以这样删除")?;
    let before = files_under(&f, &rel);
    f.delete_dir(&rel)?;
    let prefix = format!("{}/", rel);
    {
        let mut stamps = state.stamps.lock().unwrap();
        stamps.retain(|k, _| !k.starts_with(&format!("{}|{}", lib, prefix)));
    }
    state.mark_dirty(&lib, before.iter().cloned());
    let _ = state.props.remove_nodes(&lib, &[rel.clone()]);
    for p in &before {
        let _ = state.props.remove_nodes(&lib, &[p.clone()]);
    }
    state.sidecar_sync();
    Ok(before.len())
}

/// Every folder of a read-write folder library (paths), to pick where to move something.
#[tauri::command]
pub async fn folder_dirs(lib: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let l = state.library(&lib)?;
    Ok(state.folder(&l)?.dirs())
}

/// The folder of a library in read-write mode; `None` for internal libraries.
/// A read-only folder is refused. The title list is out of date afterwards.
fn rw_folder(state: &AppState, lib: &str) -> Result<Option<Arc<FolderLibrary>>, String> {
    let l = state.library(lib)?;
    if l.kind != "folder" {
        return Ok(None);
    }
    if !l.writable {
        return Err("这个文件夹库是只读的".into());
    }
    state.titles.write().unwrap().remove(lib);
    Ok(Some(state.folder(&l)?))
}

/// Rename a note. For a file of a folder library `title` is the new file
/// name without its suffix (the rules are checked here); the result is the
/// note's id or path, which changes with a file name.
#[tauri::command]
pub async fn rename_note(lib: String, id: String, title: String, state: State<'_, AppState>) -> Result<String, String> {
    if let Some(f) = rw_folder(&state, &lib)? {
        let before = files_under(&f, &id);
        let new_rel = f.rename(&id, &title)?;
        let is_file = before.len() == 1 && before[0] == id;
        state.after_move(&lib, &id, &new_rel, &before, if is_file { Some(title.trim()) } else { None });
        return Ok(new_rel);
    }
    let store = notes_of(&state, &lib)?;
    let nid = note_id(&id)?;
    store.update(nid, Some(&title), None)?;
    // the notes below carry its title in their folders
    state.mark_dirty(&lib, store.descendants(nid).unwrap_or_default().iter().map(|i| i.to_string()));
    Ok(id)
}

#[tauri::command]
pub async fn save_note(lib: String, id: String, content: String, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(f) = rw_folder(&state, &lib)? {
        let key = format!("{}|{}", lib, id);
        let seen = state.stamps.lock().unwrap().get(&key).cloned();
        // pictures: kept in the note's own folder, a Markdown note links to them (other programs show them too)
        let content = f.prepare_save(&id, &content)?;
        let stamp = f.write_edit(&id, &content, seen.as_deref())?;
        state.stamps.lock().unwrap().insert(key, stamp);
        state.mark_dirty(&lib, [id]);
        return Ok(());
    }
    notes_of(&state, &lib)?.update(note_id(&id)?, None, Some(&content))?;
    state.mark_dirty(&lib, [id]);
    Ok(())
}

/// Add a Markdown note, mind map or whiteboard (`kind`: `md`, `mindmap`,
/// `whiteboard`) as a file in the folder `parent` of a read-write folder
/// library. Without a `title` the file is named `笔记 n` (`脑图 n`, `白板 n`).
#[tauri::command]
pub async fn create_folder_file(
    lib: String,
    parent: String,
    kind: Option<String>,
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<NewNoteDto, String> {
    let f = rw_folder(&state, &lib)?.ok_or("只有读写模式的外部文件夹可以这样新建")?;
    let rel = f.create(&parent, title.as_deref(), folder::EditKind::parse(kind.as_deref().unwrap_or("md")))?;
    state.mark_dirty(&lib, [rel.clone()]);
    let mut chain = Vec::new();
    let mut cur = String::new();
    for part in rel.split('/') {
        if !cur.is_empty() {
            cur.push('/');
        }
        cur.push_str(part);
        chain.push(cur.clone());
    }
    Ok(NewNoteDto { lib, id: rel, chain })
}

/// Make a folder inside the folder `parent` (path, "" for the top) of a
/// read-write folder library, to organize the files. Returns its path.
#[tauri::command]
pub async fn create_folder_dir(lib: String, parent: String, name: String, state: State<'_, AppState>) -> Result<NewNoteDto, String> {
    let f = rw_folder(&state, &lib)?.ok_or("只有读写模式的外部文件夹可以新建文件夹")?;
    let rel = f.create_dir(&parent, &name)?;
    let mut chain = Vec::new();
    let mut cur = String::new();
    for part in rel.split('/') {
        if !cur.is_empty() {
            cur.push('/');
        }
        cur.push_str(part);
        chain.push(cur.clone());
    }
    Ok(NewNoteDto { lib, id: rel, chain })
}

/// The rules for the names of local files, for the boxes that ask for one.
#[tauri::command]
pub async fn file_name_rules() -> Result<String, String> {
    Ok(folder::NAME_RULES.to_string())
}

/// Whether a path of folders typed in a box (`a/b\c`) is made of good names.
#[tauri::command]
pub async fn check_file_path(path: String) -> Result<(), String> {
    folder::split_path(&path).map(|_| ())
}

/// Whether a name (without its suffix) can be a local file name.
#[tauri::command]
pub async fn check_file_name(name: String) -> Result<(), String> {
    folder::check_name(name.trim())
}

/// Keep a picture pasted into a note: the request body is the picture, the
/// headers `lib` and `mime` say where and what. Returns the name to refer to
/// it as, `![](attach:<name>)`.
#[tauri::command]
pub async fn note_attach(request: tauri::ipc::Request<'_>, state: State<'_, AppState>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("没有图片数据".into());
    };
    let header = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let l = state.library(&header("lib"))?;
    if l.kind == "folder" && l.writable {
        // kept in the folder of the note (the header `path` says which note)
        let note = percent_decode(&header("path"));
        return state.folder(&l)?.store_picture(&note, data, &header("mime"));
    }
    if l.kind != "notes" {
        return Err("只有自建知识库和读写模式的本地文件夹里的笔记可以放图片".into());
    }
    smart_library_core::attach::store(&state.attachments_dir(&l.id), data, &header("mime"))
}

/// Write bytes to a file the user chose (the request body is the content,
/// the header `path` the percent-encoded path).
#[tauri::command]
pub async fn save_bytes(request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("没有内容".into());
    };
    let path = percent_decode(request.headers().get("path").and_then(|v| v.to_str().ok()).unwrap_or(""));
    if path.is_empty() {
        return Err("没有文件路径".into());
    }
    std::fs::write(&path, data).map_err(|e| format!("{}: {}", path, e))
}

/// The bytes of a picture kept for the notes of a library.
#[tauri::command]
pub async fn note_picture(lib: String, name: String, state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    let l = state.library(&lib)?;
    let file = if l.kind == "folder" {
        state.folder(&l)?.find_picture(&name)
    } else {
        smart_library_core::attach::path_of(&state.attachments_dir(&l.id), &name)
    }
    .ok_or("图片名不对")?;
    Ok(tauri::ipc::Response::new(std::fs::read(file).map_err(|e| e.to_string())?))
}

/// Add a clipping to a mind map: a new child of its root (`link`: the place
/// it comes from, `tag`: what it is from).
#[tauri::command]
pub async fn mindmap_add_clip(lib: String, id: String, topic: String, link: String, tag: String, state: State<'_, AppState>) -> Result<(), String> {
    let store = notes_of(&state, &lib)?;
    let id = note_id(&id)?;
    let note = store.get(id)?.ok_or("笔记不存在")?;
    if note.content_type != smart_library_core::mindmap::TYPE {
        return Err("这篇笔记不是脑图".into());
    }
    let data = smart_library_core::mindmap::add_clip(&note.content, &topic, &link, &tag)?;
    store.update(id, None, Some(&data))?;
    state.mark_dirty(&lib, [id.to_string()]);
    Ok(())
}

/// Add text at the end of a note.
#[tauri::command]
pub async fn note_append(lib: String, id: String, text: String, state: State<'_, AppState>) -> Result<(), String> {
    notes_of(&state, &lib)?.append(note_id(&id)?, &text)?;
    state.mark_dirty(&lib, [id]);
    Ok(())
}

/// Delete a note and everything below it; returns the number removed.
#[tauri::command]
pub async fn delete_note(lib: String, id: String, state: State<'_, AppState>) -> Result<usize, String> {
    if let Some(f) = rw_folder(&state, &lib)? {
        // a file of a folder library goes to the trash inside `.smart-library`
        f.delete(&id)?;
        state.mark_dirty(&lib, [id.clone()]);
        state.stamps.lock().unwrap().remove(&format!("{}|{}", lib, id));
        let _ = state.props.remove_nodes(&lib, &[id]);
        state.sidecar_sync();
        return Ok(1);
    }
    let store = notes_of(&state, &lib)?;
    let nid = note_id(&id)?;
    let gone: Vec<String> = store.descendants(nid).unwrap_or_default().iter().map(|i| i.to_string()).collect();
    let n = store.delete(nid)?;
    state.mark_dirty(&lib, gone.iter().cloned());
    let _ = state.props.remove_nodes(&lib, &gone);
    Ok(n)
}

#[tauri::command]
pub async fn move_note(
    lib: String,
    id: String,
    parent: Option<String>,
    index: usize,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let parent = match parent.filter(|p| !p.is_empty()) {
        Some(p) => Some(note_id(&p)?),
        None => None,
    };
    let store = notes_of(&state, &lib)?;
    let nid = note_id(&id)?;
    store.move_to(nid, parent, index)?;
    // the notes below it are in other folders now
    state.mark_dirty(&lib, store.descendants(nid).unwrap_or_default().iter().map(|i| i.to_string()));
    Ok(())
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
    /// folder libraries: path below the root
    rel: Option<String>,
    /// folder libraries: a folder whose children are not loaded yet
    lazy: bool,
    /// an internal library's mind map
    mind: bool,
    /// an internal library's whiteboard
    board: bool,
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
            rel: None,
            lazy: false,
            mind: false,
            board: false,
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
                    rel: None,
                    lazy: false,
                    mind: false,
            board: false,
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
                rel: None,
                lazy: false,
                mind: false,
            board: false,
                children,
            }
        }
    }
}

fn note_nodes(n: &NotesStore, parent: Option<i64>) -> Result<Vec<NodeDto>, String> {
    Ok(n.children(parent)?
        .into_iter()
        .map(|r| NodeDto {
            title: r.title,
            book_id: None,
            start: None,
            path: Some(r.id.to_string()),
            href: None,
            rel: Some(r.id.to_string()),
            lazy: r.has_children,
            mind: r.mind,
            board: r.board,
            children: Vec::new(),
        })
        .collect())
}

fn folder_nodes(f: &FolderLibrary, rel: &str) -> Result<Vec<NodeDto>, String> {
    Ok(f.children(rel)?
        .into_iter()
        .map(|e| {
            // in read-write mode a map or board shows with its own icon
            let kind = if !e.is_dir { folder::EditKind::of(&e.name) } else { None };
            NodeDto {
                title: if e.is_dir { e.name.clone() } else { folder::title_of(&e.name).to_string() },
                book_id: None,
                start: None,
                path: if e.is_dir { None } else { Some(e.rel.clone()) },
                href: None,
                rel: Some(e.rel),
                lazy: e.is_dir,
                mind: kind == Some(folder::EditKind::MindMap),
                board: kind == Some(folder::EditKind::Whiteboard),
                children: Vec::new(),
            }
        })
        .collect())
}

/// One level of a folder library, loaded when its folder is opened.
#[tauri::command]
pub async fn folder_children(lib: String, rel: String, state: State<'_, AppState>) -> Result<Vec<NodeDto>, String> {
    let l = state.library(&lib)?;
    if l.kind == "notes" {
        let parent = if rel.is_empty() { None } else { Some(rel.parse().map_err(|_| "笔记编号不对")?) };
        let n = state.notes(&l)?;
        return note_nodes(&n, parent);
    }
    let f = state.folder(&l)?;
    folder_nodes(&f, &rel)
}

#[tauri::command]
pub async fn tree(lib: String, state: State<'_, AppState>) -> Result<Vec<NodeDto>, String> {
    let l = state.library(&lib)?;
    if l.kind == "notes" {
        let n = state.notes(&l)?;
        return note_nodes(&n, None);
    }
    if l.kind == "folder" {
        let f = state.folder(&l)?;
        return folder_nodes(&f, "");
    }
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
    /// internal libraries: the Markdown source, for editing
    raw: Option<String>,
    /// `mindmap` for a mind map note, else empty
    content_type: String,
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
    let asset_prefix = format!("{}{}/", ASSET_PREFIX, l.id);

    if l.kind == "notes" {
        let store = state.notes(&l)?;
        let path = path.ok_or("没有指定笔记")?;
        let id: i64 = path.parse().map_err(|_| format!("不是笔记：{}", path))?;
        let note = store.get(id)?.ok_or("笔记不存在")?;
        if note.content_type == smart_library_core::mindmap::TYPE || note.content_type == smart_library_core::whiteboard::TYPE {
            // a mind map or a whiteboard: the reader shows it with its own component
            return Ok(ReaderDto {
                lib: l.id.clone(),
                kind: l.kind.clone(),
                book_id: None,
                title: note.title.clone(),
                author: store.folders_of(id)?.join(" / "),
                path: path.clone(),
                juan: 0,
                label: String::new(),
                volumes: Vec::new(),
                chapters: Vec::new(),
                lines: Vec::new(),
                html: String::new(),
                hits: 0,
                first_hit_lb: None,
                raw: Some(note.content),
                content_type: if note.content_type == smart_library_core::mindmap::TYPE { "mindmap" } else { "whiteboard" }.into(),
            });
        }
        let plain = notes::article(&note.content, None);
        let (ranges, hits, first_hit_lb) = hits_of(&plain.text, query.as_deref(), phrase);
        let a = notes::article(&note.content, Some(&RenderOptions { highlights: ranges.clone(), asset_prefix }));
        let siblings = store.children(note.parent)?;
        return Ok(ReaderDto {
            lib: l.id.clone(),
            kind: l.kind.clone(),
            book_id: None,
            title: note.title.clone(),
            author: store.folders_of(id)?.join(" / "),
            path: path.clone(),
            juan: 0,
            label: String::new(),
            volumes: siblings
                .iter()
                .enumerate()
                .map(|(i, s)| VolumeItemDto { idx: i as u16 + 1, label: s.title.clone(), path: s.id.to_string(), exists: true })
                .collect(),
            chapters: a
                .headings
                .iter()
                .map(|h| ChapterDto {
                    title: h.text.clone(),
                    path: path.clone(),
                    lb: Some(h.lb.clone()),
                    depth: h.level.saturating_sub(1) as usize,
                })
                .collect(),
            lines: line_view(&a.text, &ranges),
            html: a.html.unwrap_or_default(),
            hits,
            first_hit_lb,
            raw: Some(note.content),
            content_type: String::new(),
        });
    }
    if l.kind == "folder" {
        let f = state.folder(&l)?;
        let path = path.ok_or("没有指定文件")?;
        let name = path.rsplit('/').next().unwrap_or(&path);
        let edit_kind = if f.writable { folder::EditKind::of(name) } else { None };
        // an editable file: its text goes to the editor (`raw`), the stamp is kept to notice outside changes
        let raw = match edit_kind {
            Some(_) => {
                let (text, stamp) = f.read_edit(&path)?;
                state.stamps.lock().unwrap().insert(format!("{}|{}", l.id, path), stamp);
                Some(if edit_kind == Some(folder::EditKind::Markdown) { folder::to_editor(&text) } else { text })
            }
            None => None,
        };
        if matches!(edit_kind, Some(folder::EditKind::MindMap) | Some(folder::EditKind::Whiteboard)) {
            return Ok(ReaderDto {
                lib: l.id.clone(),
                kind: l.kind.clone(),
                book_id: None,
                title: folder::title_of(name).to_string(),
                author: path.rsplit_once('/').map(|(d, _)| d.replace('/', " / ")).unwrap_or_default(),
                path: path.clone(),
                juan: 0,
                label: String::new(),
                volumes: Vec::new(),
                chapters: Vec::new(),
                lines: Vec::new(),
                html: String::new(),
                hits: 0,
                first_hit_lb: None,
                raw,
                content_type: edit_kind.unwrap().content_type().into(),
            });
        }
        let plain = f.article(&path, None)?;
        let (ranges, hits, first_hit_lb) = hits_of(&plain.text, query.as_deref(), phrase);
        let a = f.article(&path, Some(&RenderOptions { highlights: ranges.clone(), asset_prefix }))?;
        let title = folder::title_of(name).to_string();
        let dir = path.rsplit_once('/').map(|(d, _)| d.replace('/', " / ")).unwrap_or_default();
        return Ok(ReaderDto {
            lib: l.id.clone(),
            kind: l.kind.clone(),
            book_id: None,
            title,
            author: dir,
            path: path.clone(),
            juan: 0,
            label: String::new(),
            volumes: f
                .siblings(&path)
                .iter()
                .enumerate()
                .map(|(i, s)| VolumeItemDto {
                    idx: i as u16 + 1,
                    label: folder::title_of(&s.name).to_string(),
                    path: s.rel.clone(),
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
            raw,
            content_type: String::new(),
        });
    }
    let bc = state.bookcase_of(&l)?;

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
            raw: None,
            content_type: String::new(),
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
        raw: None,
        content_type: String::new(),
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
    if l.kind == "notes" {
        let path = path.ok_or("没有指定笔记")?;
        let id: i64 = path.parse().map_err(|_| format!("不是笔记：{}", path))?;
        let note = state.notes(&l)?.get(id)?.ok_or("笔记不存在")?;
        return Ok(NodeIdDto { tab: format!("{}|{}", l.id, path), lib: l.id.clone(), book_id: None, path, title: note.title });
    }
    if l.kind == "folder" {
        let path = path.ok_or("没有指定文件")?;
        let name = path.rsplit('/').next().unwrap_or(&path);
        return Ok(NodeIdDto {
            tab: format!("{}|{}", l.id, path),
            lib: l.id.clone(),
            book_id: None,
            title: folder::title_of(name).to_string(),
            path,
        });
    }
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

/// A node of the tree as a search range: its library and what is below it.
/// CBETA and HTML libraries list the books / articles (the front end has the
/// whole tree); folder and note libraries are loaded lazily, so only `rel`
/// (folder path / note id) comes and the files or notes are found here.
#[derive(Deserialize, Clone)]
pub struct WithinDto {
    lib: String,
    #[serde(default)]
    books: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    /// the subtrees of folder / notes libraries (a folder path or a note id)
    #[serde(default)]
    rels: Vec<String>,
    rel: Option<String>,
    /// the whole library (a library root in the search range list)
    #[serde(default)]
    whole: bool,
}

fn resolve_within(state: &AppState, w: &WithinDto) -> Result<Within, String> {
    let l = state.library(&w.lib)?;
    let mut within = Within { books: w.books.clone(), paths: w.paths.clone() };
    let mut rels: Vec<&str> = w.rels.iter().map(|r| r.as_str()).collect();
    rels.extend(w.rel.as_deref());
    rels.retain(|r| !r.is_empty());
    if !rels.is_empty() {
        if l.kind == "folder" {
            let prefixes: Vec<String> = rels.iter().map(|r| format!("{}/", r)).collect();
            within.paths = state
                .folder(&l)?
                .files()
                .into_iter()
                .filter(|f| rels.contains(&f.rel.as_str()) || prefixes.iter().any(|p| f.rel.starts_with(p)))
                .map(|f| f.rel)
                .collect();
        } else if l.kind == "notes" {
            let store = state.notes(&l)?;
            let mut ids = std::collections::BTreeSet::new();
            for rel in &rels {
                let id: i64 = rel.parse().map_err(|_| "笔记编号不对")?;
                ids.extend(store.descendants(id)?);
            }
            within.paths = ids.iter().map(|i| i.to_string()).collect();
        }
    }
    Ok(within)
}

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

/// The search range list as one range per library (`None`: the whole library).
fn ranges_of(state: &AppState, withins: Vec<WithinDto>) -> Result<HashMap<String, Option<Within>>, String> {
    let mut ranges: HashMap<String, Option<Within>> = HashMap::new();
    for w in withins {
        let item = if w.whole { None } else { Some(resolve_within(state, &w)?) };
        match (ranges.get_mut(&w.lib), item) {
            (Some(None), _) => {}
            (Some(slot), None) => *slot = None,
            (Some(Some(have)), Some(more)) => {
                have.books.extend(more.books);
                have.paths.extend(more.paths);
            }
            (None, item) => {
                ranges.insert(w.lib.clone(), item);
            }
        }
    }
    Ok(ranges)
}

#[derive(Serialize)]
pub struct RandomDto {
    lib: String,
    book_id: Option<String>,
    title: String,
    subtitle: String,
    path: String,
    lb: String,
    sentence: String,
}

fn random_below(n: usize) -> usize {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let mut x = (t ^ SEQ.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed).wrapping_mul(0x2545_F491_4F6C_DD1D)) | 1;
    for _ in 0..3 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
    }
    (x % n.max(1) as u64) as usize
}

/// A random sentence ("I'm feeling lucky"): a random document of the search
/// range (and filters of the current library), a random sentence of it.
#[tauri::command]
pub async fn random_sentence(
    lib: Option<String>,
    filters: Option<Vec<FilterDto>>,
    withins: Option<Vec<WithinDto>>,
    state: State<'_, AppState>,
) -> Result<RandomDto, String> {
    let ranges = ranges_of(&state, withins.unwrap_or_default())?;
    let mut libs = state.libraries.read().unwrap().clone();
    if !ranges.is_empty() {
        libs.retain(|l| ranges.contains_key(&l.id));
    }
    if let Some(id) = lib.as_ref().filter(|id| libs.iter().any(|l| &l.id == *id)) {
        libs.retain(|l| &l.id == id);
    }
    let filters: Vec<Filter> = filters
        .unwrap_or_default()
        .into_iter()
        .filter(|f| !f.values.is_empty() && lib.is_some())
        .map(|f| Filter { root: f.root, values: f.values })
        .collect();
    // several tries: some documents have no sentence of a good length
    for _ in 0..8 {
        let mut counts = Vec::new();
        for l in &libs {
            if let Some(idx) = state.index(&l.id) {
                let mut r = SearchRequest::new("");
                r.within = ranges.get(&l.id).cloned().flatten();
                r.filters = filters.clone();
                counts.push((l.clone(), idx.count(&r)?));
            }
        }
        let total: usize = counts.iter().map(|c| c.1).sum();
        if total == 0 {
            return Err("范围内没有可搜索的文档（需要先建立索引）".into());
        }
        let mut k = random_below(total);
        let mut chosen = None;
        for (l, n) in &counts {
            if k < *n {
                chosen = Some(l.clone());
                break;
            }
            k -= n;
        }
        let l = chosen.ok_or("没有可用的知识库")?;
        let idx = state.index(&l.id).ok_or("索引不可用")?;
        let mut req = SearchRequest::new("");
        req.within = ranges.get(&l.id).cloned().flatten();
        req.filters = filters.clone();
        req.limit = 1;
        req.offset = k;
        let r = idx.search(&req)?;
        let Some(h) = r.hits.first() else { continue };
        let vt = if is_article(&l) {
            article_of(&state, &l, &h.path, None).map(|a| a.text)
        } else {
            state.bookcase_of(&l)?.volume_text(&h.path)
        };
        let Ok(vt) = vt else { continue };
        // sentences: cut after 。？！； of the punctuated text, 8-60 chars, mostly Chinese
        let mut sentences: Vec<String> = Vec::new();
        let mut cur = String::new();
        for c in vt.text.chars() {
            if c == '\n' {
                cur.clear();
                continue;
            }
            cur.push(c);
            if "。？！；".contains(c) {
                let t = cur.trim().to_string();
                let len = t.chars().count();
                let han = t.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
                if (8..=60).contains(&len) && han * 10 >= len * 7 {
                    sentences.push(t);
                }
                cur.clear();
            }
        }
        if sentences.is_empty() {
            continue;
        }
        let sentence = sentences[random_below(sentences.len())].clone();
        let lb = locate(&vt, &sentence, MatchMode::Phrase).first().map(|o| o.lb.clone()).unwrap_or_default();
        let subtitle = if is_article(&l) { String::new() } else { format!("卷{}", smart_library_core::bookcase::chinese_number(h.juan as u32)) };
        return Ok(RandomDto {
            lib: l.id.clone(),
            book_id: if is_article(&l) { None } else { Some(h.book.clone()) },
            title: h.title.clone(),
            subtitle,
            path: h.path.clone(),
            lb,
            sentence,
        });
    }
    Err("没有找到合适的句子，请再试一次".into())
}

/// One facet filter of the search page: one of the `values` of a dimension.
#[derive(Deserialize, Clone)]
pub struct FilterDto {
    root: String,
    values: Vec<String>,
}

/// Full-text search. `query` may be empty (everything, to browse); `filters`
/// are the ticked facet values (one of the values of every dimension);
/// `withins` the search range list: the union of its items, per library.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn search(
    query: String,
    phrase: bool,
    lib: Option<String>,
    filters: Option<Vec<FilterDto>>,
    withins: Option<Vec<WithinDto>>,
    page: Option<usize>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<SearchDto, String> {
    let start = std::time::Instant::now();
    let mut req = SearchRequest::new(&query);
    req.mode = if phrase { MatchMode::Phrase } else { MatchMode::Loose };
    req.limit = limit.unwrap_or(20).clamp(1, 100);
    req.filters = filters
        .unwrap_or_default()
        .into_iter()
        .filter(|f| !f.values.is_empty())
        .map(|f| Filter { root: f.root, values: f.values })
        .collect();

    let mut libs = state.libraries.read().unwrap().clone();
    // a search range: those libraries only; per library the union of its
    // items (`None`: the whole library)
    let ranges = ranges_of(&state, withins.unwrap_or_default())?;
    if !ranges.is_empty() {
        libs.retain(|l| ranges.contains_key(&l.id));
    }
    // notes edited since the last search join the index now
    state.flush_dirty(&libs);
    // counts per library ignore the filters (those belong to one library)
    let mut counts = Vec::new();
    for l in &libs {
        if let Some(idx) = state.index(&l.id) {
            let mut r = SearchRequest::new(&query);
            r.mode = req.mode;
            r.within = ranges.get(&l.id).cloned().flatten();
            counts.push(LibCountDto {
                id: l.id.clone(),
                title: l.title.clone(),
                count: idx.count(&r)?,
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
    req.within = ranges.get(&selected).cloned().flatten();
    let l = state.library(&selected)?;
    let idx = state.index(&selected).ok_or("索引不可用")?;
    let html = if l.kind == "html" { Some(state.html_tree(&l)?) } else { None };

    let page = page.unwrap_or(1).max(1);
    req.offset = (page - 1) * req.limit;
    let r = idx.search(&req)?;
    let browsing = req.query.trim().is_empty();

    let mut hits = Vec::new();
    for h in &r.hits {
        let text = if browsing {
            Err(String::new())
        } else if is_article(&l) {
            article_of(&state, &l, &h.path, None).map(|a| a.text)
        } else {
            state.bookcase_of(&l)?.volume_text(&h.path)
        };
        let (occurrences, sn) = match text {
            _ if browsing => (0, Vec::new()), // no words to show
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
            None if l.kind == "notes" => h
                .path
                .parse::<i64>()
                .ok()
                .and_then(|id| state.notes(&l).ok()?.folders_of(id).ok())
                .map(|f| f.join(" / "))
                .unwrap_or_default(),
            None if l.kind == "folder" => h.path.rsplit_once('/').map(|(d, _)| d.replace('/', " / ")).unwrap_or_default(),
            None => format!("卷{}", smart_library_core::bookcase::chinese_number(h.juan as u32)),
        };
        hits.push(HitDto {
            lib: l.id.clone(),
            book_id: if is_article(&l) { None } else { Some(h.book.clone()) },
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
    within: Option<WithinDto>,
    state: State<'_, AppState>,
) -> Result<QuickDto, String> {
    let start = std::time::Instant::now();
    let limit = limit.unwrap_or(200).clamp(1, 5000);
    // `#...`: a command to go to a CBETA book, juan or line, nothing else
    let command = query.trim_start().starts_with('#');
    let words = query_words(&query);
    let libs = state.libraries.read().unwrap().clone();
    let indexes = titles_of(&state, &libs);
    let within = match &within {
        Some(w) => Some((w.lib.clone(), resolve_within(&state, w)?)),
        None => None,
    };
    let mut counts = Vec::new();
    let mut all = Vec::new(); // (rank, library order, entry)
    for (n, (l, t)) in indexes.iter().enumerate() {
        let mut m = if command { Vec::new() } else { t.search(&words) };
        if let Some((wl, w)) = &within {
            if &l.id != wl {
                m.clear();
            } else {
                let books: HashSet<&str> = w.books.iter().map(|s| s.as_str()).collect();
                let paths: HashSet<&str> = w.paths.iter().map(|s| s.as_str()).collect();
                m.retain(|(_, i)| {
                    let e = &t.entries[*i];
                    t.book_id(e).is_some_and(|b| books.contains(b)) || e.path.as_deref().is_some_and(|p| paths.contains(p))
                });
            }
        }
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
    let notes_store = if l.kind == "notes" { Some(state.notes(&l)?) } else { None };
    let lib_title = l.title.clone();
    state.titles.write().unwrap().remove(&lib);
    let folder = if l.kind == "folder" {
        // the folder may have changed: read it afresh
        state.folders.write().unwrap().remove(&lib);
        state.titles.write().unwrap().remove(&lib);
        Some(state.folder(&l)?)
    } else {
        None
    };
    let bc = if folder.is_some() || notes_store.is_some() { None } else { Some(state.bookcase_of(&l)?) };
    let html = if l.kind == "html" { Some(state.html_tree(&l)?) } else { None };
    if !state.building.lock().unwrap().insert(lib.clone()) {
        return Err("正在建立这个知识库的索引".to_string());
    }
    let dir = state.index_dir(&lib);
    let overlay = state.props.overlay(&lib).unwrap_or_default();
    state.dirty.lock().unwrap().remove(&lib); // the build reads everything
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
            let stats = match (&folder, &bc, &html) {
                _ if notes_store.is_some() => {
                    let n = notes_store.as_ref().unwrap();
                    let docs = notes::docs(&n.rows()?, &lib_title);
                    let meta = serde_json::json!({ "kind": "notes", "data_version": n.fingerprint() });
                    smart_library_core::search::build_index(&dir, meta, docs.len(), docs.into_iter().map(Ok), &overlay, progress)?
                }
                (Some(f), _, _) => {
                    let files = f.files();
                    let meta = serde_json::json!({
                        "kind": "folder",
                        "data_version": f.fingerprint(&files),
                    });
                    smart_library_core::search::build_index(&dir, meta, files.len(), f.docs(&files), &overlay, progress)?
                }
                (_, Some(bc), Some(t)) => SearchIndex::build_html(bc, t, &dir, &overlay, progress)?,
                (_, Some(bc), None) => SearchIndex::build(bc, &dir, &overlay, progress)?,
                _ => return Err("没有数据".into()),
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

// ------------------------------------------------------------ citation

/// The place of a passage as a citation, to copy with the text: CBETA's own
/// format `(CBETA 2025.Q1, T15, no. 614, p. 269c29)`, else
/// `—— 《title》 · library · ¶3`.
#[tauri::command]
pub async fn cite_text(
    lib: String,
    path: String,
    book_id: Option<String>,
    lb: Option<String>,
    title: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let l = state.library(&lib)?;
    if l.kind == "cbeta" {
        let bc = state.bookcase_of(&l)?;
        let vol = path.split('/').nth(2).unwrap_or("");
        let id = book_id.unwrap_or_default();
        let split = id.find(|c: char| c.is_ascii_digit()).unwrap_or(id.len());
        let number = id[split..].trim_start_matches('0');
        let mut s = format!("(CBETA {}, {}, no. {}", bc.data_version(), vol, if number.is_empty() { "0" } else { number });
        if let Some(lb) = lb.filter(|l| l.len() > 4 && l.is_char_boundary(4)) {
            let (page, rest) = lb.split_at(4);
            s.push_str(&format!(", p. {}{}", page.trim_start_matches('0'), rest));
        }
        s.push(')');
        return Ok(s);
    }
    Ok(format!("—— 《{}》 · {}{}", title, l.title, lb.map(|b| format!(" · {}", b)).unwrap_or_default()))
}

// ------------------------------------------------------------ marks

#[derive(Serialize)]
pub struct MarkDto {
    id: i64,
    kind: String,
    lib: String,
    /// the library's title now, else the one saved with the mark
    lib_title: String,
    /// the library is still bound (the mark can be opened)
    available: bool,
    path: String,
    book_id: Option<String>,
    lb: Option<String>,
    quote: String,
    note: String,
    doc_title: String,
    author: String,
    created: i64,
    tags: Vec<String>,
}

#[derive(Serialize)]
pub struct TagCountDto {
    tag: String,
    count: i64,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mark_add(
    kind: String,
    lib: String,
    path: String,
    book_id: Option<String>,
    lb: Option<String>,
    quote: String,
    note: String,
    doc_title: String,
    author: String,
    tags: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let lib_title = state.library(&lib)?.title;
    let id = state.marks.add(
        &MarkNew { kind, lib, path, book_id, lb, quote, note, doc_title, lib_title, author },
        &parse_tags(&tags),
    )?;
    state.sidecar_sync();
    Ok(id)
}

#[tauri::command]
pub async fn mark_update(id: i64, note: Option<String>, tags: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    state.marks.update(id, note.as_deref(), tags.map(|t| parse_tags(&t)).as_deref())?;
    state.sidecar_sync();
    Ok(())
}

#[tauri::command]
pub async fn mark_delete(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state.marks.delete(id)?;
    state.sidecar_sync();
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mark_list(
    tag: Option<String>,
    text: Option<String>,
    lib: Option<String>,
    path: Option<String>,
    book_id: Option<String>,
    kinds: Option<Vec<String>>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<MarkDto>, String> {
    let libs = state.libraries.read().unwrap().clone();
    let rows = state.marks.list(
        &MarkFilter {
            tag: tag.filter(|t| !t.is_empty()),
            text: text.unwrap_or_default(),
            lib: lib.filter(|l| !l.is_empty()),
            path,
            book_id,
            kinds: kinds.unwrap_or_default(),
        },
        limit.unwrap_or(500).clamp(1, 5000),
    )?;
    Ok(rows
        .into_iter()
        .map(|m| {
            let l = libs.iter().find(|l| l.id == m.lib);
            MarkDto {
                available: l.is_some(),
                lib_title: l.map(|l| l.title.clone()).unwrap_or(m.lib_title),
                id: m.id,
                kind: m.kind,
                lib: m.lib,
                path: m.path,
                book_id: m.book_id,
                lb: m.lb,
                quote: m.quote,
                note: m.note,
                doc_title: m.doc_title,
                author: m.author,
                created: m.created,
                tags: m.tags,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn mark_tags(state: State<'_, AppState>) -> Result<Vec<TagCountDto>, String> {
    Ok(state.marks.tags()?.into_iter().map(|(tag, count)| TagCountDto { tag, count }).collect())
}

#[tauri::command]
pub async fn node_tags_get(lib: String, book_id: Option<String>, path: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.marks.node_tags(&lib, book_id.as_deref(), &path)
}

#[tauri::command]
pub async fn node_tags_set(
    lib: String,
    book_id: Option<String>,
    path: String,
    doc_title: String,
    tags: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let lib_title = state.library(&lib)?.title;
    state.marks.set_node_tags(&lib, book_id.as_deref(), &path, &doc_title, &lib_title, &parse_tags(&tags))?;
    state.sidecar_sync();
    Ok(())
}

// ------------------------------------------------------------ properties

#[derive(Serialize)]
pub struct PropsDto {
    /// what the user set on this node (a dimension with an empty list: set to nothing)
    own: Dims,
    /// what the data itself gives (before the user's properties)
    derived: Dims,
    tags: Vec<String>,
}

/// The values the data gives a node, as facets say.
fn derived_dims(state: &AppState, l: &Library, book_id: Option<&str>, path: &str) -> Result<Dims, String> {
    use smart_library_core::library::cbeta_facets;
    let facets = match l.kind.as_str() {
        "cbeta" => match book_id {
            Some(b) => cbeta_facets(state.bookcase_of(l)?.as_ref(), b).unwrap_or_default(),
            None => Vec::new(),
        },
        "html" => {
            let t = state.html_tree(l)?;
            let bc = state.bookcase_of(l)?;
            let only: HashSet<String> = [path.to_string()].into_iter().collect();
            let (_, mut docs) = t.docs(&bc, Some(&only));
            docs.next().and_then(|d| d.ok()).map(|d| d.facets).unwrap_or_default()
        }
        "folder" => {
            let f = state.folder(l)?;
            let files = f.files();
            match files.iter().find(|x| x.rel == path) {
                Some(file) => f.docs(std::slice::from_ref(file)).next().and_then(|d| d.ok()).map(|d| d.facets).unwrap_or_default(),
                None => Vec::new(), // a folder: the data gives it nothing
            }
        }
        "notes" => match path.parse::<i64>() {
            Ok(id) => {
                let mut bulei = vec!["bulei".to_string()];
                bulei.extend(state.notes(l)?.folders_of(id)?.into_iter().take(2));
                if bulei.len() > 1 { vec![bulei] } else { Vec::new() }
            }
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };
    Ok(props::dims_of(&facets))
}

/// The properties of a node, for the properties dialog.
#[tauri::command]
pub async fn props_get(lib: String, book_id: Option<String>, path: String, state: State<'_, AppState>) -> Result<PropsDto, String> {
    let l = state.library(&lib)?;
    Ok(PropsDto {
        own: state.props.get(&lib, book_id.as_deref(), &path)?,
        derived: derived_dims(&state, &l, book_id.as_deref(), &path).unwrap_or_default(),
        tags: state.marks.node_tags(&lib, book_id.as_deref(), &path).unwrap_or_default(),
    })
}

#[derive(Serialize)]
pub struct PropsSaved {
    /// the documents of the index that are being updated (0: no index, or none affected)
    updating: usize,
}

/// Keep the properties of a node, and bring the index up to date for the
/// documents they apply to (in the background; `index-done` tells when).
#[tauri::command]
pub async fn props_set(
    lib: String,
    book_id: Option<String>,
    path: String,
    dims: Dims,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PropsSaved, String> {
    let l = state.library(&lib)?;
    state.props.set(&lib, book_id.as_deref(), &path, &dims)?;
    state.sidecar_sync();
    let dir = state.index_dir(&lib);
    if !dir.join("smart-library-index.json").exists() {
        return Ok(PropsSaved { updating: 0 });
    }
    if state.building.lock().unwrap().contains(&lib) {
        // a rebuild is running: it may have read the old properties
        return Err("正在建立索引：属性已保存，索引完成后请再保存一次或重建索引".into());
    }
    // the documents the properties apply to
    enum Job {
        Cbeta(Arc<Bookcase>, HashSet<String>),
        Html(Arc<Bookcase>, Arc<HtmlTree>, HashSet<String>),
        Folder(Arc<FolderLibrary>, Vec<folder::FolderFile>),
        Notes(Arc<NotesStore>, HashSet<i64>),
    }
    let job = match l.kind.as_str() {
        "cbeta" => Job::Cbeta(state.bookcase_of(&l)?, book_id.iter().cloned().collect()),
        "html" => Job::Html(state.bookcase_of(&l)?, state.html_tree(&l)?, [path.clone()].into_iter().collect()),
        "folder" => {
            let f = state.folder(&l)?;
            let prefix = format!("{}/", path);
            let files = f.files().into_iter().filter(|x| x.rel == path || x.rel.starts_with(&prefix)).collect();
            Job::Folder(f, files)
        }
        "notes" => {
            let store = state.notes(&l)?;
            let ids = store.descendants(note_id(&path)?)?.into_iter().collect();
            Job::Notes(store, ids)
        }
        _ => return Ok(PropsSaved { updating: 0 }),
    };
    let overlay = state.props.overlay(&lib)?;
    let lib_title = l.title.clone();
    state.building.lock().unwrap().insert(lib.clone());
    state.indexes.write().unwrap().remove(&lib);
    std::thread::spawn(move || {
        let result = (|| -> Result<usize, String> {
            use smart_library_core::search::update_docs;
            match &job {
                Job::Cbeta(bc, books) => update_docs(&dir, smart_library_core::library::cbeta_docs(bc, Some(books)).1, &overlay),
                Job::Html(bc, t, paths) => update_docs(&dir, t.docs(bc, Some(paths)).1, &overlay),
                Job::Folder(f, files) => update_docs(&dir, f.docs(files), &overlay),
                Job::Notes(store, ids) => {
                    let rows: Vec<_> = store.rows()?.into_iter().filter(|r| ids.contains(&r.id)).collect();
                    update_docs(&dir, notes::docs(&rows, &lib_title).into_iter().map(Ok), &overlay)
                }
            }
        })();
        let state = app.state::<AppState>();
        state.building.lock().unwrap().remove(&lib);
        state.indexes.write().unwrap().remove(&lib); // opened again with the new documents
        let done = match result {
            Ok(n) => IndexDoneDto { lib: lib.clone(), ok: true, message: format!("属性已更新到索引（{} 篇）", n) },
            Err(e) => IndexDoneDto { lib: lib.clone(), ok: false, message: format!("更新索引失败：{}，可以重建索引", e) },
        };
        let _ = app.emit("index-done", done);
    });
    Ok(PropsSaved { updating: 1 })
}

// ------------------------------------------------------------ history

#[derive(Serialize)]
pub struct RecentDto {
    lib: String,
    lib_title: String,
    path: String,
    book_id: Option<String>,
    title: String,
    lb: Option<String>,
    mode: Option<String>,
    /// unix seconds
    opened: i64,
    visits: i64,
}

#[tauri::command]
pub async fn recent_touch(lib: String, path: String, book_id: Option<String>, title: String, state: State<'_, AppState>) -> Result<(), String> {
    state.history.touch(&lib, &path, book_id.as_deref(), &title)
}

#[tauri::command]
pub async fn recent_progress(lib: String, path: String, lb: String, mode: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    state.history.set_progress(&lib, &path, &lb, mode.as_deref())
}

#[tauri::command]
pub async fn recent_list(filter: Option<String>, limit: Option<usize>, state: State<'_, AppState>) -> Result<Vec<RecentDto>, String> {
    let libs = state.libraries.read().unwrap().clone();
    let rows = state.history.list(filter.as_deref().unwrap_or(""), limit.unwrap_or(300).clamp(1, 2000))?;
    // documents of libraries that are no longer bound are not listed
    Ok(rows
        .into_iter()
        .filter_map(|e| {
            let l = libs.iter().find(|l| l.id == e.lib)?;
            Some(RecentDto {
                lib_title: l.title.clone(),
                lib: e.lib,
                path: e.path,
                book_id: e.book_id,
                title: e.title,
                lb: e.lb,
                mode: e.mode,
                opened: e.opened,
                visits: e.visits,
            })
        })
        .collect())
}

/// What was ever opened in a library, to mark it in the tree.
#[derive(Serialize)]
pub struct OpenedDto {
    books: Vec<String>,
    paths: Vec<String>,
}

#[tauri::command]
pub async fn opened_set(lib: String, state: State<'_, AppState>) -> Result<OpenedDto, String> {
    let rows = state.history.opened(&lib)?;
    let mut books: Vec<String> = rows.iter().filter_map(|(_, b)| b.clone()).collect();
    books.sort();
    books.dedup();
    Ok(OpenedDto { books, paths: rows.into_iter().map(|(p, _)| p).collect() })
}

#[tauri::command]
pub async fn recent_remove(lib: String, path: String, state: State<'_, AppState>) -> Result<(), String> {
    state.history.remove(&lib, &path)
}

#[tauri::command]
pub async fn recent_clear(state: State<'_, AppState>) -> Result<(), String> {
    state.history.clear()
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

// ------------------------------------------------------------ program info, fonts, find

#[derive(Serialize)]
pub struct AppInfo {
    name: String,
    /// YY.MM.DD
    version: String,
    data_dir: String,
    /// the address the `sl` protocol is reached by (pictures of notes)
    asset_prefix: String,
}

/// Cargo's version is semver (`26.11.8`); the program shows it as a date.
fn display_version() -> String {
    let v = env!("CARGO_PKG_VERSION");
    let p: Vec<&str> = v.split('.').collect();
    if p.len() == 3 {
        format!("{}.{:0>2}.{:0>2}", p[0], p[1], p[2])
    } else {
        v.to_string()
    }
}

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> Result<AppInfo, String> {
    Ok(AppInfo {
        name: "Smart Library".into(),
        version: display_version(),
        data_dir: state.data_dir.display().to_string(),
        asset_prefix: ASSET_PREFIX.to_string(),
    })
}

#[derive(Serialize, Clone)]
pub struct FontDto {
    family: String,
    label: String,
    cjk: bool,
}

/// The fonts installed on this computer (made once per run).
#[tauri::command]
pub async fn list_fonts() -> Result<Vec<FontDto>, String> {
    static FONTS: std::sync::OnceLock<Vec<FontDto>> = std::sync::OnceLock::new();
    if let Some(f) = FONTS.get() {
        return Ok(f.clone());
    }
    let list = tauri::async_runtime::spawn_blocking(|| {
        crate::fonts::installed()
            .into_iter()
            .map(|f| FontDto { family: f.family, label: f.label, cjk: f.cjk })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(FONTS.get_or_init(|| list).clone())
}

/// Open the browser's own find bar (what Ctrl+F does) by pressing the keys:
/// the page cannot do it, and the bar takes the selected text as its input.
#[tauri::command]
pub async fn find_in_page() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL,
        };
        fn key(vk: u16, up: bool) -> INPUT {
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 },
                },
            }
        }
        let f = b'F' as u16;
        let inputs = [key(VK_CONTROL, false), key(f, false), key(f, true), key(VK_CONTROL, true)];
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            return Ok(());
        }
        Err("无法发送按键".into())
    }
    #[cfg(not(windows))]
    {
        Err("不支持".into())
    }
}

// ------------------------------------------------------------ export / import

#[derive(Deserialize)]
pub struct ExportReq {
    lib: String,
    /// the whole library
    #[serde(default)]
    whole: bool,
    /// CBETA / HTML libraries: index paths (`/0/3/5`) of the picked nodes
    #[serde(default)]
    keys: Vec<String>,
    /// folder / notes libraries: paths or note ids of the picked nodes
    #[serde(default)]
    rels: Vec<String>,
    /// folder libraries: also keep the original files
    #[serde(default)]
    with_files: bool,
    /// the ZIP file to write
    out: String,
}

#[derive(Serialize)]
pub struct ExportDone {
    path: String,
    docs: usize,
    files: usize,
}

/// What one node of the export stands for.
enum Src {
    None,
    /// CBETA book: its volumes from `from` up to (not including) `to`
    Cbeta { book: String, from: Option<String>, to: Option<String> },
    Html(String),
    Folder(String),
}

struct Plan {
    title: String,
    src: Src,
    /// internal libraries: the note
    note: Option<i64>,
    /// the node as the user properties know it: (book id, path)
    key: Option<(Option<String>, String)>,
    children: Vec<Plan>,
}

/// Parts of big CBETA books start at their `start` volume: book id -> starts.
fn part_starts(nodes: &[NodeDto], out: &mut HashMap<String, Vec<String>>) {
    for n in nodes {
        if let (Some(b), Some(s)) = (&n.book_id, &n.start) {
            out.entry(b.clone()).or_default().push(s.clone());
        }
        part_starts(&n.children, out);
    }
}

fn plan_of_dto(n: &NodeDto, kind: &str, parts: &HashMap<String, Vec<String>>) -> Plan {
    let src = if kind == "html" {
        n.path.clone().map(Src::Html).unwrap_or(Src::None)
    } else if let Some(book) = &n.book_id {
        let to = parts
            .get(book)
            .and_then(|v| v.iter().filter(|s| n.start.as_ref().is_none_or(|f| *s > f)).min().cloned());
        Src::Cbeta { book: book.clone(), from: n.start.clone(), to }
    } else {
        Src::None
    };
    // CBETA: the whole book only (a part of a big book is not a node of its own)
    let key = if kind == "html" {
        n.path.clone().map(|p| (None, p))
    } else {
        n.book_id.clone().filter(|_| n.start.is_none()).map(|b| (Some(b), String::new()))
    };
    Plan {
        title: n.title.clone(),
        src,
        note: None,
        key,
        children: n.children.iter().map(|c| plan_of_dto(c, kind, parts)).collect(),
    }
}

fn plan_of_folder(f: &FolderLibrary, rel: &str, name: &str, is_dir: bool) -> Result<Plan, String> {
    let title = if is_dir {
        name.to_string()
    } else {
        name.rsplit_once('.').map(|(s, _)| s.to_string()).filter(|s| !s.is_empty()).unwrap_or(name.to_string())
    };
    if !is_dir {
        return Ok(Plan { title, src: Src::Folder(rel.to_string()), note: None, key: Some((None, rel.to_string())), children: Vec::new() });
    }
    let mut children = Vec::new();
    for e in f.children(rel)? {
        children.push(plan_of_folder(f, &e.rel, &e.name, e.is_dir)?);
    }
    Ok(Plan { title, src: Src::None, note: None, key: Some((None, rel.to_string())), children })
}

fn plan_of_note(s: &NotesStore, id: i64) -> Result<Plan, String> {
    let n = s.get(id)?.ok_or("笔记不存在")?;
    let mut children = Vec::new();
    for c in s.children(Some(id))? {
        children.push(plan_of_note(s, c.id)?);
    }
    Ok(Plan { title: n.title, src: Src::None, note: Some(id), key: Some((None, id.to_string())), children })
}

/// The picked nodes without those below another picked node.
fn drop_nested(keys: &[String]) -> Vec<String> {
    keys.iter()
        .filter(|k| !keys.iter().any(|o| o != *k && k.starts_with(&format!("{}/", o))))
        .cloned()
        .collect()
}

struct Exporter<'a> {
    state: &'a AppState,
    lib: Library,
    w: smart_library_core::exchange::PackageWriter,
    with_files: bool,
    done: usize,
    files: usize,
    total: usize,
    app: AppHandle,
    /// pictures of notes already in the package
    attached: HashSet<String>,
}

impl Exporter<'_> {
    fn tick(&mut self) {
        self.done += 1;
        let _ = self.app.emit(
            "export-progress",
            ProgressDto { lib: self.lib.id.clone(), done: self.done, total: self.total.max(self.done) },
        );
    }

    /// The volumes of a CBETA part.
    fn volumes(&self, book: &str, from: &Option<String>, to: &Option<String>) -> Result<Vec<smart_library_core::bookcase::VolumeRef>, String> {
        let bc = self.state.bookcase_of(&self.lib)?;
        Ok(bc
            .volumes(book)
            .into_iter()
            .filter(|v| v.exists && from.as_ref().is_none_or(|f| &v.path >= f) && to.as_ref().is_none_or(|t| &v.path < t))
            .collect())
    }

    fn count(&self, p: &Plan) -> usize {
        let own = match &p.src {
            Src::None => usize::from(p.note.is_some()),
            Src::Cbeta { book, from, to } => self.volumes(book, from, to).map(|v| v.len()).unwrap_or(1),
            _ => 1,
        };
        own + p.children.iter().map(|c| self.count(c)).sum::<usize>()
    }

    fn doc(&mut self, title: &str, body_md: &str, source: &str) -> Result<String, String> {
        let name = self.w.add_doc(&smart_library_core::exchange::document(title, body_md, source))?;
        self.tick();
        Ok(name)
    }

    fn node(&mut self, p: &Plan) -> Result<serde_json::Value, String> {
        use serde_json::{json, Map, Value};
        let mut m = Map::new();
        m.insert("title".into(), json!(p.title));
        // the user's properties of the node travel with it
        if let Some((book, path)) = &p.key {
            let own = self.state.props.get(&self.lib.id, book.as_deref(), path).unwrap_or_default();
            if !own.is_empty() {
                m.insert("props".into(), json!(own));
            }
        }
        let mut children: Vec<Value> = Vec::new();
        if let Some(id) = p.note {
            let store = self.state.notes(&self.lib)?;
            let n = store.get(id)?.ok_or("笔记不存在")?;
            let file = if n.content_type == smart_library_core::whiteboard::TYPE {
                // the Markdown file lists the texts; the scene itself is kept apart
                let file = self.doc(&p.title, &smart_library_core::whiteboard::to_markdown(&n.content), "")?;
                let data = self.w.add_data(&n.content, "wb.json")?;
                m.insert("whiteboard".into(), json!(data));
                file
            } else if n.content_type == smart_library_core::mindmap::TYPE {
                // the Markdown file is the outline; the map itself is kept apart
                let file = self.doc(&p.title, &smart_library_core::mindmap::to_markdown(&n.content), "")?;
                let data = self.w.add_data(&n.content, "mm.json")?;
                m.insert("mindmap".into(), json!(data));
                file
            } else {
                self.doc(&p.title, &n.content, "")?
            };
            m.insert("file".into(), json!(file));
            // the pictures the note shows
            for name in smart_library_core::attach::refs(&n.content) {
                if self.attached.insert(name.clone()) {
                    if let Some(path) = smart_library_core::attach::path_of(&self.state.attachments_dir(&self.lib.id), &name) {
                        if let Ok(bytes) = std::fs::read(path) {
                            self.w.add_file(&format!("attachments/{}", name), &bytes)?;
                            self.files += 1;
                        }
                    }
                }
            }
        }
        match &p.src {
            Src::None => {}
            Src::Cbeta { book, from, to } => {
                let bc = self.state.bookcase_of(&self.lib)?;
                let info = bc.book(book).ok_or_else(|| format!("没有这部经：{}", book))?;
                let (btitle, author) = (info.title.clone(), info.author_info.clone());
                let version = bc.data_version();
                let vols = self.volumes(book, from, to)?;
                let single = vols.len() == 1;
                for v in &vols {
                    let html = bc.volume_html(&v.path, &RenderOptions::default())?.html;
                    let md = smart_library_core::exchange::html_to_md(&html);
                    let title = if single { p.title.clone() } else { format!("{} {}", btitle, v.label) };
                    let src = format!("CBETA {} · {} {} {} · {}", version, book, btitle, v.label, author);
                    let file = self.doc(&title, &md, src.trim())?;
                    if single {
                        m.insert("file".into(), json!(file));
                    } else {
                        children.push(json!({ "title": v.label, "file": file, "children": [] }));
                    }
                }
            }
            Src::Html(path) => {
                let bc = self.state.bookcase_of(&self.lib)?;
                let a = bc.article(path, Some(&RenderOptions::default()))?;
                let md = smart_library_core::exchange::html_to_md(&a.html.unwrap_or_default());
                let src = format!("{} · {}", self.lib.title, a.meta.authors.join("、"));
                let file = self.doc(&p.title, &md, src.trim_end_matches([' ', '·']))?;
                m.insert("file".into(), json!(file));
            }
            Src::Folder(rel) => {
                let f = self.state.folder(&self.lib)?;
                let a = f.article(rel, Some(&RenderOptions::default()))?;
                let md = smart_library_core::exchange::html_to_md(&a.html.unwrap_or_default());
                let src = format!("{} · {}", self.lib.title, rel);
                let file = self.doc(&p.title, &md, &src)?;
                m.insert("file".into(), json!(file));
                let name = rel.rsplit('/').next().unwrap_or(rel);
                if self.with_files && !folder::is_text(name) {
                    let bytes = std::fs::read(f.root.join(rel)).map_err(|e| format!("{}: {}", rel, e))?;
                    let original = format!("files/{}", rel);
                    self.w.add_file(&original, &bytes)?;
                    self.files += 1;
                    m.insert("original".into(), json!(original));
                }
            }
        }
        for c in &p.children {
            children.push(self.node(c)?);
        }
        m.insert("children".into(), Value::Array(children));
        Ok(Value::Object(m))
    }
}

/// Write the picked nodes (or the whole library) as an export package.
/// Reports `export-progress` {lib, done, total}.
#[tauri::command]
pub async fn export_library(req: ExportReq, app: AppHandle, state: State<'_, AppState>) -> Result<ExportDone, String> {
    let l = state.library(&req.lib)?;
    let mut plans: Vec<Plan> = Vec::new();
    match l.kind.as_str() {
        "folder" => {
            let f = state.folder(&l)?;
            if req.whole || req.rels.is_empty() {
                for e in f.children("")? {
                    plans.push(plan_of_folder(&f, &e.rel, &e.name, e.is_dir)?);
                }
            } else {
                for rel in drop_nested(&req.rels) {
                    let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                    let is_dir = f.abs(&rel).map(|p| p.is_dir()).unwrap_or(false);
                    plans.push(plan_of_folder(&f, &rel, &name, is_dir)?);
                }
            }
        }
        "notes" => {
            let s = state.notes(&l)?;
            if req.whole || req.rels.is_empty() {
                for c in s.children(None)? {
                    plans.push(plan_of_note(&s, c.id)?);
                }
            } else {
                let ids: Vec<i64> = req.rels.iter().filter_map(|r| r.parse().ok()).collect();
                for id in &ids {
                    let nested = ids.iter().any(|o| o != id && s.descendants(*o).map(|d| d.contains(id)).unwrap_or(false));
                    if !nested {
                        plans.push(plan_of_note(&s, *id)?);
                    }
                }
            }
        }
        _ => {
            let roots = tree(req.lib.clone(), state.clone()).await?;
            let mut parts = HashMap::new();
            part_starts(&roots, &mut parts);
            if req.whole || req.keys.is_empty() {
                plans.extend(roots.iter().map(|n| plan_of_dto(n, &l.kind, &parts)));
            } else {
                for key in drop_nested(&req.keys) {
                    let mut list: &[NodeDto] = &roots;
                    let mut node = None;
                    for i in key.split('/').filter(|s| !s.is_empty()) {
                        let n = list
                            .get(i.parse::<usize>().map_err(|_| "节点位置不对")?)
                            .ok_or("目录树已经变化，请重新选择")?;
                        list = &n.children;
                        node = Some(n);
                    }
                    if let Some(n) = node {
                        plans.push(plan_of_dto(n, &l.kind, &parts));
                    }
                }
            }
        }
    }
    let whole = req.whole || (req.keys.is_empty() && req.rels.is_empty());
    let w = smart_library_core::exchange::PackageWriter::create(Path::new(&req.out))?;
    let mut ex = Exporter { state: &state, lib: l.clone(), w, with_files: req.with_files, done: 0, files: 0, total: 0, app, attached: HashSet::new() };
    ex.total = plans.iter().map(|p| ex.count(p)).sum();
    let mut nodes = Vec::new();
    for p in &plans {
        nodes.push(ex.node(p)?);
    }
    let manifest = smart_library_core::exchange::manifest(&l.title, &l.kind, &l.source, if whole { "library" } else { "nodes" }, nodes);
    let (docs, files) = (ex.done, ex.files);
    ex.w.finish(&manifest)?;
    Ok(ExportDone { path: req.out, docs, files })
}

/// A picture of an export package goes to the attachments of the library (the same name = the same picture).
fn keep_picture(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let file = smart_library_core::attach::path_of(dir, name).ok_or("图片名不对")?;
    if !file.exists() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(file, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct ImportDone {
    status: StatusDto,
    title: String,
    notes: usize,
}

/// Read an export package: into a new internal library, or (`lib`, and
/// `parent` a note id or none for the top) below a place of an internal one.
#[tauri::command]
pub async fn import_package(path: String, lib: Option<String>, parent: Option<String>, state: State<'_, AppState>) -> Result<ImportDone, String> {
    let mut pkg = smart_library_core::exchange::Package::open(Path::new(&path))?;
    if let Some(lib_id) = lib.filter(|l| !l.is_empty()) {
        let store = notes_of(&state, &lib_id)?;
        let parent = match parent.filter(|p| !p.is_empty()) {
            Some(p) => Some(note_id(&p)?),
            None => None,
        };
        // a whole library comes as one note named like it; picked nodes come as they are
        let wrap = pkg.manifest["scope"] == "library";
        let dir = state.attachments_dir(&lib_id);
        let notes = smart_library_core::exchange::import_into(
            &store,
            &mut pkg,
            parent,
            wrap,
            &mut |name, bytes| keep_picture(&dir, name, &bytes),
            &mut |id, dims| {
                let _ = state.props.set(&lib_id, None, &id.to_string(), dims);
            },
        )?;
        return Ok(ImportDone { status: status_of(&state), title: state.library(&lib_id)?.title, notes });
    }
    let base = pkg.title();
    let title = {
        let libs = state.libraries.read().unwrap();
        let mut t = base.clone();
        let mut n = 1;
        while libs.iter().any(|l| l.title == t) {
            n += 1;
            t = if n == 2 { format!("{}（导入）", base) } else { format!("{}（导入 {}）", base, n) };
        }
        t
    };
    let status = create_notes_library(title.clone(), state.clone()).await?;
    let lib = state
        .libraries
        .read()
        .unwrap()
        .iter()
        .find(|l| l.title == title)
        .cloned()
        .ok_or("知识库没有建好")?;
    let store = state.notes(&lib)?;
    let dir = state.attachments_dir(&lib.id);
    let lib_id = lib.id.clone();
    let notes = smart_library_core::exchange::import_into(
        &store,
        &mut pkg,
        None,
        false,
        &mut |name, bytes| keep_picture(&dir, name, &bytes),
        &mut |id, dims| {
            let _ = state.props.set(&lib_id, None, &id.to_string(), dims);
        },
    )?;
    state.titles.write().unwrap().remove(&lib.id);
    Ok(ImportDone { status, title, notes })
}
