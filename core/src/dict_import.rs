//! Import dictionaries in common formats and store them as `.words`.
//!
//! Supported sources:
//! * **TXT** — one `【词条】词义` per line, the rule of the old Java
//!   `DictFileTxt` (`\\n` stands for a line break);
//! * **StarDict** — `.ifo` + `.idx` (or `.idx.gz`) + `.dict` (or `.dict.dz`),
//!   with or without `sametypesequence`;
//! * **MDict** — `.mdx`, optionally with an `.mdd` resource file. Engine
//!   version 1 and 2 are read; zlib compression and the `Encrypted="2"`
//!   (key block info encrypted) flavour are supported. LZO compression and
//!   record-block encryption (`Encrypted="1"`) are not.
//!
//! The old Java sources are `appxi-dictionary-api` (`DictFileTxt`,
//! `DictFileMdx`, `DictionaryBuilder`); this module follows their binary
//! layout, including MDict's `_fast_decrypt` key-info cipher and the
//! `@@@LINK=` redirect records.

use crate::dict_build::{build_words, BuildStats};
use crate::text::decode_auto;
use encoding_rs::{Encoding, GB18030, UTF_16BE, UTF_16LE, UTF_8};
use flate2::read::GzDecoder;
use memmap2::Mmap;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

// What the format readers hand back. Spelled out as aliases: the tuples are
// long enough that clippy's `type_complexity` (denied in CI) flags them.

/// `(dictionary name, entries as `(词条, 词义)`, skipped lines)` of a TXT file.
type TxtDict = (String, Vec<(String, String)>, usize);

/// `(name, entries, resource files, skipped entries)` of a StarDict dictionary.
type StarDictDict = (String, Vec<(String, String)>, Vec<(String, Vec<u8>)>, usize);

/// `(name, entries, resource files, warnings, skipped entries)` of an MDict dictionary.
type MdxDict = (String, Vec<(String, String)>, Vec<(String, Vec<u8>)>, Vec<String>, usize);

/// One decoded dictionary entry: its text and the files (pictures) it refers to.
type DecodedEntry = (String, Vec<(String, Vec<u8>)>);

/// Result of importing one dictionary.
#[derive(Debug, Clone)]
pub struct ImportOutcome {
    pub source: String,
    pub name: String,
    pub output: String,
    pub format: &'static str,
    pub entries: usize,
    pub variants: usize,
    pub skipped: usize,
    pub has_extra: bool,
    pub warnings: Vec<String>,
}

/// Import one file (or a directory, recursively) into `out_dir`.
///
/// A directory yields one outcome per dictionary; MDD files are handled by
/// their companion MDX and are not imported on their own.
pub fn import_path(path: &Path, out_dir: &Path) -> Result<Vec<ImportOutcome>, String> {
    if !path.exists() {
        return Err(format!("找不到词典文件：{}", path.display()));
    }
    fs::create_dir_all(out_dir)
        .map_err(|e| format!("创建词典目录失败 {}: {e}", out_dir.display()))?;
    if path.is_dir() {
        import_directory(path, out_dir)
    } else {
        Ok(vec![import_file(path, out_dir)?])
    }
}

fn import_directory(dir: &Path, out_dir: &Path) -> Result<Vec<ImportOutcome>, String> {
    let mut files = Vec::new();
    collect_sources(dir, &mut files)?;
    files.sort();
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut used_names: HashSet<String> = HashSet::new();
    for file in files {
        let Some(stem) = file_stem(&file) else {
            continue;
        };
        match import_file_with_name(&file, out_dir, &stem, &mut used_names) {
            Ok(o) => out.push(o),
            Err(e) => errors.push(e),
        }
    }
    if out.is_empty() {
        if let Some(e) = errors.first() {
            return Err(format!("没有导入任何词典：{e}"));
        }
        return Err("目录中没有可导入的词典".to_string());
    }
    if !errors.is_empty() {
        if let Some(last) = out.last_mut() {
            last.warnings.extend(errors);
        }
    }
    Ok(out)
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("读取目录失败 {}: {e}", dir.display()))?;
    let mut names: HashSet<String> = HashSet::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, out)?;
        } else {
            if let Some(name) = path
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
            {
                names.insert(name);
            }
            files.push(path);
        }
    }
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let stem_lower = stem_lower(&path);
        let kind = source_kind(&path);
        match kind {
            Some(SourceKind::Mdd) => {
                // the MDX beside it reads the resources
                if !(names.contains(&format!("{stem_lower}.mdx"))) {
                    // report by trying it; import_file explains the pairing
                    out.push(path);
                }
            }
            Some(SourceKind::StarDict) => {
                // one source per dictionary: .ifo, else .idx, else .idx.gz,
                // else .dict, else .dict.dz (the others are companions)
                let base = star_dict_base(&path)
                    .ok()
                    .and_then(|b| {
                        b.file_name()
                            .map(|n| n.to_string_lossy().to_ascii_lowercase())
                    })
                    .unwrap_or_default();
                let has_ifo = names.contains(&format!("{base}.ifo"));
                let has_idx = names.contains(&format!("{base}.idx"));
                let has_idx_gz = names.contains(&format!("{base}.idx.gz"));
                let has_dict = names.contains(&format!("{base}.dict"));
                let keep = if name.ends_with(".ifo") {
                    has_ifo
                } else if name.ends_with(".idx") {
                    !has_ifo && has_idx
                } else if name.ends_with(".idx.gz") {
                    !has_ifo && !has_idx && has_idx_gz
                } else if name.ends_with(".dict") {
                    !has_ifo && !has_idx && !has_idx_gz
                } else if name.ends_with(".dict.dz") {
                    !has_ifo && !has_idx && !has_idx_gz && !has_dict
                } else {
                    false
                };
                if keep {
                    out.push(path);
                }
            }
            Some(SourceKind::Mdx) | Some(SourceKind::Txt) => out.push(path),
            None => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Txt,
    StarDict,
    Mdx,
    Mdd,
}

fn source_kind(path: &Path) -> Option<SourceKind> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".txt") {
        Some(SourceKind::Txt)
    } else if name.ends_with(".ifo")
        || name.ends_with(".idx")
        || name.ends_with(".idx.gz")
        || name.ends_with(".dict")
        || name.ends_with(".dict.dz")
    {
        Some(SourceKind::StarDict)
    } else if name.ends_with(".mdx") {
        Some(SourceKind::Mdx)
    } else if name.ends_with(".mdd") {
        Some(SourceKind::Mdd)
    } else {
        None
    }
}

fn import_file(path: &Path, out_dir: &Path) -> Result<ImportOutcome, String> {
    let stem = file_stem(path).ok_or_else(|| format!("无法识别文件名：{}", path.display()))?;
    import_file_with_name(path, out_dir, &stem, &mut HashSet::new())
}

