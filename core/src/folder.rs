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

/// The levels of a path typed in a box (`/` or `\` between them); each one must be a good file name.
pub fn split_path(path: &str) -> Result<Vec<String>, String> {
    let parts: Vec<String> = path.split(['/', '\\']).map(|p| p.trim().to_string()).collect();
    for p in &parts {
        check_name(p)?; // an empty level (`a//b`) is refused too
    }
    Ok(parts)
}

/// The name of a file as the tree shows it: without the suffix (`a.map.json` -> `a`).
pub fn title_of(name: &str) -> &str {
    stem_of(name)
}

/// The folder next to a note that holds its pictures: `<file name>.assets`
/// (`笔记 1.md.assets`).
pub const ASSETS_SUFFIX: &str = ".assets";

fn is_assets_dir(name: &str) -> bool {
    name.strip_suffix(ASSETS_SUFFIX).is_some_and(|n| EditKind::of(n).is_some())
}

/// A file name as it is written in a Markdown link (spaces and brackets encoded).
fn link_name(name: &str) -> String {
    name.replace(' ', "%20").replace('(', "%28").replace(')', "%29")
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
                if is_assets_dir(&name) {
                    continue; // the pictures of a note, kept next to it
                }
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
            // the pictures of the note (links to its .assets folder) show as the notes' own pictures
            crate::markdown::to_html(&to_editor(&text))
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

    /// New folders in the folder `parent`; `path` may name several levels
    /// (`甲/乙/丙` or `甲\乙`), the missing ones are made. Returns the path of the
    /// last one, below the root.
    pub fn create_dir(&self, parent: &str, path: &str) -> Result<String, String> {
        self.need_writable()?;
        let parts = split_path(path)?;
        let mut rel = parent.to_string();
        let mut made = false;
        for part in parts {
            let dir = self.abs(&rel)?;
            if !dir.is_dir() {
                return Err(format!("文件夹不存在：{}", rel));
            }
            let next = if rel.is_empty() { part.clone() } else { format!("{}/{}", rel, part) };
            if Self::taken(&dir, &part) {
                if !self.abs(&next)?.is_dir() {
                    return Err(format!("这里已经有叫「{}」的文件", part));
                }
            } else {
                std::fs::create_dir(dir.join(&part)).map_err(|e| format!("{}: {}", part, e))?;
                made = true;
            }
            rel = next;
        }
        if !made {
            return Err(format!("这里已经有叫「{}」的文件夹", path.trim()));
        }
        Ok(rel)
    }

    /// Give a folder another name; returns its new path.
    fn rename_dir(&self, rel: &str, name: &str) -> Result<String, String> {
        let name = name.trim();
        check_name(name)?;
        let path = self.abs(rel)?;
        let parent = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let old_name = rel.rsplit('/').next().unwrap_or(rel);
        if old_name == name {
            return Ok(rel.to_string());
        }
        if old_name.to_lowercase() != name.to_lowercase() && Self::taken(path.parent().unwrap_or(&self.root), name) {
            return Err(format!("这里已经有叫「{}」的文件或文件夹", name));
        }
        let new_rel = if parent.is_empty() { name.to_string() } else { format!("{}/{}", parent, name) };
        std::fs::rename(&path, self.abs(&new_rel)?).map_err(|e| format!("{}: {}", rel, e))?;
        Ok(new_rel)
    }

    /// Move an editable file or a folder into the folder `parent` ("" for the
    /// top); returns its new path. A folder cannot go into itself.
    pub fn move_to(&self, rel: &str, parent: &str) -> Result<String, String> {
        self.need_writable()?;
        if rel.is_empty() {
            return Err("不能移动整个知识库".into());
        }
        let path = self.abs(rel)?;
        if path.is_dir() {
            if parent == rel || parent.starts_with(&format!("{}/", rel)) {
                return Err("不能把文件夹移到它自己里面".into());
            }
        } else {
            self.editable(rel)?;
        }
        if parent.split('/').any(|p| p.starts_with('.')) {
            return Err(format!("不能移到这里：{}", parent));
        }
        let dest = self.abs(parent)?;
        if !dest.is_dir() {
            return Err(format!("文件夹不存在：{}", parent));
        }
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let here = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if here == parent {
            return Ok(rel.to_string());
        }
        if Self::taken(&dest, name) {
            return Err(format!("目标文件夹里已经有叫「{}」的文件或文件夹", name));
        }
        // a note's pictures go with it (they stay next to it, so its links stay right)
        let assets = if path.is_dir() { None } else { Some(self.assets_of(rel)?).filter(|a| a.is_dir()) };
        let dest_assets = dest.join(format!("{}{}", name, ASSETS_SUFFIX));
        if assets.is_some() && dest_assets.exists() {
            return Err(format!("目标文件夹里已经有叫「{}{}」的文件夹", name, ASSETS_SUFFIX));
        }
        std::fs::rename(&path, dest.join(name)).map_err(|e| format!("{}: {}", rel, e))?;
        if let Some(a) = assets {
            std::fs::rename(&a, &dest_assets).map_err(|e| format!("{}: {}", a.display(), e))?;
        }
        Ok(if parent.is_empty() { name.to_string() } else { format!("{}/{}", parent, name) })
    }

    /// Every folder below the root, as paths, in order (to pick a place to move to).
    pub fn dirs(&self) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(lib: &FolderLibrary, rel: &str, out: &mut Vec<String>) {
            let Ok(list) = lib.children(rel) else { return };
            for e in list.into_iter().filter(|e| e.is_dir) {
                out.push(e.rel.clone());
                if out.len() < 5000 {
                    walk(lib, &e.rel, out);
                }
            }
        }
        walk(self, "", &mut out);
        out
    }

    /// Where the pictures of the notes are kept (read-write libraries).
    pub fn attach_dir(&self) -> PathBuf {
        self.root.join(SIDECAR_DIR).join("attachments")
    }

    /// Give an editable file or a folder another name (a file keeps its
    /// suffix); returns its new path.
    pub fn rename(&self, rel: &str, main: &str) -> Result<String, String> {
        self.need_writable()?;
        if !rel.is_empty() && self.abs(rel)?.is_dir() {
            return self.rename_dir(rel, main);
        }
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
        // the pictures of the note are in a folder named after the file: it is renamed too
        let old_assets = self.assets_of(rel)?;
        let new_assets = self.assets_of(&new_rel)?;
        let has_assets = old_assets.is_dir();
        if has_assets && new_assets.exists() && old_name.to_lowercase() != file.to_lowercase() {
            return Err(format!("这里已经有叫「{}{}」的文件夹", file, ASSETS_SUFFIX));
        }
        std::fs::rename(&path, self.abs(&new_rel)?).map_err(|e| format!("{}: {}", rel, e))?;
        if has_assets {
            std::fs::rename(&old_assets, &new_assets).map_err(|e| format!("{}: {}", old_assets.display(), e))?;
            if kind == EditKind::Markdown {
                // the links in the note name the folder
                let abs = self.abs(&new_rel)?;
                if let Ok(bytes) = std::fs::read(&abs) {
                    let text = decode_auto(&bytes);
                    let from = format!("{}{}/", link_name(old_name), ASSETS_SUFFIX);
                    let to = format!("{}{}/", link_name(&file), ASSETS_SUFFIX);
                    if text.contains(&from) {
                        let _ = std::fs::write(&abs, text.replace(&from, &to).as_bytes());
                    }
                }
            }
        }
        Ok(new_rel)
    }

    /// Take an editable file out of the folder: it is moved into
    /// `.smart-library/trash/` (not deleted for good).
    /// Its pictures go with it.
    pub fn delete(&self, rel: &str) -> Result<(), String> {
        let (path, _) = self.editable(rel)?;
        let assets = self.assets_of(rel)?;
        let mut names = Vec::new();
        collect_pictures(&assets, true, &mut names);
        self.to_trash(&path)?;
        if assets.is_dir() {
            self.to_trash(&assets)?;
        }
        self.forget_pool(&names);
        Ok(())
    }

    /// Take a folder with everything in it out of the library (into the trash).
    pub fn delete_dir(&self, rel: &str) -> Result<(), String> {
        self.need_writable()?;
        if rel.is_empty() || rel.split('/').any(|p| p.starts_with('.')) {
            return Err("不能删除这个文件夹".into());
        }
        let path = self.abs(rel)?;
        if !path.is_dir() {
            return Err(format!("文件夹不存在：{}", rel));
        }
        let mut names = Vec::new();
        collect_pictures(&path, false, &mut names);
        self.to_trash(&path)?;
        self.forget_pool(&names);
        Ok(())
    }

    fn to_trash(&self, path: &Path) -> Result<(), String> {
        let trash = self.root.join(SIDECAR_DIR).join("trash");
        std::fs::create_dir_all(&trash).map_err(|e| format!("{}: {}", trash.display(), e))?;
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut target = trash.join(format!("{}-{}", secs, name));
        let mut n = 1;
        while target.exists() {
            n += 1;
            target = trash.join(format!("{}-{}-{}", secs, n, name));
        }
        std::fs::rename(path, &target).map_err(|e| format!("{}: {}", path.display(), e))
    }

    // ------------------------------------------------------------ pictures

    /// The folder holding the pictures of a note: `<file>.assets` beside it.
    fn assets_of(&self, rel: &str) -> Result<PathBuf, String> {
        let mut s = self.abs(rel)?.into_os_string();
        s.push(ASSETS_SUFFIX);
        Ok(PathBuf::from(s))
    }

    /// A picture pasted into a note: a file in the note's own `.assets` folder
    /// (so it goes when the note goes). The same picture in several notes is one
    /// file on disk: the first copy is also kept in `.smart-library/attachments`
    /// (the pool) and the other notes get hard links to it. Returns its name.
    pub fn store_picture(&self, note_rel: &str, bytes: &[u8], mime: &str) -> Result<String, String> {
        self.editable(note_rel)?;
        let name = crate::attach::name_for(bytes, mime)?;
        let doc_dir = self.assets_of(note_rel)?;
        let doc_file = doc_dir.join(&name);
        if doc_file.exists() {
            return Ok(name);
        }
        std::fs::create_dir_all(&doc_dir).map_err(|e| format!("{}: {}", doc_dir.display(), e))?;
        let pool = self.attach_dir();
        let pooled = pool.join(&name);
        if pooled.exists() {
            link_or_copy(&pooled, &doc_file)?;
        } else {
            std::fs::write(&doc_file, bytes).map_err(|e| format!("{}: {}", doc_file.display(), e))?;
            if std::fs::create_dir_all(&pool).is_ok() {
                let _ = std::fs::hard_link(&doc_file, &pooled);
            }
        }
        Ok(name)
    }

    /// The file of a picture, wherever a note of the library keeps it.
    pub fn find_picture(&self, name: &str) -> Option<PathBuf> {
        if !crate::attach::is_name(name) {
            return None;
        }
        let pooled = self.attach_dir().join(name);
        if pooled.is_file() {
            return Some(pooled);
        }
        let found = self.find_in_assets(&self.root, name)?;
        if std::fs::create_dir_all(self.attach_dir()).is_ok() {
            let _ = std::fs::hard_link(&found, &pooled); // the pool is kept up again
        }
        Some(found)
    }

    fn find_in_assets(&self, dir: &Path, name: &str) -> Option<PathBuf> {
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with('.') || !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let p = e.path();
            if is_assets_dir(&n) {
                if p.join(name).is_file() {
                    return Some(p.join(name));
                }
            } else if let Some(f) = self.find_in_assets(&p, name) {
                return Some(f);
            }
        }
        None
    }

    /// Pictures nobody refers to any more leave the pool.
    fn forget_pool(&self, names: &[String]) {
        for n in names {
            if self.find_in_assets(&self.root, n).is_none() {
                let _ = std::fs::remove_file(self.attach_dir().join(n));
            }
        }
    }

    /// The text of an editable file as it is written to disk. The pictures it
    /// refers to are made sure to be in the note's `.assets` folder, those it no
    /// longer refers to go to the trash; a Markdown note gets relative links
    /// (`![](笔记.md.assets/<名>.png)`, so other programs show them), the data of
    /// a map or board keeps `attach:<名>`.
    pub fn prepare_save(&self, rel: &str, content: &str) -> Result<String, String> {
        let (_, kind) = self.editable(rel)?;
        let names = crate::attach::refs(content);
        let doc_dir = self.assets_of(rel)?;
        for n in &names {
            let f = doc_dir.join(n);
            if !f.exists() {
                if let Some(src) = self.find_picture(n) {
                    std::fs::create_dir_all(&doc_dir).map_err(|e| format!("{}: {}", doc_dir.display(), e))?;
                    link_or_copy(&src, &f)?;
                }
            }
        }
        let mut gone = Vec::new();
        if doc_dir.is_dir() {
            let recent = std::time::Duration::from_secs(60);
            for e in std::fs::read_dir(&doc_dir).map_err(|e| e.to_string())?.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                // a picture that was just pasted may not be in the text yet
                let fresh = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < recent);
                if crate::attach::is_name(&n) && !names.contains(&n) && !fresh {
                    self.to_trash(&e.path())?;
                    gone.push(n);
                }
            }
            if std::fs::read_dir(&doc_dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
                let _ = std::fs::remove_dir(&doc_dir);
            }
        }
        self.forget_pool(&gone);
        let file_name = rel.rsplit('/').next().unwrap_or(rel);
        Ok(if kind == EditKind::Markdown { attach_to_links(content, file_name) } else { content.to_string() })
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

