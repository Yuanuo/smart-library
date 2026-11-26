// Index maintenance shared by search, source scans, and the desktop commands.
use super::*;

impl AppState {
    /// Remember that documents of a library were changed (saved, made, renamed, deleted).
    pub(super) fn mark_dirty(&self, lib: &str, paths: impl IntoIterator<Item = String>) {
        self.dirty
            .lock()
            .unwrap()
            .entry(lib.to_string())
            .or_default()
            .extend(paths);
    }

    pub(super) fn queue_folder_property_changes(
        &self,
        lib: &Library,
        folder: &FolderLibrary,
    ) -> Result<(), String> {
        let dir = self.index_dir(&lib.id);
        if !dir.join("smart-library-index.json").exists() {
            return Ok(());
        }
        let old = load_folder_snapshot(&dir);
        let current = self.props.overlay(&lib.id)?.folder_properties();
        if let Some(mut snap) = old {
            if snap.properties.is_none() && current.is_empty() {
                snap.properties = Some(current);
                save_folder_snapshot(&dir, &snap);
                return Ok(());
            }
            let paths = changed_property_paths(snap.properties.as_ref(), &current);
            if !paths.is_empty() {
                self.mark_dirty(
                    &lib.id,
                    folder
                        .files()
                        .into_iter()
                        .filter(|f| paths.iter().any(|p| file_at_path(&f.rel, p)))
                        .map(|f| f.rel),
                );
            }
        }
        Ok(())
    }

    /// Bring the indexes of `libs` up to date with the documents edited since:
    /// those that exist are written again, those that are gone are dropped.
    /// Done just before a full-text search, so saving a note stays cheap and
    /// the index only matters when it is used. Failures are left for a rebuild.
    pub(super) fn flush_dirty(&self, libs: &[Library]) {
        for l in libs {
            let dir = self.index_dir(&l.id);
            if !dir.join("smart-library-index.json").exists()
                || self.building.lock().unwrap().contains(&l.id)
            {
                continue; // no index to update yet, or a build is running; keep dirty paths for later
            }
            if l.kind == "notes" {
                if let Err(e) = self.sync_notes_index(l, &dir) {
                    eprintln!("failed to update note index for {}: {e}", l.id);
                }
                continue;
            }
            if l.kind != "folder" {
                continue;
            }
            let Ok(f) = self.folder(l) else { continue };
            let paths: HashSet<String> = match self.dirty.lock().unwrap().get_mut(&l.id) {
                Some(s) if !s.is_empty() => std::mem::take(s),
                _ => continue,
            };
            let result = (|| -> Result<(), String> {
                let overlay = self.props.overlay(&l.id)?;
                let all = f.files();
                let mut changed = paths.clone();
                let (version, folder_snap) = if let Some(old) = load_folder_snapshot(&dir) {
                    let version = f.fingerprint(&all);
                    let snap = folder_snapshot_map(&f, &all, &overlay);
                    let delta = FolderDelta::between(&old, &snap);
                    changed.extend(
                        delta
                            .added
                            .iter()
                            .chain(&delta.modified)
                            .chain(&delta.metadata)
                            .chain(&delta.removed)
                            .map(|rel| (*rel).to_owned()),
                    );
                    if old.files != snap.files || old.metas != snap.metas {
                        self.titles.write().unwrap().remove(&l.id);
                    }
                    (Some(version), Some(snap))
                } else {
                    // An older index has no baseline for external changes.
                    // Let the folder check still detect that it needs a rebuild.
                    (None, None)
                };
                let files: Vec<_> = all
                    .into_iter()
                    .filter(|file| changed.remove(&file.rel))
                    .collect();
                let removed: Vec<_> = changed.into_iter().collect();
                let docs = f.docs(&files).collect::<Result<Vec<_>, _>>()?;
                smart_library_core::search::sync_docs(
                    &dir,
                    docs.into_iter().map(Ok),
                    &removed,
                    version.as_deref(),
                    &overlay,
                )?;
                if let Some(snap) = folder_snap {
                    save_folder_snapshot(&dir, &snap);
                }
                self.indexes.write().unwrap().remove(&l.id); // opened again with the new documents
                Ok(())
            })();
            if result.is_err() {
                self.mark_dirty(&l.id, paths); // try again at the next search
            }
        }
    }