fn import_file_with_name(
    path: &Path,
    out_dir: &Path,
    stem: &str,
    used_names: &mut HashSet<String>,
) -> Result<ImportOutcome, String> {
    let kind = source_kind(path).ok_or_else(|| {
        format!(
            "不支持的词典格式：{}（支持 TXT、StarDict、MDX/MDD）",
            path.display()
        )
    })?;
    // an MDD is only a resource companion; import its MDX instead
    let actual = if kind == SourceKind::Mdd {
        let mdx = companion_path(path, "mdx")
            .ok_or_else(|| format!("{} 是 MDD 资源文件，需要同名的 MDX 词典", path.display()))?;
        mdx
    } else {
        path.to_path_buf()
    };
    let kind = source_kind(&actual).unwrap_or(kind);

    let mut warnings = Vec::new();
    let mut resources: Vec<(String, Vec<u8>)> = Vec::new();
    let (name, entries, parse_skipped) = match kind {
        SourceKind::Txt => read_txt(&actual)?,
        SourceKind::StarDict => {
            let (name, entries, res, skipped) = read_stardict(&actual, &mut warnings)?;
            resources = res;
            (name, entries, skipped)
        }
        SourceKind::Mdx => {
            let (name, entries, res, mut warns, skipped) = read_mdx(&actual)?;
            resources = res;
            warnings.append(&mut warns);
            (name, entries, skipped)
        }
        SourceKind::Mdd => unreachable!(),
    };
    if entries.is_empty() {
        return Err(format!("没有从 {} 解析到词条", actual.display()));
    }
    let name = if name.trim().is_empty() {
        stem.to_string()
    } else {
        name
    };
    let base = unique_name(&sanitize_file_name(&name), used_names);
    let words_path = out_dir.join(format!("{base}.words"));
    let extra_path = out_dir.join(format!("{base}.words_extra"));

    let metadata = serde_json::json!({
        "name": name,
        "format": match kind {
            SourceKind::Txt => "txt",
            SourceKind::StarDict => "stardict",
            SourceKind::Mdx => "mdx",
            SourceKind::Mdd => "mdd",
        },
        "source": actual.to_string_lossy(),
        "imported": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
    .to_string();

    let stats: BuildStats = build_words(&words_path, &metadata, entries)?;
    write_extra(&extra_path, resources)?;
    Ok(ImportOutcome {
        source: actual.to_string_lossy().into_owned(),
        name,
        output: words_path.to_string_lossy().into_owned(),
        format: match kind {
            SourceKind::Txt => "txt",
            SourceKind::StarDict => "stardict",
            SourceKind::Mdx => "mdx",
            SourceKind::Mdd => "mdd",
        },
        entries: stats.entries,
        variants: stats.variants,
        skipped: stats.skipped + parse_skipped,
        has_extra: extra_path.is_file(),
        warnings,
    })
}

fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

fn file_stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let lower = name.to_ascii_lowercase();
    for suffix in [
        ".txt", ".ifo", ".idx.gz", ".idx", ".dict.dz", ".dict", ".mdx", ".mdd",
    ] {
        if lower.ends_with(suffix) {
            return Some(name[..name.len() - suffix.len()].to_string());
        }
    }
    path.file_stem().map(|s| s.to_string_lossy().into_owned())
}

fn stem_lower(path: &Path) -> String {
    file_stem(path).unwrap_or_default().to_ascii_lowercase()
}

fn companion_path(path: &Path, ext: &str) -> Option<PathBuf> {
    let stem = file_stem(path)?;
    let dir = path.parent()?;
    let wanted = format!("{stem}.{ext}").to_ascii_lowercase();
    for entry in fs::read_dir(dir).ok()?.flatten() {
        if entry.file_name().to_string_lossy().to_ascii_lowercase() == wanted {
            return Some(entry.path());
        }
    }
    let p = dir.join(format!("{stem}.{ext}"));
    p.is_file().then_some(p)
}

fn sanitize_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_space = false;
    for c in name.chars() {
        let c = if c.is_control()
            || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        {
            '_'
        } else {
            c
        };
        if c == ' ' || c == '\u{3000}' {
            if last_space {
                continue;
            }
            last_space = true;
            out.push(' ');
        } else {
            last_space = false;
            out.push(c);
        }
    }
    let out = out.trim().trim_matches('.').trim().to_string();
    let out = if out.is_empty() {
        "dictionary".to_string()
    } else {
        out
    };
    out.chars().take(100).collect()
}

fn write_extra(path: &Path, resources: Vec<(String, Vec<u8>)>) -> Result<(), String> {
    if resources.is_empty() {
        if path.exists() {
            fs::remove_file(path)
                .map_err(|e| format!("清理旧图片资源失败 {}: {e}", path.display()))?;
        }
        return Ok(());
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "extra".to_string());
    let tmp = path.with_file_name(format!(".{file_name}.tmp"));
    {
        let file =
            File::create(&tmp).map_err(|e| format!("创建图片资源失败 {}: {e}", tmp.display()))?;
        let mut zip = ZipWriter::new(file);
        let mut seen = HashSet::new();
        for (name, data) in resources {
            if name.is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            let method = if already_compressed(&name) {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };
            let opts = SimpleFileOptions::default().compression_method(method);
            zip.start_file(name, opts)
                .map_err(|e| format!("写入图片资源失败: {e}"))?;
            zip.write_all(&data)
                .map_err(|e| format!("写入图片资源失败: {e}"))?;
        }
        zip.finish().map_err(|e| format!("写入图片资源失败: {e}"))?;
    }
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("替换图片资源失败 {}: {e}", path.display()))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("保存图片资源失败 {}: {e}", path.display())
    })
}

fn already_compressed(name: &str) -> bool {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "webp"
            | "avif"
            | "mp3"
            | "ogg"
            | "wav"
            | "spx"
            | "mp4"
            | "avi"
            | "zip"
            | "gz"
    )
}

fn read_text_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("读取文件失败 {}: {e}", path.display()))?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        || bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
    {
        return Ok(decode_auto(&bytes));
    }
    match std::str::from_utf8(&bytes) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Ok(GB18030.decode(&bytes).0.into_owned()),
    }
}

fn read_txt(path: &Path) -> Result<TxtDict, String> {
    let text = read_text_file(path)?;
    let name = file_stem(path).unwrap_or_else(|| "未命名词典".to_string());
    let mut entries = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((head, body)) = line.split_once('】') else {
            skipped += 1;
            continue;
        };
        let title = clean_title(&head.replace('【', ""));
        if title.is_empty() {
            skipped += 1;
            continue;
        }
        let content = body.replace("\\\\n", "\n").trim().to_string();
        entries.push((title, content));
    }
    Ok((name, entries, skipped))
}

fn clean_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut space = false;
    for c in raw.chars() {
        let c = match c {
            '\u{FEFF}' | '\u{0090}' | '\u{00A0}' | '\r' | '\n' | '\t' => ' ',
            c => c,
        };
        if c == ' ' || c == '\u{3000}' {
            if !space {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(c);
            space = false;
        }
    }
    out.trim().to_string()
}

// ---------------------------------------------------------------- StarDict

