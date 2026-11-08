//! Images pasted into notes: files in a library's `attachments` folder,
//! named after their content (so the same picture is kept once), and
//! referred to from the Markdown as `![](attach:<name>)`.

use std::path::{Path, PathBuf};

fn ext_of_mime(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        _ => None,
    }
}

/// A name made by [`store`]: 32 hex digits, a dot, a known extension.
pub fn is_name(name: &str) -> bool {
    match name.split_once('.') {
        Some((stem, ext)) => {
            stem.len() == 32
                && stem.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && matches!(ext, "png" | "jpg" | "gif" | "webp" | "bmp")
        }
        None => false,
    }
}

pub fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn fnv(bytes: &[u8], basis: u64) -> u64 {
    let mut h = basis;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Keep a picture in `dir`; returns its name.
pub fn store(dir: &Path, bytes: &[u8], mime: &str) -> Result<String, String> {
    let ext = ext_of_mime(mime).ok_or_else(|| format!("不支持的图片类型：{}", mime))?;
    if bytes.is_empty() {
        return Err("图片是空的".into());
    }
    let name = format!(
        "{:016x}{:016x}.{}",
        fnv(bytes, 0xcbf29ce484222325),
        fnv(bytes, 0x84222325cbf29ce4 ^ bytes.len() as u64),
        ext
    );
    let path = dir.join(&name);
    if !path.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {}", path.display(), e))?;
    }
    Ok(name)
}

/// The file of a name (only names made by [`store`]).
pub fn path_of(dir: &Path, name: &str) -> Option<PathBuf> {
    is_name(name).then(|| dir.join(name))
}

/// The pictures a Markdown text refers to (`attach:<name>`), once each.
pub fn refs(md: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = md;
    while let Some(at) = rest.find("(attach:") {
        let after = &rest[at + 8..];
        let end = after.find(')').unwrap_or(after.len());
        let name = &after[..end];
        if is_name(name) && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &after[end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_picture_once_under_its_content_name() {
        let dir = std::env::temp_dir().join(format!("sl-attach-{}", std::process::id()));
        let a = store(&dir, &[1, 2, 3], "image/png").unwrap();
        let b = store(&dir, &[1, 2, 3], "image/png").unwrap();
        let c = store(&dir, &[1, 2, 4], "image/png").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(is_name(&a) && a.ends_with(".png"));
        assert_eq!(std::fs::read(path_of(&dir, &a).unwrap()).unwrap(), vec![1, 2, 3]);
        assert!(path_of(&dir, "../x.png").is_none());
        assert!(store(&dir, &[1], "text/html").is_err());
        let md = format!("![](attach:{}) 和 ![x](attach:{}) 和 ![](attach:{})", a, c, a);
        assert_eq!(refs(&md), vec![a, c]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
