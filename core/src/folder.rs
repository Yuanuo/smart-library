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
//! Nothing here writes to the folder.

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

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(name)
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
}

impl FolderLibrary {
    pub fn open(root: &Path, filter: FolderFilter) -> Result<FolderLibrary, String> {
        if !root.is_dir() {
            return Err(format!("文件夹不存在：{}", root.display()));
        }
        Ok(FolderLibrary { root: root.to_path_buf(), filter })
    }

    fn abs(&self, rel: &str) -> Result<PathBuf, String> {
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
                if self.filter.accepts(&name, size) {
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
            })
        })
    }
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
    fn size_limit() {
        let d = tmp("size");
        std::fs::write(d.join("big.txt"), vec![b'a'; 2 * 1024 * 1024]).unwrap();
        let lib = FolderLibrary::open(&d, FolderFilter { exts: vec!["txt".into()], max_mb: 1 }).unwrap();
        assert!(lib.files().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