    pub(super) fn sync_notes_index(&self, lib: &Library, dir: &Path) -> Result<(), String> {
        let store = self.notes(lib)?;
        let since = SearchIndex::data_version(dir)
            .and_then(|version| version.strip_prefix("notes:")?.parse().ok());
        let Some(batch) = store.index_changes(since)? else {
            return Ok(());
        };
        let version = notes::index_version(batch.revision);
        let overlay = self.props.overlay(&lib.id)?;
        if let Some(changed) = batch.changed {
            let mut removed = changed.clone();
            let rows: Vec<_> = batch
                .rows
                .into_iter()
                .filter(|row| {
                    removed.remove(&row.id) || row.ancestors.iter().any(|id| changed.contains(id))
                })
                .collect();
            let removed: Vec<_> = removed.into_iter().map(|id| id.to_string()).collect();
            let docs = notes::docs(&rows, &lib.title).into_iter().map(Ok);
            smart_library_core::search::sync_docs(dir, docs, &removed, Some(&version), &overlay)?;
        } else {
            let docs = notes::docs(&batch.rows, &lib.title).into_iter();
            smart_library_core::search::replace_docs(dir, docs, &version, &overlay)?;
        }
        self.indexes.write().unwrap().remove(&lib.id);
        store.prune_index_changes(batch.revision)
    }

    pub(super) fn index(&self, id: &str) -> Option<Arc<SearchIndex>> {
        let lib = self.library(id).ok()?;
        if lib.kind == "zim"
            && (zim_version(&lib.source).ok()? != SearchIndex::data_version(&self.index_dir(id))?)
        {
            return None;
        }
        if let Some(i) = self.indexes.read().unwrap().get(id) {
            return Some(i.clone());
        }
        let idx = Arc::new(SearchIndex::open(&self.index_dir(id)).ok()?);
        self.indexes
            .write()
            .unwrap()
            .insert(id.to_string(), idx.clone());
        Some(idx)
    }
}

pub(super) fn index_overlay(state: &AppState, l: &Library) -> Result<props::Overlay, String> {
    let mut overlay = state.props.overlay(&l.id)?;
    if overlay.is_empty() || !matches!(l.kind.as_str(), "cbeta" | "html") {
        return Ok(overlay);
    }
    let nodes = navigation_nodes(state, l)?;
    overlay.set_nav_ancestors(navigation_ancestors(&nodes));
    Ok(overlay)
}

// ------------------------------------------------------------ index

#[derive(Serialize, Clone)]
struct IndexDoneDto {
    lib: String,
    ok: bool,
    message: String,
}

const FOLDER_SNAPSHOT_FILE: &str = "folder-snapshot.json";

#[derive(Serialize, Deserialize, Default)]
pub(super) struct FolderSnapshot {
    files: HashMap<String, (u64, u64)>,
    /// Directory relative path ("" for library root) -> (size, mtime) of its `.meta.json`.
    metas: HashMap<String, (u64, u64)>,
    #[serde(default)]
    properties: Option<std::collections::BTreeMap<String, Dims>>,
}

