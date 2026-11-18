//! Writer for AppXI `.words` dictionary files.
//!
//! The layout is the one [`crate::dict`] reads (header, title part, content
//! part). It follows the old Java `DictionaryBuilder`:
//!
//! * every title sits in the title part, so a search maps the title part and
//!   scans it without touching the definitions;
//! * definitions are zlib-compressed, deduplicated by content, and several
//!   titles may point at the same definition offset;
//! * each title also gets a lowercase, a simplified and a traditional variant
//!   (when it differs and is not already present), so readers of either
//!   character set find the entry.
//!
//! The builder does not sort titles: the source order is kept, and variants
//! are appended after the original entries, matching the old builder.

use crate::analyzer;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

const ENTRY_BASE_SIZE: usize = 17;

#[derive(Debug, Clone, Default)]
pub struct BuildStats {
    /// records written to the title part, variants included
    pub entries: usize,
    /// how many of `entries` are generated variants
    pub variants: usize,
    /// entries dropped because the title was empty or too long
    pub skipped: usize,
    /// bytes of the (uncompressed) content part and the whole file
    pub content_bytes: u64,
    pub file_bytes: u64,
}

/// Write `entries` (title, content) into `path` as a `.words` file.
///
/// `metadata` is stored as UTF-8 JSON in the header; callers usually put the
/// dictionary name, the import format and the source path there.
pub fn build_words(
    path: &Path,
    metadata: &str,
    entries: Vec<(String, String)>,
) -> Result<BuildStats, String> {
    let mut stats = BuildStats::default();

    // One shared content string per distinct content; variants clone the Arc.
    let mut content_pool: HashMap<(u64, u64), Arc<str>> = HashMap::new();
    let mut seen: HashSet<(String, (u64, u64))> = HashSet::new();
    let mut raw: Vec<(String, Arc<str>)> = Vec::with_capacity(entries.len());
    for (title, content) in entries {
        if title.is_empty() {
            stats.skipped += 1;
            continue;
        }
        let key = content_key(&content);
        if !seen.insert((title.clone(), key)) {
            stats.skipped += 1;
            continue;
        }
        let content: Arc<str> = match content_pool.get(&key) {
            Some(existing) => existing.clone(),
            None => {
                let arc: Arc<str> = Arc::from(content.as_str());
                content_pool.insert(key, arc.clone());
                arc
            }
        };
        raw.push((title, content));
    }

    // Title variants only reference titles that do not exist yet.
    let mut present: HashSet<String> = raw.iter().map(|(t, _)| t.clone()).collect();
    let mut present_folded: HashSet<String> = raw.iter().map(|(t, _)| t.to_lowercase()).collect();
    let mut variants: Vec<(String, Arc<str>)> = Vec::new();
    for (title, content) in &raw {
        for variant in title_variants(title) {
            if variant.is_empty() || variant == *title {
                continue;
            }
            let folded = variant.to_lowercase();
            if !present_folded.insert(folded) {
                continue;
            }
            if present.insert(variant.clone()) {
                variants.push((variant, content.clone()));
            }
        }
    }
    stats.variants = variants.len();
    raw.extend(variants);

    let meta_bytes = metadata.as_bytes();
    let mut title_part: Vec<u8> = Vec::with_capacity(raw.len() * 24);
    let mut content_part: Vec<u8> = Vec::new();
    let mut content_offsets: HashMap<(u64, u64), u64> = HashMap::new();
    let mut written = 0usize;

    for (title, content) in &raw {
        let title_bytes = title.as_bytes();
        let entry_size = ENTRY_BASE_SIZE + title_bytes.len();
        if entry_size > i16::MAX as usize {
            stats.skipped += 1;
            continue;
        }
        let key = content_key(content);
        let content_mark = match content_offsets.get(&key) {
            Some(off) => *off,
            None => {
                let off = content_part.len() as u64;
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder
                    .write_all(content.as_bytes())
                    .map_err(|e| format!("压缩词义失败: {e}"))?;
                let packed = encoder.finish().map_err(|e| format!("压缩词义失败: {e}"))?;
                if packed.len() > i32::MAX as usize {
                    return Err(format!("词条「{}」的词义过大", title));
                }
                content_part.extend_from_slice(&(packed.len() as i32).to_be_bytes());
                content_part.extend_from_slice(&packed);
                content_offsets.insert(key, off);
                off
            }
        };

        title_part.extend_from_slice(&(entry_size as i16).to_be_bytes());
        title_part.extend_from_slice(&(-1i32).to_be_bytes()); // pid: no category tree
        title_part.push(1u8); // TYPE_ITEM
        title_part.extend_from_slice(&(content_mark as i64).to_be_bytes());
        title_part.extend_from_slice(&(title_bytes.len() as i16).to_be_bytes());
        title_part.extend_from_slice(title_bytes);
        written += 1;
    }
    stats.entries = written;
    stats.content_bytes = content_part.len() as u64;

    let header_len = 8 + 2 + 1 + 4 + meta_bytes.len() + 4 + 4 + 16;
    let content_start = (header_len + title_part.len()) as i64;
    let content_len = content_part.len() as i64;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dictionary.words".to_string());
    let tmp_path = path.with_file_name(format!(".{file_name}.tmp"));
    {
        let file = File::create(&tmp_path)
            .map_err(|e| format!("创建词典文件失败 {}: {e}", tmp_path.display()))?;
        let mut out = BufWriter::new(file);
        out.write_all(&3i64.to_be_bytes()).map_err(write_err)?;
        out.write_all(&0i16.to_be_bytes()).map_err(write_err)?; // UTF-8
        out.write_all(&[1u8]).map_err(write_err)?; // zlib
        out.write_all(&(meta_bytes.len() as i32).to_be_bytes())
            .map_err(write_err)?;
        out.write_all(meta_bytes).map_err(write_err)?;
        out.write_all(&(written as i32).to_be_bytes())
            .map_err(write_err)?;
        out.write_all(&2i32.to_be_bytes()).map_err(write_err)?;
        out.write_all(&content_start.to_be_bytes())
            .map_err(write_err)?;
        out.write_all(&content_len.to_be_bytes())
            .map_err(write_err)?;
        out.write_all(&title_part).map_err(write_err)?;
        out.write_all(&content_part).map_err(write_err)?;
        out.flush().map_err(write_err)?;
    }
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("替换词典文件失败 {}: {e}", path.display()))?;
    }
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("写入词典文件失败 {}: {e}", path.display())
    })?;
    stats.file_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(stats)
}