fn read_stardict(path: &Path, warnings: &mut Vec<String>) -> Result<StarDictDict, String> {
    let base = star_dict_base(path)?;
    let (ifo, idx, dict) = locate_stardict(&base).ok_or_else(|| {
        format!(
            "StarDict 文件不完整，需要 .idx 和 .dict：{}",
            path.display()
        )
    })?;
    let mut name = file_stem(path).unwrap_or_else(|| "未命名词典".to_string());
    let mut sametypesequence: Option<Vec<u8>> = None;
    let mut offset_bits = 32u32;
    if let Some(ifo) = &ifo {
        let text = read_text_file(ifo)?;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "bookname" => {
                    let v = value.trim();
                    if !v.is_empty() {
                        name = v.to_string();
                    }
                }
                "sametypesequence" => {
                    let bytes = value.trim().as_bytes();
                    if !bytes.is_empty() {
                        sametypesequence = Some(bytes.to_vec());
                    }
                }
                "idxoffsetbits" => {
                    if let Ok(v) = value.trim().parse::<u32>() {
                        offset_bits = v;
                    }
                }
                _ => {}
            }
        }
    }
    let idx_bytes = read_maybe_gz(&idx)?;
    let dict_bytes = read_maybe_gz(&dict)?;
    let offset_len = if offset_bits == 64 { 8 } else { 4 };
    let mut entries = Vec::new();
    let mut resources = Vec::new();
    let mut skipped = 0usize;
    let mut pos = 0usize;
    let mut entry_no = 0usize;
    while pos < idx_bytes.len() {
        let Some(z) = idx_bytes[pos..].iter().position(|&b| b == 0) else {
            break;
        };
        let word = &idx_bytes[pos..pos + z];
        pos += z + 1;
        if pos + offset_len + 4 > idx_bytes.len() {
            warnings.push("StarDict 索引在末尾被截断".to_string());
            break;
        }
        let offset = if offset_len == 8 {
            u64::from_be_bytes(idx_bytes[pos..pos + 8].try_into().unwrap())
        } else {
            u32::from_be_bytes(idx_bytes[pos..pos + 4].try_into().unwrap()) as u64
        };
        pos += offset_len;
        let size = u32::from_be_bytes(idx_bytes[pos..pos + 4].try_into().unwrap()) as u64;
        pos += 4;
        let start = offset as usize;
        let end = start.saturating_add(size as usize);
        if end > dict_bytes.len() {
            warnings.push(format!(
                "StarDict 词条数据越界，已跳过：{}",
                String::from_utf8_lossy(word)
            ));
            skipped += 1;
            continue;
        }
        let title = clean_title(&String::from_utf8_lossy(word));
        if title.is_empty() {
            skipped += 1;
            continue;
        }
        let data = &dict_bytes[start..end];
        let (content, mut res) = decode_stardict_data(data, sametypesequence.as_deref(), entry_no)?;
        resources.append(&mut res);
        entry_no += 1;
        if content.trim().is_empty() {
            skipped += 1;
            continue;
        }
        entries.push((title, content));
    }
    Ok((name, entries, resources, skipped))
}

fn star_dict_base(path: &Path) -> Result<PathBuf, String> {
    let original = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = original.to_ascii_lowercase();
    // longest suffixes first; each branch must strip the whole suffix
    // (.idx is four characters, .dict is five)
    for suffix in [".dict.dz", ".idx.gz", ".dict", ".idx", ".ifo"] {
        if lower.ends_with(suffix) {
            let stem = original[..original.len() - suffix.len()].to_string();
            return Ok(path.with_file_name(stem));
        }
    }
    let stem =
        file_stem(path).ok_or_else(|| format!("无法识别 StarDict 文件名：{}", path.display()))?;
    Ok(path.with_file_name(stem))
}

fn locate_stardict(base: &Path) -> Option<(Option<PathBuf>, PathBuf, PathBuf)> {
    let parent = base.parent()?;
    let stem = base.file_name()?.to_string_lossy().into_owned();
    let wanted = |ext: &str| parent.join(format!("{stem}{ext}"));
    let exists_ci = |p: &Path| {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
        let target = p.file_name()?.to_string_lossy().to_ascii_lowercase();
        for entry in fs::read_dir(parent).ok()?.flatten() {
            if entry.file_name().to_string_lossy().to_ascii_lowercase() == target {
                return Some(entry.path());
            }
        }
        None
    };
    let ifo = exists_ci(&wanted(".ifo"));
    let idx = exists_ci(&wanted(".idx")).or_else(|| exists_ci(&wanted(".idx.gz")))?;
    let dict = exists_ci(&wanted(".dict")).or_else(|| exists_ci(&wanted(".dict.dz")))?;
    Some((ifo, idx, dict))
}

fn read_maybe_gz(path: &Path) -> Result<Vec<u8>, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".gz") || name.ends_with(".dz") {
        let file =
            File::open(path).map_err(|e| format!("打开压缩词典失败 {}: {e}", path.display()))?;
        let mut decoder = GzDecoder::new(file);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| format!("解压词典失败 {}: {e}", path.display()))?;
        Ok(out)
    } else {
        fs::read(path).map_err(|e| format!("读取词典失败 {}: {e}", path.display()))
    }
}

fn decode_stardict_data(data: &[u8], sametype: Option<&[u8]>, entry_no: usize) -> Result<DecodedEntry, String> {
    let mut text = String::new();
    let mut resources = Vec::new();
    if let Some(types) = sametype {
        let mut pos = 0usize;
        for (i, &ty) in types.iter().enumerate() {
            let last = i + 1 == types.len();
            let part = if last {
                let p = &data[pos.min(data.len())..];
                pos = data.len();
                p
            } else {
                if pos + 4 > data.len() {
                    break;
                }
                let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                let end = (pos + len).min(data.len());
                let p = &data[pos..end];
                pos = end;
                p
            };
            append_stardict_part(ty, part, entry_no, i, &mut text, &mut resources)?;
        }
    } else {
        // each section starts with a type byte; text sections are NUL
        // terminated, binary sections carry a 4-byte length
        let mut pos = 0usize;
        let mut seq = 0usize;
        while pos < data.len() {
            let ty = data[pos];
            pos += 1;
            let part;
            if is_stardict_text_type(ty) {
                let Some(z) = data[pos..].iter().position(|&b| b == 0) else {
                    part = &data[pos..];
                    append_stardict_part(ty, part, entry_no, seq, &mut text, &mut resources)?;
                    break;
                };
                part = &data[pos..pos + z];
                pos += z + 1;
            } else {
                if pos + 4 > data.len() {
                    break;
                }
                let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                let end = (pos + len).min(data.len());
                part = &data[pos..end];
                pos = end;
            }
            append_stardict_part(ty, part, entry_no, seq, &mut text, &mut resources)?;
            seq += 1;
        }
        if text.is_empty() && !data.is_empty() {
            // unknown layout: keep the raw text rather than losing the entry
            text = String::from_utf8_lossy(data).into_owned();
        }
    }
    Ok((text.trim().to_string(), resources))
}

fn is_stardict_text_type(ty: u8) -> bool {
    matches!(
        ty,
        b'm' | b'l' | b'h' | b'x' | b't' | b'y' | b'k' | b'w' | b'n' | b'r'
    )
}

fn append_stardict_part(
    ty: u8,
    part: &[u8],
    entry_no: usize,
    seq: usize,
    text: &mut String,
    resources: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), String> {
    match ty {
        b'm' | b'l' | b't' | b'y' | b'n' | b'r' => {
            let s = String::from_utf8_lossy(part);
            if !text.is_empty() && !s.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(&s);
        }
        b'h' | b'k' | b'w' => {
            let s = String::from_utf8_lossy(part);
            if !text.is_empty() && !s.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(&clean_markup(&s));
        }
        b'x' => {
            let s = String::from_utf8_lossy(part);
            if !text.is_empty() && !s.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(&markup_to_text(&s));
        }
        b'g' | b'P' => {
            if let Some((name, data)) = image_resource(part, entry_no, seq) {
                text.push_str(&format!("<PIC>{name}</PIC>"));
                resources.push((name, data));
            }
        }
        _ => {}
    }
    Ok(())
}

fn image_resource(data: &[u8], entry_no: usize, seq: usize) -> Option<(String, Vec<u8>)> {
    if data.is_empty() {
        return None;
    }
    let ext = if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if data.starts_with(b"GIF8") {
        "gif"
    } else if data.starts_with(b"BM") {
        "bmp"
    } else {
        return None;
    };
    let name = format!("stardict/{entry_no:06}_{seq}.{ext}");
    Some((name, data.to_vec()))
}

// -------------------------------------------------------------------- MDict

struct MdxFile {
    data: Mmap,
    number_width: usize,
    encoding: &'static Encoding,
    delimiter_width: usize,
    key_sensitive: bool,
    key_blocks: Vec<KeyBlockInfo>,
    key_block_start: usize,
    record_blocks: Vec<RecordBlockInfo>,
    record_data_start: usize,
    total_record_len: u64,
    cache_index: Option<usize>,
    cache: Vec<u8>,
}