pub(super) fn folder_snapshot_map(
    f: &folder::FolderLibrary,
    files: &[folder::FolderFile],
    overlay: &props::Overlay,
) -> FolderSnapshot {
    let mut file_map = HashMap::with_capacity(files.len());
    let mut dirs = std::collections::BTreeSet::new();
    dirs.insert(String::new());
    for file in files {
        let mtime = f
            .abs(&file.rel)
            .ok()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        file_map.insert(file.rel.clone(), (file.size, mtime));
        let mut cur = file.rel.as_str();
        while let Some((dir, _)) = cur.rsplit_once('/') {
            if !dirs.insert(dir.to_string()) {
                break;
            }
            cur = dir;
        }
    }
    let mut meta_map = HashMap::new();
    for dir in dirs {
        let meta_path = if dir.is_empty() {
            f.root.join(folder::DIR_META_FILE)
        } else {
            f.root.join(&dir).join(folder::DIR_META_FILE)
        };
        if let Ok(m) = std::fs::metadata(&meta_path) {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            meta_map.insert(dir, (m.len(), mtime));
        }
    }
    FolderSnapshot {
        files: file_map,
        metas: meta_map,
        properties: Some(overlay.folder_properties()),
    }
}

fn changed_property_paths<'a>(
    old: Option<&'a std::collections::BTreeMap<String, Dims>>,
    cur: &'a std::collections::BTreeMap<String, Dims>,
) -> Vec<&'a str> {
    let Some(old) = old else {
        // Older snapshots did not record properties; only re-index paths that
        // actually have properties now (an empty map needs no re-index).
        return cur.keys().map(String::as_str).collect();
    };
    cur.iter()
        .filter(|(path, dims)| old.get(*path) != Some(*dims))
        .map(|(path, _)| path.as_str())
        .chain(
            old.keys()
                .filter(|path| !cur.contains_key(*path))
                .map(String::as_str),
        )
        .collect()
}

fn file_at_path(file: &str, path: &str) -> bool {
    file == path || file_in_dir(file, path)
}

fn changed_meta_dirs(old: &FolderSnapshot, cur: &FolderSnapshot) -> Vec<String> {
    let mut dirs = Vec::new();
    for (dir, stamp) in &cur.metas {
        if old.metas.get(dir) != Some(stamp) {
            dirs.push(dir.clone());
        }
    }
    for dir in old.metas.keys() {
        if !cur.metas.contains_key(dir) {
            dirs.push(dir.clone());
        }
    }
    dirs
}

fn file_in_dir(file_rel: &str, dir: &str) -> bool {
    if dir.is_empty() {
        return true;
    }
    file_rel.starts_with(dir) && file_rel.as_bytes().get(dir.len()) == Some(&b'/')
}

#[derive(Default)]
struct FolderDelta<'a> {
    added: HashSet<&'a str>,
    modified: HashSet<&'a str>,
    metadata: HashSet<&'a str>,
    removed: HashSet<&'a str>,
}

impl<'a> FolderDelta<'a> {
    fn between(old: &'a FolderSnapshot, current: &'a FolderSnapshot) -> Self {
        let mut delta = Self::default();
        let meta_dirs = changed_meta_dirs(old, current);
        let prop_paths = current
            .properties
            .as_ref()
            .map(|props| changed_property_paths(old.properties.as_ref(), props))
            .unwrap_or_default();
        for (rel, stamp) in &current.files {
            match old.files.get(rel) {
                None => {
                    delta.added.insert(rel);
                }
                Some(indexed) if indexed != stamp => {
                    delta.modified.insert(rel);
                }
                _ if meta_dirs.iter().any(|dir| file_in_dir(rel, dir))
                    || prop_paths.iter().any(|path| file_at_path(rel, path)) =>
                {
                    delta.metadata.insert(rel);
                }
                _ => {}
            }
        }
        delta.removed = old
            .files
            .keys()
            .filter(|path| !current.files.contains_key(*path))
            .map(String::as_str)
            .collect();
        delta
    }
}

pub(super) fn save_folder_snapshot(index_dir: &Path, snap: &FolderSnapshot) {
    if let Ok(bytes) = serde_json::to_vec(snap) {
        let _ = std::fs::write(index_dir.join(FOLDER_SNAPSHOT_FILE), bytes);
    }
}

