//! The fonts installed on this computer, for the settings dialog.
//!
//! The browser engine only lists a few fonts to a page, but the program runs
//! locally, so it reads the font files itself (family names from the `name`
//! table; memory-mapped, so only a few pages of each file are touched). The
//! list is made once per run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ttf_parser::{fonts_in_collection, Face, PlatformId};

#[derive(Debug, Clone)]
pub struct Font {
    /// the name to use in CSS (the English family name)
    pub family: String,
    /// the name to show: the Chinese one when the font has it
    pub label: String,
    /// the font has a Chinese name: shown first
    pub cjk: bool,
}

fn font_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        if let Some(w) = std::env::var_os("WINDIR") {
            v.push(PathBuf::from(w).join("Fonts"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        v.push(PathBuf::from("/System/Library/Fonts"));
        v.push(PathBuf::from("/Library/Fonts"));
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(h).join("Library").join("Fonts"));
        }
    } else {
        v.push(PathBuf::from("/usr/share/fonts"));
        v.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(&h).join(".fonts"));
            v.push(PathBuf::from(h).join(".local").join("share").join("fonts"));
        }
    }
    v
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out, depth + 1);
        } else if let Some(ext) = p.extension().and_then(|x| x.to_str()) {
            if matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc") {
                out.push(p);
            }
        }
    }
}

fn is_chinese(lang: u16) -> bool {
    // Windows language ids: zh-CN, zh-TW, zh-HK, zh-SG, zh-MO
    matches!(lang, 0x0804 | 0x0404 | 0x0C04 | 0x1004 | 0x1404)
}

/// `(english family, chinese family)` of one face.
fn family_names(face: &Face) -> (Option<String>, Option<String>) {
    let (mut en16, mut en1, mut zh16, mut zh1, mut any) = (None, None, None, None, None);
    for n in face.names() {
        if n.platform_id != PlatformId::Windows || !(n.name_id == 1 || n.name_id == 16) {
            continue;
        }
        let Some(s) = n.to_string() else { continue };
        let s = s.trim().to_string();
        if s.is_empty() {
            continue;
        }
        let slot = match (n.language_id, n.name_id) {
            (0x0409, 16) => &mut en16,
            (0x0409, _) => &mut en1,
            (l, 16) if is_chinese(l) => &mut zh16,
            (l, _) if is_chinese(l) => &mut zh1,
            _ => {
                if any.is_none() {
                    any = Some(s);
                }
                continue;
            }
        };
        if slot.is_none() {
            *slot = Some(s);
        }
    }
    (en16.or(en1).or(any), zh16.or(zh1))
}

/// All installed font families, Chinese-named ones first.
pub fn installed() -> Vec<Font> {
    let mut files = Vec::new();
    for d in font_dirs() {
        collect(&d, &mut files, 0);
    }
    let mut found: BTreeMap<String, Font> = BTreeMap::new();
    for f in files {
        let Ok(file) = std::fs::File::open(&f) else { continue };
        // SAFETY: read-only map of a font file; a file changed meanwhile can
        // only make parsing fail or give odd names
        let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else { continue };
        let count = fonts_in_collection(&map).unwrap_or(1);
        for i in 0..count {
            let Ok(face) = Face::parse(&map, i) else { continue };
            let (en, zh) = family_names(&face);
            let Some(family) = en.or_else(|| zh.clone()) else { continue };
            if family.starts_with('@') {
                continue;
            }
            let key = family.to_lowercase();
            let cjk = zh.is_some();
            let label = zh.unwrap_or_else(|| family.clone());
            found.entry(key).or_insert(Font { family, label, cjk });
        }
    }
    let mut v: Vec<Font> = found.into_values().collect();
    v.sort_by(|a, b| b.cjk.cmp(&a.cjk).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase())));
    v
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn lists_installed_fonts() {
        let t = std::time::Instant::now();
        let v = super::installed();
        eprintln!("{} fonts in {:?}; first: {:?}", v.len(), t.elapsed(), v.iter().take(6).collect::<Vec<_>>());
        assert!(v.iter().any(|f| f.family == "Arial"));
        assert!(v.iter().any(|f| f.cjk));
    }
}
