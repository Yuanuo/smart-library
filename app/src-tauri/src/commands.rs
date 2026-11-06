//! Tauri commands. Each one converts arguments, calls `smart-library-core`
//! and returns plain serializable structs (DTOs).
//!
//! Long work (opening the bookcase, searching, reading a volume) runs in
//! `async` commands, so it never blocks the window's main thread; building
//! the index runs on its own thread and reports progress with events.

use serde::Serialize;
use smart_library_core::bookcase::Bookcase;
use smart_library_core::nav::NavNode;
use smart_library_core::search::{
    facet_label, locate, snippets, MatchMode, SearchIndex, SearchRequest,
};
use smart_library_core::source::{DirSource, Source, ZipSource};
use smart_library_core::toc::TocItem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    config_file: PathBuf,
    index_dir: PathBuf,
    bookcase_path: RwLock<Option<String>>,
    bookcase: RwLock<Option<Arc<Bookcase>>>,
    index: RwLock<Option<Arc<SearchIndex>>>,
    building: AtomicBool,
}

impl AppState {
    pub fn new(config_file: PathBuf, index_dir: PathBuf) -> Self {
        // bookcase path: config file, else the SMART_LIBRARY_BOOKCASE variable
        let configured = std::fs::read_to_string(&config_file)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["bookcase"].as_str().map(|s| s.to_string()))
            .or_else(|| std::env::var("SMART_LIBRARY_BOOKCASE").ok());
        AppState {
            config_file,
            index_dir,
            bookcase_path: RwLock::new(configured),
            bookcase: RwLock::new(None),
            index: RwLock::new(None),
            building: AtomicBool::new(false),
        }
    }

    fn bookcase(&self) -> Result<Arc<Bookcase>, String> {
        if let Some(bc) = self.bookcase.read().unwrap().as_ref() {
            return Ok(bc.clone());
        }
        let path = self
            .bookcase_path
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| "尚未指定 bookcase.zip".to_string())?;
        let bc = Arc::new(open_bookcase_at(&path)?);
        *self.bookcase.write().unwrap() = Some(bc.clone());
        Ok(bc)
    }

    fn index(&self) -> Option<Arc<SearchIndex>> {
        if let Some(i) = self.index.read().unwrap().as_ref() {
            return Some(i.clone());
        }
        let idx = Arc::new(SearchIndex::open(&self.index_dir).ok()?);
        *self.index.write().unwrap() = Some(idx.clone());
        Some(idx)
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

// ------------------------------------------------------------ status

#[derive(Serialize)]
pub struct TreeDto {
    id: String,
    title: String,
}

#[derive(Serialize)]
pub struct StatusDto {
    bookcase_path: Option<String>,
    /// error while opening the configured bookcase
    error: Option<String>,
    data_version: Option<String>,
    version: Option<String>,
    trees: Vec<TreeDto>,
    default_tree: Option<String>,
    index_ready: bool,
    /// data version the index was built for (may differ from the bookcase)
    index_data_version: Option<String>,
    index_dir: String,
    building: bool,
}

fn status_of(state: &AppState) -> StatusDto {
    let path = state.bookcase_path.read().unwrap().clone();
    let (bc, error) = match &path {
        Some(_) => match state.bookcase() {
            Ok(bc) => (Some(bc), None),
            Err(e) => (None, Some(e)),
        },
        None => (None, None),
    };
    let trees: Vec<TreeDto> = bc
        .as_ref()
        .map(|b| {
            b.trees
                .iter()
                .map(|t| TreeDto {
                    id: t.id.clone(),
                    title: t.title.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    // the CBETA trees can be read now; the user-made `a/` trees cannot yet
    let default_tree = trees
        .iter()
        .find(|t| t.id == "simple")
        .or(trees.first())
        .map(|t| t.id.clone());
    StatusDto {
        bookcase_path: path,
        error,
        data_version: bc.as_ref().map(|b| b.data_version()),
        version: bc.as_ref().map(|b| b.version.clone()),
        trees,
        default_tree,
        index_ready: state.index().is_some(),
        index_data_version: SearchIndex::data_version(&state.index_dir),
        index_dir: state.index_dir.display().to_string(),
        building: state.building.load(Ordering::SeqCst),
    }
}

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> Result<StatusDto, String> {
    Ok(status_of(&state))
}

#[tauri::command]
pub async fn open_bookcase(path: String, state: State<'_, AppState>) -> Result<StatusDto, String> {
    let path = path.trim().trim_matches('"').to_string();
    let bc = open_bookcase_at(&path)?;
    *state.bookcase.write().unwrap() = Some(Arc::new(bc));
    *state.bookcase_path.write().unwrap() = Some(path.clone());
    if let Some(dir) = state.config_file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let config = serde_json::json!({ "bookcase": path });
    std::fs::write(&state.config_file, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| format!("保存设置失败：{}", e))?;
    Ok(status_of(&state))
}

// ------------------------------------------------------------ tree

#[derive(Serialize)]
pub struct NodeDto {
    title: String,
    /// resolved book, if the leaf points to one
    book_id: Option<String>,
    /// first volume of a part of a big book (`T0220a`)
    start: Option<String>,
    /// original link (for leaves that cannot be read yet, e.g. `a/…`)
    href: Option<String>,
    children: Vec<NodeDto>,
}

fn node_dto(bc: &Bookcase, n: &NavNode) -> NodeDto {
    match n {
        NavNode::Folder { title, children } => NodeDto {
            title: title.clone(),
            book_id: None,
            start: None,
            href: None,
            children: children.iter().map(|c| node_dto(bc, c)).collect(),
        },
        NavNode::Leaf {
            href, text, children, ..
        } => {
            let entry = bc.resolve_leaf(href, text);
            NodeDto {
                title: match &entry {
                    Some(e) if !e.cloned => format!("{} {}", e.book_id, e.title),
                    Some(e) => e.title.clone(),
                    None => text.clone(),
                },
                book_id: entry.as_ref().map(|e| e.book_id.clone()),
                start: entry.as_ref().and_then(|e| e.start.clone()),
                href: Some(href.clone()),
                children: children.iter().map(|c| node_dto(bc, c)).collect(),
            }
        }
    }
}

#[tauri::command]
pub async fn tree(id: String, state: State<'_, AppState>) -> Result<Vec<NodeDto>, String> {
    let bc = state.bookcase()?;
    let doc = bc.nav(&id)?;
    Ok(doc.items.iter().map(|n| node_dto(&bc, n)).collect())
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
    /// line to jump to, e.g. `0001c18`
    lb: Option<String>,
    depth: usize,
}

#[derive(Serialize)]
pub struct ReaderDto {
    book_id: String,
    title: String,
    author: String,
    path: String,
    juan: u16,
    label: String,
    volumes: Vec<VolumeItemDto>,
    chapters: Vec<ChapterDto>,
    lines: Vec<LineDto>,
    /// number of hits of the query in this volume
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

#[tauri::command]
pub async fn open_volume(
    book_id: String,
    path: Option<String>,
    query: Option<String>,
    phrase: Option<bool>,
    state: State<'_, AppState>,
) -> Result<ReaderDto, String> {
    let bc = state.bookcase()?;
    let book = bc.book(&book_id).ok_or_else(|| format!("没有这部经：{}", book_id))?;
    let vols = bc.volumes(&book_id);
    let vol = match &path {
        Some(p) => vols.iter().find(|v| &v.path == p),
        None => vols.iter().find(|v| v.exists),
    }
    .ok_or_else(|| format!("{} 没有可读的卷", book_id))?;
    // a volume that cannot be read is skipped elsewhere; here we say why
    let vt = bc.volume_text(&vol.path)?;

    let text: Vec<char> = vt.text.chars().collect();
    let mut marks = vec![false; text.len()];
    let mut hits = 0;
    let mut first_hit_lb = None;
    if let Some(q) = query.as_deref().filter(|q| !q.trim().is_empty()) {
        let mode = if phrase.unwrap_or(false) { MatchMode::Phrase } else { MatchMode::Loose };
        let occ = locate(&vt, q, mode);
        hits = occ.len();
        first_hit_lb = occ.first().map(|o| o.lb.clone());
        for o in &occ {
            let from = vt.plain_to_text[o.from] as usize;
            let to = vt.plain_to_text[o.to - 1] as usize;
            for m in &mut marks[from..=to] {
                *m = true;
            }
        }
    }
    let lines = vt
        .line_spans()
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
        .collect();

    let mut chapters = Vec::new();
    if let Some(toc) = bc.outline(&book_id) {
        flatten_chapters(&toc.chapters, 0, &mut chapters);
    }
    Ok(ReaderDto {
        book_id: book_id.clone(),
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
        hits,
        first_hit_lb,
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
    book_id: String,
    title: String,
    juan: u16,
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
pub struct SearchDto {
    total_volumes: usize,
    total_books: usize,
    page: usize,
    pages: usize,
    millis: f64,
    facets: Vec<FacetDto>,
    hits: Vec<HitDto>,
}

#[tauri::command]
pub async fn search(
    query: String,
    phrase: bool,
    scope: Option<String>,
    drill: Option<String>,
    page: Option<usize>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<SearchDto, String> {
    let bc = state.bookcase()?;
    let idx = state.index().ok_or_else(|| "还没有索引，请先建立索引".to_string())?;
    let start = std::time::Instant::now();
    let mut req = SearchRequest::new(&query);
    req.mode = if phrase { MatchMode::Phrase } else { MatchMode::Loose };
    req.limit = limit.unwrap_or(20).clamp(1, 100);
    let page = page.unwrap_or(1).max(1);
    req.offset = (page - 1) * req.limit;
    req.scope = scope.filter(|s| !s.is_empty());
    req.facet_drill = drill.filter(|s| !s.is_empty()).into_iter().collect();
    let r = idx.search(&req)?;

    let mut hits = Vec::new();
    for h in &r.hits {
        let (occurrences, sn) = match bc.volume_text(&h.path) {
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
        hits.push(HitDto {
            book_id: h.book.clone(),
            title: h.title.clone(),
            juan: h.juan,
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
        total_volumes: r.total_volumes,
        total_books: r.total_books,
        page,
        pages: r.total_volumes.div_ceil(req.limit),
        millis: start.elapsed().as_secs_f64() * 1000.0,
        facets,
        hits,
    })
}

// ------------------------------------------------------------ index

#[derive(Serialize, Clone)]
struct ProgressDto {
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
struct IndexDoneDto {
    ok: bool,
    message: String,
}

/// Build (or rebuild) the index on a background thread. Emits
/// `index-progress` {done, total} and finally `index-done` {ok, message}.
#[tauri::command]
pub async fn build_index(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let bc = state.bookcase()?;
    if state.building.swap(true, Ordering::SeqCst) {
        return Err("正在建立索引".to_string());
    }
    let dir = state.index_dir.clone();
    // drop the open index before deleting its files
    *state.index.write().unwrap() = None;
    std::thread::spawn(move || {
        let result = (|| -> Result<String, String> {
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| format!("无法删除旧索引：{}", e))?;
            }
            let app2 = app.clone();
            let stats = SearchIndex::build(&bc, &dir, move |done, total| {
                let _ = app2.emit("index-progress", ProgressDto { done, total });
            })?;
            Ok(format!(
                "已索引 {} 卷，用时 {:.0} 秒{}",
                stats.volumes,
                stats.seconds,
                if stats.skipped.is_empty() {
                    String::new()
                } else {
                    format!("，跳过 {} 卷", stats.skipped.len())
                }
            ))
        })();
        let state = app.state::<AppState>();
        state.building.store(false, Ordering::SeqCst);
        let done = match result {
            Ok(message) => IndexDoneDto { ok: true, message },
            Err(message) => IndexDoneDto { ok: false, message },
        };
        let _ = app.emit("index-done", done);
    });
    Ok(())
}