struct KeyBlockInfo {
    num_entries: u64,
    comp_size: u64,
    decomp_size: u64,
    comp_acc: u64,
}

struct RecordBlockInfo {
    comp_size: u64,
    decomp_size: u64,
    comp_acc: u64,
    decomp_acc: u64,
}

struct MdxEntry {
    key: String,
    offset: u64,
    next: u64,
}

fn parse_key_blocks(
    data: &[u8],
    version: f32,
    delimiter_width: usize,
    count: u64,
) -> Result<(Vec<KeyBlockInfo>, usize), String> {
    let mut blocks = Vec::with_capacity(count as usize);
    let mut p = 0usize;
    let mut comp_acc = 0u64;
    for _ in 0..count {
        let (num_entries, comp_size, decomp_size) = if version >= 2.0 {
            let n = be_u64(data, p)?;
            p += 8;
            let hlen = be_u16(data, p)? as usize;
            p += 2;
            p += hlen * delimiter_width + delimiter_width;
            let tlen = be_u16(data, p)? as usize;
            p += 2;
            p += tlen * delimiter_width + delimiter_width;
            let c = be_u64(data, p)?;
            p += 8;
            let d = be_u64(data, p)?;
            p += 8;
            (n, c, d)
        } else {
            let n = be_u32(data, p)? as u64;
            p += 4;
            let hlen = data.get(p).copied().unwrap_or(0) as usize;
            p += 1 + hlen;
            let tlen = data.get(p).copied().unwrap_or(0) as usize;
            p += 1 + tlen;
            let c = be_u32(data, p)? as u64;
            p += 4;
            let d = be_u32(data, p)? as u64;
            p += 4;
            (n, c, d)
        };
        if p > data.len() {
            return Err("MDX 键块信息越界".to_string());
        }
        blocks.push(KeyBlockInfo {
            num_entries,
            comp_size,
            decomp_size,
            comp_acc,
        });
        comp_acc += comp_size;
    }
    Ok((blocks, p))
}

/// A key block info parse is accepted when nothing but zero padding follows.
fn consumes_all(data: &[u8], consumed: usize) -> bool {
    consumed <= data.len() && data[consumed..].iter().all(|&b| b == 0)
}

impl MdxFile {
    fn open(path: &Path) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|e| format!("打开 MDX 失败 {}: {e}", path.display()))?;
        // Safety: dictionary files are read-only immutable binary databases.
        let mmap = unsafe {
            Mmap::map(&file).map_err(|e| format!("映射 MDX 失败 {}: {e}", path.display()))?
        };
        if mmap.len() < 8 {
            return Err("MDX 文件过短".to_string());
        }
        let header_size = be_u32(&mmap, 0)? as usize;
        if header_size == 0 || 4 + header_size + 4 > mmap.len() {
            return Err("MDX 文件头长度无效".to_string());
        }
        let header_bytes = &mmap[4..4 + header_size];
        let header = String::from_utf16_lossy(
            &header_bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect::<Vec<_>>(),
        );
        let attrs = parse_header_attrs(&header);
        let version: f32 = attrs
            .get("GeneratedByEngineVersion")
            .and_then(|v| v.parse().ok())
            .unwrap_or(2.0);
        let encrypted: u8 = match attrs.get("Encrypted").map(|s| s.trim()) {
            Some("Yes") => 1,
            Some(v) => v.parse().unwrap_or(0),
            None => 0,
        };
        if encrypted & 1 != 0 {
            return Err("这个 MDX 的词条块已加密（Encrypted=1），暂不支持".to_string());
        }
        let mut encoding = match attrs
            .get("Encoding")
            .map(|s| s.trim().to_ascii_uppercase())
            .unwrap_or_else(|| "UTF-8".to_string())
            .as_str()
        {
            "GBK" | "GB2312" | "GB18030" => GB18030,
            "UTF-16" | "UTF-16LE" => UTF_16LE,
            "UTF-16BE" => UTF_16BE,
            other => Encoding::for_label(other.as_bytes()).unwrap_or(UTF_8),
        };
        let mut delimiter_width = if encoding == UTF_16LE || encoding == UTF_16BE {
            2
        } else {
            1
        };
        let key_sensitive = attrs
            .get("KeyCaseSensitive")
            .map(|v| v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false);
        let number_width = if version < 2.0 { 4 } else { 8 };
        let mut pos = 4 + header_size + 4;
        let read_number = |data: &[u8], pos: &mut usize| -> Result<u64, String> {
            let v = if number_width == 4 {
                be_u32(data, *pos)? as u64
            } else {
                be_u64(data, *pos)?
            };
            *pos += number_width;
            Ok(v)
        };
        let num_key_blocks = read_number(&mmap, &mut pos)?;
        let _num_entries = read_number(&mmap, &mut pos)?;
        let key_info_decomp = if version >= 2.0 {
            read_number(&mmap, &mut pos)?
        } else {
            0
        };
        let key_info_size = read_number(&mmap, &mut pos)?;
        let key_block_size = read_number(&mmap, &mut pos)?;
        if version >= 2.0 {
            pos += 4; // adler32 of the five numbers
        }
        let key_info_start = pos;
        let key_block_start = key_info_start + key_info_size as usize;
        let record_block_start = key_block_start + key_block_size as usize;
        if record_block_start > mmap.len() {
            return Err("MDX 分区偏移越界".to_string());
        }
        let key_info_raw = &mmap[key_info_start..key_info_start + key_info_size as usize];
        let key_info = if encrypted & 2 != 0 {
            mdx_decrypt(key_info_raw)?
        } else {
            key_info_raw.to_vec()
        };
        let key_info_data = if version >= 2.0 {
            let decomp = decompress_block(&key_info)?;
            let _ = key_info_decomp; // writers disagree about this count; parsing decides
            decomp
        } else {
            key_info
        };
        // MDD files often leave Encoding empty but still store their key block
        // info and keys as UTF-16LE. Try the header's width first, then the
        // other one, and accept the parse that consumes the whole block.
        let (key_blocks, _) =
            match parse_key_blocks(&key_info_data, version, delimiter_width, num_key_blocks) {
                Ok((blocks, consumed)) if consumes_all(&key_info_data, consumed) => {
                    (blocks, delimiter_width)
                }
                first => {
                    let other = if delimiter_width == 1 { 2 } else { 1 };
                    match parse_key_blocks(&key_info_data, version, other, num_key_blocks) {
                        Ok((blocks, consumed)) if consumes_all(&key_info_data, consumed) => {
                            delimiter_width = other;
                            encoding = if other == 2 { UTF_16LE } else { UTF_8 };
                            (blocks, other)
                        }
                        _ => {
                            return Err(first
                                .err()
                                .unwrap_or_else(|| "MDX 键块信息无法解析".to_string()))
                        }
                    }
                }
            };
        let mut record_blocks = Vec::new();
        let mut record_pos = record_block_start;
        let num_record_blocks = read_number(&mmap, &mut record_pos)?;
        let _record_entries = read_number(&mmap, &mut record_pos)?;
        let record_info_size = read_number(&mmap, &mut record_pos)?;
        let _record_block_size = read_number(&mmap, &mut record_pos)?;
        let record_info_start = record_pos;
        let record_data_start = record_info_start + record_info_size as usize;
        let mut comp_acc = 0u64;
        let mut decomp_acc = 0u64;
        let mut rp = record_info_start;
        for _ in 0..num_record_blocks {
            let c = read_number(&mmap, &mut rp)?;
            let d = read_number(&mmap, &mut rp)?;
            record_blocks.push(RecordBlockInfo {
                comp_size: c,
                decomp_size: d,
                comp_acc,
                decomp_acc,
            });
            comp_acc += c;
            decomp_acc += d;
        }
        if record_data_start > mmap.len() {
            return Err("MDX 词条数据偏移越界".to_string());
        }
        Ok(Self {
            data: mmap,
            number_width,
            encoding,
            delimiter_width,
            key_sensitive,
            key_blocks,
            key_block_start,
            record_blocks,
            record_data_start,
            total_record_len: decomp_acc,
            cache_index: None,
            cache: Vec::new(),
        })
    }

    fn entries(&mut self) -> Result<Vec<MdxEntry>, String> {
        let mut all = Vec::new();
        for i in 0..self.key_blocks.len() {
            let (keys, offsets) = self.read_key_block(i)?;
            for (key, offset) in keys.into_iter().zip(offsets) {
                all.push(MdxEntry {
                    key,
                    offset,
                    next: 0,
                });
            }
        }
        for i in 0..all.len() {
            all[i].next = if i + 1 < all.len() {
                all[i + 1].offset
            } else {
                self.total_record_len
            };
        }
        Ok(all)
    }

    fn read_key_block(&self, block: usize) -> Result<(Vec<String>, Vec<u64>), String> {
        let info = self
            .key_blocks
            .get(block)
            .ok_or_else(|| "MDX 键块索引越界".to_string())?;
        let start = self.key_block_start + info.comp_acc as usize;
        let end = start + info.comp_size as usize;
        if end > self.data.len() {
            return Err("MDX 键块越界".to_string());
        }
        let raw = decompress_block(&self.data[start..end])?;
        if info.decomp_size != 0 && raw.len() as u64 != info.decomp_size {
            // tolerate it; the record count below stops the loop
        }
        let mut keys = Vec::with_capacity(info.num_entries as usize);
        let mut offsets = Vec::with_capacity(info.num_entries as usize);
        let mut p = 0usize;
        for _ in 0..info.num_entries {
            let offset = if self.number_width == 4 {
                be_u32(&raw, p)? as u64
            } else {
                be_u64(&raw, p)?
            };
            p += self.number_width;
            if p > raw.len() {
                break;
            }
            let key_end = if self.delimiter_width == 2 {
                let mut q = p;
                while q + 2 <= raw.len() {
                    if raw[q] == 0 && raw[q + 1] == 0 {
                        break;
                    }
                    q += 2;
                }
                q
            } else {
                raw[p..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|q| p + q)
                    .unwrap_or(raw.len())
            };
            let key = self.encoding.decode(&raw[p..key_end]).0.into_owned();
            p = (key_end + self.delimiter_width).min(raw.len());
            offsets.push(offset);
            keys.push(key);
        }
        Ok((keys, offsets))
    }

    fn ensure_record_block(&mut self, block: usize) -> Result<&[u8], String> {
        if self.cache_index != Some(block) {
            let info = self
                .record_blocks
                .get(block)
                .ok_or_else(|| "MDX 词条块索引越界".to_string())?;
            let start = self.record_data_start + info.comp_acc as usize;
            let end = start + info.comp_size as usize;
            if end > self.data.len() {
                return Err("MDX 词条块越界".to_string());
            }
            self.cache = decompress_block(&self.data[start..end])?;
            self.cache_index = Some(block);
        }
        Ok(&self.cache)
    }

    fn read_range(&mut self, start: u64, end: u64) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity((end.saturating_sub(start)).min(1 << 20) as usize);
        let mut pos = start;
        while pos < end {
            let block = self
                .record_blocks
                .partition_point(|b| b.decomp_acc + b.decomp_size <= pos);
            if block >= self.record_blocks.len() {
                break;
            }
            let info = &self.record_blocks[block];
            let block_start = info.decomp_acc;
            let block_end = info.decomp_acc + info.decomp_size;
            let take_end = end.min(block_end);
            {
                let bytes = self.ensure_record_block(block)?;
                let from = (pos - block_start) as usize;
                let to = (take_end - block_start) as usize;
                if from < bytes.len() {
                    out.extend_from_slice(&bytes[from..to.min(bytes.len())]);
                }
            }
            if take_end <= pos {
                break;
            }
            pos = take_end;
        }
        Ok(out)
    }

    fn content(
        &mut self,
        entries: &[MdxEntry],
        index_map: &HashMap<String, usize>,
        index: usize,
    ) -> Result<String, String> {
        let mut current = index;
        for _ in 0..8 {
            let e = entries
                .get(current)
                .ok_or_else(|| "MDX 词条索引越界".to_string())?;
            let raw = self.read_range(e.offset, e.next)?;
            let text = self.encoding.decode(trim_nuls(&raw)).0.into_owned();
            let trimmed = text.trim_start();
            if let Some(target) = trimmed.strip_prefix("@@@LINK=") {
                let target = target.trim().trim_end_matches('\0');
                let next = index_map
                    .get(target)
                    .or_else(|| {
                        if self.key_sensitive {
                            None
                        } else {
                            index_map.get(&target.to_lowercase())
                        }
                    })
                    .copied();
                let Some(next) = next else {
                    return Ok(String::new());
                };
                if next == current {
                    return Ok(String::new());
                }
                current = next;
                continue;
            }
            return Ok(text);
        }
        Ok(String::new())
    }
}