fn load_folder_snapshot(index_dir: &Path) -> Option<FolderSnapshot> {
    let bytes = std::fs::read(index_dir.join(FOLDER_SNAPSHOT_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Serialize)]
pub struct FolderChangeDto {
    lib: String,
    title: String,
    added: usize,
    modified: usize,
    removed: usize,
    has_snapshot: bool,
}

/// Check all indexed local folder libraries for external file changes (add/modify/delete or `.meta.json`).
/// Returns libraries whose current disk fingerprint differs from their index's `data_version`,
/// along with counts of added, modified, and removed documents when a previous snapshot is available.
#[tauri::command]
pub async fn check_folder_changes(app: AppHandle) -> Result<Vec<FolderChangeDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let libs: Vec<Library> = state
            .libraries
            .read()
            .unwrap()
            .iter()
            .filter(|l| l.kind == "folder")
            .cloned()
            .collect();
        let mut changed = Vec::new();
        for l in libs {
            let dir = state.index_dir(&l.id);
            if !dir.join("smart-library-index.json").exists() {
                continue;
            }
            if state.building.lock().unwrap().contains(&l.id) {
                continue;
            }
            // Flush any pending in-app edits first so their fingerprint matches the index
            state.flush_dirty(std::slice::from_ref(&l));
            let indexed_ver = SearchIndex::data_version(&dir);
            let Ok(f) = state.folder(&l) else { continue };
            let files = f.files();
            let cur_ver = f.fingerprint(&files);
            let overlay = state.props.overlay(&l.id)?;
            let old_snapshot = load_folder_snapshot(&dir);
            let properties = overlay.folder_properties();
            let props_changed = old_snapshot
                .as_ref()
                .is_some_and(|snap| !changed_property_paths(snap.properties.as_ref(), &properties).is_empty());
            if indexed_ver.as_deref() != Some(cur_ver.as_str()) || props_changed {
                let (added, modified, removed, has_snapshot) = match old_snapshot {
                    Some(old) => {
                        let cur = folder_snapshot_map(&f, &files, &overlay);
                        let delta = FolderDelta::between(&old, &cur);
                        (
                            delta.added.len(),
                            delta.modified.len() + delta.metadata.len(),
                            delta.removed.len(),
                            true,
                        )
                    }
                    None => (0, 0, 0, false),
                };
                state.titles.write().unwrap().remove(&l.id);
                changed.push(FolderChangeDto {
                    lib: l.id,
                    title: l.title,
                    added,
                    modified,
                    removed,
                    has_snapshot,
                });
            }
        }
        Ok(changed)
    })
    .await
    .map_err(|e| e.to_string())?
}

const INCREMENTAL_INDEX_MAX_CHANGES: usize = 200;

enum IndexSource {
    Cbeta(Arc<Bookcase>),
    Html(Arc<Bookcase>, Arc<HtmlTree>),
    Folder(Arc<FolderLibrary>),
    Notes(Arc<NotesStore>),
    Zim(Arc<Mutex<smart_library_core::zim::Zim>>),
}

impl IndexSource {
    fn open(state: &AppState, lib: &Library) -> Result<Self, String> {
        match lib.kind.as_str() {
            "cbeta" => Ok(Self::Cbeta(state.bookcase_of(lib)?)),
            "html" => Ok(Self::Html(state.bookcase_of(lib)?, state.html_tree(lib)?)),
            "folder" => {
                let folder = state.folder(lib)?;
                if !folder.root.is_dir() {
                    return Err(format!("文件夹不存在：{}", folder.root.display()));
                }
                Ok(Self::Folder(folder))
            }
            "notes" => Ok(Self::Notes(state.notes(lib)?)),
            "zim" => Ok(Self::Zim(state.zim(lib)?)),
            _ => Err("没有可索引的数据源".into()),
        }
    }
}

