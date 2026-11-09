//! External folder libraries (design-spec 9a): a real folder on disk, linked
//! read-only. Its sub-folders are the tree (listed lazily), and every file
//! that passes the filter (suffix, size) is a document:
//!
//! * text-like files (`txt`, `md`, `html` …) are read as articles, one
//!   paragraph per line, so search, snippets and the reader work as for the
//!   HTML libraries;
//! * any other listed suffix (media, PDF …) is indexed by its "media
//!   information": name, folders, type, size, date. Real tags (ID3, EXIF,
//!   duration) and PDF text need extra crates and come later.
//!
//! A folder library is read-only unless it was linked in read-write mode
//! (design-spec 6.5). Then the files this program saves itself can be edited:
//! `*.md` (Markdown), `*.map.json` (mind map) and `*.wbd.json` (whiteboard).
//! The write operations below (`write_edit`, `create`, `rename`, `delete`) are
//! the only code that changes the folder, and refuse when it is read-only.

use crate::article::{read_article, Article};
use crate::library::IndexDoc;
use crate::render::RenderOptions;
use crate::text::decode_auto;
use std::path::{Path, PathBuf};

/// Suffixes read as text.
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "markdown", "html", "htm", "xhtml", "xml", "csv", "tsv", "json", "log", "srt", "vtt", "ini", "yaml", "yml",
];
const HTML_EXTS: &[&str] = &["html", "htm", "xhtml"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFilter {
    /// lower-case suffixes without the dot
    pub exts: Vec<String>,
    /// files larger than this are left out; 0 = no limit
    pub max_mb: u64,
}

impl Default for FolderFilter {
    fn default() -> Self {
        FolderFilter {
            exts: ["txt", "md", "html", "htm"].map(String::from).to_vec(),
            max_mb: 50,
        }
    }
}

impl FolderFilter {
    /// `"txt, .MD ;html"` -> `["txt", "md", "html"]`
    pub fn parse_exts(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s
            .split(|c: char| c == ',' || c == ';' || c == '，' || c == '；' || c.is_whitespace())
            .map(|e| e.trim().trim_start_matches(['.', '*']).to_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn accepts(&self, name: &str, size: u64) -> bool {
        let ext = ext_of(name);
        self.exts.iter().any(|e| *e == ext) && (self.max_mb == 0 || size <= self.max_mb * 1024 * 1024)
    }
}

/// The file is read as text (else its media information stands for it).
pub fn is_text(name: &str) -> bool {
    TEXT_EXTS.contains(&ext_of(name).as_str())
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

fn stem_of(name: &str) -> &str {
    if let Some(k) = EditKind::of(name) {
        return &name[..name.len() - k.suffix().len()];
    }
    name.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(name)
}

/// The name of a file as the tree shows it: without the suffix (`a.map.json` -> `a`).
pub fn title_of(name: &str) -> &str {
    stem_of(name)
}

/// What an editable file is, told by its suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Markdown,
    MindMap,
    Whiteboard,
}

impl EditKind {
    /// The kind of a file name, if this program can edit it.
    pub fn of(name: &str) -> Option<EditKind> {
        let n = name.to_lowercase();
        [EditKind::MindMap, EditKind::Whiteboard, EditKind::Markdown]
            .into_iter()
            .find(|k| n.ends_with(k.suffix()) && n.len() > k.suffix().len())
    }

    /// `md`, `mindmap`, `whiteboard` (the `kind` the menus and commands use).
    pub fn parse(kind: &str) -> EditKind {
        match kind {
            "mindmap" => EditKind::MindMap,
            "whiteboard" => EditKind::Whiteboard,
            _ => EditKind::Markdown,
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            EditKind::Markdown => ".md",
            EditKind::MindMap => ".map.json",
            EditKind::Whiteboard => ".wbd.json",
        }
    }

    /// `content_type` of the reader: empty for Markdown.
    pub fn content_type(self) -> &'static str {
        match self {
            EditKind::Markdown => "",
            EditKind::MindMap => crate::mindmap::TYPE,
            EditKind::Whiteboard => crate::whiteboard::TYPE,
        }
    }

    /// Base of an automatic name: `笔记 1`, `脑图 1`, `白板 1`.
    pub fn base_name(self) -> &'static str {
        match self {
            EditKind::Markdown => "笔记",
            EditKind::MindMap => "脑图",
            EditKind::Whiteboard => "白板",
        }
    }

    fn initial(self, title: &str) -> String {
        match self {
            EditKind::Markdown => String::new(),
            EditKind::MindMap => crate::mindmap::new_data(title),
            EditKind::Whiteboard => crate::whiteboard::new_data(),
        }
    }
}

/// Shown next to every box that names a local file.
pub const NAME_RULES: &str = "本地文件名限制：不能包含 \\ / : * ? \" < > | 和控制字符；不能以空格或点开头、结尾；不能用 CON、PRN、AUX、NUL、COM1–9、LPT1–9 等系统保留名；最长 100 个字符；同一文件夹里不分大小写重名。";

/// Whether `name` (a file name without its suffix) can be used on every
/// system. The same strict rule applies everywhere, so a library kept in git
/// works on Windows, macOS and Linux alike.
pub fn check_name(name: &str) -> Result<(), String> {
    let fail = |why: &str| Err(format!("本地文件名限制：{}", why));
    if name.trim().is_empty() {
        return fail("名称不能为空");
    }
    if name.chars().count() > 100 {
        return fail("名称最长 100 个字符");
    }
    if let Some(c) = name.chars().find(|c| "\\/:*?\"<>|".contains(*c) || c.is_control()) {
        return fail(&if c.is_control() { "不能包含控制字符".to_string() } else { format!("不能包含「{}」", c) });
    }
    if name.starts_with(' ') || name.ends_with(' ') {
        return fail("不能以空格开头或结尾");
    }
    if name.starts_with('.') || name.ends_with('.') {
        return fail("不能以点开头或结尾");
    }
    let head = name.split('.').next().unwrap_or("").trim_end().to_uppercase();
    let serial = (head.starts_with("COM") || head.starts_with("LPT")) && head.len() == 4 && matches!(head.as_bytes()[3], b'1'..=b'9');
    if ["CON", "PRN", "AUX", "NUL"].contains(&head.as_str()) || serial {
        return fail(&format!("「{}」是系统保留名", head));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct FolderEntry {
    pub name: String,
    /// path below the root, `/` separated
    pub rel: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct FolderFile {
    pub rel: String,
    pub name: String,
    pub size: u64,
}

pub struct FolderLibrary {
    pub root: PathBuf,
    pub filter: FolderFilter,
    /// linked in read-write mode (the user accepted the rules, design-spec 6.5)
    pub writable: bool,
}

/// What the program keeps inside a library folder itself (hidden from the tree and the index).
pub const SIDECAR_DIR: &str = ".smart-library";

impl FolderLibrary {
    pub fn open(root: &Path, filter: FolderFilter) -> Result<FolderLibrary, String> {
        if !root.is_dir() {
            return Err(format!("文件夹不存在：{}", root.display()));
        }
        Ok(FolderLibrary { root: root.to_path_buf(), filter, writable: false })
    }

    pub fn with_writable(mut self, writable: bool) -> FolderLibrary {
        self.writable = writable;
        self
    }

    pub fn abs(&self, rel: &str) -> Result<PathBuf, String> {
        if rel.split('/').any(|p| p == ".." || p.contains(':') || p.contains('\\')) {
            return Err(format!("路径不合法：{}", rel));
        }
        Ok(rel.split('/').filter(|p| !p.is_empty()).fold(self.root.clone(), |p, s| p.join(s)))
    }

    /// One level of the tree: folders first, then the files that pass the
    /// filter; hidden entries (`.git` …) are left out.
    pub fn children(&self, rel: &str) -> Result<Vec<FolderEntry>, String> {
        let dir = self.abs(rel)?;
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for e in std::fs::read_dir(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            let child = if rel.is_empty() { name.clone() } else { format!("{}/{}", rel, name) };
            if ft.is_dir() {
                dirs.push(FolderEntry { name, rel: child, is_dir: true });
            } else if ft.is_file() {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                // the files the program edits are always listed, also in read-only mode
                // (where a map or board shows its texts); the suffix filter is for the rest
                if self.filter.accepts(&name, size) || EditKind::of(&name).is_some() {
                    files.push(FolderEntry { name, rel: child, is_dir: false });
                }
            }
        }
        dirs.sort_by_key(|e| e.name.to_lowercase());
        files.sort_by_key(|e| e.name.to_lowercase());
        dirs.extend(files);
        Ok(dirs)
    }

    /// Every file that passes the filter, in path order. Links are not followed.
    pub fn files(&self) -> Vec<FolderFile> {
        let mut out = Vec::new();
        fn walk(lib: &FolderLibrary, rel: &str, out: &mut Vec<FolderFile>) {
            let Ok(list) = lib.children(rel) else { return };
            for e in list {
                if e.is_dir {
                    walk(lib, &e.rel, out);
                } else {
                    let size = lib.abs(&e.rel).ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
                    out.push(FolderFile { rel: e.rel, name: e.name, size });
                }
            }
        }
        walk(self, "", &mut out);
        out
    }

    /// Cheap fingerprint of a file list: count, sizes and modification times.
    pub fn fingerprint(&self, files: &[FolderFile]) -> String {
        let mut h: u64 = 1469598103934665603;
        for f in files {
            let m = self.abs(&f.rel).ok().and_then(|p| std::fs::metadata(p).ok());
            let t = m
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            for b in f.rel.bytes().chain(f.size.to_le_bytes()).chain(t.to_le_bytes()) {
                h = (h ^ b as u64).wrapping_mul(1099511628211);
            }
        }
        format!("{}:{:x}", files.len(), h)
    }

    /// The files next to `rel` (same folder), in tree order.
    pub fn siblings(&self, rel: &str) -> Vec<FolderEntry> {
        let parent = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        self.children(parent).map(|v| v.into_iter().filter(|e| !e.is_dir).collect()).unwrap_or_default()
    }

    /// Read a file as an article (paragraph view). Binary / media files give
    /// their "media information".
    pub fn article(&self, rel: &str, render: Option<&RenderOptions>) -> Result<Article, String> {
        let path = self.abs(rel)?;
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let ext = ext_of(name);
        if !TEXT_EXTS.contains(&ext.as_str()) {
            return Ok(read_article(&info_html(&path, rel, name, &ext), "", render));
        }
        if matches!(EditKind::of(name), Some(EditKind::MindMap) | Some(EditKind::Whiteboard)) {
            // a map or board: the texts in it, one per paragraph
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {}", rel, e))?;
            let data = decode_auto(&bytes);
            let text = if EditKind::of(name) == Some(EditKind::MindMap) { crate::mindmap::text(&data) } else { crate::whiteboard::text(&data) };
            return Ok(read_article(&plain_html(&text), "", render));
        }
        let size = std::fs::metadata(&path).map_err(|e| format!("{}: {}", rel, e))?.len();
        if self.filter.max_mb != 0 && size > self.filter.max_mb * 1024 * 1024 {
            return Err(format!("{}: 文件太大", rel));
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {}", rel, e))?;
        let text = decode_auto(&bytes);
        let html = if HTML_EXTS.contains(&ext.as_str()) {
            text
        } else if ext == "md" || ext == "markdown" {
            crate::markdown::to_html(&text)
        } else {
            plain_html(&text)
        };
        // `rel` as the article path would resolve relative images inside the
        // folder; they are not served, so no path is given
        Ok(read_article(&html, "", render))
    }

    // ------------------------------------------------------------ editing (read-write mode)

    fn need_writable(&self) -> Result<(), String> {
        if self.writable { Ok(()) } else { Err("这个文件夹库是只读的".into()) }
    }

    /// An editable file: its path on disk and its kind.
    fn editable(&self, rel: &str) -> Result<(PathBuf, EditKind), String> {
        self.need_writable()?;
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let kind = EditKind::of(name).ok_or_else(|| format!("不能编辑这种文件：{}", name))?;
        Ok((self.abs(rel)?, kind))
    }

    /// Changes whenever the file does (modification time and size); to notice edits made outside.
    pub fn stamp(&self, rel: &str) -> Result<String, String> {
        let m = std::fs::metadata(self.abs(rel)?).map_err(|e| format!("{}: {}", rel, e))?;
        let t = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Ok(format!("{}:{}", t, m.len()))
    }

    /// The text of an editable file and its stamp.
    pub fn read_edit(&self, rel: &str) -> Result<(String, String), String> {
        let (path, _) = self.editable(rel)?;
        let stamp = self.stamp(rel)?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {}", rel, e))?;
        Ok((decode_auto(&bytes), stamp))
    }

    /// Replace the text of an editable file (through a temporary file, so a
    /// failure leaves the old one). `expect`: the stamp the caller last saw;
    /// when the file is different now it was changed outside and nothing is
    /// written. Returns the new stamp.
    pub fn write_edit(&self, rel: &str, content: &str, expect: Option<&str>) -> Result<String, String> {
        let (path, _) = self.editable(rel)?;
        if !path.is_file() {
            return Err(format!("文件已不存在：{}", rel));
        }
        if let Some(e) = expect {
            if self.stamp(rel)? != e {
                return Err("文件在本程序之外被修改过，没有保存。请关闭这个标签再重新打开，查看最新内容。".into());
            }
        }
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let tmp = path.with_file_name(format!(".{}.sl-tmp", name));
        std::fs::write(&tmp, content.as_bytes()).map_err(|e| format!("{}: {}", rel, e))?;
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("{}: {}", rel, e));
        }
        self.stamp(rel)
    }

    /// Whether `dir` has an entry called `name` (ignoring case).
    fn taken(dir: &Path, name: &str) -> bool {
        let low = name.to_lowercase();
        std::fs::read_dir(dir)
            .map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().to_lowercase() == low))
            .unwrap_or(false)
    }

    /// The first `笔记 1`, `笔记 2` … that no file in the folder uses.
    pub fn free_name(&self, parent: &str, kind: EditKind) -> Result<String, String> {
        let dir = self.abs(parent)?;
        (1..)
            .map(|n| format!("{} {}", kind.base_name(), n))
            .find(|m| !Self::taken(&dir, &format!("{}{}", m, kind.suffix())))
            .ok_or_else(|| "没有可用的名称".to_string())
    }

    /// A new file in the folder `parent`; returns its path below the root.
    /// `main`: the name without the suffix (automatic when `None`).
    pub fn create(&self, parent: &str, main: Option<&str>, kind: EditKind) -> Result<String, String> {
        self.need_writable()?;
        let dir = self.abs(parent)?;
        if !dir.is_dir() {
            return Err(format!("文件夹不存在：{}", parent));
        }
        let main = match main.map(str::trim).filter(|m| !m.is_empty()) {
            Some(m) => m.to_string(),
            None => self.free_name(parent, kind)?,
        };
        check_name(&main)?;
        let file = format!("{}{}", main, kind.suffix());
        if Self::taken(&dir, &file) {
            return Err(format!("这里已经有叫「{}」的文件", file));
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(&file))
            .map_err(|e| format!("{}: {}", file, e))?;
        std::io::Write::write_all(&mut f, kind.initial(&main).as_bytes()).map_err(|e| format!("{}: {}", file, e))?;
        Ok(if parent.is_empty() { file } else { format!("{}/{}", parent, file) })
    }

    /// A new sub-folder in the folder `parent`; returns its path below the root.
    pub fn create_dir(&self, parent: &str, name: &str) -> Result<String, String> {
        self.need_writable()?;
        let dir = self.abs(parent)?;
        if !dir.is_dir() {
            return Err(format!("文件夹不存在：{}", parent));
        }
        let name = name.trim();
        check_name(name)?;
        if Self::taken(&dir, name) {
            return Err(format!("这里已经有叫「{}」的文件或文件夹", name));
        }
        std::fs::create_dir(dir.join(name)).map_err(|e| format!("{}: {}", name, e))?;
        Ok(if parent.is_empty() { name.to_string() } else { format!("{}/{}", parent, name) })
    }

    /// Give an editable file another name (the suffix stays); returns its new path.
    pub fn rename(&self, rel: &str, main: &str) -> Result<String, String> {
        let (path, kind) = self.editable(rel)?;
        let main = main.trim();
        check_name(main)?;
        let parent = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let file = format!("{}{}", main, kind.suffix());
        let new_rel = if parent.is_empty() { file.clone() } else { format!("{}/{}", parent, file) };
        let old_name = rel.rsplit('/').next().unwrap_or(rel);
        if old_name == file {
            return Ok(rel.to_string());
        }
        // a change of case only is the same file
        if old_name.to_lowercase() != file.to_lowercase() && Self::taken(path.parent().unwrap_or(&self.root), &file) {
            return Err(format!("这里已经有叫「{}」的文件", file));
        }
        std::fs::rename(&path, self.abs(&new_rel)?).map_err(|e| format!("{}: {}", rel, e))?;
        Ok(new_rel)
    }

    /// Take an editable file out of the folder: it is moved into
    /// `.smart-library/trash/` (not deleted for good).
    pub fn delete(&self, rel: &str) -> Result<(), String> {
        let (path, _) = self.editable(rel)?;
        let trash = self.root.join(SIDECAR_DIR).join("trash");
        std::fs::create_dir_all(&trash).map_err(|e| format!("{}: {}", trash.display(), e))?;
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let mut target = trash.join(format!("{}-{}", secs, name));
        let mut n = 1;
        while target.exists() {
            n += 1;
            target = trash.join(format!("{}-{}-{}", secs, n, name));
        }
        std::fs::rename(&path, &target).map_err(|e| format!("{}: {}", rel, e))
    }

    /// Index documents for every file.
    pub fn docs<'a>(
        &'a self,
        files: &'a [FolderFile],
    ) -> impl Iterator<Item = Result<IndexDoc, String>> + 'a {
        files.iter().map(move |f| {
            let a = self.article(&f.rel, None)?;
            let folders: Vec<&str> = {
                let mut v: Vec<&str> = f.rel.split('/').collect();
                v.pop();
                v
            };
            let mut facets = Vec::new();
            let mut bulei = vec!["bulei".to_string()];
            bulei.extend(folders.iter().take(2).map(|s| s.to_string()));
            if bulei.len() > 1 {
                facets.push(bulei);
            }
            let book = folders.last().map(|s| s.to_string()).unwrap_or_else(|| {
                self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            });
            facets.push(vec!["book".to_string(), book.clone()]);
            let ext = ext_of(&f.name);
            if !ext.is_empty() {
                facets.push(vec!["kind".to_string(), ext]);
            }
            Ok(IndexDoc {
                path: f.rel.clone(),
                book,
                juan: 0,
                title: stem_of(&f.name).to_string(),
                text: a.text.text,
                facets,
                keys: path_keys(&f.rel),
            })
        })
    }
}

