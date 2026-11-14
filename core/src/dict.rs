//! Reader and searcher for AppXI `.words` dictionary files (and `.words_extra`
//! zip resource companions).
//!
//! Binary layout (all integers big-endian, matching Java `DataOutput` / `ByteBuffer`):
//! - Header:
//!   - `version: i64` (3)
//!   - `charset_type: i16` (0=UTF-8, 1=UTF-16BE, 2=UTF-16LE, 3=US-ASCII, 4=ISO-8859-1)
//!   - `compression: u8` (0=none, 1=zlib, 2=gzip)
//!   - `metadata_len: i32`, followed by `metadata_len` UTF-8 bytes
//!   - `size: i32` (total entry count, including categories)
//!   - `content_part_positions_len: i32`, followed by `i64` offsets; `[0]` is the
//!     file offset where the content part begins.
//! - Title part (from end of header up to `content_part_start`), sequential records:
//!   - `entry_size: i16` (`17 + title_len`)
//!   - `pid: i32`
//!   - `entry_type: u8` (`0` = category, `1` = item)
//!   - `content_mark: i64` (children count for category; offset relative to `content_part_start` for item)
//!   - `title_len: i16`
//!   - `title_bytes: [u8; title_len]`
//! - Content part (starting at `content_part_start`):
//!   - At `content_mark`: `content_len: i32`, followed by `content_len` compressed bytes.

use base64::Engine as _;
use flate2::read::{GzDecoder, ZlibDecoder};
use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use zip::ZipArchive;

pub const FILE_SUFFIX: &str = ".words";
pub const TYPE_CATEGORY: u8 = 0;
pub const TYPE_ITEM: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictCharset {
    Utf8,
    Utf16Be,
    Utf16Le,
    Ascii,
    Iso8859_1,
}