fn read_mdx(path: &Path) -> Result<MdxDict, String> {
    let mut reader = MdxFile::open(path)?;
    let entries = reader.entries()?;
    let mut index_map: HashMap<String, usize> = HashMap::with_capacity(entries.len() * 2);
    for (i, e) in entries.iter().enumerate() {
        index_map.entry(e.key.clone()).or_insert(i);
        if !reader.key_sensitive {
            index_map.entry(e.key.to_lowercase()).or_insert(i);
        }
    }
    let mut warnings = Vec::new();
    let name = file_stem(path).unwrap_or_else(|| "未命名词典".to_string());
    let mdd = companion_path(path, "mdd");
    let resources = if let Some(mdd_path) = &mdd {
        match ResourceIndex::open(mdd_path) {
            Ok(r) => Some(r),
            Err(e) => {
                warnings.push(format!("没有读取 MDD 图片：{e}"));
                None
            }
        }
    } else {
        None
    };
    let mut referenced: HashSet<usize> = HashSet::new();
    let mut out_entries = Vec::new();
    let mut skipped = 0usize;
    for i in 0..entries.len() {
        let title = clean_mdx_title(&entries[i].key);
        if title.is_empty() {
            skipped += 1;
            continue;
        }
        let raw = reader.content(&entries, &index_map, i)?;
        let mut content = clean_mdx_content(&raw);
        if let Some(res) = &resources {
            content = rewrite_images(&content, res, &mut referenced);
        }
        content = clean_mdx_content(&content);
        if content.trim().is_empty() {
            skipped += 1;
            continue;
        }
        out_entries.push((title, content));
    }
    let mut files = Vec::new();
    if let Some(res) = &resources {
        let mut picked: Vec<usize> = referenced.into_iter().collect();
        picked.sort_unstable();
        for idx in picked {
            let item = &res.items[idx];
            match reader.read_range(item.offset, item.next) {
                Ok(bytes) if !bytes.is_empty() => files.push((item.zip_name.clone(), bytes)),
                Ok(_) => {}
                Err(e) => warnings.push(format!("读取图片失败 {}: {e}", item.zip_name)),
            }
        }
    }
    Ok((name, out_entries, files, warnings, skipped))
}