/// The folders above a file and the file, as nodes that can carry properties.
fn path_keys(rel: &str) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    let mut cur = String::new();
    for part in rel.split('/') {
        if !cur.is_empty() {
            cur.push('/');
        }
        cur.push_str(part);
        keys.push((String::new(), cur.clone()));
    }
    keys
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// One `<p>` per non-empty line.
fn plain_html(text: &str) -> String {
    let mut out = String::from("<html><body>");
    for line in text.lines() {
        let l = line.trim();
        if !l.is_empty() {
            out.push_str("<p>");
            out.push_str(&esc(l));
            out.push_str("</p>");
        }
    }
    out.push_str("</body></html>");
    out
}

/// Name, folders, type, size and date of a file, as a tiny article.
fn info_html(path: &Path, rel: &str, name: &str, ext: &str) -> String {
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let date = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| civil_date(d.as_secs() / 86400))
        .unwrap_or_default();
    let mut lines = vec![stem_of(name).to_string()];
    if let Some((dir, _)) = rel.rsplit_once('/') {
        lines.push(dir.replace('/', " / "));
    }
    lines.push(format!("类型 {}", ext));
    lines.push(format!("大小 {}", human_size(size)));
    if !date.is_empty() {
        lines.push(format!("修改日期 {}", date));
    }
    plain_html(&lines.join("\n"))
}