impl DictCharset {
    fn from_code(code: i16) -> Self {
        match code {
            1 => Self::Utf16Be,
            2 => Self::Utf16Le,
            3 => Self::Ascii,
            4 => Self::Iso8859_1,
            _ => Self::Utf8,
        }
    }

    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 | Self::Ascii => String::from_utf8_lossy(bytes).into_owned(),
            Self::Utf16Be => {
                let (cow, _, _) = encoding_rs::UTF_16BE.decode(bytes);
                cow.into_owned()
            }
            Self::Utf16Le => {
                let (cow, _, _) = encoding_rs::UTF_16LE.decode(bytes);
                cow.into_owned()
            }
            Self::Iso8859_1 => bytes.iter().map(|&b| b as char).collect(),
        }
    }

    pub fn encode(self, s: &str) -> Vec<u8> {
        match self {
            Self::Utf8 | Self::Ascii => s.as_bytes().to_vec(),
            Self::Utf16Be => s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect(),
            Self::Utf16Le => s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
            Self::Iso8859_1 => s.chars().map(|c| if (c as u32) <= 0xFF { c as u8 } else { b'?' }).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictCompression {
    None,
    Zlib,
    Gzip,
}

impl DictCompression {
    fn from_code(code: u8) -> Result<Self, String> {
        match code {
            0 => Ok(Self::None),
            1 => Ok(Self::Zlib),
            2 => Ok(Self::Gzip),
            other => Err(format!("不支持的词典压缩格式: {other}")),
        }
    }

    fn decompress(self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        match self {
            Self::None => Ok(bytes.to_vec()),
            Self::Zlib => {
                let mut decoder = ZlibDecoder::new(bytes);
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| format!("zlib 解压失败: {e}"))?;
                Ok(out)
            }
            Self::Gzip => {
                let mut decoder = GzDecoder::new(bytes);
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| format!("gzip 解压失败: {e}"))?;
                Ok(out)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchType {
    TitleEquals,
    TitleContains,
    TitleStartsWith,
    TitleEndsWith,
}

impl MatchType {
    /// Detect match mode from query syntax, mirroring Java `MatchType.detect`:
    /// - `"..."` / `“...”` / `'...'` / `‘...’` -> `TitleEquals`
    /// - `*...*` -> `TitleContains`
    /// - `*...` -> `TitleEndsWith`
    /// - `...*` or default -> `TitleStartsWith`
    pub fn detect(raw: &str) -> (Self, String) {
        let trimmed = raw.trim();
        let chars: Vec<char> = trimmed.chars().collect();
        if chars.is_empty() {
            return (Self::TitleStartsWith, String::new());
        }
        if chars.len() > 1 {
            let first = chars[0];
            let last = chars[chars.len() - 1];
            if (first == '"' && last == '"')
                || (first == '“' && last == '”')
                || (first == '\'' && last == '\'')
                || (first == '‘' && last == '’')
            {
                let inner: String = chars[1..chars.len() - 1].iter().collect();
                return (Self::TitleEquals, inner);
            }
            if first == '*' && last == '*' {
                let inner: String = chars[1..chars.len() - 1].iter().collect();
                return (Self::TitleContains, inner);
            }
            if first == '*' {
                let inner: String = chars[1..].iter().collect();
                return (Self::TitleEndsWith, inner);
            }
            if last == '*' {
                let inner: String = chars[..chars.len() - 1].iter().collect();
                return (Self::TitleStartsWith, inner);
            }
        } else if chars[0] == '*' {
            return (Self::TitleStartsWith, String::new());
        }
        (Self::TitleStartsWith, trimmed.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: usize,
    pub enabled: bool,
    pub has_extra: bool,
    pub metadata: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictSearchHit {
    pub dict_id: String,
    pub dict_name: String,
    pub entry_id: u32,
    pub title: String,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictSearchResult {
    pub items: Vec<DictSearchHit>,
    pub total_found: usize,
    pub has_more: bool,
    pub total_entries: usize,
    pub elapsed_ms: u64,
    pub match_type: MatchType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictEntryDetail {
    pub dict_id: String,
    pub dict_name: String,
    pub entry_id: u32,
    pub title: String,
    pub html: String,
    pub text: String,
}

pub struct Dictionary {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub version: i64,
    pub charset: DictCharset,
    pub compression: DictCompression,
    pub metadata: String,
    pub size: usize,
    title_part_start: usize,
    content_part_start: usize,
    mmap: Mmap,
    extra_path: PathBuf,
}

impl Dictionary {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("打开词典失败 {}: {e}", path.display()))?;
        let file_len = file
            .metadata()
            .map_err(|e| format!("读取词典元信息失败 {}: {e}", path.display()))?
            .len();
        // Safety: dictionary files are read-only immutable binary databases.
        let mmap = unsafe {
            Mmap::map(&file).map_err(|e| format!("映射词典失败 {}: {e}", path.display()))?
        };
        let data = &mmap[..];
        let mut cursor = 0usize;

        let version = read_i64_be(data, &mut cursor)?;
        if version != 3 {
            return Err(format!("不支持的词典版本 {version}: {}", path.display()));
        }
        let charset_code = read_i16_be(data, &mut cursor)?;
        let charset = DictCharset::from_code(charset_code);
        let comp_code = read_u8(data, &mut cursor)?;
        let compression = DictCompression::from_code(comp_code)?;

        let meta_len = read_i32_be(data, &mut cursor)?.max(0) as usize;
        if cursor + meta_len > data.len() {
            return Err(format!("词典元数据越界: {}", path.display()));
        }
        let metadata = String::from_utf8_lossy(&data[cursor..cursor + meta_len]).into_owned();
        cursor += meta_len;

        let size = read_i32_be(data, &mut cursor)?.max(0) as usize;
        let pos_len = read_i32_be(data, &mut cursor)?.max(0) as usize;
        if pos_len < 2 {
            return Err(format!("词典内容分片表无效: {}", path.display()));
        }
        let mut positions = Vec::with_capacity(pos_len);
        for _ in 0..pos_len {
            positions.push(read_i64_be(data, &mut cursor)?);
        }
        let content_part_start = positions[0] as usize;
        let title_part_start = cursor;
        if title_part_start > content_part_start || content_part_start > data.len() {
            return Err(format!("词典分区偏移无效: {}", path.display()));
        }

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未命名词典".to_string());
        let id = format!("{:08x}_{}", stable_hash(&name), file_len);
        let extra_path = PathBuf::from(format!("{}_extra", path.display()));

        Ok(Self {
            id,
            name,
            path: path.to_path_buf(),
            version,
            charset,
            compression,
            metadata,
            size,
            title_part_start,
            content_part_start,
            mmap,
            extra_path,
        })
    }

    pub fn has_extra(&self) -> bool {
        self.extra_path.is_file()
            && self
                .extra_path
                .metadata()
                .map(|m| m.len() > 1)
                .unwrap_or(false)
    }

    pub fn search(
        &self,
        keywords: &str,
        match_type: MatchType,
        limit: usize,
    ) -> Vec<DictSearchHit> {
        let mut hits = Vec::new();
        if limit == 0 {
            return hits;
        }
        let title_part = &self.mmap[self.title_part_start..self.content_part_start];
        let kw_lower = keywords.to_lowercase();
        let kw_bytes = if kw_lower.is_empty() {
            Vec::new()
        } else {
            self.charset.encode(&kw_lower)
        };
        let kw_len = kw_bytes.len();
        let ascii_ci = matches!(
            self.charset,
            DictCharset::Utf8 | DictCharset::Ascii | DictCharset::Iso8859_1
        );
        let char_step = match self.charset {
            DictCharset::Utf16Be | DictCharset::Utf16Le => 2,
            _ => 1,
        };

        let mut pos = 0usize;
        for _ in 0..self.size {
            if pos + 17 > title_part.len() {
                break;
            }
            let entry_size = i16::from_be_bytes([title_part[pos], title_part[pos + 1]]) as usize;
            if entry_size < 17 || pos + entry_size > title_part.len() {
                break;
            }
            let entry_type = title_part[pos + 6];
            let title_len =
                i16::from_be_bytes([title_part[pos + 15], title_part[pos + 16]]) as usize;

            let entry_id = pos as u32;
            pos += entry_size;

            if entry_type == TYPE_CATEGORY {
                continue;
            }
            if title_len > entry_size - 17 || title_len < kw_len {
                continue;
            }

            let title_bytes = &title_part[entry_id as usize + 17..entry_id as usize + 17 + title_len];

            let score = if kw_len == 0 {
                Some(10)
            } else if bytes_eq(title_bytes, &kw_bytes, ascii_ci) {
                Some(100)
            } else {
                match match_type {
                    MatchType::TitleEquals => None,
                    MatchType::TitleStartsWith => {
                        if bytes_eq(&title_bytes[..kw_len], &kw_bytes, ascii_ci) {
                            Some(10)
                        } else {
                            None
                        }
                    }
                    MatchType::TitleEndsWith => {
                        if bytes_eq(&title_bytes[title_len - kw_len..], &kw_bytes, ascii_ci) {
                            Some(10)
                        } else {
                            None
                        }
                    }
                    MatchType::TitleContains => {
                        if contains_bytes(title_bytes, &kw_bytes, ascii_ci, char_step) {
                            Some(10)
                        } else {
                            None
                        }
                    }
                }
            };

            if let Some(score) = score {
                hits.push(DictSearchHit {
                    dict_id: self.id.clone(),
                    dict_name: self.name.clone(),
                    entry_id,
                    title: self.charset.decode(title_bytes),
                    score,
                });
                if hits.len() >= limit {
                    break;
                }
            }
        }
        hits
    }

    pub fn read_entry_detail(&self, entry_id: u32) -> Result<DictEntryDetail, String> {
        let title_part = &self.mmap[self.title_part_start..self.content_part_start];
        let pos = entry_id as usize;
        if pos + 17 > title_part.len() {
            return Err("词条偏移越界".to_string());
        }
        let entry_size = i16::from_be_bytes([title_part[pos], title_part[pos + 1]]) as usize;
        if entry_size < 17 || pos + entry_size > title_part.len() {
            return Err("词条记录损坏".to_string());
        }
        let entry_type = title_part[pos + 6];
        let content_mark = i64::from_be_bytes([
            title_part[pos + 7],
            title_part[pos + 8],
            title_part[pos + 9],
            title_part[pos + 10],
            title_part[pos + 11],
            title_part[pos + 12],
            title_part[pos + 13],
            title_part[pos + 14],
        ]);
        let title_len = i16::from_be_bytes([title_part[pos + 15], title_part[pos + 16]]) as usize;
        if title_len > entry_size - 17 {
            return Err("词条标题长度越界".to_string());
        }
        let title = self.charset.decode(&title_part[pos + 17..pos + 17 + title_len]);

        let raw_content = if entry_type == TYPE_CATEGORY {
            String::new()
        } else {
            self.read_content_text(content_mark)?
        };

        let html = self.format_html(&title, &raw_content);
        let text = format!("{title}\n{raw_content}\n——【{}】", self.name);

        Ok(DictEntryDetail {
            dict_id: self.id.clone(),
            dict_name: self.name.clone(),
            entry_id,
            title,
            html,
            text,
        })
    }

    fn read_content_text(&self, content_mark: i64) -> Result<String, String> {
        if content_mark < 0 {
            return Err("词条内容偏移无效".to_string());
        }
        let content_part = &self.mmap[self.content_part_start..];
        let offset = content_mark as usize;
        if offset + 4 > content_part.len() {
            return Err("词条内容长度越界".to_string());
        }
        let len = i32::from_be_bytes([
            content_part[offset],
            content_part[offset + 1],
            content_part[offset + 2],
            content_part[offset + 3],
        ]);
        if len < 0 || offset + 4 + (len as usize) > content_part.len() {
            return Err("词条内容数据越界".to_string());
        }
        let compressed = &content_part[offset + 4..offset + 4 + (len as usize)];
        let raw = self.compression.decompress(compressed)?;
        Ok(self.charset.decode(&raw))
    }

    fn get_extra_resource(&self, name: &str) -> Option<Vec<u8>> {
        if !self.has_extra() {
            return None;
        }
        let file = File::open(&self.extra_path).ok()?;
        let mut archive = ZipArchive::new(file).ok()?;
        let target = name.to_lowercase();
        let mut matched_idx = None;
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                if entry.name().to_lowercase() == target {
                    matched_idx = Some(i);
                    break;
                }
            }
        }
        let idx = matched_idx?;
        let mut entry = archive.by_index(idx).ok()?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    /// Convert raw dictionary entry content to HTML, matching `DictionaryHelper.toHtmlDocument`.
    fn format_html(&self, title: &str, raw: &str) -> String {
        let mut s = if raw.is_empty() {
            "<>".to_string()
        } else {
            raw.to_string()
        };
        s = s
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("`1`", "<b>")
            .replace("`2`", "</b>")
            .replace("`6`", "\n")
            .replace("`3`", "<div>")
            .replace("`5`", "<div>")
            .replace("`7`", "</div>")
            .replace("`4`", "")
            .replace("`9`", "");

        // <SEEALSO>term</SEEALSO> -> clickable link handled by frontend
        s = s.replace(
            "<SEEALSO>",
            &format!(
                "<a href=\"#\" class=\"dict-seealso\" data-dict-id=\"{}\">",
                escape_html_attr(&self.id)
            ),
        );
        s = s.replace("</SEEALSO>", "</a>");

        s = s
            .replace("【】：", "【词义】：")
            .replace("【derivation】：", "【词源】：")
            .replace("【strokes】：", "【笔画】：")
            .replace("【pinyin】：", "【拼音】：")
            .replace("【abbreviation】：", "【简拼】：")
            .replace("【radicals】：", "【部首】：")
            .replace("【example】：", "【示例】：")
            .replace("【more】：", "【更多】：")
            .replace("【oldword】：", "【古字】：");

        s = s
            .replace("\\r", "")
            .replace("\\n", "<br>")
            .replace("\r\n", "<br>")
            .replace(['\r', '\n'], "<br>");

        if s.contains("又【") {
            s = insert_br_before_bracket(&s);
            s = s.replace("<br>【", "<br>　【");
            if s.starts_with('〔') {
                s = format!("　{s}");
            }
            if s.starts_with('【') {
                s = format!("　{s}");
            }
        }
        if s.contains("。　又") {
            s = s.replace("。　又", "。<br>又");
        }

        if self.has_extra() {
            s = self.embed_pics(&s);
        }

        s = sanitize_dict_html(&s);

        format!(
            "<div class=\"dict-entry-block\" data-dict=\"{}|{}\">\
             <h2 class=\"dict-entry-title\"><a href=\"#\" class=\"dict-seealso\" data-dict-id=\"{}\">{}</a></h2>\
             <div class=\"dict-entry-source\">——《{}》</div>\
             <div class=\"dict-entry-body\">{}</div>\
             </div>",
            escape_html_attr(&self.id),
            escape_html_attr(&self.name),
            escape_html_attr(&self.id),
            escape_html_text(title),
            escape_html_text(&self.name),
            s
        )
    }

    fn embed_pics(&self, input: &str) -> String {
        let mut out = input.to_string();
        let mut search_from = 0usize;
        while let Some(rel_start) = out[search_from..].find("<PIC>") {
            let start = search_from + rel_start;
            let Some(rel_end) = out[start + 5..].find("</PIC>") else {
                break;
            };
            let end = start + 5 + rel_end;
            let pic_name = &out[start + 5..end];
            let valid_ext = pic_name
                .rsplit_once('.')
                .map(|(_, ext)| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "jpg" | "jpeg" | "png" | "bmp" | "gif"
                    )
                })
                .unwrap_or(false);
            if valid_ext {
                if let Some(bytes) = self.get_extra_resource(pic_name) {
                    let ext = pic_name
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_ascii_lowercase())
                        .unwrap_or_else(|| "png".to_string());
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let img = format!("<img src=\"data:image/{ext};base64,{b64}\" />");
                    out.replace_range(start..end + 6, &img);
                    search_from = start + img.len();
                    continue;
                }
            }
            search_from = end + 6;
        }
        out
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DictConfig {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
}

pub struct DictManager {
    config: DictConfig,
    dicts: Vec<Arc<Dictionary>>,
}

impl DictManager {
    pub fn from_config(config: DictConfig) -> Self {
        let mut mgr = Self {
            config,
            dicts: Vec::new(),
        };
        mgr.reload();
        mgr
    }

    pub fn config(&self) -> &DictConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: DictConfig) {
        self.config = config;
        self.reload();
    }

    pub fn reload(&mut self) {
        self.dicts.clear();
        let mut seen_paths = HashSet::new();
        let mut files = Vec::new();
        for raw in &self.config.paths {
            let p = PathBuf::from(raw);
            collect_words_files(&p, &mut seen_paths, &mut files);
        }
        files.sort();
        for f in files {
            if let Ok(d) = Dictionary::open(&f) {
                self.dicts.push(Arc::new(d));
            }
        }
    }

    pub fn list(&self) -> Vec<DictInfo> {
        let disabled: HashSet<&str> = self.config.disabled.iter().map(|s| s.as_str()).collect();
        self.dicts
            .iter()
            .map(|d| DictInfo {
                id: d.id.clone(),
                name: d.name.clone(),
                path: d.path.to_string_lossy().into_owned(),
                size: d.size,
                enabled: !disabled.contains(d.id.as_str()),
                has_extra: d.has_extra(),
                metadata: d.metadata.clone(),
            })
            .collect()
    }

    fn selected_dicts(&self, scope_ids: Option<&[String]>) -> Vec<Arc<Dictionary>> {
        match scope_ids {
            Some(ids) => {
                let set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
                self.dicts
                    .iter()
                    .filter(|d| set.contains(d.id.as_str()))
                    .cloned()
                    .collect()
            }
            None => {
                let disabled: HashSet<&str> =
                    self.config.disabled.iter().map(|s| s.as_str()).collect();
                self.dicts
                    .iter()
                    .filter(|d| !disabled.contains(d.id.as_str()))
                    .cloned()
                    .collect()
            }
        }
    }

    pub fn search(
        &self,
        raw_query: &str,
        scope_ids: Option<&[String]>,
        offset: usize,
        limit: usize,
    ) -> DictSearchResult {
        let t0 = Instant::now();
        let (match_type, keywords) = MatchType::detect(raw_query);
        let dicts = self.selected_dicts(scope_ids);
        let total_entries: usize = dicts.iter().map(|d| d.size).sum();

        let per_dict_limit = offset.saturating_add(limit).saturating_add(1);
        let mut all_hits: Vec<DictSearchHit> = if dicts.len() >= 4 {
            std::thread::scope(|s| {
                let handles: Vec<_> = dicts
                    .iter()
                    .map(|d| {
                        let kw = &keywords;
                        s.spawn(move || d.search(kw, match_type, per_dict_limit))
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            })
        } else {
            dicts
                .iter()
                .flat_map(|d| d.search(&keywords, match_type, per_dict_limit))
                .collect()
        };

        // Exact title matches first, preserving dictionary/entry order otherwise.
        all_hits.sort_by_key(|a| std::cmp::Reverse(a.score));
        let total_found = all_hits.len();
        let has_more = total_found > offset.saturating_add(limit);
        let items: Vec<DictSearchHit> = all_hits.into_iter().skip(offset).take(limit).collect();

        DictSearchResult {
            items,
            total_found,
            has_more,
            total_entries,
            elapsed_ms: t0.elapsed().as_millis() as u64,
            match_type,
        }
    }

    pub fn get_entry(&self, dict_id: &str, entry_id: u32) -> Result<DictEntryDetail, String> {
        let dict = self
            .dicts
            .iter()
            .find(|d| d.id == dict_id)
            .ok_or_else(|| format!("未找到词典: {dict_id}"))?;
        dict.read_entry_detail(entry_id)
    }

    pub fn lookup_exact(
        &self,
        word: &str,
        scope_ids: Option<&[String]>,
        limit: usize,
    ) -> Vec<DictEntryDetail> {
        let trimmed = word.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let dicts = self.selected_dicts(scope_ids);
        let mut details = Vec::new();
        for d in &dicts {
            let hits = d.search(trimmed, MatchType::TitleEquals, 10);
            for h in hits {
                if let Ok(detail) = d.read_entry_detail(h.entry_id) {
                    details.push(detail);
                    if details.len() >= limit {
                        return details;
                    }
                }
            }
        }
        details
    }
}

fn collect_words_files(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    if !path.exists() {
        return;
    }
    if path.is_file() {
        let is_words = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_lowercase().ends_with(FILE_SUFFIX))
            .unwrap_or(false);
        if is_words {
            let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if seen.insert(canon.clone()) {
                out.push(canon);
            }
        }
    } else if path.is_dir() {
        if let Ok(rd) = std::fs::read_dir(path) {
            for entry in rd.flatten() {
                collect_words_files(&entry.path(), seen, out);
            }
        }
    }
}

fn read_u8(data: &[u8], cursor: &mut usize) -> Result<u8, String> {
    if *cursor >= data.len() {
        return Err("词典文件头意外结束".to_string());
    }
    let v = data[*cursor];
    *cursor += 1;
    Ok(v)
}

fn read_i16_be(data: &[u8], cursor: &mut usize) -> Result<i16, String> {
    if *cursor + 2 > data.len() {
        return Err("词典文件头意外结束".to_string());
    }
    let v = i16::from_be_bytes([data[*cursor], data[*cursor + 1]]);
    *cursor += 2;
    Ok(v)
}

fn read_i32_be(data: &[u8], cursor: &mut usize) -> Result<i32, String> {
    if *cursor + 4 > data.len() {
        return Err("词典文件头意外结束".to_string());
    }
    let v = i32::from_be_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ]);
    *cursor += 4;
    Ok(v)
}

fn read_i64_be(data: &[u8], cursor: &mut usize) -> Result<i64, String> {
    if *cursor + 8 > data.len() {
        return Err("词典文件头意外结束".to_string());
    }
    let v = i64::from_be_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
        data[*cursor + 4],
        data[*cursor + 5],
        data[*cursor + 6],
        data[*cursor + 7],
    ]);
    *cursor += 8;
    Ok(v)
}

fn bytes_eq(a: &[u8], b: &[u8], ascii_ci: bool) -> bool {
    if ascii_ci {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8], ascii_ci: bool, step: usize) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    let step = step.max(1);
    let end = haystack.len() - needle.len();
    if ascii_ci {
        (0..=end)
            .step_by(step)
            .any(|i| haystack[i..i + needle.len()].eq_ignore_ascii_case(needle))
    } else {
        (0..=end)
            .step_by(step)
            .any(|i| &haystack[i..i + needle.len()] == needle)
    }
}

