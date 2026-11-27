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
use smart_library_core::ai_chat::AiChatStore;
use smart_library_core::bookcase::Bookcase;
use smart_library_core::folder::{self, FolderFilter, FolderLibrary};
use smart_library_core::history::History;
use smart_library_core::marks::{parse_tags, MarkFilter, MarkNew, Marks};
use smart_library_core::props::{self, Dims, Props};
use smart_library_core::library::{classify_trees, HtmlTree, TreeGroup};
use smart_library_core::nav::NavNode;
use smart_library_core::notes::{self, NotesStore};
use smart_library_core::render::RenderOptions;
use smart_library_core::search::{facet_label, locate, snippets, Filter, Hit, MatchMode, SearchIndex, SearchRequest, Within};
use smart_library_core::snapshots::{SnapshotMeta, SnapshotPolicy, SnapshotRecord, SnapshotStore};
use smart_library_core::source::{DirSource, Source, ZipSource};
use smart_library_core::cite::{parse_citations, parse_command};
use smart_library_core::dict::{DictConfig, DictEntryDetail, DictInfo, DictManager, DictSearchResult};
use smart_library_core::titles::{query_words, TitleIndex};
use smart_library_core::toc::TocItem;
use smart_library_core::volume::VolumeText;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// Tree shown for CBETA/HTML; for ZIM, the optional Wikimedia project ID.
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
    /// remote git URL when this folder library is a read-only git clone
    #[serde(default)]
    pub remote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

type ZimEntry = (String, Arc<Mutex<smart_library_core::zim::Zim>>);

pub struct AppState {
    config_dir: PathBuf,
    data_dir: PathBuf,
    workspace_file: PathBuf,
    window_file: PathBuf,
    window_state: Mutex<Option<WindowState>>,
    settings_lock: Mutex<()>,
    libraries: RwLock<Vec<Library>>,
    sources: RwLock<HashMap<String, Arc<Bookcase>>>,
    html: RwLock<HashMap<String, Arc<HtmlTree>>>,
    folders: RwLock<HashMap<String, Arc<FolderLibrary>>>,
    zims: RwLock<HashMap<String, ZimEntry>>,
    notes: RwLock<HashMap<String, Arc<NotesStore>>>,
    /// read-write folder libraries: the stamp of each open file as last read or
    /// saved (`<lib>|<path>`), to notice changes made outside
    stamps: Mutex<HashMap<String, String>>,
    /// Edited folder paths, per library. SQLite notes keep their own index journal.
    dirty: Mutex<HashMap<String, HashSet<String>>>,
    /// Folder indexes compared with the source since this process started.
    checked_folder_indexes: Mutex<HashSet<String>>,
    /// the stamp of each read-write folder's `meta.json` as we last wrote or read it
    sidecar_seen: Mutex<HashMap<String, String>>,
    indexes: RwLock<HashMap<String, Arc<SearchIndex>>>,
    /// title quick search, per library (built on first use)
    titles: RwLock<HashMap<String, Arc<TitleIndex>>>,
    /// monotonically increases whenever `titles` changes (used to invalidate book-ref caches)
    titles_version: AtomicU64,
    /// libraries whose title list is being built right now (the window shows
    /// progress on the library node; a second `prepare_titles` does not start
    /// the same work again)
    titles_building: Mutex<HashSet<String>>,
    building: Mutex<HashSet<String>>,
    /// set by `cancel_bind` while the window waits for a long bind (opening a
    /// huge ZIM archive); the operation checks it and gives up
    bind_cancel: AtomicBool,
    /// true if `marks.db` or `props.db` failed to open on disk and fell back to in-memory
    db_degraded: bool,
    history: History,
    marks: Marks,
    /// user properties of nodes (部类, 时域, 人物, 藏经, 典类)
    props: Props,
    ai_chat: AiChatStore,
    pub(crate) snapshots: SnapshotStore,
    dicts: RwLock<Option<DictManager>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&config_dir);
        let _ = std::fs::create_dir_all(&data_dir);
        let libraries = load_registry(&config_dir);
        smart_library_core::dynasty::set_user(&std::fs::read_to_string(config_dir.join("dynasties.txt")).unwrap_or_default());
        smart_library_core::pinyin::set_user(&std::fs::read_to_string(config_dir.join("pinyin.txt")).unwrap_or_default());
        let data_dir_for_history = data_dir.join("history.db");
        let window_file = data_dir.join("window.json");
        let window_state = std::fs::read_to_string(&window_file)
            .ok()
            .and_then(|s| serde_json::from_str::<WindowState>(&s).ok());
        let mut db_degraded = false;
        let history = History::open(&data_dir_for_history).unwrap_or_else(|e| {
            eprintln!("failed to open history.db: {e}");
            History::memory().unwrap()
        });
        let marks = Marks::open(&data_dir_for_history.with_file_name("marks.db")).unwrap_or_else(|e| {
            eprintln!("failed to open marks.db: {e}");
            db_degraded = true;
            Marks::memory().unwrap()
        });
        let props = Props::open(&data_dir_for_history.with_file_name("props.db")).unwrap_or_else(|e| {
            eprintln!("failed to open props.db: {e}");
            db_degraded = true;
            Props::memory().unwrap()
        });
        let ai_chat = AiChatStore::open(&data_dir_for_history.with_file_name("ai_chat.db")).unwrap_or_else(|e| {
            eprintln!("failed to open ai_chat.db: {e}");
            AiChatStore::memory().unwrap()
        });
        let snapshots = SnapshotStore::open(&data_dir_for_history.with_file_name("snapshots.db")).unwrap_or_else(|e| {
            eprintln!("failed to open snapshots.db: {e}");
            SnapshotStore::memory().unwrap()
        });
        AppState {
            workspace_file: data_dir.join("workspace.json"),
            window_file,
            window_state: Mutex::new(window_state),
            settings_lock: Mutex::new(()),
            config_dir,
            data_dir,
            libraries: RwLock::new(libraries),
            sources: RwLock::new(HashMap::new()),
            html: RwLock::new(HashMap::new()),
            folders: RwLock::new(HashMap::new()),
            zims: RwLock::new(HashMap::new()),
            notes: RwLock::new(HashMap::new()),
            stamps: Mutex::new(HashMap::new()),
            dirty: Mutex::new(HashMap::new()),
            checked_folder_indexes: Mutex::new(HashSet::new()),
            sidecar_seen: Mutex::new(HashMap::new()),
            indexes: RwLock::new(HashMap::new()),
            titles: RwLock::new(HashMap::new()),
            titles_version: AtomicU64::new(1),
            titles_building: Mutex::new(HashSet::new()),
            building: Mutex::new(HashSet::new()),
            bind_cancel: AtomicBool::new(false),
            db_degraded,
            history,
            marks,
            props,
            ai_chat,
            snapshots,
            dicts: RwLock::new(None),
        }
    }

    pub(crate) fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub(crate) fn library(&self, id: &str) -> Result<Library, String> {
        self.libraries
            .read()
            .unwrap()
            .iter()
            .find(|l| l.id == id)
            .cloned()
            .ok_or_else(|| format!("没有这个知识库：{}", id))
    }

    pub fn saved_window_state(&self) -> Option<WindowState> {
        self.window_state.lock().unwrap().clone()
    }

    pub fn update_window_state(&self, x: i32, y: i32, width: u32, height: u32, maximized: bool) {
        let mut guard = self.window_state.lock().unwrap();
        if maximized {
            if let Some(prev) = guard.as_mut() {
                prev.maximized = true;
            } else {
                *guard = Some(WindowState { x, y, width, height, maximized: true });
            }
        } else if width >= 400 && height >= 300 {
            *guard = Some(WindowState { x, y, width, height, maximized: false });
        }
    }

    pub fn set_window_maximized(&self, maximized: bool) {
        if let Some(prev) = self.window_state.lock().unwrap().as_mut() {
            prev.maximized = maximized;
        }
    }

    pub fn save_window_state(&self) {
        let state = self.window_state.lock().unwrap().clone();
        if let Some(ws) = state {
            if let Some(dir) = self.window_file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let tmp = self.window_file.with_extension("json.tmp");
            if let Ok(bytes) = serde_json::to_vec(&ws) {
                if std::fs::write(&tmp, bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, &self.window_file);
                }
            }
        }
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
        let _ = self.queue_folder_property_changes(lib, &f);
        self.folders.write().unwrap().insert(lib.id.clone(), f.clone());
        Ok(f)
    }

    fn zim(&self, lib: &Library) -> Result<Arc<Mutex<smart_library_core::zim::Zim>>, String> {
        self.zim_with(lib, None)
    }

    /// Open (or reuse) a ZIM. Parsing a big archive takes a while; `progress`
    /// gets `(done, total)` entries while it happens, and what was parsed is
    /// kept on disk so the next open does not parse the archive again.
    fn zim_with(
        &self,
        lib: &Library,
        progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Arc<Mutex<smart_library_core::zim::Zim>>, String> {
        let stamp = zim_version(&lib.source)?;
        if let Some((old, z)) = self.zims.read().unwrap().get(&lib.id) {
            if *old == stamp { return Ok(z.clone()); }
        }
        let path = Path::new(&lib.source);
        let cache = std::fs::read(self.zim_cache_path(&lib.id)).ok();
        let z = match cache.as_deref().and_then(|b| smart_library_core::zim::Zim::restore_cached(path, b, &stamp)) {
            Some(z) => z,
            None => {
                let z = match progress {
                    Some(cb) => smart_library_core::zim::Zim::open_with(path, |done, total| {
                        cb(done, total);
                        true
                    })?,
                    None => smart_library_core::zim::Zim::open(path)?,
                };
                self.write_zim_cache(&lib.id, &z, &stamp);
                z
            }
        };
        let z = Arc::new(Mutex::new(z));
        self.zims.write().unwrap().insert(lib.id.clone(), (stamp, z.clone()));
        self.titles.write().unwrap().remove(&lib.id);
        self.indexes.write().unwrap().remove(&lib.id);
        Ok(z)
    }

    /// Where the parsed entry table of a ZIM is kept (see `Zim::store_entries`).
    fn zim_cache_path(&self, id: &str) -> PathBuf {
        self.data_dir.join("repos").join(id).join("zim.bin")
    }

    fn write_zim_cache(&self, id: &str, z: &smart_library_core::zim::Zim, source: &str) {
        let path = self.zim_cache_path(id);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
            let tmp = path.with_extension("bin.tmp");
            if std::fs::write(&tmp, z.store_entries(source)).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
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

    /// Delete the pictures of an internal library that no note has referred to for `older_than`
    /// seconds; how many and how many bytes.
    pub fn sweep_pictures(&self, lib: &Library, older_than: i64) -> Result<(usize, u64), String> {
        let dir = self.attachments_dir(&lib.id);
        let on_disk: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| smart_library_core::attach::is_name(n)).collect(),
            Err(_) => Vec::new(),
        };
        let gone = self.notes(lib)?.sweep_pictures(older_than, &on_disk)?;
        let mut bytes = 0u64;
        let mut n = 0usize;
        for name in gone {
            let path = dir.join(&name);
            bytes += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&path).is_ok() {
                n += 1;
            }
        }
        Ok((n, bytes))
    }

    /// The daily clean-up: pictures unreferenced for 24 hours.
    pub fn sweep_all_pictures(&self) {
        let libs: Vec<Library> = self.libraries.read().unwrap().clone();
        for l in libs.iter().filter(|l| l.kind == "notes") {
            let _ = self.sweep_pictures(l, 24 * 3600);
        }
    }

    fn titles(&self, lib: &Library) -> Result<Arc<TitleIndex>, String> {
        self.titles_with(lib, None)
    }

    /// Build (or reuse) the title list; `progress` is passed to the ZIM open,
    /// which is the slow part of a large archive.
    fn titles_with(&self, lib: &Library, progress: Option<&dyn Fn(usize, usize)>) -> Result<Arc<TitleIndex>, String> {
        if let Some(t) = self.titles.read().unwrap().get(&lib.id) {
            return Ok(t.clone());
        }
        // A bookcase tree or a big ZIM takes a while to turn into titles;
        // both are read again only when the source itself changed.
        let stamp = match lib.kind.as_str() {
            "zim" => zim_version(&lib.source).ok().map(|v| format!("zim:{v}")),
            "cbeta" | "html" => self
                .bookcase_of(lib)
                .ok()
                .map(|bc| format!("{}:{}:{}", lib.kind, bc.data_version(), lib.tree)),
            _ => None,
        };
        if let Some(stamp) = &stamp {
            if let Some(mut t) = std::fs::read(self.titles_cache_path(&lib.id))
                .ok()
                .and_then(|b| TitleIndex::restore(&b, stamp))
            {
                t.add_aliases(&self.props.all_aliases(&lib.id).unwrap_or_default());
                let t = Arc::new(t);
                self.titles.write().unwrap().insert(lib.id.clone(), t.clone());
                self.titles_version.fetch_add(1, Ordering::Relaxed);
                return Ok(t);
            }
        }
        let mut built = if lib.kind == "zim" {
            TitleIndex::zim(&self.zim_with(lib, progress)?.lock().unwrap(), &lib.title)
        } else if lib.kind == "notes" {
            TitleIndex::notes(&self.notes(lib)?.rows()?, &lib.title)
        } else if lib.kind == "folder" {
            let f = self.folder(lib)?;
            let name = Path::new(&lib.source).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            TitleIndex::folder(&f.files(), &f.dirs(), &name)
        } else if lib.kind == "html" {
            TitleIndex::html(&*self.html_tree(lib)?)
        } else {
            TitleIndex::cbeta(&*self.bookcase_of(lib)?, &lib.tree)?
        };
        if let Some(stamp) = &stamp {
            let path = self.titles_cache_path(&lib.id);
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
                let tmp = path.with_extension("bin.tmp");
                if std::fs::write(&tmp, built.store(stamp)).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        }
        built.add_aliases(&self.props.all_aliases(&lib.id).unwrap_or_default());
        let t = Arc::new(built);
        self.titles.write().unwrap().insert(lib.id.clone(), t.clone());
        self.titles_version.fetch_add(1, Ordering::Relaxed);
        Ok(t)
    }

    /// The cached title list of a library (see `TitleIndex::store`).
    fn titles_cache_path(&self, id: &str) -> PathBuf {
        self.data_dir.join("repos").join(id).join("titles.bin")
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
        let _ = self.ai_chat.rekey(lib, from, to, title);
        if let Err(e) = self.snapshots.rekey(lib, from, to) {
            eprintln!("failed to move snapshots for {lib}: {from} -> {to}: {e}");
        }
        self.sidecar_sync(lib);
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

    /// Write the changed read-write library's properties and marks into its
    /// `.smart-library/meta.json` (only when they differ from the file).
    /// Called after each change; a failure (a folder on a read-only disk …)
    /// does not stop the change, the data stays in our own files.
    fn sidecar_sync(&self, lib: &str) {
        if self.db_degraded {
            return;
        }
        let Ok(l) = self.library(lib) else { return };
        if l.kind == "folder" && l.writable {
            let _ = self.sidecar_write(&l);
        }
    }

    fn sidecar_write(&self, l: &Library) -> Result<bool, String> {
        if self.db_degraded {
            return Err("本地属性或收藏数据库处于只读降级状态，已跳过同步以保护文件夹元数据".into());
        }
        let props = self.props.all(&l.id)?.into_iter().filter(|(b, _, _)| b.is_empty()).map(|(_, p, d)| (p, d)).collect();
        let marks = self.marks.export_lib(&l.id)?;
        let root = Path::new(&l.source);
        let wrote = smart_library_core::sidecar::write(root, &smart_library_core::sidecar::Sidecar { props, marks })?;
        self.note_sidecar(l);
        Ok(wrote)
    }

    /// The file is as we have it now.
    fn note_sidecar(&self, l: &Library) {
        if let Some(s) = smart_library_core::sidecar::stamp(Path::new(&l.source)) {
            self.sidecar_seen.lock().unwrap().insert(l.id.clone(), s);
        }
    }

    /// The file of a read-write folder replaces what we keep for the library
    /// (it is the copy that travels with the folder).
    fn sidecar_load(&self, l: &Library) -> Result<(), String> {
        let Some(sc) = smart_library_core::sidecar::read(Path::new(&l.source), &l.id)? else { return Ok(()) };
        let props: Vec<(String, String, Dims)> = sc.props.into_iter().map(|(p, d)| (String::new(), p, d)).collect();
        self.props.replace_lib(&l.id, &props)?;
        let marks: Vec<_> = sc.marks.into_iter().map(|mut m| { m.lib_title = l.title.clone(); m }).collect();
        self.marks.replace_lib(&l.id, &marks)?;
        self.note_sidecar(l);
        Ok(())
    }

    /// A folder library has just become read-write: what its file has and what
    /// we kept so far are put together, and the file is written.
    fn sidecar_enable(&self, l: &Library) -> Result<(), String> {
        let mut imported_props = false;
        if let Some(sc) = smart_library_core::sidecar::read(Path::new(&l.source), &l.id)? {
            for (p, d) in sc.props {
                if self.props.get(&l.id, None, &p)?.is_empty() {
                    self.props.set(&l.id, None, &p, &d)?;
                    imported_props = true;
                }
            }
            let marks: Vec<_> = sc.marks.into_iter().map(|mut m| { m.lib_title = l.title.clone(); m }).collect();
            self.marks.merge_lib(&l.id, &marks)?;
        }
        self.sidecar_write(l)?;
        if imported_props {
            if let Ok(f) = self.folder(l) {
                let _ = self.queue_folder_property_changes(l, &f);
            }
        }
        Ok(())
    }

    fn save_registry(&self) -> Result<(), String> {
        let libs = self.libraries.write().unwrap();
        self.write_registry(&libs)
    }

    fn write_registry(&self, libs: &[Library]) -> Result<(), String> {
        std::fs::create_dir_all(&self.config_dir).map_err(|e| e.to_string())?;
        let json = serde_json::json!({ "libraries": libs });
        let temp = self.config_dir.join("libraries.json.tmp");
        let path = self.config_dir.join("libraries.json");
        std::fs::write(&temp, serde_json::to_string_pretty(&json).unwrap())
            .and_then(|()| std::fs::rename(&temp, &path))
            .map_err(|e| format!("保存知识库设置失败：{}", e))
    }

    /// Source settings change what the derived index can contain. Keep a
    /// marker on disk so a restart cannot reopen an index for the old rules.
    fn edit_library_source(&self, id: &str, update: impl FnOnce(&[Library], &mut Library) -> Result<bool, String>) -> Result<(), String> {
        let building = self.building.lock().unwrap();
        if building.contains(id) {
            return Err("正在建立这个知识库的索引，请完成后再修改资料源设置".into());
        }
        let mut libs = self.libraries.write().unwrap();
        let pos = libs.iter().position(|l| l.id == id).ok_or("找不到这个知识库")?;
        let mut next = libs[pos].clone();
        if !update(&libs, &mut next)? {
            return Ok(());
        }
        let dir = self.index_dir(id);
        let marker = dir.join("rebuild-required");
        let marked = dir.is_dir() && !marker.exists();
        if marked {
            std::fs::write(&marker, b"").map_err(|e| format!("无法标记旧索引：{}", e))?;
        }
        let old = std::mem::replace(&mut libs[pos], next);
        if let Err(e) = self.write_registry(&libs) {
            libs[pos] = old;
            if marked {
                let _ = std::fs::remove_file(&marker);
            }
            return Err(e);
        }
        self.indexes.write().unwrap().remove(id);
        self.folders.write().unwrap().remove(id);
        self.titles.write().unwrap().remove(id);
        self.checked_folder_indexes.lock().unwrap().remove(id);
        if old.source != libs[pos].source {
            self.html.write().unwrap().remove(id);
            self.zims.write().unwrap().remove(id);
            self.sidecar_seen.lock().unwrap().remove(id);
            if !libs.iter().any(|l| l.source == old.source) {
                self.sources.write().unwrap().remove(&old.source);
            }
            let _ = std::fs::remove_file(self.titles_cache_path(id));
            let _ = std::fs::remove_file(self.zim_cache_path(id));
        }
        Ok(())
    }
}

