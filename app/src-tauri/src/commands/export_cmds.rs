use super::*;

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
    /// `"smartdir"` for local-folder packages; empty or `"smartlib"` for standard packages
    #[serde(default)]
    format: String,
    /// display title for `.smartdir` (node / parent / library title)
    #[serde(default)]
    title: String,
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
            ProgressDto { lib: self.lib.id.clone(), done: self.done, total: self.total.max(self.done), stage: None },
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
    if req.format == "smartdir" {
        if l.kind != "folder" {
            return Err("只有本地文件夹类型的知识库可以导出为 .smartdir".into());
        }
        let export_title = if !req.title.trim().is_empty() {
            req.title.trim().to_string()
        } else if !req.whole && req.rels.len() == 1 {
            req.rels[0].rsplit('/').next().unwrap_or(&l.title).to_string()
        } else {
            l.title.clone()
        };
        let lib_id = l.id.clone();
        let rels = req.rels;
        let whole = req.whole;
        let out = req.out;
        let app2 = app.clone();
        let rep = tauri::async_runtime::spawn_blocking(move || {
            let state = app2.state::<AppState>();
            let f = state.folder(&l)?;
            smart_library_core::exchange::export_folder_dir(
                &f,
                &export_title,
                &rels,
                whole,
                Path::new(&out),
                &mut |done, total, stage| {
                    let _ = app2.emit(
                        "export-progress",
                        ProgressDto {
                            lib: lib_id.clone(),
                            done,
                            total: total.max(done),
                            stage: (!stage.is_empty()).then(|| stage.to_string()),
                        },
                    );
                },
            )
            .map(|rep| (out, rep))
        })
        .await
        .map_err(|e| e.to_string())??;
        return Ok(ExportDone { path: rep.0, docs: rep.1.docs, files: rep.1.files });
    }
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
    let with_files = req.with_files;
    let out = req.out;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let w = smart_library_core::exchange::PackageWriter::create(Path::new(&out))?;
        let mut ex = Exporter { state: &state, lib: l.clone(), w, with_files, done: 0, files: 0, total: 0, app: app2.clone(), attached: HashSet::new() };
        ex.total = plans.iter().map(|p| ex.count(p)).sum();
        let mut nodes = Vec::new();
        for p in &plans {
            nodes.push(ex.node(p)?);
        }
        let manifest = smart_library_core::exchange::manifest(&l.title, &l.kind, &l.source, if whole { "library" } else { "nodes" }, nodes);
        let (docs, files) = (ex.done, ex.files);
        let _ = app2.emit(
            "export-progress",
            ProgressDto {
                lib: l.id.clone(),
                done: docs,
                total: ex.total.max(docs),
                stage: Some("正在写入压缩包，请稍候…".into()),
            },
        );
        ex.w.finish(&manifest)?;
        Ok(ExportDone { path: out, docs, files })
    })
    .await
    .map_err(|e| e.to_string())?
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
pub struct PackageInfo {
    kind: String,
    title: String,
    scope: String,
    files: usize,
    exts: Vec<String>,
    max_mb: u64,
    default_dest: String,
}

fn default_import_dir(data_dir: &Path, title: &str) -> String {
    let raw: String = title
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = raw.trim().trim_end_matches(['.', ' ']);
    let base_name = if trimmed.is_empty() || folder::check_name(trimmed).is_err() {
        "imported"
    } else {
        trimmed
    };
    let root = data_dir.join("folders");
    let mut cand = root.join(base_name);
    let mut n = 2usize;
    while cand.exists() {
        cand = root.join(format!("{}-{}", base_name, n));
        n += 1;
    }
    cand.to_string_lossy().into_owned()
}