fn stable_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

fn escape_html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_attr(s: &str) -> String {
    escape_html_text(s).replace('"', "&quot;")
}

fn insert_br_before_bracket(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut prev: Option<char> = None;
    for ch in s.chars() {
        if ch == '【' {
            if let Some(p) = prev {
                if p != '】' && p != '又' && p != '>' {
                    out.push_str("<br>　");
                }
            }
        }
        out.push(ch);
        prev = Some(ch);
    }
    out
}

const DICT_ALLOWED_TAGS: &[&str] = &[
    "a", "b", "big", "blockquote", "br", "caption", "code", "dd", "div", "dl", "dt", "em", "font",
    "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "li", "mark", "ol", "p", "pre", "rp",
    "rt", "ruby", "s", "small", "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot",
    "th", "thead", "tr", "u", "ul",
];

const DICT_VOID_TAGS: &[&str] = &["br", "hr", "img"];

const DICT_DROP_CONTENT_TAGS: &[&str] = &["script", "style", "iframe", "object", "embed", "svg", "math"];

const DICT_STRIP_TAGS: &[&str] = &[
    "applet", "audio", "base", "body", "button", "canvas", "form", "frame", "frameset", "head",
    "html", "input", "link", "meta", "noscript", "option", "select", "source", "template",
    "textarea", "title", "track", "video",
];