fn clean_mdx_title(raw: &str) -> String {
    let mut title = raw.trim().to_string();
    title = title.replace(['\u{FEFF}', '\u{0090}'], "");
    for ch in ['[', ']', '【', '】', '“', '”', '"', '\'', '‘', '’'] {
        title = title.replace(ch, "");
    }
    title = clean_title(&title);
    if title.starts_with(':') {
        return String::new();
    }
    title
}

fn clean_mdx_content(raw: &str) -> String {
    let text = trim_nuls(raw.as_bytes());
    let text = String::from_utf8_lossy(text);
    remove_markup_blocks(&text, "script")
        .pipe(|s| remove_markup_blocks(&s, "style"))
        .pipe(|s| remove_void_tags(&s, "link"))
        .trim()
        .to_string()
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

fn trim_nuls(mut bytes: &[u8]) -> &[u8] {
    while bytes.last() == Some(&0) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

struct ResourceIndex {
    items: Vec<ResourceItem>,
    by_name: HashMap<String, usize>,
    by_base: HashMap<String, usize>,
}

struct ResourceItem {
    zip_name: String,
    offset: u64,
    next: u64,
}

impl ResourceIndex {
    fn open(path: &Path) -> Result<Self, String> {
        let mut reader = MdxFile::open(path)?;
        let entries = reader.entries()?;
        let mut items = Vec::new();
        let mut by_name = HashMap::new();
        let mut by_base: HashMap<String, Option<usize>> = HashMap::new();
        for e in entries {
            let Some(zip_name) = normalize_resource_name(&e.key) else {
                continue;
            };
            let key = zip_name.to_lowercase();
            if by_name.contains_key(&key) {
                continue;
            }
            let idx = items.len();
            let base = key.rsplit('/').next().unwrap_or(&key).to_string();
            by_name.insert(key, idx);
            by_base
                .entry(base)
                .and_modify(|v| *v = None)
                .or_insert(Some(idx));
            items.push(ResourceItem {
                zip_name,
                offset: e.offset,
                next: e.next,
            });
        }
        Ok(Self {
            items,
            by_name,
            by_base: by_base
                .into_iter()
                .filter_map(|(k, v)| v.map(|i| (k, i)))
                .collect(),
        })
    }

    fn lookup(&self, src: &str) -> Option<usize> {
        let src = src.trim();
        if src.is_empty() || src.to_ascii_lowercase().starts_with("data:") {
            return None;
        }
        let src = src.strip_prefix("file://").unwrap_or(src);
        let file = src.split(['?', '#']).next().unwrap_or(src);
        let normalized = normalize_resource_name(file)?;
        let key = normalized.to_lowercase();
        if let Some(i) = self.by_name.get(&key) {
            return Some(*i);
        }
        let base = key.rsplit('/').next().unwrap_or(&key);
        self.by_base.get(base).copied()
    }
}

fn normalize_resource_name(raw: &str) -> Option<String> {
    let mut s = percent_decode(raw.trim());
    s = s.replace('\\', "/");
    s = s.strip_prefix("file://").unwrap_or(&s).to_string();
    // the name ends up inside <PIC>…</PIC>, so it must not contain markup
    s = s
        .chars()
        .map(|c| {
            if c == '<' || c == '>' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    while let Some(rest) = s.strip_prefix('/') {
        s = rest.to_string();
    }
    let parts: Vec<&str> = s
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn rewrite_images(
    content: &str,
    resources: &ResourceIndex,
    referenced: &mut HashSet<usize>,
) -> String {
    let lower = content.to_ascii_lowercase();
    let mut out = String::with_capacity(content.len());
    let mut i = 0usize;
    while let Some(rel) = lower[i..].find("<img") {
        let start = i + rel;
        let after = start + 4;
        if after >= content.len()
            || !matches!(
                content.as_bytes()[after],
                b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/'
            )
        {
            out.push_str(&content[i..after]);
            i = after;
            continue;
        }
        let Some(tag_len) = find_tag_end(&content[start..]) else {
            out.push_str(&content[i..]);
            i = content.len();
            break;
        };
        let tag_end = start + tag_len + 1;
        let tag = &content[start..tag_end];
        if let Some(src) = extract_attr(tag, "src") {
            if let Some(idx) = resources.lookup(&src) {
                if is_embeddable_image(&resources.items[idx].zip_name) {
                    referenced.insert(idx);
                    out.push_str(&content[i..start]);
                    out.push_str("<PIC>");
                    out.push_str(&resources.items[idx].zip_name);
                    out.push_str("</PIC>");
                    i = tag_end;
                    continue;
                }
            }
        }
        out.push_str(&content[i..tag_end]);
        i = tag_end;
    }
    out.push_str(&content[i..]);
    out
}

/// The `.words` reader's `<PIC>` embedding understands these formats only.
fn is_embeddable_image(name: &str) -> bool {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "bmp" | "gif")
}

fn find_tag_end(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut quote = 0u8;
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

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut at = 0usize;
    while let Some(rel) = lower[at..].find(name) {
        let p = at + rel;
        let before_ok = p == 0
            || lower.as_bytes()[p - 1].is_ascii_whitespace()
            || lower.as_bytes()[p - 1] == b'<';
        let after = p + name.len();
        let after_ok = after < lower.len() && lower.as_bytes()[after] == b'=';
        if before_ok && after_ok {
            let mut q = after + 1;
            let bytes = tag.as_bytes();
            while q < bytes.len() && bytes[q].is_ascii_whitespace() {
                q += 1;
            }
            if q >= bytes.len() {
                return None;
            }
            let quote = bytes[q];
            if quote == b'"' || quote == b'\'' {
                let start = q + 1;
                let end = tag[start..].find(quote as char).map(|e| start + e)?;
                return Some(tag[start..end].to_string());
            }
            let start = q;
            let end = tag[start..]
                .find(|c: char| c.is_ascii_whitespace() || c == '>')
                .map(|e| start + e)
                .unwrap_or(tag.len());
            return Some(tag[start..end].to_string());
        }
        at = after;
    }
    None
}

fn remove_markup_blocks(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(&open) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(close_rel) = after.to_ascii_lowercase().find(&close) else {
            break;
        };
        let Some(end_rel) = after[close_rel..].find('>') else {
            break;
        };
        rest = &after[close_rel + end_rel + 1..];
    }
    out
}

fn remove_void_tags(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(&open) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find('>') else {
            out.push_str(after);
            break;
        };
        rest = &after[end + 1..];
    }
    out
}

fn clean_markup(input: &str) -> String {
    let s = remove_markup_blocks(input, "script");
    let s = remove_markup_blocks(&s, "style");
    remove_void_tags(&s, "link")
}

fn markup_to_text(input: &str) -> String {
    let s = clean_markup(input);
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(lt) = rest.find('<') {
        out.push_str(&decode_entities(&rest[..lt]));
        let after = &rest[lt..];
        let Some(gt) = after.find('>') else {
            out.push_str(after);
            return out;
        };
        let tag = &after[1..gt].trim().to_ascii_lowercase();
        let closing = tag.starts_with('/');
        let name = tag.trim_start_matches('/').split_whitespace().next().unwrap_or("");
        let line_break = if closing {
            matches!(name, "br" | "p" | "div" | "tr" | "li" | "def" | "ex" | "k")
        } else {
            name == "br"
        };
        if line_break {
            out.push('\n');
        }
        rest = &after[gt + 1..];
    }
    out.push_str(&decode_entities(rest));
    let mut lines = out.lines().map(str::trim).collect::<Vec<_>>();
    lines.retain(|l| !l.is_empty());
    lines.join("\n")
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let Some(semi) = after.find(';').filter(|&s| s <= 16) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..semi];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| {
                    if let Some(hex) = n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        n.parse::<u32>().ok()
                    }
                })
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &after[semi + 1..];
        } else {
            out.push('&');
            rest = &after[1..];
        }
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------- MDict crypto