fn write_err(e: std::io::Error) -> String {
    format!("写入词典失败: {e}")
}

/// Lowercase, simplified and traditional forms of a title.
fn title_variants(title: &str) -> [String; 3] {
    let lower = title.to_lowercase();
    let simplified: String = title.chars().map(analyzer::to_simplified).collect();
    let traditional: String = title.chars().map(analyzer::to_traditional).collect();
    [lower, simplified, traditional]
}

/// A 128-bit content fingerprint. Used only for deduplication; a collision
/// between two definitions would be astronomically unlikely.
fn content_key(s: &str) -> (u64, u64) {
    let mut a = 0xcbf2_9ce4_8422_2325u64;
    let mut b = 0x8422_2325_cbf2_9ce4u64;
    for &byte in s.as_bytes() {
        a ^= byte as u64;
        a = a.wrapping_mul(0x0000_0100_0000_01b3);
        b = b.rotate_left(5) ^ (byte as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        b = b.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::{Dictionary, MatchType};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sl_dict_build_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_readable_words_with_variants() {
        let dir = temp_dir("variants");
        let path = dir.join("测试.words");
        let stats = build_words(
            &path,
            r#"{"kind":"test"}"#,
            vec![
                ("四聖諦".to_string(), "苦集灭道。".to_string()),
                ("Four Noble Truths".to_string(), "苦集灭道。".to_string()),
                ("".to_string(), "空标题".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(stats.entries, 3);
        assert_eq!(stats.variants, 1);

        let dict = Dictionary::open(&path).unwrap();
        assert_eq!(dict.size, 3);
        for title in ["四聖諦", "四圣谛", "four noble truths", "FOUR NOBLE TRUTHS"] {
            let hits = dict.search(title, MatchType::TitleEquals, 10);
            assert_eq!(hits.len(), 1, "missing title {title}");
        }
        let hits = dict.search("四聖諦", MatchType::TitleEquals, 10);
        let detail = dict.read_entry_detail(hits[0].entry_id).unwrap();
        assert!(detail.text.contains("苦集灭道"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shares_one_definition_between_titles() {
        let dir = temp_dir("dedup");
        let shared = dir.join("共享.words");
        build_words(
            &shared,
            "",
            vec![
                ("甲".to_string(), "同样的词义".to_string()),
                ("乙".to_string(), "同样的词义".to_string()),
            ],
        )
        .unwrap();
        let distinct = dir.join("不同.words");
        build_words(
            &distinct,
            "",
            vec![
                ("甲".to_string(), "第一个词义".to_string()),
                ("乙".to_string(), "第二个词义".to_string()),
            ],
        )
        .unwrap();
        // the shared definition is stored once, so the file is smaller
        assert!(
            std::fs::metadata(&shared).unwrap().len() < std::fs::metadata(&distinct).unwrap().len()
        );
        let dict = Dictionary::open(&shared).unwrap();
        assert_eq!(dict.search("甲", MatchType::TitleEquals, 10).len(), 1);
        assert_eq!(dict.search("乙", MatchType::TitleEquals, 10).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