#[tauri::command]
pub async fn inspect_package(path: String, state: State<'_, AppState>) -> Result<PackageInfo, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pkg = smart_library_core::exchange::Package::open(Path::new(&path))?;
        let is_dir = pkg.is_dir_package();
        let filter = pkg.dir_filter();
        let title = pkg.title();
        let default_dest = if is_dir { default_import_dir(&data_dir, &title) } else { String::new() };
        Ok(PackageInfo {
            kind: if is_dir { "smartdir".into() } else { "smartlib".into() },
            title,
            scope: pkg.manifest["scope"].as_str().unwrap_or("library").to_string(),
            files: if is_dir { pkg.data_file_count() } else { 0 },
            exts: filter.exts,
            max_mb: filter.max_mb,
            default_dest,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct ImportDone {
    status: StatusDto,
    title: String,
    notes: usize,
}

fn unique_library_title(state: &AppState, base: &str) -> String {
    let libs = state.libraries.read().unwrap();
    let mut t = base.to_string();
    let mut n = 1;
    while libs.iter().any(|l| l.title == t) {
        n += 1;
        t = if n == 2 { format!("{}（导入）", base) } else { format!("{}（导入 {}）", base, n) };
    }
    t
}

/// Read an export package (`.smartlib` or `.smartdir`): into a new internal library, or (`lib`, and
/// `parent` a note id or none for the top) below a place of an internal one.
#[tauri::command]
pub async fn import_package(
    path: String,
    lib: Option<String>,
    parent: Option<String>,
    title: Option<String>,
    library_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ImportDone, String> {
    let mut pkg = tauri::async_runtime::spawn_blocking({
        let path = path.clone();
        move || smart_library_core::exchange::Package::open(Path::new(&path))
    })
    .await
    .map_err(|e| e.to_string())??;
    let is_dir = pkg.is_dir_package();
    if let Some(lib_id) = lib.filter(|l| !l.is_empty()) {
        let parent = match parent.filter(|p| !p.is_empty()) {
            Some(p) => Some(note_id(&p)?),
            None => None,
        };
        let app2 = app.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let state = app2.state::<AppState>();
            let store = notes_of(&state, &lib_id)?;
            // a whole library comes as one note named like it; picked nodes come as they are
            let wrap = pkg.manifest["scope"] == "library";
            let dir = state.attachments_dir(&lib_id);
            let notes = if is_dir {
                smart_library_core::exchange::import_dir_into_notes(
                    &store,
                    &mut pkg,
                    parent,
                    wrap,
                    &mut |name, bytes| keep_picture(&dir, name, &bytes),
                    &mut |id, dims| {
                        let _ = state.props.set(&lib_id, None, &id.to_string(), dims);
                    },
                )?
            } else {
                smart_library_core::exchange::import_into(
                    &store,
                    &mut pkg,
                    parent,
                    wrap,
                    &mut |name, bytes| keep_picture(&dir, name, &bytes),
                    &mut |id, dims| {
                        let _ = state.props.set(&lib_id, None, &id.to_string(), dims);
                    },
                )?
            };
            Ok(ImportDone { status: status_of(&state), title: state.library(&lib_id)?.title, notes })
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let base = title.filter(|t| !t.trim().is_empty()).map(|t| t.trim().to_string()).unwrap_or_else(|| pkg.title());
    let title = unique_library_title(&state, &base);
    let _ = create_notes_library(title.clone(), library_id, state.clone()).await?;
    let lib = state
        .libraries
        .read()
        .unwrap()
        .iter()
        .find(|l| l.title == title)
        .cloned()
        .ok_or("知识库没有建好")?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let store = state.notes(&lib)?;
        let dir = state.attachments_dir(&lib.id);
        let lib_id = lib.id.clone();
        let notes = if is_dir {
            smart_library_core::exchange::import_dir_into_notes(
                &store,
                &mut pkg,
                None,
                false,
                &mut |name, bytes| keep_picture(&dir, name, &bytes),
                &mut |id, dims| {
                    let _ = state.props.set(&lib_id, None, &id.to_string(), dims);
                },
            )?
        } else {
            smart_library_core::exchange::import_into(
                &store,
                &mut pkg,
                None,
                false,
                &mut |name, bytes| keep_picture(&dir, name, &bytes),
                &mut |id, dims| {
                    let _ = state.props.set(&lib_id, None, &id.to_string(), dims);
                },
            )?
        };
        state.titles.write().unwrap().remove(&lib.id);
        Ok(ImportDone { status: status_of(&state), title, notes })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Extract a `.smartdir` package into an empty local directory and bind it as a local folder library.
#[tauri::command]
pub async fn import_dir_as_folder(
    path: String,
    dest: String,
    title: Option<String>,
    writable: Option<bool>,
    library_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<ImportDone, String> {
    let dest_trimmed = dest.trim().trim_matches('"');
    if dest_trimmed.is_empty() {
        return Err("请指定目标空文件夹".into());
    }
    let dest_path = Path::new(dest_trimmed);
    let created = !dest_path.exists();
    if created {
        std::fs::create_dir_all(dest_path).map_err(|e| format!("无法创建目标文件夹 {}：{}", dest_trimmed, e))?;
    }
    let canon = match folder::canonical(dest_trimmed) {
        Ok(c) => c,
        Err(e) => {
            if created {
                let _ = std::fs::remove_dir(dest_path);
            }
            return Err(e);
        }
    };
    let check_res: Result<(), String> = {
        let libs = state.libraries.read().unwrap();
        let existing: Vec<&str> = libs.iter().filter(|l| l.kind == "folder").map(|l| l.source.as_str()).collect();
        if let Some(c) = folder::conflict(&existing, &canon) {
            Err(c)
        } else {
            super::library_id(library_id.as_deref(), "folder", &libs).map(|_| ())
        }
    };
    if let Err(e) = check_res {
        if created {
            let _ = std::fs::remove_dir(&canon);
        }
        return Err(e);
    }
    let extracted = tauri::async_runtime::spawn_blocking({
        let path = path.clone();
        let canon = canon.clone();
        move || {
            let mut pkg = smart_library_core::exchange::Package::open(Path::new(&path))?;
            if !pkg.is_dir_package() {
                return Err("只有 .smartdir 文件夹数据包可以导入为本地文件夹知识库".into());
            }
            let pkg_title = pkg.title();
            let filter = pkg.dir_filter();
            let count = smart_library_core::exchange::extract_dir_package(&mut pkg, Path::new(&canon))?;
            Ok::<_, String>((count, pkg_title, filter))
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    let (count, pkg_title, filter) = match extracted {
        Ok(v) => v,
        Err(e) => {
            if created {
                let _ = std::fs::remove_dir_all(&canon);
            }
            return Err(e);
        }
    };
    let base = title.filter(|t| !t.trim().is_empty()).map(|t| t.trim().to_string()).unwrap_or(pkg_title);
    let lib_title = unique_library_title(&state, &base);
    let exts = filter.exts.join(" ");
    let want_writable = writable.unwrap_or(false);
    let _ = add_folder(canon.clone(), Some(lib_title.clone()), exts, filter.max_mb, Some(want_writable), library_id, state.clone()).await?;
    if want_writable {
        let added = state.libraries.read().unwrap().iter().find(|l| l.source == canon).cloned();
        if let Some(l) = added {
            let _ = state.sidecar_enable(&l);
        }
    }
    Ok(ImportDone { status: status_of(&state), title: lib_title, notes: count })
}