/// MDict's `_fast_decrypt` (the old Java `ByteUtils._fast_decrypt`).
fn fast_decrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut previous = 0x36u8;
    for (i, &byte) in data.iter().enumerate() {
        let swapped = byte.rotate_left(4);
        let k = key[i % key.len()];
        out.push(swapped ^ previous ^ (i as u8) ^ k);
        previous = byte;
    }
    out
}

/// Decrypt an `Encrypted="2"` key block info block. The first eight bytes
/// (compression type and checksum) stay readable; the payload is encrypted
/// with a key derived from bytes 4..8 and the MDict constant 0x3695.
fn mdx_decrypt(comp: &[u8]) -> Result<Vec<u8>, String> {
    if comp.len() < 8 {
        return Err("MDX 加密块过短".to_string());
    }
    let mut material = Vec::with_capacity(8);
    material.extend_from_slice(&comp[4..8]);
    material.extend_from_slice(&0x3695u32.to_le_bytes());
    let key = ripemd128(&material);
    let mut out = Vec::with_capacity(comp.len());
    out.extend_from_slice(&comp[..8]);
    out.extend_from_slice(&fast_decrypt(&comp[8..], &key));
    Ok(out)
}

fn ripemd128(message: &[u8]) -> [u8; 16] {
    const R: [usize; 64] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9,
        5, 2, 14, 11, 8, 3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12, 1, 9, 11, 10, 0, 8,
        12, 4, 13, 3, 7, 15, 14, 5, 6, 2,
    ];
    const RR: [usize; 64] = [
        5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12, 6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8,
        12, 4, 9, 1, 2, 15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13, 8, 6, 4, 1, 3, 11,
        15, 0, 5, 12, 2, 13, 9, 7, 10, 14,
    ];
    const S: [u32; 64] = [
        11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8, 7, 6, 8, 13, 11, 9, 7, 15, 7, 12,
        15, 9, 11, 7, 13, 12, 11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5, 11, 12, 14,
        15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12,
    ];
    const SS: [u32; 64] = [
        8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6, 9, 13, 15, 7, 12, 8, 9, 11, 7, 7,
        12, 7, 6, 15, 13, 11, 9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5, 15, 5, 8, 11,
        14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8,
    ];
    const K: [u32; 4] = [0x0000_0000, 0x5a82_7999, 0x6ed9_eba1, 0x8f1b_bcdc];
    const KK: [u32; 4] = [0x50a2_8be6, 0x5c4d_d124, 0x6d70_3ef3, 0x0000_0000];

    let mut msg = message.to_vec();
    let bit_len = (message.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    let mut h = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    for chunk in msg.as_chunks::<64>().0 {
        let mut x = [0u32; 16];
        for (i, w) in x.iter_mut().enumerate() {
            *w = u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        let (mut a1, mut b1, mut c1, mut d1) = (h[0], h[1], h[2], h[3]);
        let (mut a2, mut b2, mut c2, mut d2) = (h[0], h[1], h[2], h[3]);
        for i in 0..64 {
            let j = i / 16;
            let f = match j {
                0 => b1 ^ c1 ^ d1,
                1 => (b1 & c1) | (!b1 & d1),
                2 => (b1 | !c1) ^ d1,
                _ => (b1 & d1) | (c1 & !d1),
            };
            let t = a1
                .wrapping_add(f)
                .wrapping_add(x[R[i]])
                .wrapping_add(K[j])
                .rotate_left(S[i]);
            a1 = d1;
            d1 = c1;
            c1 = b1;
            b1 = t;
            let f = match j {
                0 => (b2 & d2) | (c2 & !d2),
                1 => (b2 | !c2) ^ d2,
                2 => (b2 & c2) | (!b2 & d2),
                _ => b2 ^ c2 ^ d2,
            };
            let t = a2
                .wrapping_add(f)
                .wrapping_add(x[RR[i]])
                .wrapping_add(KK[j])
                .rotate_left(SS[i]);
            a2 = d2;
            d2 = c2;
            c2 = b2;
            b2 = t;
        }
        let t = h[1].wrapping_add(c1).wrapping_add(d2);
        h[1] = h[2].wrapping_add(d1).wrapping_add(a2);
        h[2] = h[3].wrapping_add(a1).wrapping_add(b2);
        h[3] = h[0].wrapping_add(b1).wrapping_add(c2);
        h[0] = t;
    }
    let mut out = [0u8; 16];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

// -------------------------------------------------------------- binary help

fn be_u16(data: &[u8], pos: usize) -> Result<u16, String> {
    data.get(pos..pos + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| "MDX 数据意外结束".to_string())
}

fn be_u32(data: &[u8], pos: usize) -> Result<u32, String> {
    data.get(pos..pos + 4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "MDX 数据意外结束".to_string())
}

fn be_u64(data: &[u8], pos: usize) -> Result<u64, String> {
    data.get(pos..pos + 8)
        .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "MDX 数据意外结束".to_string())
}

fn decompress_block(block: &[u8]) -> Result<Vec<u8>, String> {
    if block.len() < 8 {
        return Err("MDX 压缩块过短".to_string());
    }
    let kind = u32::from_le_bytes(block[0..4].try_into().unwrap());
    let payload = &block[8..];
    match kind {
        0 => Ok(payload.to_vec()),
        2 => {
            let mut decoder = flate2::read::ZlibDecoder::new(payload);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| format!("MDX 解压失败: {e}"))?;
            Ok(out)
        }
        1 => Err("这个 MDX 使用 LZO 压缩，暂不支持".to_string()),
        other => Err(format!("未知的 MDX 压缩方式 {other}")),
    }
}