/// Build (or rebuild) the index of a library on a background thread. Emits
/// `index-progress` {lib, done, total} and finally `index-done` {lib, ok, message}.
/// When `incremental` is true for a folder library that already has an index and snapshot,
/// updates only the added/modified/removed documents if the change count is within
/// `INCREMENTAL_INDEX_MAX_CHANGES`, otherwise falls back to a full rebuild.
#[tauri::command]
pub async fn build_index(
    lib: String,
    incremental: Option<bool>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let l = state.library(&lib)?;
    let source = IndexSource::open(&state, &l)?;
    let zim_stamp = if matches!(&source, IndexSource::Zim(_)) {
        zim_version(&l.source)?
    } else {
        String::new()
    };
    let lib_title = l.title.clone();
    state.titles.write().unwrap().remove(&lib);
    let overlay = index_overlay(&state, &l)?;
    if !state.building.lock().unwrap().insert(lib.clone()) {
        return Err("正在建立这个知识库的索引".to_string());
    }
    let dir = state.index_dir(&lib);
    let try_incremental = incremental.unwrap_or(false);
    let pending = state.dirty.lock().unwrap().remove(&lib).unwrap_or_default();
    // drop the open index before deleting or updating its files
    state.indexes.write().unwrap().remove(&lib);
    std::thread::spawn(move || {
        let result = (|| -> Result<String, String> {
            let app2 = app.clone();
            let lib2 = lib.clone();
            let progress = move |done, total| {
                let _ = app2.emit(
                    "index-progress",
                    ProgressDto {
                        lib: lib2.clone(),
                        done,
                        total,
                        stage: None,
                    },
                );
            };
            if try_incremental {
                if let IndexSource::Folder(f) = &source {
                    if dir.join("smart-library-index.json").exists() {
                        if let Some(old_snap) = load_folder_snapshot(&dir) {
                            let files = f.files();
                            let cur_snap = folder_snapshot_map(f, &files, &overlay);
                            let cur_ver = f.fingerprint(&files);
                            let mut delta = FolderDelta::between(&old_snap, &cur_snap);
                            let changed_files: Vec<_> = files
                                .iter()
                                .filter(|file| {
                                    let path = file.rel.as_str();
                                    if delta.added.contains(path)
                                        || delta.modified.contains(path)
                                        || delta.metadata.contains(path)
                                    {
                                        true
                                    } else if pending.contains(path) {
                                        delta.modified.insert(path);
                                        true
                                    } else {
                                        false
                                    }
                                })
                                .cloned()
                                .collect();
                            let added_count = delta.added.len();
                            let modified_count = delta.modified.len();
                            let meta_affected_count = delta.metadata.len();
                            let removed: Vec<_> =
                                delta.removed.into_iter().map(str::to_owned).collect();
                            let total_changes = changed_files.len() + removed.len();
                            if total_changes == 0 {
                                if SearchIndex::data_version(&dir).as_deref()
                                    == Some(cur_ver.as_str())
                                {
                                    save_folder_snapshot(&dir, &cur_snap);
                                    return Ok("索引已是最新".into());
                                }
                                // Fingerprint changed with no affected files; fall through to full rebuild.
                            } else if total_changes <= INCREMENTAL_INDEX_MAX_CHANGES {
                                progress(0, total_changes);
                                let start = std::time::Instant::now();
                                if smart_library_core::search::sync_docs(
                                    &dir,
                                    f.docs(&changed_files),
                                    &removed,
                                    Some(&cur_ver),
                                    &overlay,
                                )
                                .is_ok()
                                {
                                    save_folder_snapshot(&dir, &cur_snap);
                                    progress(total_changes, total_changes);
                                    let mut parts = Vec::new();
                                    if added_count > 0 {
                                        parts.push(format!("新增 {} 篇", added_count));
                                    }
                                    if modified_count > 0 {
                                        parts.push(format!("修改 {} 篇", modified_count));
                                    }
                                    if meta_affected_count > 0 {
                                        parts
                                            .push(format!("元数据更新 {} 篇", meta_affected_count));
                                    }
                                    if !removed.is_empty() {
                                        parts.push(format!("删除 {} 篇", removed.len()));
                                    }
                                    return Ok(format!(
                                        "已增量更新索引（{}），用时 {:.1} 秒",
                                        parts.join("、"),
                                        start.elapsed().as_secs_f64()
                                    ));
                                }
                                // Incremental sync failed (e.g. old index version); fall through to full rebuild.
                            }
                        }
                    }
                }
            }
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| format!("无法删除旧索引：{}", e))?;
            }
            let stats = match &source {
                IndexSource::Zim(archive) => {
                    let mut z = archive.lock().unwrap();
                    let jobs = z.article_jobs();
                    let total = jobs.len();
                    let docs = jobs.into_iter().map(|i| z.index_doc(i));
                    smart_library_core::search::build_index(
                        &dir,
                        serde_json::json!({"kind": "zim", "data_version": zim_stamp}),
                        total,
                        docs,
                        &overlay,
                        progress,
                    )?
                }
                IndexSource::Notes(n) => {
                    let batch = n
                        .index_changes(None)?
                        .expect("an index build reads every note");
                    let docs = notes::docs(&batch.rows, &lib_title);
                    let meta = serde_json::json!({ "kind": "notes", "data_version": notes::index_version(batch.revision) });
                    let stats = smart_library_core::search::build_index(
                        &dir,
                        meta,
                        docs.len(),
                        docs.into_iter().map(Ok),
                        &overlay,
                        progress,
                    )?;
                    let _ = n.prune_index_changes(batch.revision);
                    stats
                }
                IndexSource::Folder(f) => {
                    let files = f.files();
                    let meta = serde_json::json!({
                        "kind": "folder",
                        "data_version": f.fingerprint(&files),
                    });
                    let stats = smart_library_core::search::build_index(
                        &dir,
                        meta,
                        files.len(),
                        f.docs(&files),
                        &overlay,
                        progress,
                    )?;
                    save_folder_snapshot(&dir, &folder_snapshot_map(f, &files, &overlay));
                    stats
                }
                IndexSource::Html(bc, t) => {
                    SearchIndex::build_html(bc, t, &dir, &overlay, progress)?
                }
                IndexSource::Cbeta(bc) => SearchIndex::build(bc, &dir, &overlay, progress)?,
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
        if result.is_err() && !pending.is_empty() {
            state.mark_dirty(&lib, pending);
        }
        state.building.lock().unwrap().remove(&lib);
        let done = match result {
            Ok(message) => IndexDoneDto {
                lib: lib.clone(),
                ok: true,
                message,
            },
            Err(message) => IndexDoneDto {
                lib: lib.clone(),
                ok: false,
                message,
            },
        };
        let _ = app.emit("index-done", done);
    });
    Ok(())
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
    aliases: Option<Vec<String>>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PropsSaved, String> {
    let l = state.library(&lib)?;
    if l.kind == "folder" {
        state.folder(&l)?;
    }
    state.props.set(&lib, book_id.as_deref(), &path, &dims)?;
    if let Some(names) = aliases {
        state
            .props
            .set_aliases(&lib, book_id.as_deref(), &path, &names)?;
        state.titles.write().unwrap().remove(&lib); // the quick search reads them again
    }
    state.sidecar_sync(&lib);
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
        Cbeta(Arc<Bookcase>, Option<HashSet<String>>),
        Html(Arc<Bookcase>, Arc<HtmlTree>, Option<HashSet<String>>),
        Folder(Arc<FolderLibrary>, Vec<folder::FolderFile>, String),
        Notes(Arc<NotesStore>, Option<HashSet<i64>>),
        Zim(Arc<Mutex<smart_library_core::zim::Zim>>, Vec<usize>),
    }
    let job = match IndexSource::open(&state, &l)? {
        IndexSource::Cbeta(bc) => Job::Cbeta(
            bc,
            if path.starts_with("@nav/") {
                Some(nav_targets(&state, &l, &path)?)
            } else if let Some(b) = &book_id {
                Some(nav_document_targets(&state, &l, b)?)
            } else {
                None
            },
        ),
        IndexSource::Html(bc, tree) => Job::Html(
            bc,
            tree,
            if path.is_empty() {
                None
            } else if path.starts_with("@nav/") {
                Some(nav_targets(&state, &l, &path)?)
            } else {
                Some(nav_document_targets(&state, &l, &path)?)
            },
        ),
        IndexSource::Folder(f) => {
            let files: Vec<_> = f
                .files()
                .into_iter()
                .filter(|file| file_at_path(&file.rel, &path))
                .collect();
            Job::Folder(f, files, path.clone())
        }
        IndexSource::Notes(store) => {
            let ids = if path.is_empty() {
                None
            } else {
                Some(store.descendants(note_id(&path)?)?.into_iter().collect())
            };
            Job::Notes(store, ids)
        }
        IndexSource::Zim(z) => {
            let jobs = {
                let locked = z.lock().unwrap();
                if path.is_empty() {
                    locked.article_jobs()
                } else {
                    vec![locked.entry_index(&path).ok_or("ZIM 文章不存在")?]
                }
            };
            Job::Zim(z, jobs)
        }
    };
    let overlay = index_overlay(&state, &l)?;
    let lib_title = l.title.clone();
    state.building.lock().unwrap().insert(lib.clone());
    state.indexes.write().unwrap().remove(&lib);
    std::thread::spawn(move || {
        let result = (|| -> Result<usize, String> {
            use smart_library_core::search::update_docs;
            match &job {
                Job::Cbeta(bc, books) => update_docs(
                    &dir,
                    smart_library_core::library::cbeta_docs(bc, books.as_ref()).1,
                    &overlay,
                ),
                Job::Html(bc, t, paths) => {
                    update_docs(&dir, t.docs(bc, paths.as_ref()).1, &overlay)
                }
                Job::Folder(f, files, prop_path) => {
                    let n = update_docs(&dir, f.docs(files), &overlay)?;
                    if let Some(mut snap) = load_folder_snapshot(&dir) {
                        let cur_props = overlay.folder_properties();
                        if prop_path.is_empty() {
                            snap.properties = Some(cur_props);
                            save_folder_snapshot(&dir, &snap);
                        } else if let Some(props) = snap.properties.as_mut() {
                            if let Some(d) = cur_props.get(prop_path) {
                                props.insert(prop_path.clone(), d.clone());
                            } else {
                                props.remove(prop_path);
                            }
                            save_folder_snapshot(&dir, &snap);
                        }
                    }
                    Ok(n)
                }
                Job::Notes(store, ids) => {
                    let rows: Vec<_> = store
                        .rows()?
                        .into_iter()
                        .filter(|r| ids.as_ref().is_none_or(|ids| ids.contains(&r.id)))
                        .collect();
                    update_docs(
                        &dir,
                        notes::docs(&rows, &lib_title).into_iter().map(Ok),
                        &overlay,
                    )
                }
                Job::Zim(z, jobs) => {
                    let mut z = z.lock().unwrap();
                    update_docs(&dir, jobs.iter().map(|&i| z.index_doc(i)), &overlay)
                }
            }
        })();
        let state = app.state::<AppState>();
        state.building.lock().unwrap().remove(&lib);
        state.indexes.write().unwrap().remove(&lib); // opened again with the new documents
        let done = match result {
            Ok(n) => IndexDoneDto {
                lib: lib.clone(),
                ok: true,
                message: format!("属性已更新到索引（{} 篇）", n),
            },
            Err(e) => IndexDoneDto {
                lib: lib.clone(),
                ok: false,
                message: format!("更新索引失败：{}，可以重建索引", e),
            },
        };
        let _ = app.emit("index-done", done);
    });
    Ok(PropsSaved { updating: 1 })
}