/// Make a link of a file for another path: hard link when possible, else a copy.
fn link_or_copy(src: &Path, dst: &Path) -> Result<(), String> {
    if std::fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dst).map(|_| ()).map_err(|e| format!("{}: {}", dst.display(), e))
}

/// The names of the pictures in `dir`; `is_assets`: `dir` is a `.assets` folder
/// itself, else the `.assets` folders below it are looked at.
fn collect_pictures(dir: &Path, is_assets: bool, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_assets {
            if crate::attach::is_name(&n) {
                out.push(n);
            }
        } else if is_dir {
            collect_pictures(&e.path(), is_assets_dir(&n), out);
        }
    }
}

/// `attach:<name>` of a note's Markdown -> a link relative to the note
/// (`笔记%201.md.assets/<name>`).
fn attach_to_links(content: &str, file_name: &str) -> String {
    let mut out = content.to_string();
    for name in crate::attach::refs(content) {
        out = out.replace(&format!("attach:{}", name), &format!("{}{}/{}", link_name(file_name), ASSETS_SUFFIX, name));
    }
    out
}

/// `](<anything>.assets/<name>)` -> `](attach:<name>)` (a link to a picture of
/// a note, as `attach_to_links` writes it).
fn links_to_attach(content: &str) -> String {
    const KEY: &str = ".assets/";
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(at) = rest.find(KEY) {
        let after = &rest[at + KEY.len()..];
        let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '.')).unwrap_or(after.len());
        let name = &after[..end];
        let start = rest[..at].rfind("](");
        let linked = start.filter(|&s| !rest[s + 2..at].contains([')', '\n', ' ', '<']));
        match linked {
            Some(s) if crate::attach::is_name(name) => {
                out.push_str(&rest[..s + 2]);
                out.push_str("attach:");
                out.push_str(name);
                rest = &after[end..];
            }
            _ => {
                out.push_str(&rest[..at + KEY.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The Markdown of a note as the editor gets it: the links to its pictures
/// are `![](attach:<name>)`.
pub fn to_editor(content: &str) -> String {
    links_to_attach(&legacy_links_to_attach(content))
}

/// The links the first version wrote (`](../.smart-library/attachments/<name>)`).
fn legacy_links_to_attach(content: &str) -> String {
    const KEY: &str = ".smart-library/attachments/";
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(at) = rest.find(KEY) {
        let before = &rest[..at];
        let after = &rest[at + KEY.len()..];
        let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '.')).unwrap_or(after.len());
        let name = &after[..end];
        let stem = before.trim_end_matches("../");
        if crate::attach::is_name(name) && stem.ends_with("](") {
            out.push_str(stem);
            out.push_str("attach:");
        } else {
            out.push_str(before);
            out.push_str(KEY);
        }
        out.push_str(name);
        rest = &after[end..];
    }
    out.push_str(rest);
    out
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
        // a Markdown file is shown rendered, not as its source
        let two = lib.article("a/two.md", Some(&RenderOptions { highlights: vec![], asset_prefix: String::new() })).unwrap();
        let html = two.html.unwrap_or_default();
        assert!(html.contains("head h1") && !html.contains("# 标题"), "{html}");
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
        // several levels at once, the existing ones are reused
        assert_eq!(lib.create_dir("资料", "三/四\\五").unwrap(), "资料/三/四/五");
        assert_eq!(lib.create_dir("", "资料/三/六").unwrap(), "资料/三/六");
        assert!(lib.create_dir("", "资料/三").is_err()); // nothing new
        assert!(lib.create_dir("", "资料//七").is_err());
        assert!(lib.create_dir("", "资料/a:b").is_err());
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
    fn renames_and_moves_folders_and_files() {
        let d = tmp("mv");
        let lib = FolderLibrary::open(&d, FolderFilter::default()).unwrap().with_writable(true);
        lib.create_dir("", "甲").unwrap();
        lib.create_dir("", "乙").unwrap();
        lib.create_dir("甲", "内").unwrap();
        lib.create("甲/内", Some("文"), EditKind::Markdown).unwrap();
        // rename a folder: what is inside follows
        assert_eq!(lib.rename("甲", "丙").unwrap(), "丙");
        assert!(lib.rename("丙", "乙").is_err());
        assert!(lib.rename("丙", "a/b").is_err());
        assert!(d.join("丙/内/文.md").is_file());
        assert_eq!(lib.rename("丙", "丙").unwrap(), "丙");
        // move a file and a folder
        assert_eq!(lib.move_to("丙/内/文.md", "乙").unwrap(), "乙/文.md");
        assert_eq!(lib.move_to("丙/内", "乙").unwrap(), "乙/内");
        assert!(lib.move_to("乙", "乙/内").is_err()); // into itself
        assert!(lib.move_to("乙", "乙").is_err());
        assert!(lib.move_to("乙/文.md", "不存在").is_err());
        assert!(lib.move_to("乙/文.md", ".smart-library").is_err());
        assert_eq!(lib.move_to("乙/文.md", "乙").unwrap(), "乙/文.md"); // already there
        lib.create("丙", Some("文"), EditKind::Markdown).unwrap();
        assert!(lib.move_to("丙/文.md", "乙").is_err()); // name taken
        std::fs::write(d.join("x.txt"), "t").unwrap();
        assert!(lib.move_to("x.txt", "乙").is_err()); // not a file of the program
        assert_eq!(lib.dirs(), vec!["丙", "乙", "乙/内"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pictures_are_links_to_the_notes_own_folder() {
        let name = "0123456789abcdef0123456789abcdef.png";
        let editor = format!("前 ![](attach:{}) 后 ![x](http://a/b.png)", name);
        let file = attach_to_links(&editor, "笔记 1.md");
        assert_eq!(file, format!("前 ![](笔记%201.md.assets/{}) 后 ![x](http://a/b.png)", name));
        assert_eq!(to_editor(&file), editor);
        // the links of the first version are read too
        let old = format!("![](../.smart-library/attachments/{})", name);
        assert_eq!(to_editor(&old), format!("![](attach:{})", name));
        // not ours: left alone
        let other = "![](pics/x.assets/zz.png) [a](b.assets/c)";
        assert_eq!(to_editor(other), other);
    }

    #[test]
    fn pictures_follow_their_note_and_are_cleaned_up() {
        let d = tmp("pic");
        let lib = FolderLibrary::open(&d, FolderFilter::default()).unwrap().with_writable(true);
        let a = lib.create("", Some("甲 乙"), EditKind::Markdown).unwrap();
        let b = lib.create("", Some("丙"), EditKind::Markdown).unwrap();
        let png = [137u8, 80, 78, 71, 1, 2, 3];
        let n = lib.store_picture(&a, &png, "image/png").unwrap();
        // the same picture in another note is one file on disk
        assert_eq!(lib.store_picture(&b, &png, "image/png").unwrap(), n);
        assert!(d.join(format!("甲 乙.md.assets/{}", n)).is_file());
        assert!(d.join(format!("丙.md.assets/{}", n)).is_file());
        assert!(d.join(format!(".smart-library/attachments/{}", n)).is_file());
        // the folders of pictures are not part of the tree
        let top: Vec<_> = lib.children("").unwrap().into_iter().map(|e| e.rel).collect();
        assert_eq!(top, vec!["丙.md", "甲 乙.md"]);
        assert_eq!(lib.find_picture(&n).unwrap().file_name().unwrap().to_string_lossy(), n);
        assert!(lib.find_picture("../x.png").is_none());

        // saving: the text gets the relative link, and a picture it no longer has goes
        let text = lib.prepare_save(&a, &format!("![](attach:{})", n)).unwrap();
        assert_eq!(text, format!("![](甲%20乙.md.assets/{})", n));
        assert_eq!(to_editor(&text), format!("![](attach:{})", n));

        // renaming the note renames its folder and the links in it
        lib.write_edit(&a, &text, None).unwrap();
        let a2 = lib.rename(&a, "新 名").unwrap();
        assert!(d.join(format!("新 名.md.assets/{}", n)).is_file());
        let (t, _) = lib.read_edit(&a2).unwrap();
        assert!(t.contains(&format!("新%20名.md.assets/{}", n)), "{t}");
        // moving it takes the folder along
        lib.create_dir("", "目录").unwrap();
        let a3 = lib.move_to(&a2, "目录").unwrap();
        assert!(d.join(format!("目录/新 名.md.assets/{}", n)).is_file());
        assert!(!d.join(format!("新 名.md.assets")).exists());

        // a picture older than a minute that the text does not name is taken away; the pool keeps
        // what the other note uses
        let old = d.join(format!("丙.md.assets/{}", n));
        let _ = old; // (age check: a new file is spared)
        assert_eq!(lib.prepare_save(&b, "无图").unwrap(), "无图");
        assert!(d.join(format!("丙.md.assets/{}", n)).is_file(), "a picture pasted a moment ago stays");

        // deleting the note takes its pictures; the last copy leaves the pool
        lib.delete(&b).unwrap();
        assert!(!d.join("丙.md.assets").exists());
        assert!(lib.find_picture(&n).is_some()); // still in 新 名's folder
        lib.delete(&a3).unwrap();
        assert!(lib.find_picture(&n).is_none());
        assert!(!d.join(format!(".smart-library/attachments/{}", n)).exists());
        // deleting a folder takes what is in it
        lib.create_dir("", "夹").unwrap();
        let c = lib.create("夹", Some("文"), EditKind::Markdown).unwrap();
        let n2 = lib.store_picture(&c, &[9, 9, 9], "image/png").unwrap();
        lib.delete_dir("夹").unwrap();
        assert!(!d.join("夹").exists());
        assert!(lib.find_picture(&n2).is_none());
        assert!(lib.delete_dir("").is_err());
        assert!(lib.delete_dir(".smart-library").is_err());
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