fn human_size(n: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 3 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{} B", n) } else { format!("{:.1} {}", v, U[i]) }
}

/// Days since 1970-01-01 -> `YYYY-MM-DD` (proleptic Gregorian).
fn civil_date(days: u64) -> String {
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// The canonical form of a folder path, for comparing.
pub fn canonical(path: &str) -> Result<String, String> {
    let p = std::fs::canonicalize(path.trim().trim_matches('"')).map_err(|e| format!("{}: {}", path, e))?;
    if !p.is_dir() {
        return Err(format!("不是文件夹：{}", path));
    }
    Ok(p.to_string_lossy().into_owned())
}

fn parts(p: &str) -> Vec<String> {
    p.replace('\\', "/").to_lowercase().split('/').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Why `new` cannot be linked next to the `existing` folders (canonical
/// paths): same folder, a folder above it, or a folder inside it is already
/// linked (the files would be indexed twice).
pub fn conflict(existing: &[&str], new: &str) -> Option<String> {
    let n = parts(new);
    for e in existing {
        let p = parts(e);
        if p == n {
            return Some(format!("这个文件夹已经导入过：{}", e));
        }
        if p.len() < n.len() && n[..p.len()] == p[..] {
            return Some(format!("它的上级文件夹已经导入过：{}", e));
        }
        if n.len() < p.len() && p[..n.len()] == n[..] {
            return Some(format!("它里面的文件夹已经导入过：{}（请先解除绑定）", e));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sl-folder-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn conflicts() {
        let e = ["C:\\Data\\Notes"];
        assert!(conflict(&e, "c:/data/notes").unwrap().contains("已经导入"));
        assert!(conflict(&e, "C:\\Data\\Notes\\sub").unwrap().contains("上级"));
        assert!(conflict(&e, "C:\\Data").unwrap().contains("里面"));
        assert!(conflict(&e, "C:\\Data\\Notes2").is_none());
    }

    #[test]
    fn parses_suffixes() {
        assert_eq!(FolderFilter::parse_exts("TXT, .md；*.html md"), vec!["html", "md", "txt"]);
    }

    #[test]
    fn lists_filters_and_reads() {
        let d = tmp("read");
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("a/b/one.txt"), "如是我聞\n一時佛在").unwrap();
        std::fs::write(d.join("a/two.md"), "# 标题\n正文").unwrap();
        std::fs::write(d.join("a/skip.bin"), "x").unwrap();
        std::fs::write(d.join("a/song.mp3"), vec![0u8; 2048]).unwrap();
        let lib = FolderLibrary::open(&d, FolderFilter { exts: vec!["txt".into(), "md".into(), "mp3".into()], max_mb: 1 }).unwrap();
        let top: Vec<_> = lib.children("").unwrap().into_iter().map(|e| e.rel).collect();
        assert_eq!(top, vec!["a"]);
        let a: Vec<_> = lib.children("a").unwrap().into_iter().map(|e| (e.rel, e.is_dir)).collect();
        assert_eq!(a[0], ("a/b".to_string(), true));
        assert!(!a.iter().any(|(r, _)| r.ends_with("skip.bin")));
        let files = lib.files();
        assert_eq!(files.len(), 3);
        let docs: Vec<IndexDoc> = lib.docs(&files).map(|d| d.unwrap()).collect();
        let one = docs.iter().find(|d| d.path == "a/b/one.txt").unwrap();
        assert_eq!(one.title, "one");
        assert_eq!(one.book, "b");
        assert!(one.text.contains("一時佛在"));
        let song = docs.iter().find(|d| d.path == "a/song.mp3").unwrap();
        assert!(song.text.contains("song") && song.text.contains("2.0 KB"));
        assert!(lib.article("../x", None).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn file_names_are_strict() {
        for ok in ["笔记 1", "a.b", "读书笔记（二）", "x-y_z"] {
            assert!(check_name(ok).is_ok(), "{}", ok);
        }
        for bad in ["", "  ", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", " a", "a ", ".a", "a.", "con", "NUL.txt", "Com1", "lpt9", "tab\there"] {
            assert!(check_name(bad).is_err(), "{:?}", bad);
        }
        assert!(check_name("com0").is_ok() && check_name("comma").is_ok());
        assert!(check_name(&"长".repeat(101)).is_err());
        assert!(check_name(&"长".repeat(100)).is_ok());
        assert!(check_name("a|b").unwrap_err().contains("本地文件名限制"));
    }

    #[test]
    fn kinds_follow_the_suffix() {
        assert_eq!(EditKind::of("a.md"), Some(EditKind::Markdown));
        assert_eq!(EditKind::of("A.MAP.JSON"), Some(EditKind::MindMap));
        assert_eq!(EditKind::of("a.wbd.json"), Some(EditKind::Whiteboard));
        assert_eq!(EditKind::of("a.json"), None);
        assert_eq!(EditKind::of(".md"), None);
        assert_eq!(title_of("图.map.json"), "图");
        assert_eq!(title_of("x.y.txt"), "x.y");
    }

    #[test]
    fn read_only_by_default() {
        let d = tmp("ro");
        std::fs::write(d.join("a.md"), "x").unwrap();
        let lib = FolderLibrary::open(&d, FolderFilter::default()).unwrap();
        assert!(lib.write_edit("a.md", "y", None).is_err());
        assert!(lib.create("", None, EditKind::Markdown).is_err());
        assert!(lib.rename("a.md", "b").is_err());
        assert!(lib.delete("a.md").is_err());
        assert!(lib.create_dir("", "新").is_err());
        assert_eq!(std::fs::read_to_string(d.join("a.md")).unwrap(), "x");
        // going back to read-only keeps the notes in the tree (a map shows its texts)
        let rw = FolderLibrary::open(&d, FolderFilter { exts: vec!["txt".into()], max_mb: 50 }).unwrap().with_writable(true);
        rw.create("", Some("图"), EditKind::MindMap).unwrap();
        rw.create("", Some("记"), EditKind::Markdown).unwrap();
        let ro = FolderLibrary::open(&d, FolderFilter { exts: vec!["txt".into()], max_mb: 50 }).unwrap();
        let names: Vec<_> = ro.children("").unwrap().into_iter().map(|e| e.rel).collect();
        assert_eq!(names, vec!["a.md", "图.map.json", "记.md"]);
        assert!(ro.article("图.map.json", None).unwrap().text.text.contains("图"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn edits_creates_renames_and_deletes() {
        let d = tmp("rw");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        // only the md / map / board files are listed besides the filter's suffixes
        let lib = FolderLibrary::open(&d, FolderFilter { exts: vec!["txt".into()], max_mb: 50 }).unwrap().with_writable(true);

        let a = lib.create("sub", None, EditKind::Markdown).unwrap();
        assert_eq!(a, "sub/笔记 1.md");
        assert_eq!(lib.create("sub", None, EditKind::Markdown).unwrap(), "sub/笔记 2.md");
        let m = lib.create("", None, EditKind::MindMap).unwrap();
        let w = lib.create("", Some("白板甲"), EditKind::Whiteboard).unwrap();
        assert_eq!((m.as_str(), w.as_str()), ("脑图 1.map.json", "白板甲.wbd.json"));
        assert!(lib.create("", Some("白板甲"), EditKind::Whiteboard).is_err());
        assert!(lib.create("", Some("BAD:name"), EditKind::Markdown).is_err());
        assert!(lib.create("nope", None, EditKind::Markdown).is_err());
        let top: Vec<_> = lib.children("").unwrap().into_iter().map(|e| e.rel).collect();
        assert_eq!(top, vec!["sub", "白板甲.wbd.json", "脑图 1.map.json"]);
        // folders to organize the files
        assert_eq!(lib.create_dir("", "资料").unwrap(), "资料");
        assert_eq!(lib.create_dir("资料", "二级").unwrap(), "资料/二级");
        assert!(lib.create_dir("", "资料").is_err());
        assert!(lib.create_dir("", "ZIDIAO:x").is_err());
        assert!(lib.create_dir("无", "x").is_err());
        assert_eq!(lib.create("资料/二级", None, EditKind::Markdown).unwrap(), "资料/二级/笔记 1.md");
        let t: Vec<_> = lib.children("").unwrap().into_iter().map(|e| e.rel).collect();
        assert_eq!(t[0], "sub");
        assert_eq!(t[1], "资料");
        // the map starts with its root, and its text is found by the index
        let (text, _) = lib.read_edit(&m).unwrap();
        assert!(text.contains("\"nodeData\""));
        assert!(lib.article(&m, None).unwrap().text.text.contains("脑图 1"));

        // save; a change made outside is noticed
        let (_, stamp) = lib.read_edit(&a).unwrap();
        let stamp = lib.write_edit(&a, "# 题\n正文", Some(&stamp)).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("sub/笔记 1.md")).unwrap(), "# 题\n正文");
        assert!(!d.join("sub/.笔记 1.md.sl-tmp").exists());
        std::fs::write(d.join("sub/笔记 1.md"), "别处改的 内容更长").unwrap();
        let e = lib.write_edit(&a, "我的", Some(&stamp)).unwrap_err();
        assert!(e.contains("被修改"));
        assert_eq!(std::fs::read_to_string(d.join("sub/笔记 1.md")).unwrap(), "别处改的 内容更长");
        assert!(lib.write_edit(&a, "强制", None).is_ok());

        // other kinds of file and paths outside the root are refused
        std::fs::write(d.join("x.txt"), "t").unwrap();
        assert!(lib.write_edit("x.txt", "y", None).is_err());
        assert!(lib.write_edit("../x.md", "y", None).is_err());
        assert!(lib.delete("../x.md").is_err());

        // rename keeps the suffix and refuses a taken name
        assert_eq!(lib.rename(&a, "读书笔记").unwrap(), "sub/读书笔记.md");
        assert!(lib.rename("sub/读书笔记.md", "笔记 2").is_err());
        assert!(lib.rename("sub/读书笔记.md", "a/b").is_err());
        assert_eq!(lib.rename("sub/读书笔记.md", "读书笔记").unwrap(), "sub/读书笔记.md");
        assert_eq!(lib.rename("sub/读书笔记.md", "读书笔记".to_uppercase().as_str()).is_ok(), true);

        // delete moves to the trash inside .smart-library, which the tree does not show
        lib.delete(&m).unwrap();
        assert!(!d.join(&m).exists());
        let trashed: Vec<_> = std::fs::read_dir(d.join(SIDECAR_DIR).join("trash")).unwrap().flatten().collect();
        assert_eq!(trashed.len(), 1);
        assert!(lib.children("").unwrap().iter().all(|e| !e.rel.starts_with('.')));
        assert!(lib.files().iter().all(|f| !f.rel.contains(SIDECAR_DIR)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn size_limit() {
        let d = tmp("size");
        std::fs::write(d.join("big.txt"), vec![b'a'; 2 * 1024 * 1024]).unwrap();
        let lib = FolderLibrary::open(&d, FolderFilter { exts: vec!["txt".into()], max_mb: 1 }).unwrap();
        assert!(lib.files().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