/// Sanitize dictionary HTML body from an allowlist of formatting tags and attributes.
/// Non-HTML bracketed text such as `<>` or `<梵>` is escaped so it remains visible.
fn sanitize_dict_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let next = input[i..].find('<').map(|p| i + p).unwrap_or(bytes.len());
            out.push_str(&input[i..next].replace('>', "&gt;"));
            i = next;
            continue;
        }
        let rest = &input[i..];
        if rest.starts_with("<!--") {
            i = rest.find("-->").map(|p| i + p + 3).unwrap_or(bytes.len());
            continue;
        }
        let Some(end_rel) = find_tag_end(rest) else {
            out.push_str("&lt;");
            i += 1;
            continue;
        };
        let raw_inner = &rest[1..end_rel];
        let trimmed = raw_inner.trim();
        if let Some(closing) = trimmed.strip_prefix('/') {
            let name = closing.trim().to_ascii_lowercase();
            if DICT_ALLOWED_TAGS.contains(&name.as_str()) && !DICT_VOID_TAGS.contains(&name.as_str()) {
                out.push_str("</");
                out.push_str(&name);
                out.push('>');
            } else if !DICT_DROP_CONTENT_TAGS.contains(&name.as_str())
                && !DICT_STRIP_TAGS.contains(&name.as_str())
            {
                out.push_str("&lt;");
                out.push_str(&escape_html_text(raw_inner));
                out.push_str("&gt;");
            }
            i += end_rel + 1;
            continue;
        }

        let body = trimmed.trim_end_matches('/').trim_end();
        let (raw_name, attr_str) = match body.find(|c: char| c.is_whitespace()) {
            Some(p) => (&body[..p], &body[p..]),
            None => (body, ""),
        };
        let name = raw_name.to_ascii_lowercase();
        let valid_name = !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric());
        if !valid_name {
            out.push_str("&lt;");
            out.push_str(&escape_html_text(raw_inner));
            out.push_str("&gt;");
            i += end_rel + 1;
            continue;
        }

        if DICT_DROP_CONTENT_TAGS.contains(&name.as_str()) {
            let after_open = i + end_rel + 1;
            if trimmed.ends_with('/') {
                i = after_open;
                continue;
            }
            let close_pat = format!("</{name}");
            let lower_tail = input[after_open..].to_ascii_lowercase();
            if let Some(cpos) = lower_tail.find(&close_pat) {
                let close_start = after_open + cpos;
                let close_end = input[close_start..]
                    .find('>')
                    .map(|p| close_start + p + 1)
                    .unwrap_or(bytes.len());
                i = close_end;
            } else {
                i = bytes.len();
            }
            continue;
        }

        if DICT_STRIP_TAGS.contains(&name.as_str()) {
            i += end_rel + 1;
            continue;
        }

        if !DICT_ALLOWED_TAGS.contains(&name.as_str()) {
            out.push_str("&lt;");
            out.push_str(&escape_html_text(raw_inner));
            out.push_str("&gt;");
            i += end_rel + 1;
            continue;
        }

        out.push('<');
        out.push_str(&name);
        for (k, v) in parse_dict_attrs(attr_str) {
            if is_safe_dict_attr(&name, &k, &v) {
                out.push(' ');
                out.push_str(&k);
                out.push_str("=\"");
                out.push_str(&escape_html_attr(&v));
                out.push('"');
            }
        }
        if DICT_VOID_TAGS.contains(&name.as_str()) && trimmed.ends_with('/') {
            out.push_str(" />");
        } else {
            out.push('>');
        }
        i += end_rel + 1;
    }
    out
}

