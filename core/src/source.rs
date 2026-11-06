//! Where the bookcase files come from: a zip, a directory, or memory (tests).
//! Paths are always relative to the bookcase root (the folder holding `index.xml`),
//! e.g. `catalog.txt` or `XML/T/T01/T01n0004_001.xml`.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zip::ZipArchive;

pub trait Source: Send + Sync {
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    fn exists(&self, path: &str) -> bool;
    /// All file paths (relative), unsorted.
    fn list(&self) -> Vec<String>;
}

/// A `bookcase.zip`. Files are read on demand (no extraction).
///
/// Entry names are taken from the raw bytes: UTF-8 when the bytes are valid
/// UTF-8, otherwise GBK. (CBETA's own entries are ASCII; the `a/` menus in
/// some bookcases are GBK and carry no UTF-8 flag, so `ZipFile::name()`
/// would decode them as CP437.)
pub struct ZipSource {
    archive: Mutex<ZipArchive<File>>,
    /// relative path -> entry index in the archive
    names: HashMap<String, usize>,
}

/// Decode a zip entry name: UTF-8 first, GBK as fallback.
pub fn decode_entry_name(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(raw).0.into_owned(),
    }
}

impl ZipSource {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let file = File::open(path.as_ref()).map_err(|e| format!("{}: {}", path.as_ref().display(), e))?;
        let mut archive = ZipArchive::new(file).map_err(|e| format!("not a zip file: {}", e))?;

        let mut all: Vec<(String, usize)> = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i).map_err(|e| format!("zip entry {}: {}", i, e))?;
            all.push((decode_entry_name(entry.name_raw()), i));
        }

        let candidates = ["Bookcase/CBETA/", "CBETA/", "", "cbeta/Bookcase/CBETA/"];
        let prefix = candidates
            .iter()
            .find(|p| all.iter().any(|(n, _)| *n == format!("{}index.xml", p)))
            .map(|p| p.to_string())
            .ok_or_else(|| "index.xml not found in the zip (expected Bookcase/CBETA/index.xml)".to_string())?;

        let mut names = HashMap::with_capacity(all.len());
        for (n, i) in all {
            if let Some(rel) = n.strip_prefix(prefix.as_str()) {
                if !rel.is_empty() && !rel.ends_with('/') {
                    names.insert(rel.to_string(), i);
                }
            }
        }
        Ok(ZipSource {
            archive: Mutex::new(archive),
            names,
        })
    }
}

impl Source for ZipSource {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let index = *self.names.get(path)?;
        let mut archive = self.archive.lock().ok()?;
        let mut entry = archive.by_index(index).ok()?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    fn exists(&self, path: &str) -> bool {
        self.names.contains_key(path)
    }

    fn list(&self) -> Vec<String> {
        self.names.keys().cloned().collect()
    }
}

/// An unzipped bookcase directory.
pub struct DirSource {
    root: PathBuf,
}

impl DirSource {
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, String> {
        for sub in ["", "CBETA", "Bookcase/CBETA", "cbeta/Bookcase/CBETA"] {
            let root = dir.as_ref().join(sub);
            if root.join("index.xml").is_file() {
                return Ok(DirSource { root });
            }
        }
        Err("index.xml not found in the directory".to_string())
    }
}

impl Source for DirSource {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        fs::read(self.root.join(path)).ok()
    }

    fn exists(&self, path: &str) -> bool {
        self.root.join(path).is_file()
    }

    fn list(&self) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            if let Ok(rd) = fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        walk(&p, base, out);
                    } else if let Ok(rel) = p.strip_prefix(base) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out);
        out
    }
}

/// In-memory source, mainly for tests.
#[derive(Default)]
pub struct MemSource {
    files: HashMap<String, Vec<u8>>,
}

impl MemSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, path: &str, data: impl Into<Vec<u8>>) -> &mut Self {
        self.files.insert(path.to_string(), data.into());
        self
    }
}

impl Source for MemSource {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path).cloned()
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn list(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_and_gbk_entry_names() {
        assert_eq!(decode_entry_name(b"XML/T/T01/T01n0004_001.xml"), "XML/T/T01/T01n0004_001.xml");
        assert_eq!(decode_entry_name("a/LZ/經.html".as_bytes()), "a/LZ/經.html");
        // "大乘" in GBK
        assert_eq!(decode_entry_name(&[b'a', b'/', 0xB4, 0xF3, 0xB3, 0xCB]), "a/大乘");
    }
}