fn parse_header_attrs(header: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut rest = header;
    while let Some(eq) = rest.find("=\"") {
        let before = &rest[..eq];
        let key_start = before
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map(|p| p + 1)
            .unwrap_or(0);
        let key = &before[key_start..];
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        if !key.is_empty() {
            map.insert(key.to_string(), after[..end].to_string());
        }
        rest = &after[end + 1..];
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ripemd128_known_vectors() {
        assert_eq!(hex(&ripemd128(b"")), "cdf26213a150dc3ecb610f18f6b38b46");
        assert_eq!(hex(&ripemd128(b"a")), "86be7afa339d0fc7cfc785e72f578d33");
        assert_eq!(hex(&ripemd128(b"abc")), "c14a12199c66e4ba84636b0f69144c77");
        assert_eq!(
            hex(&ripemd128(b"message digest")),
            "9e327b3d6e523062afc1132d7df9d1b8"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn mdx_key_derivation_matches_the_sample() {
        assert_eq!(
            hex(&ripemd128(&[
                0xd9, 0x6a, 0x09, 0x04, 0x95, 0x36, 0x00, 0x00
            ])),
            "55aca8912f91e61269148263468ac403"
        );
    }

    #[test]
    fn parses_mdx_header_attributes() {
        let attrs = parse_header_attrs(
            r#"<Dictionary GeneratedByEngineVersion="2.0" Encrypted="2" Encoding="GBK" Title="词典"/>"#,
        );
        assert_eq!(attrs.get("GeneratedByEngineVersion").unwrap(), "2.0");
        assert_eq!(attrs.get("Encoding").unwrap(), "GBK");
        assert_eq!(attrs.get("Title").unwrap(), "词典");
    }

    #[test]
    fn rewrites_image_tags_to_pic_markers() {
        let resources = ResourceIndex {
            items: vec![
                ResourceItem {
                    zip_name: "images/lotus.png".to_string(),
                    offset: 0,
                    next: 0,
                },
                ResourceItem {
                    zip_name: "style.css".to_string(),
                    offset: 0,
                    next: 0,
                },
            ],
            by_name: HashMap::from([
                ("images/lotus.png".to_string(), 0),
                ("style.css".to_string(), 1),
            ]),
            by_base: HashMap::from([("lotus.png".to_string(), 0), ("style.css".to_string(), 1)]),
        };
        let mut referenced = HashSet::new();
        let text = r#"<p>图</p><img width="10" src="images/lotus.png"><img src="missing.png"><img src="style.css">"#;
        let out = rewrite_images(text, &resources, &mut referenced);
        assert!(out.contains("<PIC>images/lotus.png</PIC>"));
        assert!(out.contains("missing.png"));
        assert!(out.contains(r#"<img src="style.css">"#));
        assert_eq!(referenced.len(), 1);
    }

    #[test]
    fn txt_rule_reads_bracket_entries() {
        let dir = std::env::temp_dir().join(format!("sl_dict_txt_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("小词典.txt");
        fs::write(
            &path,
            "【般若】智慧。\\\\n梵语。\n【菩提】觉。\n没有方括号的行\n",
        )
        .unwrap();
        let (name, entries, skipped) = read_txt(&path).unwrap();
        assert_eq!(name, "小词典");
        assert_eq!(skipped, 1);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "般若");
        assert_eq!(entries[0].1, "智慧。\n梵语。");
        let _ = fs::remove_dir_all(&dir);
    }

    fn sample(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../samples/dict")
            .join(rel)
    }

    fn temp_out(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sl_dict_import_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn imports_txt_sample() {
        use crate::dict::{Dictionary, MatchType};
        let out_dir = temp_out("txt");
        let outcomes = import_path(&sample("txt/佛学小词典.txt"), &out_dir).unwrap();
        assert_eq!(outcomes.len(), 1);
        let dict = Dictionary::open(Path::new(&outcomes[0].output)).unwrap();
        let hits = dict.search("般若", MatchType::TitleEquals, 10);
        assert_eq!(hits.len(), 1);
        let detail = dict.read_entry_detail(hits[0].entry_id).unwrap();
        assert!(detail.text.contains("智慧"));
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn star_dict_base_strips_each_suffix() {
        let base = PathBuf::from("C:/dicts/藏汉大辞典1");
        for suffix in [".ifo", ".idx", ".dict", ".idx.gz", ".dict.dz"] {
            let path = PathBuf::from(format!("{}{}", base.display(), suffix));
            assert_eq!(
                star_dict_base(&path).unwrap(),
                base,
                "suffix {suffix} was stripped wrongly"
            );
        }
    }

    #[test]
    fn imports_stardict_from_any_of_its_files() {
        use crate::dict::{Dictionary, MatchType};
        for file in ["stardict/测试星译.idx", "stardict/测试星译.dict"] {
            let out_dir = temp_out("stardict-any");
            let outcomes = import_path(&sample(file), &out_dir).unwrap();
            assert_eq!(outcomes.len(), 1, "{file}");
            let dict = Dictionary::open(Path::new(&outcomes[0].output)).unwrap();
            assert_eq!(dict.search("般若", MatchType::TitleEquals, 10).len(), 1);
            let _ = fs::remove_dir_all(&out_dir);
        }
    }

    #[test]
    fn imports_a_directory_with_gzipped_idx() {
        use crate::dict::{Dictionary, MatchType};
        use flate2::write::GzEncoder;
        let src_dir = temp_out("stardict-gz-src");
        fs::copy(
            sample("stardict/测试星译.dict"),
            src_dir.join("gz词典.dict"),
        )
        .unwrap();
        let idx = fs::read(sample("stardict/测试星译.idx")).unwrap();
        let file = File::create(src_dir.join("gz词典.idx.gz")).unwrap();
        let mut encoder = GzEncoder::new(file, flate2::Compression::default());
        encoder.write_all(&idx).unwrap();
        encoder.finish().unwrap();

        let out_dir = temp_out("stardict-gz");
        let outcomes = import_path(&src_dir, &out_dir).unwrap();
        assert_eq!(outcomes.len(), 1, "only .idx.gz should be the source");
        let dict = Dictionary::open(Path::new(&outcomes[0].output)).unwrap();
        assert_eq!(dict.search("般若", MatchType::TitleEquals, 10).len(), 1);
        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn imports_stardict_sample() {
        use crate::dict::{Dictionary, MatchType};
        let out_dir = temp_out("stardict");
        let outcomes = import_path(&sample("stardict/测试星译.ifo"), &out_dir).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].name, "测试星译词典");
        let dict = Dictionary::open(Path::new(&outcomes[0].output)).unwrap();
        let hits = dict.search("般若", MatchType::TitleEquals, 10);
        assert_eq!(hits.len(), 1);
        assert!(dict
            .read_entry_detail(hits[0].entry_id)
            .unwrap()
            .text
            .contains("智慧"));
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn reads_mdx_sample_keys() {
        let path = sample("mdx/测试词典.mdx");
        let data = fs::read(&path).unwrap();
        let hs = be_u32(&data, 0).unwrap() as usize;
        let header = String::from_utf16_lossy(
            &data[4..4 + hs]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect::<Vec<_>>(),
        );
        let attrs = parse_header_attrs(&header);
        assert_eq!(attrs.get("Encrypted").map(String::as_str), Some("2"));
        let mut file = MdxFile::open(&path).unwrap();
        let entries = file.entries().unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].key, "般若");
    }

    #[test]
    fn decrypts_sample_key_info() {
        let data = fs::read(sample("mdx/测试词典.mdx")).unwrap();
        let hs = be_u32(&data, 0).unwrap() as usize;
        let pos = 4 + hs + 4;
        let info_size = be_u64(&data, pos + 24).unwrap() as usize;
        let info = &data[pos + 44..pos + 44 + info_size];
        let dec = mdx_decrypt(info).unwrap();
        assert_eq!(&dec[8..10], &[0x78, 0x9c]);
        assert!(decompress_block(&dec).is_ok());
    }

    #[test]
    fn imports_encrypted_mdx_with_resources_and_redirect() {
        use crate::dict::{Dictionary, MatchType};
        let out_dir = temp_out("mdx");
        let outcomes = import_path(&sample("mdx/测试词典.mdx"), &out_dir).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(
            outcomes[0].has_extra,
            "the MDD image should be in .words_extra"
        );
        let dict = Dictionary::open(Path::new(&outcomes[0].output)).unwrap();
        let hits = dict.search("般若", MatchType::TitleEquals, 10);
        assert_eq!(hits.len(), 1);
        let detail = dict.read_entry_detail(hits[0].entry_id).unwrap();
        assert!(detail.html.contains("data:image/png;base64,"));
        // 智慧 is an @@@LINK= redirect, so its body is 般若's
        let redirect = dict.search("智慧", MatchType::TitleEquals, 10);
        assert_eq!(redirect.len(), 1);
        let detail = dict.read_entry_detail(redirect[0].entry_id).unwrap();
        assert!(detail.text.contains("prajñā"));
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn imports_a_directory_and_skips_companion_files() {
        let out_dir = temp_out("dir");
        let outcomes = import_path(&sample(""), &out_dir).unwrap();
        // TXT, one StarDict (not the .idx/.dict twice), one MDX (not the MDD)
        assert_eq!(outcomes.len(), 3);
        let names: Vec<&str> = outcomes.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"测试词典"));
        assert!(names.contains(&"测试星译词典"));
        assert!(names.contains(&"佛学小词典"));
        let _ = fs::remove_dir_all(&out_dir);
    }
}