fn find_tag_end(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut quote: u8 = 0;
    for (idx, &b) in bytes.iter().enumerate().skip(1) {
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'"' || b == b'\'' {
            quote = b;
        } else if b == b'>' {
            return Some(idx);
        } else if b == b'<' {
            return None;
        }
    }
    None
}

fn parse_dict_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'/' {
            break;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != b'/' {
            i += 1;
        }
        let key = s[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let val = if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let q = bytes[i];
                i += 1;
                let vstart = i;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
                let v = s[vstart..i].to_string();
                if i < bytes.len() {
                    i += 1;
                }
                v
            } else {
                let vstart = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                s[vstart..i].to_string()
            }
        } else {
            String::new()
        };
        if !key.is_empty() {
            out.push((key, val));
        }
    }
    out
}

fn is_safe_dict_attr(tag: &str, key: &str, val: &str) -> bool {
    if key.starts_with("on") {
        return false;
    }
    match (tag, key) {
        (_, "class" | "title") => true,
        ("a", "data-dict-id") => true,
        ("a", "href") => {
            let v = val.trim();
            v == "#"
                || v.len() >= 7 && v[..7].eq_ignore_ascii_case("http://")
                || v.len() >= 8 && v[..8].eq_ignore_ascii_case("https://")
        }
        ("img", "src") => {
            let v = val.trim().to_ascii_lowercase();
            (v.starts_with("data:image/png;base64,")
                || v.starts_with("data:image/jpeg;base64,")
                || v.starts_with("data:image/jpg;base64,")
                || v.starts_with("data:image/gif;base64,")
                || v.starts_with("data:image/bmp;base64,"))
                && v.split_once(";base64,")
                    .is_some_and(|(_, b64)| b64.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=')))
        }
        ("img", "alt" | "width" | "height") => true,
        ("font", "color" | "size") => !val.contains(':') && !val.contains('('),
        ("td" | "th" | "tr" | "table" | "p" | "div", "colspan" | "rowspan" | "align" | "valign") => {
            !val.contains(':') && !val.contains('(')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn build_synthetic_words(path: &Path, charset_code: i16, entries: &[(&str, &str)]) {
        let charset = DictCharset::from_code(charset_code);
        let meta = br#"{"build-name":"test"}"#;
        let mut title_part = Vec::new();
        let mut content_part = Vec::new();

        // Root category
        let root_title = charset.encode("ROOT");
        let root_size = (17 + root_title.len()) as i16;
        title_part.extend_from_slice(&root_size.to_be_bytes());
        title_part.extend_from_slice(&(-1i32).to_be_bytes());
        title_part.push(TYPE_CATEGORY);
        title_part.extend_from_slice(&(entries.len() as i64).to_be_bytes());
        title_part.extend_from_slice(&(root_title.len() as i16).to_be_bytes());
        title_part.extend_from_slice(&root_title);

        for (title, body) in entries {
            let content_mark = content_part.len() as i64;
            let raw_body = charset.encode(body);
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&raw_body).unwrap();
            let comp_body = enc.finish().unwrap();
            content_part.extend_from_slice(&(comp_body.len() as i32).to_be_bytes());
            content_part.extend_from_slice(&comp_body);

            let t_bytes = charset.encode(title);
            let e_size = (17 + t_bytes.len()) as i16;
            title_part.extend_from_slice(&e_size.to_be_bytes());
            title_part.extend_from_slice(&0i32.to_be_bytes());
            title_part.push(TYPE_ITEM);
            title_part.extend_from_slice(&content_mark.to_be_bytes());
            title_part.extend_from_slice(&(t_bytes.len() as i16).to_be_bytes());
            title_part.extend_from_slice(&t_bytes);
        }

        let header_len = 8 + 2 + 1 + 4 + meta.len() + 4 + 4 + 16;
        let content_start = (header_len + title_part.len()) as i64;
        let content_len = content_part.len() as i64;

        let mut file = File::create(path).unwrap();
        file.write_all(&3i64.to_be_bytes()).unwrap();
        file.write_all(&charset_code.to_be_bytes()).unwrap();
        file.write_all(&[1u8]).unwrap();
        file.write_all(&(meta.len() as i32).to_be_bytes()).unwrap();
        file.write_all(meta).unwrap();
        file.write_all(&((entries.len() + 1) as i32).to_be_bytes()).unwrap();
        file.write_all(&2i32.to_be_bytes()).unwrap();
        file.write_all(&content_start.to_be_bytes()).unwrap();
        file.write_all(&content_len.to_be_bytes()).unwrap();
        file.write_all(&title_part).unwrap();
        file.write_all(&content_part).unwrap();
    }

    #[test]
    fn match_type_detect_rules() {
        assert_eq!(MatchType::detect("四谛"), (MatchType::TitleStartsWith, "四谛".into()));
        assert_eq!(MatchType::detect("四谛*"), (MatchType::TitleStartsWith, "四谛".into()));
        assert_eq!(MatchType::detect("*四谛"), (MatchType::TitleEndsWith, "四谛".into()));
        assert_eq!(MatchType::detect("*四谛*"), (MatchType::TitleContains, "四谛".into()));
        assert_eq!(MatchType::detect("\"四谛\""), (MatchType::TitleEquals, "四谛".into()));
        assert_eq!(MatchType::detect("“四谛”"), (MatchType::TitleEquals, "四谛".into()));
    }

    #[test]
    fn read_and_search_synthetic_words() {
        let dir = std::env::temp_dir().join(format!("sl_dict_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let words_path = dir.join("测试词典.words");
        build_synthetic_words(
            &words_path,
            1, // UTF-16BE
            &[
                ("四谛", "苦集灭道四谛。"),
                ("四圣谛", "又称四谛。"),
                ("八正道", "正见乃至正定。"),
            ],
        );

        let dict = Dictionary::open(&words_path).unwrap();
        assert_eq!(dict.size, 4);
        assert_eq!(dict.charset, DictCharset::Utf16Be);

        let hits = dict.search("四", MatchType::TitleStartsWith, 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "四谛");

        let exact = dict.search("四谛", MatchType::TitleEquals, 10);
        assert_eq!(exact.len(), 1);
        let detail = dict.read_entry_detail(exact[0].entry_id).unwrap();
        assert!(detail.text.contains("苦集灭道四谛"));
        assert!(detail.html.contains("测试词典"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_html_blocks_scripts_and_handlers() {
        let dir = std::env::temp_dir().join(format!("sl_dict_xss_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let words_path = dir.join("安全测试.words");
        build_synthetic_words(
            &words_path,
            0, // UTF-8
            &[(
                "测试<script>",
                "`1`粗体`2`<SEEALSO>四谛</SEEALSO>&lt;script&gt;alert(1)&lt;/script&gt;<img src=x onerror=alert(1)><a href=\"javascript:evil()\" onclick=\"steal()\">链接</a><梵>",
            )],
        );

        let dict = Dictionary::open(&words_path).unwrap();
        let hits = dict.search("测试<script>", MatchType::TitleEquals, 10);
        assert_eq!(hits.len(), 1);
        let detail = dict.read_entry_detail(hits[0].entry_id).unwrap();
        assert!(!detail.html.contains("<script"));
        assert!(!detail.html.contains("alert(1)"));
        assert!(!detail.html.contains("onerror"));
        assert!(!detail.html.contains("onclick"));
        assert!(!detail.html.contains("javascript:"));
        assert!(detail.html.contains("<b>粗体</b>"));
        assert!(detail.html.contains("class=\"dict-seealso\""));
        assert!(detail.html.contains("&lt;梵&gt;"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