/// `libraries.json`; or, the first time, the bookcase of the old single
/// setting (`config.json` / `SMART_LIBRARY_BOOKCASE`) bound with its 部類
/// tree as library `cbeta` (which keeps the existing index).
fn load_registry(config_dir: &Path) -> Vec<Library> {
    if let Ok(s) = std::fs::read_to_string(config_dir.join("libraries.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Ok(mut libs) = serde_json::from_value::<Vec<Library>>(v["libraries"].clone()) {
                for l in &mut libs {
                    if !l.source.is_empty() {
                        l.source = folder::strip_win_prefix(&l.source);
                    }
                }
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
            remote: String::new(),
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
    if l.kind == "zim" {
        let z = state.zim(l)?;
        let mut z = z.lock().unwrap();
        let i = z.entry_index(path).ok_or("ZIM 文章不存在")?;
        z.article(i, render)
    } else if l.kind == "folder" {
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
    folder::strip_win_prefix(a)
        .replace('/', "\\")
        .eq_ignore_ascii_case(&folder::strip_win_prefix(b).replace('/', "\\"))
}

fn source_is_missing(lib: &Library) -> bool {
    match lib.kind.as_str() {
        "notes" => false,
        "folder" => !Path::new(&lib.source).is_dir(),
        _ => !Path::new(&lib.source).exists(),
    }
}

fn zim_version(path: &str) -> Result<String, String> {
    let m = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let modified = m.modified().map_err(|e| e.to_string())?.duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?;
    Ok(format!("{}-{}-{}-content3", m.len(), modified.as_secs(), modified.subsec_nanos()))
}

/// Is this ZIM still there and readable? Header only (see `Zim::probe`), so
/// the window's status call stays fast however large the archive is.
fn zim_probe_error(path: &str) -> Option<String> {
    smart_library_core::zim::Zim::probe(Path::new(path)).err()
}

/// `lz`, then `lz-2` … if taken.
fn new_id(base: &str, libs: &[Library]) -> String {
    let base: String = base.to_ascii_lowercase().bytes().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_').map(char::from).take(50).collect();
    let base = base.trim_matches(['-', '_']);
    let base = if base.is_empty() { "lib" } else { base };
    if !libs.iter().any(|l| l.id.eq_ignore_ascii_case(base)) {
        return base.to_string();
    }
    (2..).map(|n| format!("{}-{}", base, n)).find(|id| !libs.iter().any(|l| l.id.eq_ignore_ascii_case(id))).unwrap()
}

fn library_id(requested: Option<&str>, fallback: &str, libs: &[Library]) -> Result<String, String> {
    let id = requested.unwrap_or("").trim();
    if id.is_empty() { return Ok(new_id(fallback, libs)); }
    if id.len() > 64 || !id.bytes().next().is_some_and(|c| c.is_ascii_alphanumeric()) || !id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_') {
        return Err("知识库 ID 须以小写字母或数字开头，只能包含小写字母、数字、连字符和下划线，最多 64 位".into());
    }
    if libs.iter().any(|l| l.id.eq_ignore_ascii_case(id)) { return Err("知识库 ID 已被使用".into()); }
    Ok(id.to_string())
}

/// Bytes of an image for the `sl` protocol: `<lib>/<path in the bookcase>`.
pub fn asset(state: &AppState, url_path: &str) -> Option<(Vec<u8>, &'static str)> {
    let decoded = percent_decode(url_path);
    let (lib, path) = decoded.split_once('/')?;
    let lib = state.library(lib).ok()?;
    if lib.kind == "zim" {
        let z = state.zim(&lib).ok()?;
        let mut z = z.lock().ok()?;
        let i = z.entry_index(path)?;
        let mime = match z.entries[i].mime.as_deref()? {
            "image/png" => "image/png", "image/jpeg" => "image/jpeg", "image/gif" => "image/gif", "image/webp" => "image/webp", _ => return None,
        };
        return Some((z.read(i).ok()?, mime));
    }
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
    /// remote git URL (non-empty for read-only git remote libraries)
    remote: String,
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
            let (error, data_version) = if source_is_missing(&l) {
                (Some(format!("找不到资料源：{}", l.source)), None)
            } else if l.kind == "zim" {
                // header and MIME table only: opening a huge archive parses
                // every entry and would keep the tree blank for seconds
                (zim_probe_error(&l.source), zim_version(&l.source).ok())
            } else if l.kind == "notes" {
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
            let index_data_version = SearchIndex::data_version(&dir);
            let index_ready = state.index(&l.id).is_some() && (l.kind != "zim" || data_version.is_some() && data_version == index_data_version);
            LibraryDto {
                writable,
                remote: l.remote,
                error,
                data_version,
                index_ready,
                index_data_version,
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

/// What the window is waiting for while a library is read and bound. A huge
/// ZIM can be many gigabytes with millions of entries, so the bind dialog
/// shows `done / total` while its pointer table is parsed, and offers to give
/// up (`cancel_bind`).
#[derive(Serialize, Clone)]
struct BindProgressDto {
    label: String,
    done: usize,
    total: usize,
}

fn bind_progress(app: &AppHandle, label: &str, done: usize, total: usize) {
    let _ = app.emit("bind-progress", BindProgressDto { label: label.to_string(), done, total });
}

/// Give up on the long operation the bind dialog is waiting for.
#[tauri::command]
pub async fn cancel_bind(state: State<'_, AppState>) -> Result<(), String> {
    state.bind_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// What a ZIM file says about itself: quick enough to run as soon as the file
/// is picked, so 「绑定」only becomes available for an archive we can read.
#[derive(Serialize)]
pub struct ZimInfoDto {
    major: u16,
    minor: u16,
    entries: usize,
    clusters: usize,
    bytes: u64,
}

#[tauri::command]
pub async fn inspect_zim(path: String) -> Result<ZimInfoDto, String> {
    let path = std::fs::canonicalize(path.trim().trim_matches('"')).map_err(|e| e.to_string())?;
    let info = smart_library_core::zim::Zim::probe(&path)?;
    Ok(ZimInfoDto {
        major: info.major,
        minor: info.minor,
        entries: info.entries,
        clusters: info.clusters,
        bytes: info.bytes,
    })
}

#[tauri::command]
pub async fn add_zim(
    path: String,
    library_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    let path = std::fs::canonicalize(path.trim().trim_matches('"')).map_err(|e| e.to_string())?;
    state.bind_cancel.store(false, Ordering::SeqCst);
    bind_progress(&app, "正在读取 ZIM…", 0, 0);
    let z = smart_library_core::zim::Zim::open_with(&path, |done, total| {
        bind_progress(&app, "正在读取 ZIM…", done, total);
        !state.bind_cancel.load(Ordering::SeqCst)
    })?;
    let stamp = zim_version(&path.to_string_lossy())?;
    let path = folder::strip_win_prefix(&path.to_string_lossy());
    let id = {
        let mut libs = state.libraries.write().unwrap();
        if libs.iter().any(|l| l.kind == "zim" && same_path(&l.source, &path)) { return Err("这个 ZIM 数据包已经绑定".into()); }
        let title = Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "ZIM".into());
        let id = self::library_id(library_id.as_deref(), "zim", &libs)?;
        libs.push(Library { id: id.clone(), kind: "zim".into(), title, source: path, tree: String::new(), exts: vec![], max_mb: 0, writable: false, remote: String::new() });
        id
    };
    // Keep what was just read: a big archive takes minutes to open, and
    // indexing would open it again from the start otherwise.
    state.write_zim_cache(&id, &z, &stamp);
    state.zims.write().unwrap().insert(id, (stamp, Arc::new(Mutex::new(z))));
    state.save_registry()?;
    bind_progress(&app, "完成", 1, 1);
    Ok(status_of(&state))
}

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
pub async fn inspect_source(path: String, app: AppHandle, state: State<'_, AppState>) -> Result<SourceDto, String> {
    let path = path.trim().trim_matches('"').to_string();
    bind_progress(&app, "正在读取数据包…", 0, 0);
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
    library_ids: Option<HashMap<String, String>>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    let path = path.trim().trim_matches('"').to_string();
    bind_progress(&app, "正在绑定…", 0, 0);
    let bc = state.source(&path)?;
    let choices = classify_trees(&bc);
    {
        let mut guard = state.libraries.write().unwrap();
        let mut libs = guard.clone();
        let ids = library_ids.unwrap_or_default();
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
                    let id = self::library_id(ids.get(&t.id).map(String::as_str), "cbeta", &libs)?;
                    libs.push(Library {
                        id,
                        kind: "cbeta".into(),
                        title: t.title.clone(),
                        source: path.clone(),
                        tree: t.id.clone(),
                        exts: Vec::new(),
                        max_mb: 0,
                        writable: false,
                        remote: String::new(),
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
            let id = self::library_id(ids.get(&t.id).map(String::as_str), &code.to_ascii_lowercase(), &libs)?;
            libs.push(Library {
                id,
                kind: "html".into(),
                title: t.title.clone(),
                source: path.clone(),
                tree: t.id.clone(),
                exts: Vec::new(),
                max_mb: 0,
                writable: false,
                remote: String::new(),
            });
        }
        *guard = libs;
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
/// For remote git libraries, `delete_local` can optionally remove the cloned local directory.
#[tauri::command]
pub async fn unbind_library(id: String, delete_local: Option<bool>, state: State<'_, AppState>) -> Result<StatusDto, String> {
    if state.building.lock().unwrap().contains(&id) {
        return Err("正在建立这个知识库的索引，请稍后再解除绑定".into());
    }
    let removed = state.library(&id).ok();
    let own_lib = removed.as_ref().map(|l| l.kind == "notes").unwrap_or(false);
    let remote_dir = if delete_local.unwrap_or(false) {
        removed
            .as_ref()
            .filter(|l| l.kind == "folder" && !l.remote.is_empty() && !l.source.is_empty())
            .map(|l| PathBuf::from(&l.source))
    } else {
        None
    };
    state.libraries.write().unwrap().retain(|l| l.id != id);
    state.indexes.write().unwrap().remove(&id);
    state.html.write().unwrap().remove(&id);
    state.folders.write().unwrap().remove(&id);
    state.checked_folder_indexes.lock().unwrap().remove(&id);
    state.zims.write().unwrap().remove(&id);
    state.notes.write().unwrap().remove(&id);
    let _ = state.history.remove_lib(&id);
    if own_lib {
        let _ = state.props.remove_lib(&id);
        let _ = state.ai_chat.remove_lib(&id);
        // the notes database goes for good, and with it its deleted snapshots
        let _ = state.snapshots.remove_lib(&id);
    }
    state.titles.write().unwrap().remove(&id);
    state.save_registry()?;
    let dir = state.data_dir.join("repos").join(&id);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    if let Some(local_dir) = remote_dir {
        if local_dir.is_dir() {
            std::fs::remove_dir_all(&local_dir).map_err(|e| format!("已解除绑定，但删除本地数据失败：{}", e))?;
        }
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
    library_id: Option<String>,
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
        let id = self::library_id(library_id.as_deref(), "folder", &libs)?;
        libs.push(Library {
            id,
            kind: "folder".into(),
            title: title.filter(|t| !t.trim().is_empty()).unwrap_or(name),
            source: canon,
            tree: String::new(),
            exts,
            max_mb,
            writable: writable.unwrap_or(false),
            remote: String::new(),
        });
    }
    state.save_registry()?;
    Ok(status_of(&state))
}

#[tauri::command]
pub async fn set_folder_filter(id: String, exts: String, max_mb: u64, state: State<'_, AppState>) -> Result<StatusDto, String> {
    let exts = FolderFilter::parse_exts(&exts);
    if exts.is_empty() {
        return Err("请至少填写一种文件后缀".into());
    }
    state.edit_library_source(&id, |_, l| {
        if l.kind != "folder" {
            return Err("只有文件夹知识库可以修改收录规则".into());
        }
        if l.exts == exts && l.max_mb == max_mb {
            return Ok(false);
        }
        l.exts = exts;
        l.max_mb = max_mb;
        Ok(true)
    })?;
    Ok(status_of(&state))
}

/// Move a binding to the same data at a new location, preserving its library ID.
#[tauri::command]
pub async fn relink_library(id: String, path: String, state: State<'_, AppState>) -> Result<StatusDto, String> {
    let original = state.library(&id)?;
    let path = match original.kind.as_str() {
        "notes" => return Err("内部知识库没有可更改的资料源路径".into()),
        "folder" => folder::canonical(&path)?,
        _ => {
            let path = std::fs::canonicalize(path.trim().trim_matches('"')).map_err(|e| e.to_string())?;
            folder::strip_win_prefix(&path.to_string_lossy())
        }
    };
    if same_path(&path, &original.source) {
        return Ok(status_of(&state));
    }
    let bookcase = match original.kind.as_str() {
        "folder" => {
            if !original.remote.is_empty() {
                git_cmds::validate_relinked_repo(&state, &path, &original.remote)?;
            }
            None
        }
        "zim" => {
            if let Some(error) = zim_probe_error(&path) { return Err(error); }
            None
        }
        "cbeta" | "html" => {
            let bookcase = Arc::new(open_bookcase_at(&path)?);
            let matches = classify_trees(&bookcase).into_iter().any(|tree| {
                tree.id == original.tree && matches!((&*original.kind, tree.group), ("cbeta", TreeGroup::Cbeta) | ("html", TreeGroup::Html(_)))
            });
            if !matches { return Err("新位置的数据包缺少这个知识库正在使用的目录".into()); }
            Some(bookcase)
        }
        _ => return Err("这种知识库没有可更改的资料源路径".into()),
    };
    state.edit_library_source(&id, |libraries, current| {
        if current.source != original.source || current.kind != original.kind || current.tree != original.tree {
            return Err("资料源设置已变化，请重新打开此对话框".into());
        }
        if current.kind == "folder" {
            let existing = libraries.iter().filter(|l| l.kind == "folder" && l.id != id).map(|l| l.source.as_str()).collect::<Vec<_>>();
            if let Some(error) = folder::conflict(&existing, &path) { return Err(error); }
        } else if libraries.iter().any(|l| l.id != id && l.kind == current.kind && l.tree == current.tree && same_path(&l.source, &path)) {
            return Err("新位置的这份资料已经绑定为其他知识库".into());
        }
        current.source = path.clone();
        Ok(true)
    })?;
    if let Some(bookcase) = bookcase {
        state.sources.write().unwrap().insert(path, bookcase);
    }
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
        if !l.remote.is_empty() {
            return Err("远程知识库（Git）目前仅支持只读模式".into());
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
fn add_notes_library(state: &AppState, title: &str, requested_id: Option<&str>) -> Result<Library, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("请填写知识库名称".into());
    }
    let lib = {
        let mut libs = state.libraries.write().unwrap();
        if libs.iter().any(|l| l.title == title) {
            return Err("已经有同名的知识库".into());
        }
        let id = library_id(requested_id, "notes", &libs)?;
        let lib = Library { id, kind: "notes".into(), title, source: String::new(), tree: String::new(), exts: Vec::new(), max_mb: 0, writable: true, remote: String::new() };
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
pub async fn create_notes_library(title: String, library_id: Option<String>, state: State<'_, AppState>) -> Result<StatusDto, String> {
    add_notes_library(&state, &title, library_id.as_deref())?;
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

/// Reorder the registered libraries. Any registered library omitted from `ids`
/// is kept at the end in its existing order.
#[tauri::command]
pub async fn reorder_libraries(ids: Vec<String>, state: State<'_, AppState>) -> Result<StatusDto, String> {
    {
        let mut libs = state.libraries.write().unwrap();
        let mut remaining = std::mem::take(&mut *libs);
        let mut ordered = Vec::with_capacity(remaining.len());
        for id in ids {
            if let Some(pos) = remaining.iter().position(|l| l.id == id) {
                ordered.push(remaining.remove(pos));
            }
        }
        ordered.extend(remaining);
        *libs = ordered;
    }
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
        libs.push(add_notes_library(&state, "我的笔记", None)?);
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
            .or_else(|| libs.iter().find(|l| l.id != smart_library_core::diary::LIBRARY_ID && l.title != smart_library_core::diary::LIBRARY))
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
    let ids = diary::ids_of(date)?;
    let existing = {
        let libs = state.libraries.read().unwrap();
        libs.iter()
            .find(|l| l.kind == "notes" && l.id == diary::LIBRARY_ID)
            .or_else(|| libs.iter().find(|l| l.kind == "notes" && l.title == diary::LIBRARY))
            .cloned()
    };
    let lib = match existing {
        Some(l) => l,
        None => add_notes_library(state, diary::LIBRARY, Some(diary::LIBRARY_ID))?,
    };
    state.titles.write().unwrap().remove(&lib.id);
    let store = state.notes(&lib)?;
    let year_id = if store.get(ids[0])?.is_some() {
        ids[0]
    } else {
        store.create_with_id(ids[0], None, &titles[0], "", "md")?
    };
    let month_id = if store.get(ids[1])?.is_some() {
        ids[1]
    } else {
        store.create_with_id(ids[1], Some(year_id), &titles[1], "", "md")?
    };
    let day_id = if store.get(ids[2])?.is_some() {
        ids[2]
    } else {
        store.create_with_id(ids[2], Some(month_id), &titles[2], "", "md")?
    };
    let dto = NewNoteDto { lib: lib.id, chain: store.chain(day_id)?.iter().map(|i| i.to_string()).collect(), id: day_id.to_string() };
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
    let _ = state.props.remove_nodes(&lib, std::slice::from_ref(&rel));
    let _ = state.props.remove_nodes(&lib, &before);
    state.sidecar_sync(&lib);
    Ok(before.len())
}

#[derive(Serialize)]
pub struct TrashItemDto {
    pub entry: String,
    pub rel: String,
    pub kind: String,
    pub time: i64,
}

/// What is in the trash of a read-write local folder: deleted files and folders
/// (with the place they came from), newest first.
#[tauri::command]
pub async fn trash_list(lib: String, state: State<'_, AppState>) -> Result<Vec<TrashItemDto>, String> {
    let f = rw_folder(&state, &lib)?.ok_or("只有读写模式的外部文件夹有回收站")?;
    Ok(f.trash()?
        .into_iter()
        .map(|t| TrashItemDto { entry: t.entry, rel: t.rel, kind: t.kind, time: t.time })
        .collect())
}

#[derive(Serialize)]
pub struct RestoreDto {
    pub rel: String,
    pub pictures: usize,
}

/// Put a trashed file or folder back where it was; the pictures its notes use
/// come back too. Returns the new path and how many pictures returned.
#[tauri::command]
pub async fn trash_restore(lib: String, entry: String, state: State<'_, AppState>) -> Result<RestoreDto, String> {
    let f = rw_folder(&state, &lib)?.ok_or("只有读写模式的外部文件夹有回收站")?;
    let r = f.restore(&entry)?;
    // what came back must be indexed when the full-text search next runs
    let files = files_under(&f, &r.rel);
    state.mark_dirty(&lib, files.iter().cloned());
    Ok(RestoreDto { rel: r.rel, pictures: r.pictures })
}

/// Clean up what the program leaves behind: in every read-write local folder the trash
/// (`.smart-library/trash`) and the pictures no note refers to; in every internal library the
/// pictures no note refers to (those from the last minute are spared). Returns a sentence about what was removed.
#[derive(Serialize)]
pub struct DynastyDto {
    name: String,
    start: i32,
    end: i32,
    aliases: Vec<String>,
}

/// The table of periods in use (preset and the user's rows), for ordering and the editor.
#[tauri::command]
pub async fn dynasty_table(state: State<'_, AppState>) -> Result<Vec<DynastyDto>, String> {
    let user = std::fs::read_to_string(state.config_dir.join("dynasties.txt")).unwrap_or_default();
    Ok(smart_library_core::dynasty::merged(&user)
        .into_iter()
        .map(|d| DynastyDto { name: d.name, start: d.start, end: d.end, aliases: d.aliases })
        .collect())
}

/// `[the user's rows as text, the preset as text]`.
#[tauri::command]
pub async fn dynasty_text(state: State<'_, AppState>) -> Result<(String, String), String> {
    let user = std::fs::read_to_string(state.config_dir.join("dynasties.txt")).unwrap_or_default();
    Ok((user, smart_library_core::dynasty::preset_text().to_string()))
}

/// Keep the user's rows (an empty text removes them). Indexes built before show the old
/// names until they are built again.
#[tauri::command]
pub async fn set_dynasty_text(text: String, state: State<'_, AppState>) -> Result<usize, String> {
    std::fs::create_dir_all(&state.config_dir).map_err(|e| e.to_string())?;
    let path = state.config_dir.join("dynasties.txt");
    if text.trim().is_empty() {
        let _ = std::fs::remove_file(&path);
    } else {
        std::fs::write(&path, &text).map_err(|e| e.to_string())?;
    }
    smart_library_core::dynasty::set_user(&text);
    Ok(smart_library_core::dynasty::parse(&text).len())
}

/// Traditional -> simplified pairs, as one string `繁简繁简…` (for showing text in simplified characters).
#[tauri::command]
pub async fn t2s_pairs() -> String {
    smart_library_core::analyzer::simplified_pairs().into_iter().flat_map(|(a, b)| [a, b]).collect()
}

/// The text of a file the user chose to import a table from.
#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {}", path, e))?;
    Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
}

fn clean_data(state: &AppState, trash: bool) -> String {
    let libs: Vec<Library> = state.libraries.read().unwrap().clone();
    let (mut count, mut bytes) = (0, 0);
    let mut failed = Vec::new();
    for l in libs {
        let result = if l.kind == "folder" && l.writable {
            if source_is_missing(&l) { Err(format!("找不到资料源：{}", l.source)) }
            else { state.folder(&l).and_then(|f| if trash { f.empty_trash() } else { f.clean_pictures() }) }
        } else if l.kind == "notes" && !trash && state.attachments_dir(&l.id).is_dir() {
            state.sweep_pictures(&l, 60)
        } else {
            continue;
        };
        match result {
            Ok((n, size)) => { count += n; bytes += size; }
            Err(e) => failed.push(format!("{}：{}", l.title, e)),
        }
    }
    let what = if trash { "回收站中的文件" } else { "未引用图片" };
    let mut msg = format!("已清理{} {} 项（{:.1} MB）。", what, count, bytes as f64 / 1_048_576.0);
    if !failed.is_empty() {
        msg.push_str(&format!("\n没有清理成功：{}", failed.join("；")));
    }
    msg
}

#[tauri::command]
pub async fn clean_unused_pictures(state: State<'_, AppState>) -> Result<String, String> {
    Ok(clean_data(&state, false))
}

#[tauri::command]
pub async fn empty_folder_trash(state: State<'_, AppState>) -> Result<String, String> {
    Ok(clean_data(&state, true))
}

/// The file that travels with a read-write folder was changed outside (git pull, a
/// synchronised copy): read it again. True when something was read.
#[tauri::command]
pub async fn sidecar_refresh(state: State<'_, AppState>) -> Result<bool, String> {
    let libs: Vec<Library> = state.libraries.read().unwrap().iter().filter(|l| l.kind == "folder" && l.writable).cloned().collect();
    let mut changed = false;
    for l in libs {
        let now = smart_library_core::sidecar::stamp(Path::new(&l.source));
        let seen = state.sidecar_seen.lock().unwrap().get(&l.id).cloned();
        if now.is_some() && now != seen {
            state.sidecar_load(&l)?;
            state.queue_folder_property_changes(&l, &*state.folder(&l)?)?;
            changed = true;
        }
    }
    Ok(changed)
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
    Ok(id)
}

#[tauri::command]
pub async fn save_note(lib: String, id: String, content: String, expected_stamp: Option<String>, state: State<'_, AppState>) -> Result<Option<String>, String> {
    if let Some(f) = rw_folder(&state, &lib)? {
        let key = format!("{}|{}", lib, id);
        let mut stamps = state.stamps.lock().unwrap();
        let seen = expected_stamp.as_ref().or_else(|| stamps.get(&key));
        // pictures: kept in the note's own folder, a Markdown note links to them (other programs show them too)
        let stamp = f.save_edit(&id, &content, seen.map(String::as_str))?;
        stamps.insert(key, stamp.clone());
        state.mark_dirty(&lib, [id]);
        return Ok(Some(stamp));
    }
    notes_of(&state, &lib)?.update(note_id(&id)?, None, Some(&content))?;
    Ok(None)
}

#[tauri::command]
pub fn folder_file_stamp(lib: String, path: String, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let l = state.library(&lib)?;
    if l.kind != "folder" {
        return Ok(None);
    }
    let f = state.folder(&l)?;
    Ok(f.stamp(&path).ok())
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
    let name = smart_library_core::attach::store(&state.attachments_dir(&l.id), data, &header("mime"))?;
    state.notes(&l)?.register_picture(&name)?;
    Ok(name)
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
pub async fn mindmap_add_clips(lib: String, id: String, clips: Vec<MindmapClipDto>, state: State<'_, AppState>) -> Result<(), String> {
    let store = notes_of(&state, &lib)?;
    let id = note_id(&id)?;
    let note = store.get(id)?.ok_or("笔记不存在")?;
    if note.content_type != smart_library_core::mindmap::TYPE {
        return Err("这篇笔记不是脑图".into());
    }
    let mut data = note.content;
    for clip in clips {
        data = smart_library_core::mindmap::add_clip(&data, &clip.topic, &clip.link, &clip.tag)?;
    }
    store.update(id, None, Some(&data))?;
    Ok(())
}

#[derive(Deserialize)]
pub struct MindmapClipDto {
    topic: String,
    link: String,
    tag: String,
}

/// Add text at the end of a note.
#[tauri::command]
pub async fn note_append(lib: String, id: String, text: String, state: State<'_, AppState>) -> Result<(), String> {
    notes_of(&state, &lib)?.append(note_id(&id)?, &text)?;
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
        state.sidecar_sync(&lib);
        return Ok(1);
    }
    let store = notes_of(&state, &lib)?;
    let nid = note_id(&id)?;
    let note = store.get(nid)?.ok_or("笔记不存在")?;
    // keep the subtree as a `deleted` snapshot before it leaves the database;
    // refusing when that fails is better than losing the notes for good
    let root = store.subtree(nid)?;
    // properties, tags and other names go with the notes (they are removed below)
    let mut meta = Vec::new();
    collect_deleted_meta(&state, &lib, &root, &mut meta)?;
    let tree = notes::DeletedTree {
        format: notes::DELETED_FORMAT.to_string(),
        // where it stood, by titles: restoring rebuilds this path
        path: store.folders_of(nid)?,
        parent: None,
        ancestors: Vec::new(),
        meta,
        root,
    };
    let meta = state.snapshots.create_deleted(&lib, &id, &note.title, &tree.encode()?)?;
    // its pictures keep their files while it waits in the trash
    let _ = store.hold_deleted_pictures(meta.id, &tree);
    let gone: Vec<String> = store.descendants(nid).unwrap_or_default().iter().map(|i| i.to_string()).collect();
    let n = match store.delete(nid) {
        Ok(n) => n,
        Err(e) => {
            // the notes stayed: the snapshot would only be a copy of them
            let _ = store.release_deleted_pictures(meta.id);
            let _ = state.snapshots.delete(meta.id);
            return Err(e);
        }
    };
    // the node tags are in the snapshot too: drop the old records, so a restore
    // under a new id does not leave a second (dangling) tag behind
    for id in &gone {
        let _ = state.marks.set_node_tags(&lib, None, id, "", "", &[]);
    }
    let _ = state.props.remove_nodes(&lib, &gone);
    Ok(n)
}

/// The properties, tags and other names of every note in a deleted subtree.
fn collect_deleted_meta(
    state: &AppState,
    lib: &str,
    node: &notes::DeletedNode,
    out: &mut Vec<notes::DeletedMeta>,
) -> Result<(), String> {
    let path = node.id.to_string();
    let props = state.props.get(lib, None, &path)?;
    let aliases = state.props.aliases(lib, None, &path)?;
    let tags = state.marks.node_tags(lib, None, &path)?;
    if !props.is_empty() || !aliases.is_empty() || !tags.is_empty() {
        out.push(notes::DeletedMeta { id: node.id, props, aliases, tags });
    }
    for c in &node.children {
        collect_deleted_meta(state, lib, c, out)?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct DeletedNoteDto {
    /// the snapshot id
    pub id: i64,
    pub title: String,
    /// how many notes the subtree holds (the root included)
    pub count: usize,
    pub created_at: i64,
    pub size_bytes: usize,
}

#[derive(Serialize)]
pub struct RestoredNoteDto {
    pub lib: String,
    pub id: String,
    /// from the top of the library down to the restored note
    pub chain: Vec<String>,
    pub title: String,
    pub count: usize,
    /// parent titles that had to be made to rebuild the path
    pub created: Vec<String>,
    /// an entry of the first version whose parent was gone: it fell back (not used by new entries)
    pub relocated: bool,
}

/// Deleted notes of an internal library (its trash), newest first.
#[tauri::command]
pub async fn list_deleted_notes(lib: String, state: State<'_, AppState>) -> Result<Vec<DeletedNoteDto>, String> {
    let l = state.library(&lib)?;
    if l.kind != "notes" {
        return Err("只有默认知识库有笔记回收站".into());
    }
    let mut out = Vec::new();
    for m in state.snapshots.list_deleted(&lib)? {
        let count = state
            .snapshots
            .get(m.id)?
            .and_then(|r| notes::DeletedTree::decode(&r.content).ok())
            .map(|t| t.len())
            .unwrap_or(0);
        out.push(DeletedNoteDto { id: m.id, title: m.label, count, created_at: m.created_at, size_bytes: m.size_bytes });
    }
    Ok(out)
}

/// Put a deleted note (and everything below it) back where it was.
#[tauri::command]
pub async fn restore_deleted_note(lib: String, id: i64, state: State<'_, AppState>) -> Result<RestoredNoteDto, String> {
    let l = state.library(&lib)?;
    if l.kind != "notes" {
        return Err("只有默认知识库有笔记回收站".into());
    }
    let rec = state.snapshots.get(id)?.ok_or("回收站里没有这一项")?;
    if rec.kind != "deleted" || rec.lib != lib {
        return Err("这不是这个知识库的已删除笔记".into());
    }
    let tree = notes::DeletedTree::decode(&rec.content)?;
    let store = notes_of(&state, &lib)?;
    // only entries of the first version can fall back; a title path always rebuilds
    let relocated = if tree.path.is_empty() {
        let actual = store.deleted_parent(&tree)?;
        tree.parent.is_some() && actual != tree.parent
    } else {
        false
    };
    let (root, made, created) = store.restore_deleted(&tree)?;
    // properties, tags and other names go back onto the notes (under their new ids)
    if !tree.meta.is_empty() {
        let ids: std::collections::HashMap<i64, i64> = made.iter().copied().collect();
        let lib_title = state.library(&lib)?.title;
        for m in &tree.meta {
            let Some(&nid) = ids.get(&m.id) else { continue };
            let path = nid.to_string();
            if !m.props.is_empty() {
                state.props.set(&lib, None, &path, &m.props)?;
            }
            if !m.aliases.is_empty() {
                state.props.set_aliases(&lib, None, &path, &m.aliases)?;
            }
            if !m.tags.is_empty() {
                let title = store.get(nid)?.map(|n| n.title).unwrap_or_default();
                state.marks.set_node_tags(&lib, None, &path, &title, &lib_title, &m.tags)?;
            }
        }
    }
    let _ = store.release_deleted_pictures(id);
    state.titles.write().unwrap().remove(&lib);
    state.snapshots.delete(id)?;
    let note = store.get(root)?.ok_or("恢复后找不到笔记")?;
    Ok(RestoredNoteDto {
        lib,
        id: root.to_string(),
        chain: store.chain(root)?.iter().map(|i| i.to_string()).collect(),
        title: note.title,
        count: made.len(),
        created,
        relocated,
    })
}

/// Throw away every deleted note of an internal library; returns how many.
#[tauri::command]
pub async fn purge_deleted_notes(lib: String, state: State<'_, AppState>) -> Result<usize, String> {
    let l = state.library(&lib)?;
    if l.kind != "notes" {
        return Err("只有默认知识库有笔记回收站".into());
    }
    let store = state.notes(&l)?;
    for m in state.snapshots.list_deleted(&lib)? {
        let _ = store.release_deleted_pictures(m.id);
    }
    state.snapshots.purge_deleted(&lib)
}

#[derive(Serialize)]
pub struct MoveTargetDto {
    path: String,
    label: String,
}

#[tauri::command]
pub async fn note_move_targets(
    lib: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<MoveTargetDto>, String> {
    let store = notes_of(&state, &lib)?;
    let nid = note_id(&id)?;
    let rows = store.rows()?;
    let current_parent = rows
        .iter()
        .find(|r| r.id == nid)
        .and_then(|r| r.ancestors.last().copied());
    let mut out = Vec::new();
    if current_parent.is_some() {
        out.push(MoveTargetDto {
            path: String::new(),
            label: "（知识库顶层）".to_string(),
        });
    }
    for r in rows {
        if r.id == nid || r.ancestors.contains(&nid) || Some(r.id) == current_parent {
            continue;
        }
        let label = if r.folders.is_empty() {
            r.title
        } else {
            format!("{} / {}", r.folders.join(" / "), r.title)
        };
        out.push(MoveTargetDto {
            path: r.id.to_string(),
            label,
        });
    }
    Ok(out)
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
    /// CBETA: the book's table of contents file inside the data package
    toc: Option<String>,
    /// CBETA/HTML: the navigation file this level of the tree comes from
    nav: Option<String>,
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

fn node_dto(bc: &Bookcase, html: Option<&HtmlTree>, nav_file: Option<&str>, n: &NavNode) -> NodeDto {
    match n {
        NavNode::Folder { title, children } => NodeDto {
            title: title.clone(),
            book_id: None,
            start: None,
            toc: None,
            nav: nav_file.map(str::to_string),
            path: None,
            href: None,
            rel: None,
            lazy: false,
            mind: false,
            board: false,
            children: children.iter().map(|c| node_dto(bc, html, nav_file, c)).collect(),
        },
        NavNode::Leaf { href, text, children, .. } => {
            let children = children.iter().map(|c| node_dto(bc, html, nav_file, c)).collect();
            if let Some(t) = html {
                let p = href.split('#').next().unwrap_or("");
                return NodeDto {
                    title: text.clone(),
                    book_id: None,
                    start: None,
                    toc: None,
                    nav: None,
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
            let toc = entry
                .as_ref()
                .filter(|e| e.start.is_none())
                .and_then(|e| bc.book_source_path(&e.book_id));
            NodeDto {
                title: match &entry {
                    Some(e) if !e.cloned => format!("{} {}", e.book_id, e.title),
                    Some(e) => e.title.clone(),
                    None => text.clone(),
                },
                book_id: entry.as_ref().map(|e| e.book_id.clone()),
                start: entry.as_ref().and_then(|e| e.start.clone()),
                toc,
                nav: None,
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

fn navigation_nodes(state: &AppState, l: &Library) -> Result<Vec<NodeDto>, String> {
    let bc = state.bookcase_of(l)?;
    let html = if l.kind == "html" { Some(state.html_tree(l)?) } else { None };
    let doc = bc.nav(&l.tree)?;
    let nav_file = bc.trees.iter().find(|t| t.id == l.tree).map(|t| t.file.clone());
    Ok(doc.items.iter().map(|n| node_dto(&bc, html.as_deref(), nav_file.as_deref(), n)).collect())
}

fn navigation_ancestors(nodes: &[NodeDto]) -> HashMap<String, Vec<(String, String)>> {
    let mut map = HashMap::new();
    fn walk(nodes: &[NodeDto], prefix: &str, ancestors: &mut Vec<(String, String)>, map: &mut HashMap<String, Vec<(String, String)>>) {
        for (i, n) in nodes.iter().enumerate() {
            let key = format!("{prefix}/{i}");
            if let Some(doc) = n.book_id.as_ref().or(n.path.as_ref()) {
                let keys: &mut Vec<_> = map.entry(doc.clone()).or_default();
                for ancestor in ancestors.iter() { if !keys.contains(ancestor) { keys.push(ancestor.clone()); } }
            }
            let prop_key = if let Some(path) = &n.path { (String::new(), path.clone()) }
                else if let Some(book) = &n.book_id { (book.clone(), String::new()) }
                else { (String::new(), format!("@nav{key}")) };
            ancestors.push(prop_key);
            walk(&n.children, &key, ancestors, map);
            ancestors.pop();
        }
    }
    walk(nodes, "", &mut Vec::new(), &mut map);
    map
}

fn nav_targets(state: &AppState, l: &Library, path: &str) -> Result<HashSet<String>, String> {
    let nodes = navigation_nodes(state, l)?;
    let mut list = nodes.as_slice();
    let mut selected = None;
    for part in path.strip_prefix("@nav/").ok_or("目录节点 ID 无效")?.split('/') {
        let index: usize = part.parse().map_err(|_| "目录节点 ID 无效")?;
        let node = list.get(index).ok_or("目录节点不存在")?;
        selected = Some(node);
        list = &node.children;
    }
    fn gather(node: &NodeDto, kind: &str, out: &mut HashSet<String>) {
        if let Some(value) = if kind == "cbeta" { &node.book_id } else { &node.path } { out.insert(value.clone()); }
        for child in &node.children { gather(child, kind, out); }
    }
    let mut result = HashSet::new();
    gather(selected.ok_or("目录节点不存在")?, &l.kind, &mut result);
    Ok(result)
}

fn nav_document_targets(state: &AppState, l: &Library, document: &str) -> Result<HashSet<String>, String> {
    let nodes = navigation_nodes(state, l)?;
    fn walk(nodes: &[NodeDto], kind: &str, document: &str, inside: bool, out: &mut HashSet<String>) {
        for node in nodes {
            let id = if kind == "cbeta" { node.book_id.as_deref() } else { node.path.as_deref() };
            let included = inside || id == Some(document);
            if included { if let Some(id) = id { out.insert(id.to_string()); } }
            walk(&node.children, kind, document, included, out);
        }
    }
    let mut result = HashSet::new();
    walk(&nodes, &l.kind, document, false, &mut result);
    result.insert(document.to_string());
    Ok(result)
}

fn note_nodes(n: &NotesStore, parent: Option<i64>) -> Result<Vec<NodeDto>, String> {
    Ok(n.children(parent)?
        .into_iter()
        .map(|r| NodeDto {
            title: r.title,
            book_id: None,
            start: None,
            toc: None,
            nav: None,
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
    Ok(folder_entry_nodes(f.children(rel)?))
}

fn folder_entry_nodes(entries: Vec<folder::FolderEntry>) -> Vec<NodeDto> {
    entries
        .into_iter()
        .map(|e| {
            // in read-write mode a map or board shows with its own icon
            let kind = if !e.is_dir { folder::EditKind::of(&e.name) } else { None };
            NodeDto {
                title: if e.is_dir { e.name.clone() } else { folder::title_of(&e.name).to_string() },
                book_id: None,
                start: None,
                toc: None,
                nav: None,
                path: if e.is_dir { None } else { Some(e.rel.clone()) },
                href: None,
                rel: Some(e.rel),
                lazy: e.is_dir,
                mind: kind == Some(folder::EditKind::MindMap),
                board: kind == Some(folder::EditKind::Whiteboard),
                children: Vec::new(),
            }
        })
        .collect()
}

#[derive(Serialize)]
pub struct FolderDirectoryDto {
    nodes: Vec<NodeDto>,
    excluded: Vec<folder::FolderIndexIssue>,
}

#[tauri::command]
pub async fn folder_directory(lib: String, rel: String, state: State<'_, AppState>) -> Result<FolderDirectoryDto, String> {
    let l = state.library(&lib)?;
    if l.kind != "folder" {
        return Err("只有文件夹知识库提供磁盘目录信息".into());
    }
    let (entries, excluded) = state.folder(&l)?.children_with_exclusions(&rel)?;
    Ok(FolderDirectoryDto { nodes: folder_entry_nodes(entries), excluded })
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
    if l.kind == "zim" { return Ok(Vec::new()); }
    if l.kind == "notes" {
        let n = state.notes(&l)?;
        return note_nodes(&n, None);
    }
    if l.kind == "folder" {
        let f = state.folder(&l)?;
        return folder_nodes(&f, "");
    }
    let bc = state.bookcase_of(&l)?;
    let nav_file = bc.trees.iter().find(|t| t.id == l.tree).map(|t| t.file.clone());
    if l.kind == "html" {
        let t = state.html_tree(&l)?;
        Ok(t.nav.items.iter().map(|n| node_dto(&bc, Some(&t), nav_file.as_deref(), n)).collect())
    } else {
        let doc = bc.nav(&l.tree)?;
        Ok(doc.items.iter().map(|n| node_dto(&bc, None, nav_file.as_deref(), n)).collect())
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
    /// raw source (for editing, or read-only mindmap/whiteboard rendering)
    raw: Option<String>,
    /// whether this document can be edited in place
    writable: bool,
    /// folder libraries: file modification stamp (`<mtime_nanos>:<size>`) to detect external edits
    stamp: Option<String>,
    /// extensible per-document view metadata from reading history (e.g. mindmap direction)
    view_extra: Option<serde_json::Value>,
    /// `mindmap` / `whiteboard`, else empty
    content_type: String,
    attribution: Option<String>,
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
            (occ.iter().map(|o| (o.from, o.to)).collect(), smart_library_core::search::match_region_count(&occ), first)
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
    let view_extra = |p: &str| state.history.get_extra(&l.id, p).ok().flatten();

    if l.kind == "zim" {
        let path = path.ok_or("没有指定文章")?;
        let z = state.zim(&l)?;
        let mut z = z.lock().unwrap();
        let i = z.entry_index(&path).ok_or("ZIM 文章不存在")?;
        let title = z.entries[i].title.clone();
        let plain = z.article(i, None)?;
        let (ranges, hits, first_hit_lb) = hits_of(&plain.search_view(), query.as_deref(), phrase);
        let a = z.article(i, Some(&RenderOptions { highlights: ranges.clone(), asset_prefix }))?;
        return Ok(ReaderDto { lib: l.id.clone(), kind: l.kind.clone(), book_id: None, title, author: String::new(), path: path.clone(), juan: 0, label: String::new(), volumes: vec![],
            chapters: a.headings.iter().map(|h| ChapterDto { title: h.text.clone(), path: path.clone(), lb: Some(h.lb.clone()), depth: h.level.saturating_sub(1) as usize }).collect(),
            lines: line_view(&a.text, &ranges), html: a.html.unwrap_or_default(), hits, first_hit_lb, raw: None, writable: false, stamp: None, view_extra: view_extra(&path), content_type: String::new(), attribution: a.attribution });
    }

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
                writable: true,
                stamp: None,
                view_extra: view_extra(&path),
                content_type: if note.content_type == smart_library_core::mindmap::TYPE { "mindmap" } else { "whiteboard" }.into(),
                attribution: None,
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
            writable: true,
            stamp: None,
            view_extra: view_extra(&path),
            content_type: String::new(),
            attribution: None,
        });
    }
    if l.kind == "folder" {
        let f = state.folder(&l)?;
        let path = path.ok_or("没有指定文件")?;
        let name = path.rsplit('/').next().unwrap_or(&path);
        let file_kind = folder::EditKind::of(name);
        let edit_kind = if f.writable { file_kind } else { None };
        // an editable file: its text goes to the editor (`raw`), the stamp is kept to notice outside changes
        let (raw, stamp) = match edit_kind {
            Some(_) => {
                let (text, stamp) = f.read_edit(&path)?;
                state.stamps.lock().unwrap().insert(format!("{}|{}", l.id, path), stamp.clone());
                let raw = if edit_kind == Some(folder::EditKind::Markdown) { folder::to_editor(&text) } else { text };
                (Some(raw), Some(stamp))
            }
            None => (None, f.stamp(&path).ok()),
        };
        if matches!(file_kind, Some(folder::EditKind::MindMap) | Some(folder::EditKind::Whiteboard)) {
            let raw = match raw {
                Some(t) => Some(t),
                None => Some(f.read_raw(&path)?),
            };
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
                writable: f.writable,
                stamp,
                view_extra: view_extra(&path),
                content_type: file_kind.unwrap().content_type().into(),
                attribution: None,
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
            writable: edit_kind.is_some(),
            stamp,
            view_extra: view_extra(&path),
            content_type: String::new(),
            attribution: None,
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
            writable: false,
            stamp: None,
            view_extra: view_extra(&path),
            content_type: String::new(),
            attribution: None,
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
        writable: false,
        stamp: None,
        view_extra: view_extra(&vol.path),
        content_type: String::new(),
        attribution: None,
    })
}

#[derive(Serialize)]
pub struct RawDto {
    /// file, `<package>!/<path>`, or `library / note` for an internal note
    file: String,
    /// xml | html | markdown | json | text
    lang: String,
    text: String,
}

fn raw_lang(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "xml" => "xml",
        "html" | "htm" | "xhtml" => "html",
        "md" | "markdown" => "markdown",
        "json" => "json",
        _ => "text",
    }
}

/// The original text of a document, for the raw data window. Read-only: the
/// file as it is, without the reader's rendering.
#[tauri::command]
pub async fn raw_source(
    lib: String,
    path: Option<String>,
    book_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<RawDto, String> {
    let l = state.library(&lib)?;
    match l.kind.as_str() {
        "notes" => {
            let id: i64 = path.ok_or("没有指定笔记")?.parse().map_err(|_| "笔记编号不对")?;
            let note = state.notes(&l)?.get(id)?.ok_or("笔记不存在")?;
            let lang = match note.content_type.as_str() {
                "mindmap" | "whiteboard" => "json",
                _ => "markdown",
            };
            Ok(RawDto { file: format!("{} / {}", l.title, note.title), lang: lang.to_string(), text: note.content })
        }
        "folder" => {
            let path = path.ok_or("没有指定文件")?;
            let p = state.folder(&l)?.abs(&path)?;
            let name = path.rsplit('/').next().unwrap_or(&path);
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
            if smart_library_core::doctext::is_document(&ext) || !folder::is_text_file(name) {
                return Err(format!("{}：这是二进制文档，没有可显示的原始文本", name));
            }
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
            Ok(RawDto {
                file: p.display().to_string(),
                lang: raw_lang(name).to_string(),
                text: smart_library_core::text::decode_auto(&bytes),
            })
        }
        "cbeta" | "html" => {
            let bc = state.bookcase_of(&l)?;
            let path = match path {
                Some(p) => p,
                None if l.kind == "cbeta" => {
                    let book = book_id.ok_or("没有指定文档")?;
                    bc.first_existing_volume(&book).ok_or_else(|| format!("{} 没有可读的卷", book))?
                }
                None => return Err("没有指定文档".into()),
            };
            let bytes = bc.source.read(&path).ok_or_else(|| format!("{}：数据包里没有这个文件", path))?;
            Ok(RawDto {
                file: format!("{}!/{}", l.source, path),
                lang: raw_lang(&path).to_string(),
                text: smart_library_core::text::decode_auto(&bytes),
            })
        }
        "zim" => {
            let path = path.ok_or("没有指定条目")?;
            let z = state.zim(&l)?;
            let mut z = z.lock().unwrap();
            let index = z.entry_index(&path).ok_or_else(|| format!("没有这个条目：{}", path))?;
            let bytes = z.read(index)?;
            Ok(RawDto {
                file: format!("{}!/{}", l.source, path),
                lang: "html".to_string(),
                text: smart_library_core::text::decode_auto(&bytes),
            })
        }
        kind => Err(format!("这种知识库没有原始文本：{}", kind)),
    }
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

#[derive(Serialize)]
pub struct ZimLinkDto {
    lib: Option<String>,
    path: Option<String>,
    lb: Option<String>,
    external_url: Option<String>,
}

#[tauri::command]
pub async fn resolve_zim_link(lib: String, path: String, href: String, state: State<'_, AppState>) -> Result<ZimLinkDto, String> {
    let l = state.library(&lib)?;
    if l.kind != "zim" { return Err("不是 ZIM 知识库".into()); }
    let (target_lib, source_path, target_href) = if let Some(url) = smart_library_core::zim::external_url(&href) {
        if let Some((wiki_id, _)) = smart_library_core::zim::wiki_url(&href) {
            let libraries = state.libraries.read().unwrap();
            let candidate = libraries.iter().find(|x| x.kind == "zim" && x.id == wiki_id)
                .or_else(|| libraries.iter().find(|x| x.kind == "zim" && x.tree == wiki_id)).cloned();
            drop(libraries);
            if let Some(target) = candidate {
                if let Ok(z) = state.zim(&target) {
                    let mut z = z.lock().unwrap();
                    let found = z.resolve_wiki_url(&href);
                    if let Ok((target_path, fragment)) = found {
                        let lb = if let Some(f) = &fragment {
                            z.entry_index(&target_path).and_then(|i| z.article(i, None).ok()).and_then(|a| a.anchors.get(f).cloned())
                        } else { None };
                        if fragment.is_none() || lb.is_some() {
                            return Ok(ZimLinkDto { lib: Some(target.id), path: Some(target_path), lb, external_url: None });
                        }
                    }
                }
            }
        }
        return Ok(ZimLinkDto { lib: None, path: None, lb: None, external_url: Some(url.to_string()) });
    } else { (l, path, href) };
    let z = state.zim(&target_lib)?;
    let mut z = z.lock().unwrap();
    let (path, fragment) = z.resolve_link(&source_path, &target_href)?;
    let lb = if let Some(fragment) = fragment {
        let i = z.entry_index(&path).ok_or("ZIM 文章不存在")?;
        let article = z.article(i, None)?;
        Some(article.anchors.get(&fragment).cloned().ok_or_else(|| format!("文章中没有找到章节：{fragment}"))?)
    } else { None };
    Ok(ZimLinkDto { lib: Some(target_lib.id), path: Some(path), lb, external_url: None })
}

fn open_external_url_sync(url: &str) -> Result<(), String> {
    let url = smart_library_core::zim::external_url(url).ok_or("不支持的外部链接")?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let operation: Vec<u16> = std::ffi::OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = std::ffi::OsStr::new(url.as_str()).encode_wide().chain(Some(0)).collect();
        let result = unsafe { ShellExecuteW(std::ptr::null_mut(), operation.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
        if result as isize <= 32 { return Err("无法打开系统浏览器".into()); }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url.as_str())
            .spawn()
            .map_err(|_| "无法打开系统浏览器".to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url.as_str())
            .spawn()
            .map_err(|_| "无法打开系统浏览器".to_string())?;
        Ok(())
    }
    #[cfg(not(any(windows, unix)))]
    { let _ = url; Err("当前系统暂不支持打开外部链接".into()) }
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    open_external_url_sync(&url)
}

#[tauri::command]
pub async fn open_in_file_manager(lib: String, rel: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    let l = state.library(&lib)?;
    if l.kind != "folder" {
        return Err("只有本地文件夹类型的知识库可以打开本地位置".into());
    }
    let f = state.folder(&l)?;
    let target = match rel.filter(|r| !r.is_empty()) {
        Some(r) => f.abs(&r)?,
        None => f.root.clone(),
    };
    if !target.exists() {
        return Err(format!("路径不存在：{}", target.display()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if target.is_file() {
            let escaped = target.display().to_string().replace('"', "");
            std::process::Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", escaped))
                .spawn()
                .map_err(|e| format!("无法打开资源管理器：{}", e))?;
        } else {
            std::process::Command::new("explorer")
                .arg(&target)
                .spawn()
                .map_err(|e| format!("无法打开资源管理器：{}", e))?;
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if target.is_file() {
            cmd.arg("-R");
        }
        cmd.arg(&target)
            .spawn()
            .map_err(|e| format!("无法打开 Finder：{}", e))?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = if target.is_file() { target.parent().unwrap_or(&target) } else { target.as_path() };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("无法打开文件管理器：{}", e))?;
        Ok(())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = target;
        Err("当前系统暂不支持打开本地位置".into())
    }
}

fn external_context_menu_script(ai_on: bool, token: &str) -> String {
    let ai_flag = if ai_on { "true" } else { "false" };
    format!(
        r#"(function() {{
  if (window.__slExtMenuInstalled) return;
  window.__slExtMenuInstalled = true;
  const AI_ON = {ai_flag};
  const TOKEN = "{token}";
  const clip = (s, n = 18) => s.length > n ? s.slice(0, n) + "…" : s;
  const pageVol = () => ({{ title: document.title || location.href, url: location.href }});
  const sendAction = (payload) => {{
    try {{
      location.href = "http://smartlib-action.localhost/?token=" + encodeURIComponent(TOKEN) + "&data=" + encodeURIComponent(JSON.stringify(payload));
    }} catch (_) {{}}
  }};
  const copyStr = async (s) => {{
    try {{
      await navigator.clipboard.writeText(s);
    }} catch (_) {{
      try {{ document.execCommand("copy"); }} catch (_) {{}}
    }}
  }};

  let host = null;
  let shadow = null;
  let box = null;

  function ensureHost() {{
    if (host && document.documentElement.contains(host)) return;
    host = document.createElement("div");
    host.id = "__sl_ext_ctx_host";
    host.style.cssText = "position:fixed;inset:0;z-index:2147483647;pointer-events:none;";
    shadow = host.attachShadow({{ mode: "closed" }});
    const style = document.createElement("style");
    style.textContent = `
      .menu {{
        position: fixed;
        min-width: 210px;
        max-width: 320px;
        padding: 5px;
        border-radius: 8px;
        font: 13px/1.4 "Microsoft YaHei", "PingFang SC", system-ui, sans-serif;
        box-shadow: 0 8px 28px rgba(0,0,0,0.22);
        pointer-events: auto;
        user-select: none;
        background: #ffffff;
        color: #1f2328;
        border: 1px solid rgba(0,0,0,0.12);
      }}
      @media (prefers-color-scheme: dark) {{
        .menu {{
          background: #25262b;
          color: #e4e6eb;
          border-color: rgba(255,255,255,0.14);
        }}
      }}
      .item {{
        display: block;
        width: 100%;
        text-align: left;
        border: 0;
        background: transparent;
        color: inherit;
        font: inherit;
        padding: 6px 10px;
        border-radius: 5px;
        cursor: pointer;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
      }}
      .item:hover:not(:disabled) {{
        background: rgba(47, 117, 195, 0.14);
        color: #2f75c3;
      }}
      @media (prefers-color-scheme: dark) {{
        .item:hover:not(:disabled) {{
          background: rgba(96, 165, 250, 0.18);
          color: #60a5fa;
        }}
      }}
      .item:disabled {{
        opacity: 0.45;
        cursor: default;
      }}
      .sep {{
        height: 1px;
        margin: 4px 6px;
        background: rgba(128,128,128,0.22);
      }}
    `;
    box = document.createElement("div");
    box.className = "menu";
    box.style.display = "none";
    shadow.appendChild(style);
    shadow.appendChild(box);
    (document.documentElement || document.body).appendChild(host);
  }}

  function closeMenu() {{
    if (box) box.style.display = "none";
    if (host) host.style.pointerEvents = "none";
  }}

  window.addEventListener("mousedown", (e) => {{
    if (e.target !== host) closeMenu();
  }}, true);
  window.addEventListener("keydown", (e) => {{
    if (e.key === "Escape") closeMenu();
  }}, true);
  window.addEventListener("scroll", closeMenu, true);
  window.addEventListener("blur", closeMenu);

  window.addEventListener("contextmenu", (e) => {{
    e.preventDefault();
    e.stopPropagation();
    ensureHost();
    const text = (window.getSelection()?.toString() || "").trim();
    const items = [];
    if (text) {{
      const q = clip(text);
      items.push({{ label: "复制文字", action: () => copyStr(text) }});
      items.push("sep");
      items.push({{ label: "全文检索：" + q, action: () => sendAction({{ type: "search", text }}) }});
      items.push({{ label: "全文检索（精确检索）：" + q, action: () => sendAction({{ type: "phrase", text }}) }});
      items.push({{ label: "快捷检索（经名、作译者等）：" + q, action: () => sendAction({{ type: "quick", text }}) }});
      items.push({{ label: "页内查找：" + q, action: () => window.find && window.find(text, false, false, true) }});
      items.push("sep");
      items.push({{ label: "加入笔记…", action: () => sendAction({{ type: "note", text, vol: pageVol() }}) }});
      const d = new Date();
      const pad = (n) => String(n).padStart(2, "0");
      const today = `${{d.getFullYear()}}-${{pad(d.getMonth() + 1)}}-${{pad(d.getDate())}}`;
      items.push({{ label: `追加到今日记：${{today}}`, action: () => sendAction({{ type: "diary", text, vol: pageVol() }}) }});
      items.push({{ label: "追加到最近的笔记", action: () => sendAction({{ type: "note-last", text, vol: pageVol() }}) }});
      if (AI_ON) {{
        items.push("sep");
        items.push({{ label: "发给 AI 助手", action: () => sendAction({{ type: "ai-external", text, vol: pageVol(), autoSend: false }}) }});
        items.push({{ label: "AI 解释选中文字", action: () => sendAction({{ type: "ai-external", text, vol: pageVol(), prompt: "请用通俗准确的现代汉语解释以上引文，并说明其核心背景与要点。", autoSend: true }}) }});
      }}
    }} else {{
      items.push({{ label: "后退", action: () => history.back() }});
      items.push({{ label: "前进", action: () => history.forward() }});
      items.push({{ label: "刷新", action: () => location.reload() }});
      items.push("sep");
      items.push({{ label: "复制页面链接", action: () => copyStr(location.href) }});
      items.push({{ label: "在系统浏览器中打开", action: () => sendAction({{ type: "open-system-browser", url: location.href }}) }});
    }}

    box.innerHTML = "";
    for (const it of items) {{
      if (it === "sep") {{
        const s = document.createElement("div");
        s.className = "sep";
        box.appendChild(s);
      }} else {{
        const b = document.createElement("button");
        b.type = "button";
        b.className = "item";
        b.textContent = it.label;
        b.onclick = (ev) => {{
          ev.stopPropagation();
          closeMenu();
          it.action();
        }};
        box.appendChild(b);
      }}
    }}
    host.style.pointerEvents = "auto";
    box.style.display = "block";
    box.style.left = "0px";
    box.style.top = "0px";
    const r = box.getBoundingClientRect();
    const x = Math.max(6, Math.min(e.clientX, window.innerWidth - r.width - 8));
    const y = Math.max(6, Math.min(e.clientY, window.innerHeight - r.height - 8));
    box.style.left = x + "px";
    box.style.top = y + "px";
  }}, true);
}})();"#
    )
}

static EXT_WIN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_external_token(seq: u64) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h1 = RandomState::new().build_hasher();
    h1.write_u64(seq);
    h1.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(h1.finish() ^ (std::process::id() as u64));
    format!("{:016x}{:016x}", h1.finish(), h2.finish())
}

fn parse_external_action_url(nav_url: &tauri::Url, expected_token: &str) -> Option<serde_json::Value> {
    if nav_url.scheme() != "http" || nav_url.host_str() != Some("smartlib-action.localhost") {
        return None;
    }
    let mut token_ok = false;
    let mut raw_data = None;
    for (k, v) in nav_url.query_pairs() {
        if k == "token" && v == expected_token && !expected_token.is_empty() {
            token_ok = true;
        } else if k == "data" {
            raw_data = Some(v.into_owned());
        }
    }
    if !token_ok {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&raw_data?).ok()
}

fn create_external_window(app: &AppHandle, parsed: tauri::Url, ai_on: bool) -> Result<(), String> {
    let seq = EXT_WIN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let label = format!("ext-{seq}");
    let token = next_external_token(seq);
    let expected_token = token.clone();
    let title = parsed.host_str().unwrap_or("外部链接").to_string();
    let app_for_nav = app.clone();
    let app_for_new = app.clone();
    tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(parsed))
        .title(&title)
        .inner_size(1100.0, 780.0)
        .min_inner_size(640.0, 480.0)
        .initialization_script(external_context_menu_script(ai_on, &token))
        .on_navigation(move |nav_url| {
            if nav_url.scheme() == "http" && nav_url.host_str() == Some("smartlib-action.localhost") {
                if let Some(payload) = parse_external_action_url(nav_url, &expected_token) {
                    if payload.get("type").and_then(|v| v.as_str()) == Some("open-system-browser") {
                        if let Some(u) = payload.get("url").and_then(|v| v.as_str()) {
                            let _ = open_external_url_sync(u);
                        }
                    } else {
                        let _ = app_for_nav.emit_to("main", "popout-action", payload);
                    }
                }
                return false;
            }
            true
        })
        .on_new_window(move |new_url, _features| {
            if smart_library_core::zim::external_url(new_url.as_str()).is_some() {
                let app_handle = app_for_new.clone();
                std::thread::spawn(move || {
                    let _ = create_external_window(&app_handle, new_url, ai_on);
                });
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn open_external_window(url: String, ai_on: Option<bool>, app: AppHandle) -> Result<(), String> {
    let valid = smart_library_core::zim::external_url(&url).ok_or("不支持的外部链接")?;
    let parsed = tauri::Url::parse(valid.as_str()).map_err(|e| e.to_string())?;
    create_external_window(&app, parsed, ai_on.unwrap_or(false))
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
    if l.kind == "zim" {
        let path = path.ok_or("没有指定文章")?;
        let z = state.zim(&l)?;
        let z = z.lock().unwrap();
        let i = z.entry_index(&path).ok_or("ZIM 文章不存在")?;
        return Ok(NodeIdDto { tab: format!("{}|{}", l.id, path), lib: l.id.clone(), book_id: None, path, title: z.entries[i].title.clone() });
    }
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
    library_title: String,
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
    /// Hit counts of indexed libraries, plus the selected library if its index is unavailable.
    libraries: Vec<LibCountDto>,
    unavailable_libraries: Vec<String>,
    /// The library whose results follow, or None for a merged page.
    lib: Option<String>,
    index_unavailable: bool,
    kind: String,
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
    merge: Option<bool>,
    app: AppHandle,
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

    let libs = state.libraries.read().unwrap().clone();
    // notes edited since the last search join the index now
    if state.flush_dirty(&libs) {
        let _ = app.emit("note-index-synced", ());
    }
    let mut indexed = Vec::new();
    let mut unavailable_libraries = Vec::new();
    for l in &libs {
        if let Some(idx) = state.index(&l.id) {
            indexed.push((l, idx));
        } else {
            unavailable_libraries.push(l.title.clone());
        }
    }
    if indexed.is_empty() {
        return Err("还没有可搜索的知识库：请先为知识库建立索引".into());
    }
    // Each indexed library has its own range. Libraries without a range are searched in full.
    let mut withins = withins.unwrap_or_default();
    withins.retain(|w| indexed.iter().any(|(lib, _)| lib.id == w.lib));
    let mut ranges = ranges_of(&state, withins)?;
    // Counts per library ignore attribute filters (those belong to the displayed library).
    let mut counts = Vec::new();
    for (l, idx) in &indexed {
        let mut r = SearchRequest::new(&query);
        r.mode = req.mode;
        r.within = ranges.get(&l.id).cloned().flatten();
        counts.push(LibCountDto {
            id: l.id.clone(),
            title: l.title.clone(),
            count: idx.count(&r)?,
        });
    }
    let page = page.unwrap_or(1).max(1);
    if let Some(requested) = lib.as_ref().filter(|id| merge == Some(false) && !counts.iter().any(|count| &count.id == *id)) {
        if let Some(library) = libs.iter().find(|item| &item.id == requested) {
            counts.push(LibCountDto { id: library.id.clone(), title: library.title.clone(), count: 0 });
            return Ok(SearchDto {
                libraries: counts,
                unavailable_libraries,
                lib: Some(library.id.clone()),
                index_unavailable: true,
                kind: library.kind.clone(),
                total_volumes: 0,
                total_books: 0,
                page: 1,
                pages: 0,
                millis: start.elapsed().as_secs_f64() * 1000.0,
                facets: Vec::new(),
                hits: Vec::new(),
            });
        }
    }
    let merge_results = match (merge, lib.as_ref()) {
        (Some(true), None) => req.filters.is_empty(),
        (Some(false), Some(id)) => !libs.iter().any(|library| &library.id == id),
        _ => false,
    };
    if merge_results && counts.len() > 1 {
        let sizes: Vec<_> = counts.iter().map(|library| library.count).collect();
        let total_volumes = sizes.iter().sum::<usize>();
        let pages = total_volumes.div_ceil(req.limit);
        let page = page.min(pages.max(1));
        let slots = merged_search_slots(&sizes, (page - 1) * req.limit, req.limit);
        let mut slices = vec![None; indexed.len()];
        for &(source, rank) in &slots {
            slices[source].get_or_insert(rank..rank).end = rank + 1;
        }
        let mut by_library = Vec::with_capacity(indexed.len());
        for ((library, index), slice) in indexed.iter().zip(slices) {
            let mut hits = Vec::new();
            if let Some(slice) = slice {
                let mut request = SearchRequest::new(&query);
                request.mode = req.mode;
                request.within = ranges.remove(&library.id).flatten();
                request.offset = slice.start;
                request.limit = slice.len();
                hits = search_hit_dtos(&state, library, &request, &index.top_hits(&request)?)?;
            }
            by_library.push(hits.into_iter());
        }
        let hits = slots.into_iter().filter_map(|(source, _)| by_library[source].next()).collect();
        return Ok(SearchDto {
            libraries: counts,
            unavailable_libraries,
            lib: None,
            index_unavailable: false,
            kind: String::new(),
            total_volumes,
            total_books: 0,
            page,
            pages,
            millis: start.elapsed().as_secs_f64() * 1000.0,
            facets: Vec::new(),
            hits,
        });
    }
    // the asked library, else the first one with hits
    let selected = lib.as_ref()
        .filter(|id| counts.iter().any(|c| &c.id == *id))
        .cloned()
        .or_else(|| counts.iter().find(|c| c.count > 0).map(|c| c.id.clone()))
        .unwrap_or_else(|| counts[0].id.clone());
    if lib.is_some_and(|id| id != selected) {
        req.filters.clear();
    }
    req.within = ranges.remove(&selected).flatten();
    let (l, idx) = indexed.iter().find(|(l, _)| l.id == selected).ok_or("索引不可用")?;
    req.offset = (page - 1) * req.limit;
    let r = idx.search(&req)?;
    let hits = search_hit_dtos(&state, l, &req, &r.hits)?;
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
        unavailable_libraries,
        lib: Some(selected),
        index_unavailable: false,
        kind: l.kind.clone(),
        total_volumes: r.total_volumes,
        total_books: r.total_books,
        page,
        pages: r.total_volumes.div_ceil(req.limit),
        millis: start.elapsed().as_secs_f64() * 1000.0,
        facets,
        hits,
    })
}

/// Interleave equal ranks in library order. Skip complete rounds before the page
/// using hit counts, so no earlier pages have to be fetched from any index.
fn merged_search_slots(sizes: &[usize], offset: usize, limit: usize) -> Vec<(usize, usize)> {
    let before = |rank| sizes.iter().map(|&size| size.min(rank)).sum::<usize>();
    let max_rank = sizes.iter().copied().max().unwrap_or(0);
    let (mut lo, mut hi) = (0, max_rank);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if before(mid) <= offset {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut skip = offset - before(lo);
    let mut slots = Vec::new();
    for rank in lo..max_rank {
        for (source, _) in sizes.iter().enumerate().filter(|(_, size)| **size > rank) {
            if skip > 0 {
                skip -= 1;
            } else {
                slots.push((source, rank));
                if slots.len() == limit {
                    return slots;
                }
            }
        }
    }
    slots
}

#[cfg(test)]
mod merged_search_tests {
    use super::merged_search_slots;

    #[test]
    fn pages_interleave_ranked_hits_across_uneven_libraries() {
        let sizes = [4, 0, 2, 1];
        assert_eq!(merged_search_slots(&sizes, 0, 2), [(0, 0), (2, 0)]);
        assert_eq!(merged_search_slots(&sizes, 2, 2), [(3, 0), (0, 1)]);
        assert_eq!(merged_search_slots(&sizes, 4, 2), [(2, 1), (0, 2)]);
        assert_eq!(merged_search_slots(&sizes, 6, 2), [(0, 3)]);
        assert!(merged_search_slots(&sizes, 7, 2).is_empty());
    }
}

fn search_hit_dtos(state: &AppState, l: &Library, req: &SearchRequest, ranked: &[Hit]) -> Result<Vec<HitDto>, String> {
    let html = if l.kind == "html" && !ranked.is_empty() { Some(state.html_tree(l)?) } else { None };
    let browsing = req.query.trim().is_empty();
    let mut hits = Vec::with_capacity(ranked.len());
    for h in ranked {
        let text = if browsing {
            Err(String::new())
        } else if is_article(l) {
            article_of(state, l, &h.path, None).map(|a| a.search_view())
        } else {
            state.bookcase_of(l)?.volume_text(&h.path)
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
                (smart_library_core::search::match_region_count(&occ), sn)
            }
            Err(_) => (0, Vec::new()), // listed but unreadable: shown without snippets
        };
        let subtitle = match &html {
            Some(t) => t.leaf(&h.path).map(|x| x.folders.join(" / ")).unwrap_or_default(),
            None if l.kind == "notes" => h
                .path
                .parse::<i64>()
                .ok()
                .and_then(|id| state.notes(l).ok()?.folders_of(id).ok())
                .map(|f| f.join(" / "))
                .unwrap_or_default(),
            None if l.kind == "zim" => String::new(),
            None if l.kind == "folder" => h.path.rsplit_once('/').map(|(d, _)| d.replace('/', " / ")).unwrap_or_default(),
            None => format!("卷{}", smart_library_core::bookcase::chinese_number(h.juan as u32)),
        };
        hits.push(HitDto {
            lib: l.id.clone(),
            library_title: l.title.clone(),
            book_id: if is_article(l) { None } else { Some(h.book.clone()) },
            title: h.title.clone(),
            subtitle,
            path: h.path.clone(),
            score: h.score,
            occurrences,
            snippets: sn,
        });
    }
    Ok(hits)
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
    pinyin: Option<bool>,
    state: State<'_, AppState>,
) -> Result<QuickDto, String> {
    let start = std::time::Instant::now();
    let limit = limit.unwrap_or(200).clamp(1, 5000);
    let use_pinyin = pinyin.unwrap_or(false);
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
        let mut m = if command { Vec::new() } else { t.search_with_pinyin(&words, use_pinyin) };
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

#[tauri::command]
pub fn to_pinyin(text: String) -> String {
    smart_library_core::pinyin::to_pinyin(&text)
}

#[tauri::command]
pub fn pinyin_convert(text: String) -> smart_library_core::pinyin::PinyinResult {
    smart_library_core::pinyin::convert(&text)
}

#[tauri::command]
pub async fn pinyin_text(state: State<'_, AppState>) -> Result<String, String> {
    Ok(std::fs::read_to_string(state.config_dir.join("pinyin.txt")).unwrap_or_default())
}

#[tauri::command]
pub async fn set_pinyin_text(text: String, state: State<'_, AppState>) -> Result<usize, String> {
    std::fs::create_dir_all(&state.config_dir).map_err(|e| e.to_string())?;
    let path = state.config_dir.join("pinyin.txt");
    if text.trim().is_empty() {
        let _ = std::fs::remove_file(&path);
    } else {
        std::fs::write(&path, &text).map_err(|e| e.to_string())?;
    }
    let n = smart_library_core::pinyin::set_user(&text);
    for t in state.titles.read().unwrap().values() {
        t.invalidate_pinyin();
    }
    Ok(n)
}

#[derive(Serialize, Clone)]
struct TitlesProgressDto {
    lib: String,
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
struct TitlesDoneDto {
    lib: String,
}

/// Build the title lists in the background, so that the first quick search
/// does not wait. A big ZIM takes a while to open; the library's node in the
/// tree shows `titles-progress` until `titles-done` arrives.
#[tauri::command]
pub async fn prepare_titles(app: AppHandle) -> Result<(), String> {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let libs = state.libraries.read().unwrap().clone();
        let st: &AppState = &state;
        std::thread::scope(|s| {
            for l in &libs {
                let app = app.clone();
                s.spawn(move || {
                    if st.titles.read().unwrap().contains_key(&l.id) {
                        return;
                    }
                    if !st.titles_building.lock().unwrap().insert(l.id.clone()) {
                        return; // another prepare_titles is building this one
                    }
                    let emit = |done: usize, total: usize| {
                        let _ = app.emit("titles-progress", TitlesProgressDto { lib: l.id.clone(), done, total });
                    };
                    emit(0, 0);
                    let _ = st.titles_with(l, Some(&emit));
                    st.titles_building.lock().unwrap().remove(&l.id);
                    let _ = app.emit("titles-done", TitlesDoneDto { lib: l.id.clone() });
                });
            }
        });
    });
    Ok(())
}

#[derive(Serialize)]
pub struct BookRefTargetDto {
    pub lib: String,
    pub lib_title: String,
    pub is_current_lib: bool,
    pub kind: &'static str,
    pub title: String,
    pub context: String,
    pub author: String,
    pub book_id: Option<String>,
    pub path: Option<String>,
    pub folder_path: Option<String>,
    pub lb: Option<String>,
    pub is_alias: bool,
}

#[derive(Serialize)]
pub struct ResolveBookRefsDto {
    pub version: u64,
    pub pending: bool,
    pub matches: HashMap<String, Vec<BookRefTargetDto>>,
}

/// Resolve book titles mentioned inside an opened read-only document against
/// already-loaded title indexes (current library first, then other libraries,
/// script-folded via `TitleIndex::exact_matches`). Never blocks on large
/// libraries still building in `prepare_titles`; sets `pending = true` so the
/// reader can refresh once `titles-done` arrives.
#[tauri::command]
pub async fn resolve_book_refs(
    lib: String,
    path: Option<String>,
    book_id: Option<String>,
    titles: Vec<String>,
    state: State<'_, AppState>,
) -> Result<ResolveBookRefsDto, String> {
    let libs = state.libraries.read().unwrap().clone();
    let mut ready: Vec<(Library, Arc<TitleIndex>)> = Vec::with_capacity(libs.len());
    let mut pending = false;

    for l in &libs {
        if let Some(t) = state.titles.read().unwrap().get(&l.id).cloned() {
            ready.push((l.clone(), t));
            continue;
        }
        let is_building = state.titles_building.lock().unwrap().contains(&l.id);
        let is_fast = matches!(l.kind.as_str(), "notes" | "folder") || l.id == lib;
        if !is_building && is_fast {
            if let Ok(t) = state.titles(l) {
                ready.push((l.clone(), t));
                continue;
            }
        }
        pending = true;
    }

    let version = state.titles_version.load(Ordering::Relaxed);
    let mut matches: HashMap<String, Vec<BookRefTargetDto>> = HashMap::new();
    let mut seen_queries: HashSet<String> = HashSet::new();

    for raw in titles {
        let trimmed = raw.trim();
        if trimmed.is_empty() || !seen_queries.insert(trimmed.to_string()) {
            continue;
        }
        // (is_other_lib, is_folder, is_alias, lib_order, entry_order, dto)
        let mut hits: Vec<(u8, u8, u8, usize, usize, BookRefTargetDto)> = Vec::new();
        let mut seen_targets: HashSet<(String, Option<String>, Option<String>)> = HashSet::new();

        for (lib_order, (l, idx)) in ready.iter().enumerate() {
            let is_current_lib = l.id == lib;
            let raw_hits = idx.exact_matches(trimmed);

            // Collect matched article paths in this library first so a book folder that
            // already has a matched document directly inside it is not shown twice.
            let matched_article_paths: Vec<&str> = raw_hits
                .iter()
                .filter_map(|&(entry_idx, _)| {
                    let e = &idx.entries[entry_idx];
                    (e.kind == smart_library_core::titles::TitleKind::Article)
                        .then(|| e.path.as_deref())
                        .flatten()
                })
                .collect();

            for (entry_order, (entry_idx, is_alias)) in raw_hits.into_iter().enumerate() {
                let e = &idx.entries[entry_idx];
                let is_folder = e.kind == smart_library_core::titles::TitleKind::Folder;
                let target_book_id = idx.book_id(e).map(|s| s.to_string());
                let mut target_path = e.path.clone();
                let mut folder_path: Option<String> = None;
                let context = idx.context(e).to_string();

                if is_folder {
                    let Some(fp) = e.path.as_deref() else { continue };
                    let prefix = format!("{}/", fp.trim_end_matches('/'));
                    // Skip self-reference when the currently opened document is inside this book folder.
                    if is_current_lib && path.as_deref().is_some_and(|p| p == fp || p.starts_with(&prefix)) {
                        continue;
                    }
                    // Suppress redundant parent folder when a concrete document inside it already matched.
                    if matched_article_paths.iter().any(|p| p.starts_with(&prefix)) {
                        continue;
                    }
                    // Folders locate in the library tree rather than opening an arbitrary first file.
                    folder_path = Some(fp.to_string());
                    target_path = None;
                } else if is_current_lib {
                    // Skip self-reference to the currently opened document.
                    let same_book = target_book_id.is_some()
                        && target_book_id == book_id
                        && (target_path.is_none() || target_path == path);
                    let same_path = target_path.is_some() && target_path == path;
                    if same_book || same_path {
                        continue;
                    }
                }

                let dedup_key = (
                    l.id.clone(),
                    target_book_id.clone(),
                    target_path.clone().or_else(|| folder_path.clone()),
                );
                if !seen_targets.insert(dedup_key) {
                    continue;
                }

                hits.push((
                    if is_current_lib { 0 } else { 1 },
                    if is_folder { 1 } else { 0 },
                    if is_alias { 1 } else { 0 },
                    lib_order,
                    entry_order,
                    BookRefTargetDto {
                        lib: l.id.clone(),
                        lib_title: l.title.clone(),
                        is_current_lib,
                        kind: e.kind.as_str(),
                        title: e.title.clone(),
                        context,
                        author: idx.author(e).to_string(),
                        book_id: target_book_id,
                        path: target_path,
                        folder_path,
                        lb: e.lb.clone(),
                        is_alias,
                    },
                ));
            }
        }

        if !hits.is_empty() {
            hits.sort_by(|a, b| (a.0, a.1, a.2, a.3, a.4).cmp(&(b.0, b.1, b.2, b.3, b.4)));
            matches.insert(trimmed.to_string(), hits.into_iter().map(|h| h.5).collect());
        }
    }

    Ok(ResolveBookRefsDto { version, pending, matches })
}

#[derive(Serialize)]
pub struct SiblingKepansDto {
    pub path: String,
    pub title: String,
    pub kepans: Vec<smart_library_core::article::KepanEntry>,
}

/// Extract ordered 科判 outlines across sibling documents in the same series
/// (`第n课`/`第n修法`…) or folder, ignoring `.json` files and `科判` docs.
#[tauri::command]
pub async fn sibling_kepans(
    lib: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<SiblingKepansDto>, String> {
    let l = state.library(&lib)?;
    match l.kind.as_str() {
        "notes" => {
            let store = state.notes(&l)?;
            tauri::async_runtime::spawn_blocking(move || {
                let id: i64 = path.parse().map_err(|_| format!("不是笔记：{}", path))?;
                let note = store.get(id)?.ok_or("笔记不存在")?;
                let children = store.children(note.parent)?;
                let sibs: Vec<(String, String)> = children
                    .iter()
                    .filter(|s| !s.mind && !s.board)
                    .map(|s| (s.id.to_string(), s.title.clone()))
                    .collect();
                let selected = smart_library_core::article::select_kepan_siblings(&sibs, &path, &note.title);
                let opts = RenderOptions::default();
                let mut out = Vec::with_capacity(selected.len());
                for (sp, st) in selected {
                    if let Ok(sid) = sp.parse::<i64>() {
                        if let Ok(Some(sn)) = store.get(sid) {
                            let a = notes::article(&sn.content, Some(&opts));
                            out.push(SiblingKepansDto { path: sp, title: st, kepans: a.kepans });
                        }
                    }
                }
                Ok(out)
            })
            .await
            .map_err(|e| format!("读取兄弟笔记科判失败：{}", e))?
        }
        "folder" => {
            let f = state.folder(&l)?;
            tauri::async_runtime::spawn_blocking(move || {
                let cur_name = path.rsplit('/').next().unwrap_or(&path);
                let cur_title = folder::title_of(cur_name).to_string();
                let sibs: Vec<(String, String)> = f
                    .siblings(&path)
                    .into_iter()
                    .filter(|s| {
                        !matches!(
                            folder::EditKind::of(&s.name),
                            Some(folder::EditKind::MindMap) | Some(folder::EditKind::Whiteboard)
                        )
                    })
                    .map(|s| {
                        let t = folder::title_of(&s.name).to_string();
                        (s.rel, t)
                    })
                    .collect();
                let selected = smart_library_core::article::select_kepan_siblings(&sibs, &path, &cur_title);
                let opts = RenderOptions::default();
                let mut out = Vec::with_capacity(selected.len());
                for (sp, st) in selected {
                    if let Ok(a) = f.article(&sp, Some(&opts)) {
                        out.push(SiblingKepansDto { path: sp, title: st, kepans: a.kepans });
                    }
                }
                Ok(out)
            })
            .await
            .map_err(|e| format!("读取兄弟文档科判失败：{}", e))?
        }
        "html" => {
            let bc = state.bookcase_of(&l)?;
            let t = state.html_tree(&l)?;
            tauri::async_runtime::spawn_blocking(move || {
                let leaf = t.leaf(&path).ok_or_else(|| format!("目录里没有这篇文章：{}", path))?;
                let sibs: Vec<(String, String)> = t
                    .siblings(&path)
                    .iter()
                    .map(|s| (s.path.clone(), s.title.clone()))
                    .collect();
                let selected = smart_library_core::article::select_kepan_siblings(&sibs, &path, &leaf.title);
                let opts = RenderOptions::default();
                let mut out = Vec::with_capacity(selected.len());
                for (sp, st) in selected {
                    if let Ok(a) = bc.article(&sp, Some(&opts)) {
                        out.push(SiblingKepansDto { path: sp, title: st, kepans: a.kepans });
                    }
                }
                Ok(out)
            })
            .await
            .map_err(|e| format!("读取兄弟文章科判失败：{}", e))?
        }
        _ => Ok(Vec::new()),
    }
}

fn parse_para_lb_num(lb: &str) -> Option<usize> {
    lb.strip_prefix('¶')?.parse::<usize>().ok()
}

/// Extract detected 科判 (with in-doc + optional cross-sibling parent/child links)
/// and grouped 颂词/偈颂 from a document for AI / MCP tools.
pub async fn ai_doc_structure(
    lib: String,
    path: String,
    include_kepan: bool,
    include_verses: bool,
    include_siblings: bool,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let l = state.library(&lib)?;
    let (title, kepans_raw, verses_raw) = match l.kind.as_str() {
        "notes" => {
            let store = state.notes(&l)?;
            let p = path.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let id: i64 = p.parse().map_err(|_| format!("不是笔记：{}", p))?;
                let note = store.get(id)?.ok_or("笔记不存在")?;
                let a = notes::article(&note.content, Some(&RenderOptions::default()));
                Ok::<_, String>((note.title, a.kepans, a.verses))
            })
            .await
            .map_err(|e| format!("读取笔记结构失败：{}", e))??
        }
        "folder" => {
            let f = state.folder(&l)?;
            let p = path.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let name = p.rsplit('/').next().unwrap_or(&p);
                let title = folder::title_of(name).to_string();
                let a = f.article(&p, Some(&RenderOptions::default()))?;
                Ok::<_, String>((title, a.kepans, a.verses))
            })
            .await
            .map_err(|e| format!("读取文档结构失败：{}", e))??
        }
        "html" => {
            let bc = state.bookcase_of(&l)?;
            let t = state.html_tree(&l)?;
            let p = path.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let leaf = t.leaf(&p).ok_or_else(|| format!("目录里没有这篇文章：{}", p))?;
                let a = bc.article(&p, Some(&RenderOptions::default()))?;
                Ok::<_, String>((leaf.title.clone(), a.kepans, a.verses))
            })
            .await
            .map_err(|e| format!("读取文章结构失败：{}", e))??
        }
        other => {
            return Err(format!(
                "当前知识库类型（{}）暂不支持文档科判/偈颂结构识别（支持 folder / html / notes）",
                other
            ));
        }
    };

    let linked_kepans = if include_kepan {
        if include_siblings && !kepans_raw.is_empty() {
            let sibs = sibling_kepans(lib.clone(), path.clone(), state).await.unwrap_or_default();
            if sibs.iter().any(|s| s.path == path) {
                let doc_refs: Vec<(&str, &str, &[smart_library_core::article::KepanEntry])> = sibs
                    .iter()
                    .map(|s| (s.path.as_str(), s.title.as_str(), s.kepans.as_slice()))
                    .collect();
                smart_library_core::article::build_kepan_links(&doc_refs, &path)
            } else {
                smart_library_core::article::build_kepan_links(&[(&path, &title, &kepans_raw)], &path)
            }
        } else {
            smart_library_core::article::build_kepan_links(&[(&path, &title, &kepans_raw)], &path)
        }
    } else {
        Vec::new()
    };

    // Group contiguous verse paragraphs (consecutive ¶N) and attach nearest preceding kepan.
    let mut grouped_verses: Vec<serde_json::Value> = Vec::new();
    if include_verses && !verses_raw.is_empty() {
        let kepan_positions: Vec<(usize, &str, &str)> = linked_kepans
            .iter()
            .filter_map(|k| parse_para_lb_num(&k.lb).map(|n| (n, k.lb.as_str(), k.label.as_str())))
            .collect();
        let mut cur_lb = String::new();
        let mut cur_last_num: Option<usize> = None;
        let mut cur_lines: Vec<String> = Vec::new();

        let flush = |out: &mut Vec<serde_json::Value>,
                     lb: &mut String,
                     lines: &mut Vec<String>,
                     kpos: &[(usize, &str, &str)]| {
            if lines.is_empty() {
                return;
            }
            let start_num = parse_para_lb_num(lb);
            let nearest_kepan = start_num.and_then(|sn| kpos.iter().rev().find(|(kn, _, _)| *kn < sn));
            let mut obj = serde_json::Map::new();
            obj.insert("lb".into(), serde_json::json!(lb.clone()));
            obj.insert("text".into(), serde_json::json!(lines.join("\n")));
            if let Some((_, klb, klabel)) = nearest_kepan {
                obj.insert("kepan_lb".into(), serde_json::json!(klb));
                if !klabel.is_empty() {
                    obj.insert("kepan_label".into(), serde_json::json!(klabel));
                }
            }
            out.push(serde_json::Value::Object(obj));
            lb.clear();
            lines.clear();
        };

        for v in &verses_raw {
            let num = parse_para_lb_num(&v.lb);
            let contiguous = match (cur_last_num, num) {
                (Some(prev), Some(now)) => now == prev + 1,
                _ => false,
            };
            if !cur_lines.is_empty() && !contiguous {
                flush(&mut grouped_verses, &mut cur_lb, &mut cur_lines, &kepan_positions);
            }
            if cur_lines.is_empty() {
                cur_lb = v.lb.clone();
            }
            cur_lines.push(v.text.clone());
            cur_last_num = num;
        }
        flush(&mut grouped_verses, &mut cur_lb, &mut cur_lines, &kepan_positions);
    }

    let mut out = serde_json::Map::new();
    out.insert("lib".into(), serde_json::json!(lib));
    out.insert("path".into(), serde_json::json!(path));
    out.insert("title".into(), serde_json::json!(title));
    if include_kepan {
        out.insert("kepan_count".into(), serde_json::json!(linked_kepans.len()));
        out.insert("kepans".into(), serde_json::to_value(&linked_kepans).unwrap_or_default());
    }
    if include_verses {
        out.insert("verse_count".into(), serde_json::json!(grouped_verses.len()));
        out.insert("verses".into(), serde_json::Value::Array(grouped_verses));
    }
    Ok(serde_json::Value::Object(out))
}

/// Extract unique `《…》` book references from any document and optionally resolve
/// each title against loaded library title indexes (respecting `denied_libs`).
pub async fn ai_doc_book_refs(
    lib: String,
    book_id: Option<String>,
    path: Option<String>,
    resolve: bool,
    denied_libs: &std::collections::HashSet<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let dto = open_node(lib.clone(), book_id.clone(), path.clone(), None, None, state.clone()).await?;
    let owned_lines: Vec<(String, String)> = dto
        .lines
        .iter()
        .map(|l| (l.lb.clone(), l.parts.iter().map(|p| p.t.as_str()).collect::<String>()))
        .collect();
    let occs = smart_library_core::article::extract_book_refs(
        owned_lines.iter().map(|(lb, text)| (lb.as_str(), text.as_str())),
    );

    let (resolved_map, pending) = if resolve && !occs.is_empty() {
        let titles: Vec<String> = occs.iter().map(|o| o.title.clone()).collect();
        match resolve_book_refs(lib.clone(), Some(dto.path.clone()), dto.book_id.clone(), titles, state).await {
            Ok(res) => (res.matches, res.pending),
            Err(_) => (HashMap::new(), false),
        }
    } else {
        (HashMap::new(), false)
    };

    let refs: Vec<serde_json::Value> = occs
        .into_iter()
        .map(|o| {
            let mut obj = serde_json::Map::new();
            obj.insert("title".into(), serde_json::json!(o.title));
            obj.insert("count".into(), serde_json::json!(o.count));
            obj.insert("lbs".into(), serde_json::json!(o.lbs));
            if resolve {
                let targets: Vec<serde_json::Value> = resolved_map
                    .get(&o.title)
                    .map(|list| {
                        list.iter()
                            .filter(|t| !denied_libs.contains(&t.lib))
                            .map(|t| {
                                let mut tobj = serde_json::Map::new();
                                tobj.insert("lib".into(), serde_json::json!(t.lib));
                                tobj.insert("lib_title".into(), serde_json::json!(t.lib_title));
                                tobj.insert("kind".into(), serde_json::json!(t.kind));
                                tobj.insert("title".into(), serde_json::json!(t.title));
                                if let Some(ref b) = t.book_id {
                                    tobj.insert("book_id".into(), serde_json::json!(b));
                                }
                                if let Some(ref p) = t.path {
                                    tobj.insert("path".into(), serde_json::json!(p));
                                }
                                if let Some(ref fp) = t.folder_path {
                                    tobj.insert("folder_path".into(), serde_json::json!(fp));
                                }
                                if let Some(ref lb) = t.lb {
                                    tobj.insert("lb".into(), serde_json::json!(lb));
                                }
                                if !t.author.is_empty() {
                                    tobj.insert("author".into(), serde_json::json!(t.author));
                                }
                                serde_json::Value::Object(tobj)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                obj.insert("targets".into(), serde_json::Value::Array(targets));
            }
            serde_json::Value::Object(obj)
        })
        .collect();

    Ok(serde_json::json!({
        "lib": dto.lib,
        "path": dto.path,
        "title": dto.title,
        "ref_count": refs.len(),
        "pending": pending,
        "refs": refs,
    }))
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
        &MarkNew { kind, lib: lib.clone(), path, book_id, lb, quote, note, doc_title, lib_title, author },
        &parse_tags(&tags),
    )?;
    state.sidecar_sync(&lib);
    Ok(id)
}

#[tauri::command]
pub async fn mark_update(id: i64, note: Option<String>, tags: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    let lib = state.marks.library_of(id)?;
    state.marks.update(id, note.as_deref(), tags.map(|t| parse_tags(&t)).as_deref())?;
    if let Some(lib) = lib { state.sidecar_sync(&lib); }
    Ok(())
}

#[tauri::command]
pub async fn mark_delete(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let lib = state.marks.library_of(id)?;
    state.marks.delete(id)?;
    if let Some(lib) = lib { state.sidecar_sync(&lib); }
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
    state.sidecar_sync(&lib);
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
    /// other names (found by the title quick search)
    aliases: Vec<String>,
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
            let facets = match files.iter().find(|x| x.rel == path) {
                Some(file) => f.docs(std::slice::from_ref(file)).next().and_then(|d| d.ok()).map(|d| d.facets).unwrap_or_default(),
                None => Vec::new(),
            };
            let mut dims = props::dims_of(&facets);
            if let Some(meta_dims) = f.dir_meta_dims(path) {
                for (k, v) in meta_dims {
                    if !v.is_empty() {
                        dims.insert(k, v);
                    }
                }
            }
            return Ok(dims);
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
    let mut derived = derived_dims(&state, &l, book_id.as_deref(), &path).unwrap_or_default();
    if book_id.is_some() || !path.is_empty() {
        let overlay = index_overlay(&state, &l)?;
        let inherited = if l.kind == "folder" {
            state.folder(&l)?.inherited_properties(&path, &overlay)
        } else {
            let parents: Vec<(String, String)> = match l.kind.as_str() {
                "cbeta" | "html" if path.starts_with("@nav/") => {
                    let mut current = "@nav".to_string();
                    let mut parts: Vec<_> = path.trim_start_matches("@nav/").split('/').collect();
                    parts.pop();
                    parts.into_iter().map(|part| { current.push('/'); current.push_str(part); (String::new(), current.clone()) }).collect()
                }
                "cbeta" | "html" => overlay.nav_ancestors(book_id.as_deref().unwrap_or(&path)),
                "notes" => state.notes(&l)?.rows()?.into_iter().find(|r| r.id.to_string() == path)
                    .map(|r| r.ancestors.into_iter().map(|id| (String::new(), id.to_string())).collect()).unwrap_or_default(),
                _ => Vec::new(),
            };
            overlay.merged(&parents)
        };
        for (dim, values) in inherited {
            if props::is_dim(&dim) || dim == "index_content" { derived.insert(dim, values); }
        }
    }
    Ok(PropsDto {
        own: state.props.get(&lib, book_id.as_deref(), &path)?,
        derived,
        tags: state.marks.node_tags(&lib, book_id.as_deref(), &path).unwrap_or_default(),
        aliases: state.props.aliases(&lib, book_id.as_deref(), &path).unwrap_or_default(),
    })
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
pub async fn recent_bump(lib: String, path: String, state: State<'_, AppState>) -> Result<(), String> {
    state.history.bump(&lib, &path)
}

#[tauri::command]
pub async fn recent_progress(lib: String, path: String, lb: String, mode: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    state.history.set_progress(&lib, &path, &lb, mode.as_deref())
}

#[tauri::command]
pub async fn recent_patch_extra(
    lib: String,
    path: String,
    patch: serde_json::Value,
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.history.patch_extra(&lib, &path, &patch, title.as_deref())
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

// ------------------------------------------------------------ user settings

/// Load user settings from `<config_dir>/settings.json`, or `null` if not yet created.
#[tauri::command]
pub async fn load_settings(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let file = state.config_dir.join("settings.json");
    match std::fs::read_to_string(&file) {
        Ok(s) => Ok(serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
        Err(_) => Ok(serde_json::Value::Null),
    }
}

/// Atomically persist user settings to `<config_dir>/settings.json` and broadcast
/// `settings-changed` so all open windows stay in sync.
#[tauri::command]
pub async fn save_settings(
    settings: serde_json::Value,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let _guard = state.settings_lock.lock().unwrap();
        std::fs::create_dir_all(&state.config_dir).map_err(|e| e.to_string())?;
        let file = state.config_dir.join("settings.json");
        let tmp = state.config_dir.join("settings.json.tmp");
        let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("settings-changed", &settings);
    Ok(())
}

// ------------------------------------------------------------ program info, fonts, find

#[derive(Serialize)]
pub struct AppInfo {
    name: String,
    /// YY.MM.DD
    version: String,
    data_dir: String,
    config_dir: String,
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
        config_dir: state.config_dir.display().to_string(),
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
pub async fn find_in_page(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        if !window.is_focused().unwrap_or(false) {
            let _ = window.set_focus();
            if !window.is_focused().unwrap_or(false) {
                return Err("当前窗口未处于活动焦点".into());
            }
        }
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            VK_CONTROL, VK_MENU, VK_SHIFT,
        };
        fn key(vk: u16, up: bool) -> INPUT {
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 },
                },
            }
        }
        let mut inputs = Vec::with_capacity(6);
        unsafe {
            if GetAsyncKeyState(VK_SHIFT as i32) < 0 {
                inputs.push(key(VK_SHIFT, true));
            }
            if GetAsyncKeyState(VK_MENU as i32) < 0 {
                inputs.push(key(VK_MENU, true));
            }
        }
        let f = b'F' as u16;
        inputs.extend_from_slice(&[key(VK_CONTROL, false), key(f, false), key(f, true), key(VK_CONTROL, true)]);
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            return Ok(());
        }
        Err("无法发送按键".into())
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("不支持".into())
    }
}

#[derive(Serialize, Clone)]
struct ProgressDto {
    lib: String,
    done: usize,
    total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<String>,
}

mod index_cmds;
pub use index_cmds::*;

#[path = "commands/export_cmds.rs"]
mod export_cmds;
pub use export_cmds::*;

#[path = "commands/git_cmds.rs"]
mod git_cmds;
pub use git_cmds::*;

#[path = "commands/ai_cmds.rs"]
mod ai_cmds;
pub use ai_cmds::*;

#[path = "commands/dict_cmds.rs"]
mod dict_cmds;
pub use dict_cmds::*;

// ------------------------------------------------------------ snapshots

impl AppState {
    pub fn sweep_expired_snapshots(&self) {
        let _ = self.snapshots.cleanup_expired_ai();
    }
}

pub(crate) fn read_writable_raw(state: &AppState, lib: &str, id: &str) -> Result<(String, String, String), String> {
    if let Some(f) = rw_folder(state, lib)? {
        let name = id.rsplit('/').next().unwrap_or(id);
        let title = folder::title_of(name).to_string();
        let kind = folder::EditKind::of(name)
            .map(|k| k.content_type().to_string())
            .unwrap_or_else(|| "md".to_string());
        let (raw, stamp) = f.read_edit(id)?;
        state.stamps.lock().unwrap().insert(format!("{}|{}", lib, id), stamp);
        return Ok((title, kind, raw));
    }
    let note = notes_of(state, lib)?
        .get(note_id(id)?)?
        .ok_or_else(|| "笔记不存在".to_string())?;
    let ctype = if note.content_type.is_empty() {
        "md".to_string()
    } else {
        note.content_type
    };
    Ok((note.title, ctype, note.content))
}

#[tauri::command]
pub async fn list_snapshots(lib: String, doc_id: String, state: State<'_, AppState>) -> Result<Vec<SnapshotMeta>, String> {
    state.snapshots.list(&lib, &doc_id)
}

#[tauri::command]
pub async fn create_snapshot(
    lib: String,
    doc_id: String,
    kind: Option<String>,
    label: Option<String>,
    content: Option<String>,
    state: State<'_, AppState>,
) -> Result<SnapshotMeta, String> {
    let (_, ctype, stored_raw) = read_writable_raw(&state, &lib, &doc_id)?;
    let body = content.unwrap_or(stored_raw);
    let kind_str = kind.as_deref().unwrap_or("manual");
    let label_str = label.unwrap_or_else(|| {
        if kind_str == "ai" {
            "AI 修改前".to_string()
        } else {
            "手动快照".to_string()
        }
    });
    state.snapshots.create(&lib, &doc_id, kind_str, &label_str, &ctype, &body)
}

#[tauri::command]
pub async fn get_snapshot(id: i64, state: State<'_, AppState>) -> Result<Option<SnapshotRecord>, String> {
    state.snapshots.get(id)
}

#[tauri::command]
pub async fn restore_snapshot(id: i64, state: State<'_, AppState>) -> Result<SnapshotRecord, String> {
    let rec = state
        .snapshots
        .get(id)?
        .ok_or_else(|| "快照不存在或已被删除".to_string())?;
    if rec.kind == "deleted" {
        return Err("已删除的笔记请在知识库的「回收站…」里恢复".into());
    }
    save_note(rec.lib.clone(), rec.doc_id.clone(), rec.content.clone(), None, state).await?;
    Ok(rec)
}

#[tauri::command]
pub async fn delete_snapshot(id: i64, state: State<'_, AppState>) -> Result<bool, String> {
    // throwing away a deleted note for good: its pictures are no longer held
    if let Some(rec) = state.snapshots.get(id)? {
        if rec.kind == "deleted" {
            if let Ok(l) = state.library(&rec.lib) {
                if l.kind == "notes" {
                    if let Ok(store) = state.notes(&l) {
                        let _ = store.release_deleted_pictures(id);
                    }
                }
            }
        }
    }
    state.snapshots.delete(id)
}

#[tauri::command]
pub async fn get_snapshot_policy(state: State<'_, AppState>) -> Result<SnapshotPolicy, String> {
    state.snapshots.policy()
}

#[tauri::command]
pub async fn set_snapshot_policy(
    ai_max_per_doc: usize,
    ai_ttl_days: u64,
    state: State<'_, AppState>,
) -> Result<SnapshotPolicy, String> {
    state.snapshots.set_policy(ai_max_per_doc, ai_ttl_days)
}

#[cfg(test)]
mod library_id_tests {
    use super::*;

    #[test]
    fn validates_unique_immutable_ids_and_safe_defaults() {
        let libs = vec![Library { id: "wikiquote".into(), kind: "zim".into(), title: "test".into(), source: String::new(), tree: String::new(), exts: vec![], max_mb: 0, writable: false, remote: String::new() }];
        assert_eq!(library_id(Some("wikipedia"), "zim", &libs).unwrap(), "wikipedia");
        assert!(library_id(Some("wikiquote"), "zim", &libs).is_err());
        assert!(library_id(Some("../private"), "zim", &libs).is_err());
        assert!(library_id(Some(".hidden"), "zim", &libs).is_err());
        assert!(library_id(Some("ABC"), "zim", &libs).is_err());
        assert_eq!(library_id(None, "wikiquote", &libs).unwrap(), "wikiquote-2");
        assert_eq!(library_id(None, "../坏", &libs).unwrap(), "lib");
    }

    #[test]
    fn raw_lang_follows_the_file_suffix() {
        assert_eq!(raw_lang("XML/T/T01/T01n0004_001.xml"), "xml");
        assert_eq!(raw_lang("a/x.html"), "html");
        assert_eq!(raw_lang("notes/readme.md"), "markdown");
        assert_eq!(raw_lang("a.map.json"), "json");
        assert_eq!(raw_lang("plain.txt"), "text");
        assert_eq!(raw_lang("noext"), "text");
    }

    #[test]
    fn navigation_groups_inherit_properties_and_descendants_override() {
        fn node(path: Option<&str>, children: Vec<NodeDto>) -> NodeDto {
            NodeDto { title: "".into(), book_id: None, start: None, toc: None, nav: None, path: path.map(str::to_string), href: None, rel: None, lazy: false, mind: false, board: false, children }
        }
        let nodes = vec![node(None, vec![node(Some("article"), vec![node(Some("child"), vec![])])])];
        let props = Props::memory().unwrap();
        props.set("lib", None, "@nav/0", &[("index_content".into(), vec!["off".into()])].into_iter().collect()).unwrap();
        props.set("lib", None, "article", &[("index_content".into(), vec!["on".into()])].into_iter().collect()).unwrap();
        let mut overlay = props.overlay("lib").unwrap();
        overlay.set_nav_ancestors(navigation_ancestors(&nodes));
        let mut doc = smart_library_core::library::IndexDoc { path: "child".into(), book: "".into(), juan: 0, title: "标题".into(), text: "正文".into(), facets: vec![], keys: vec![(String::new(), "child".into())] };
        overlay.apply(&mut doc);
        assert_eq!(doc.text, "正文");
        let mut sibling = doc.clone(); sibling.path = "article".into(); sibling.keys = vec![(String::new(), "article".into())];
        props.set("lib", None, "article", &Default::default()).unwrap();
        let mut overlay = props.overlay("lib").unwrap(); overlay.set_nav_ancestors(navigation_ancestors(&nodes));
        overlay.apply(&mut sibling);
        assert!(sibling.text.is_empty());
    }

    #[test]
    fn folder_index_follows_rename_after_restart_before_focus_scan() {
        let tmp = std::env::temp_dir().join(format!("sl_folder_restart_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let root = tmp.join("folder");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Before.md"), "雪岭梅香").unwrap();
        let lib = Library {
            id: "folder-1".into(),
            kind: "folder".into(),
            title: "测试文件夹".into(),
            source: root.to_string_lossy().into_owned(),
            tree: String::new(),
            exts: vec!["md".into()],
            max_mb: 0,
            writable: true,
            remote: String::new(),
        };
        let open = || AppState::new(tmp.join("cfg"), tmp.join("data"));
        {
            let state = open();
            let f = state.folder(&lib).unwrap();
            let files = f.files();
            let overlay = props::Overlay::default();
            let dir = state.index_dir(&lib.id);
            smart_library_core::search::build_index(
                &dir, serde_json::json!({"data_version": f.fingerprint(&files)}),
                files.len(), f.docs(&files), &overlay, |_, _| {},
            ).unwrap();
            save_folder_snapshot(&dir, &folder_snapshot_map(&f, &files, &overlay));
            let new = f.rename("Before.md", "After").unwrap();
            state.after_move(&lib.id, "Before.md", &new, &["Before.md".into()], Some("After"));
        }
        {
            let state = open();
            state.flush_dirty(std::slice::from_ref(&lib));
            let index = SearchIndex::open(&state.index_dir(&lib.id)).unwrap();
            let result = index.search(&SearchRequest::new("雪岭梅香")).unwrap();
            assert_eq!(result.hits.len(), 1);
            assert_eq!(result.hits[0].path, "After.md");
        }
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn folder_index_flush_includes_external_changes() {
        let tmp = std::env::temp_dir().join(format!("sl_folder_index_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let root = tmp.join("folder");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("local.md"), "原来甲文").unwrap();
        std::fs::write(root.join("external.md"), "原来乙文").unwrap();
        let state = AppState::new(tmp.join("cfg"), tmp.join("data"));
        let lib = Library {
            id: "folder-1".into(),
            kind: "folder".into(),
            title: "测试文件夹".into(),
            source: root.to_string_lossy().into_owned(),
            tree: String::new(),
            exts: vec!["md".into()],
            max_mb: 0,
            writable: true,
            remote: String::new(),
        };
        let folder = state.folder(&lib).unwrap();
        let files = folder.files();
        let dir = state.index_dir(&lib.id);
        smart_library_core::search::build_index(
            &dir, serde_json::json!({"data_version": folder.fingerprint(&files)}),
            files.len(), folder.docs(&files), &Default::default(), |_, _| {},
        ).unwrap();
        save_folder_snapshot(&dir, &folder_snapshot_map(&folder, &files, &Default::default()));

        std::fs::write(root.join("local.md"), "内部正文改写").unwrap();
        std::fs::write(root.join("external.md"), "外部正文改写").unwrap();
        state.mark_dirty(&lib.id, ["local.md".into()]);
        state.flush_dirty(std::slice::from_ref(&lib));

        {
            let index = SearchIndex::open(&dir).unwrap();
            for text in ["内部正文改写", "外部正文改写"] {
                assert_eq!(index.count(&SearchRequest::new(text)).unwrap(), 1);
            }
            assert_eq!(index.count(&SearchRequest::new("原来乙文")).unwrap(), 0);
        }
        assert_eq!(SearchIndex::data_version(&dir), Some(folder.fingerprint(&folder.files())));
        drop(state);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn imported_folder_properties_refresh_index_without_file_edits() {
        let tmp = std::env::temp_dir().join(format!("sl_sidecar_index_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let root = tmp.join("folder");
        std::fs::create_dir_all(root.join("专题")).unwrap();
        std::fs::write(root.join("专题/文章.md"), "受影响的文章").unwrap();
        std::fs::write(root.join("其他.md"), "其他文章").unwrap();
        let lib = Library {
            id: "folder-1".into(),
            kind: "folder".into(),
            title: "测试文件夹".into(),
            source: root.to_string_lossy().into_owned(),
            tree: String::new(),
            exts: vec!["md".into()],
            max_mb: 0,
            writable: true,
            remote: String::new(),
        };
        let open = || AppState::new(tmp.join("cfg"), tmp.join("data"));
        let version;
        {
            let state = open();
            let folder = state.folder(&lib).unwrap();
            let files = folder.files();
            let overlay = props::Overlay::default();
            let dir = state.index_dir(&lib.id);
            version = folder.fingerprint(&files);
            smart_library_core::search::build_index(
                &dir, serde_json::json!({"data_version": version}), files.len(),
                folder.docs(&files), &overlay, |_, _| {},
            ).unwrap();
            save_folder_snapshot(&dir, &folder_snapshot_map(&folder, &files, &overlay));
        }
        let props = vec![("专题".into(), Dims::from([("period".into(), vec!["唐".into()])]))];
        smart_library_core::sidecar::write(
            &root, &smart_library_core::sidecar::Sidecar { props, marks: vec![] },
        ).unwrap();
        {
            let state = open();
            state.flush_dirty(std::slice::from_ref(&lib));
            let dir = state.index_dir(&lib.id);
            assert_eq!(SearchIndex::data_version(&dir), Some(version));
            let mut req = SearchRequest::new("");
            req.filters = vec![Filter { root: "period".into(), values: vec!["唐".into()] }];
            let result = SearchIndex::open(&dir).unwrap().search(&req).unwrap();
            assert_eq!(result.hits.len(), 1);
            assert_eq!(result.hits[0].path, "专题/文章.md");
        }
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn old_folder_index_recovers_people_from_more_specific_source_metadata() {
        let tmp = std::env::temp_dir().join(format!("sl_layered_props_test_{}", std::process::id()));
        let root = tmp.join("folder");
        std::fs::create_dir_all(root.join("专题")).unwrap();
        for path in ["默认.md", "专题/目录.md", "专题/条目.md", "专题/编辑.md"] {
            std::fs::write(root.join(path), "正文").unwrap();
        }
        std::fs::write(
            root.join("专题/.meta.json"),
            r#"{"*":{"person":"目录人物"},"list":[{"name":"条目.md","person":"条目人物"},{"name":"编辑.md","person":"资料原值"}]}"#,
        ).unwrap();
        let lib = Library {
            id: "folder".into(),
            kind: "folder".into(),
            title: "测试文件夹".into(),
            source: root.to_string_lossy().into_owned(),
            tree: String::new(),
            exts: vec!["md".into()],
            max_mb: 0,
            writable: false,
            remote: String::new(),
        };
        let open = || AppState::new(tmp.join("cfg"), tmp.join("data"));
        {
            let state = open();
            for (path, name) in [("", "默认人物"), ("专题/编辑.md", "本地编辑")] {
                state.props.set(&lib.id, None, path, &Dims::from([("person".into(), vec![name.into()])])).unwrap();
            }
            let folder = state.folder(&lib).unwrap();
            let files = folder.files();
            let overlay = state.props.overlay(&lib.id).unwrap();
            let dir = state.index_dir(&lib.id);
            smart_library_core::search::build_index(
                &dir, serde_json::json!({"data_version": folder.fingerprint(&files)}),
                files.len(), folder.docs(&files), &overlay, |_, _| {},
            ).unwrap();
            let mut old = serde_json::to_value(folder_snapshot_map(&folder, &files, &overlay)).unwrap();
            old.as_object_mut().unwrap().remove("layered_properties");
            std::fs::write(dir.join("folder-snapshot.json"), serde_json::to_vec(&old).unwrap()).unwrap();
        }
        {
            let state = open();
            state.flush_dirty(std::slice::from_ref(&lib));
            let dir = state.index_dir(&lib.id);
            let index = SearchIndex::open(&dir).unwrap();
            for (name, path) in [
                ("默认人物", "默认.md"),
                ("目录人物", "专题/目录.md"),
                ("条目人物", "专题/条目.md"),
                ("本地编辑", "专题/编辑.md"),
            ] {
                let mut req = SearchRequest::new("");
                req.filters = vec![Filter { root: "person".into(), values: vec![name.into()] }];
                let result = index.search(&req).unwrap();
                assert_eq!(result.hits.len(), 1, "{name}");
                assert_eq!(result.hits[0].path, path);
            }
        }
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn legacy_folder_snapshot_without_properties_does_not_dirty_untagged_folder() {
        let tmp = std::env::temp_dir().join(format!("sl_legacy_snap_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let root = tmp.join("folder");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.md"), "正文内容").unwrap();
        let lib = Library {
            id: "folder-legacy".into(),
            kind: "folder".into(),
            title: "旧快照文件夹".into(),
            source: root.to_string_lossy().into_owned(),
            tree: String::new(),
            exts: vec!["md".into()],
            max_mb: 0,
            writable: false,
            remote: String::new(),
        };
        let state = AppState::new(tmp.join("cfg"), tmp.join("data"));
        let folder = state.folder(&lib).unwrap();
        let files = folder.files();
        let dir = state.index_dir(&lib.id);
        smart_library_core::search::build_index(
            &dir, serde_json::json!({"data_version": folder.fingerprint(&files)}), files.len(),
            folder.docs(&files), &props::Overlay::default(), |_, _| {},
        ).unwrap();
        // Write a legacy folder-snapshot.json with no `properties` field.
        std::fs::write(
            dir.join("folder-snapshot.json"),
            serde_json::to_vec(&serde_json::json!({ "files": { "a.md": [12, 0] }, "metas": {} })).unwrap(),
        ).unwrap();

        state.queue_folder_property_changes(&lib, &folder).unwrap();
        assert!(state.dirty.lock().unwrap().get(&lib.id).is_none_or(|s| s.is_empty()));
        // Snapshot should now record the empty properties map.
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("folder-snapshot.json")).unwrap()).unwrap();
        assert_eq!(saved["properties"], serde_json::json!({}));
        drop(state);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn sidecar_enable_queues_imported_properties_for_cached_folder() {
        let tmp = std::env::temp_dir().join(format!("sl_sidecar_enable_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let root = tmp.join("folder");
        std::fs::create_dir_all(root.join("专题")).unwrap();
        std::fs::write(root.join("专题/文章.md"), "受影响的文章").unwrap();
        let mut lib = Library {
            id: "folder-enable".into(),
            kind: "folder".into(),
            title: "切换读写文件夹".into(),
            source: root.to_string_lossy().into_owned(),
            tree: String::new(),
            exts: vec!["md".into()],
            max_mb: 0,
            writable: false,
            remote: String::new(),
        };
        let state = AppState::new(tmp.join("cfg"), tmp.join("data"));
        let folder = state.folder(&lib).unwrap(); // cached while read-only
        let files = folder.files();
        let dir = state.index_dir(&lib.id);
        let overlay = props::Overlay::default();
        smart_library_core::search::build_index(
            &dir, serde_json::json!({"data_version": folder.fingerprint(&files)}), files.len(),
            folder.docs(&files), &overlay, |_, _| {},
        ).unwrap();
        save_folder_snapshot(&dir, &folder_snapshot_map(&folder, &files, &overlay));

        let props = vec![("专题".into(), Dims::from([("period".into(), vec!["宋".into()])]))];
        smart_library_core::sidecar::write(
            &root, &smart_library_core::sidecar::Sidecar { props, marks: vec![] },
        ).unwrap();

        lib.writable = true;
        state.sidecar_enable(&lib).unwrap();
        state.flush_dirty(std::slice::from_ref(&lib));
        let mut req = SearchRequest::new("");
        req.filters = vec![Filter { root: "period".into(), values: vec!["宋".into()] }];
        let result = SearchIndex::open(&dir).unwrap().search(&req).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].path, "专题/文章.md");
        drop(state);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn note_index_recovers_saved_changes_after_restart() {
        let tmp = std::env::temp_dir().join(format!("sl_notes_index_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let lib = Library {
            id: "notes-1".into(),
            kind: "notes".into(),
            title: "测试笔记".into(),
            source: String::new(),
            tree: String::new(),
            exts: vec![],
            max_mb: 0,
            writable: true,
            remote: String::new(),
        };
        let open = || AppState::new(tmp.join("cfg"), tmp.join("data"));
        {
            let state = open();
            let store = state.notes(&lib).unwrap();
            let parent = store.create(None, "原分类", "").unwrap();
            let child = store.create(Some(parent), "子文档", "原始正文").unwrap();
            let gone = store.create(Some(parent), "已删文档", "删除样本").unwrap();
            store.create(Some(parent), "未改动子文档", "其他正文").unwrap();
            let batch = store.index_changes(None).unwrap().unwrap();
            let docs = notes::docs(&batch.rows, &lib.title);
            smart_library_core::search::build_index(
                &state.index_dir(&lib.id),
                serde_json::json!({"data_version": "4:123:12"}),
                docs.len(), docs.into_iter().map(Ok), &Default::default(), |_, _| {},
            ).unwrap();
            state.sync_notes_index(&lib, &state.index_dir(&lib.id)).unwrap();
            store.update(parent, Some("新分类"), None).unwrap();
            store.update(child, None, Some("保存正文")).unwrap();
            store.delete(gone).unwrap();
        }
        {
            let state = open();
            let dir = state.index_dir(&lib.id);
            assert!(state.flush_dirty(std::slice::from_ref(&lib)));
            assert!(!state.flush_dirty(std::slice::from_ref(&lib)));
            let index = SearchIndex::open(&dir).unwrap();
            for text in ["保存正文", "其他正文"] {
                let result = index.search(&SearchRequest::new(text)).unwrap();
                assert_eq!(result.hits.len(), 1);
                assert_eq!(result.hits[0].book, "新分类");
            }
            for text in ["原始正文", "删除样本"] {
                assert_eq!(index.count(&SearchRequest::new(text)).unwrap(), 0);
            }
        }
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn flush_dirty_preserves_paths_while_building() {
        let tmp = std::env::temp_dir().join(format!("sl_flush_dirty_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let state = AppState::new(tmp.join("cfg"), tmp.join("data"));
        let lib = Library {
            id: "folder-1".into(),
            kind: "folder".into(),
            title: "测试文件夹".into(),
            source: String::new(),
            tree: String::new(),
            exts: vec![],
            max_mb: 0,
            writable: true,
            remote: String::new(),
        };
        // Simulate an existing index directory and an in-flight build
        let idx_dir = state.index_dir(&lib.id);
        std::fs::create_dir_all(&idx_dir).unwrap();
        std::fs::write(idx_dir.join("smart-library-index.json"), "{}").unwrap();
        state.building.lock().unwrap().insert(lib.id.clone());
        state.mark_dirty(&lib.id, ["42".to_string()]);

        // Calling flush_dirty while building must NOT discard the dirty path
        state.flush_dirty(std::slice::from_ref(&lib));
        assert!(state.dirty.lock().unwrap().get(&lib.id).is_some_and(|s| s.contains("42")));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn degraded_db_never_overwrites_sidecar_meta() {
        let tmp = std::env::temp_dir().join(format!("sl_degraded_db_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let folder_root = tmp.join("folder");
        std::fs::create_dir_all(&folder_root).unwrap();
        let mut dims = Dims::new();
        dims.insert("period".into(), vec!["唐".into()]);
        smart_library_core::sidecar::write(
            &folder_root,
            &smart_library_core::sidecar::Sidecar {
                props: vec![("doc.md".into(), dims)],
                marks: vec![],
            },
        )
        .unwrap();

        let mut state = AppState::new(tmp.join("cfg"), tmp.join("data"));
        state.db_degraded = true;
        let lib = Library {
            id: "folder-1".into(),
            kind: "folder".into(),
            title: "文件夹".into(),
            source: folder_root.to_string_lossy().into_owned(),
            tree: String::new(),
            exts: vec!["md".into()],
            max_mb: 0,
            writable: true,
            remote: String::new(),
        };
        state.libraries.write().unwrap().push(lib.clone());
        state.sidecar_sync(&lib.id);
        assert!(state.sidecar_write(&lib).is_err());

        // Existing meta.json on disk must still contain the original property
        let sc = smart_library_core::sidecar::read(&folder_root, &lib.id).unwrap().unwrap();
        assert_eq!(sc.props.len(), 1);
        assert_eq!(sc.props[0].0, "doc.md");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn external_action_url_requires_matching_token() {
        let token = next_external_token(1);
        let forged = tauri::Url::parse("http://smartlib-action.localhost/?data=%7B%22type%22%3A%22diary%22%7D").unwrap();
        assert!(parse_external_action_url(&forged, &token).is_none());

        let wrong = tauri::Url::parse("http://smartlib-action.localhost/?token=wrong&data=%7B%22type%22%3A%22diary%22%7D").unwrap();
        assert!(parse_external_action_url(&wrong, &token).is_none());

        let valid = tauri::Url::parse(&format!(
            "http://smartlib-action.localhost/?token={token}&data=%7B%22type%22%3A%22search%22%2C%22text%22%3A%22%E5%9B%9B%E8%B0%9B%22%7D"
        ))
        .unwrap();
        let parsed = parse_external_action_url(&valid, &token).unwrap();
        assert_eq!(parsed.get("type").and_then(|v| v.as_str()), Some("search"));
        assert_eq!(parsed.get("text").and_then(|v| v.as_str()), Some("四谛"));
    }
}

/// Restart into the build that was just installed. On Windows the NSIS
/// installer launches and the running process exits inside `install()`, but on
/// macOS and Linux the updater only replaces the bundle under the running app,
/// so the update dialog asks for this restart once `install()` returns.
#[tauri::command]
pub fn relaunch_after_update(app: AppHandle) {
    if !cfg!(windows) {
        app.restart();
    }
}
