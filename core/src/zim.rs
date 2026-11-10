//! Read-only access to ZIM 5 archives. No source files are modified.
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MAX_CLUSTER: u64 = 256 * 1024 * 1024;
const MAX_STRING: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Entry {
    pub namespace: u8,
    pub path: String,
    pub title: String,
    pub mime: Option<String>,
    pub redirect: Option<u32>,
    cluster: u32,
    blob: u32,
}

pub struct Zim {
    file: File,
    pub entries: Vec<Entry>,
    clusters: Vec<u64>,
    file_len: u64,
    cached: Option<(u32, Vec<u8>, bool)>,
    paths: HashMap<String, usize>,
}

fn u16le(b: &[u8]) -> u16 {
    u16::from_le_bytes(b.try_into().unwrap())
}
fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes(b.try_into().unwrap())
}
fn u64le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().unwrap())
}
fn read_at(file: &mut File, pos: u64, buf: &mut [u8]) -> Result<(), String> {
    file.seek(SeekFrom::Start(pos))
        .and_then(|_| file.read_exact(buf))
        .map_err(|e| e.to_string())
}
fn cstring(file: &mut File) -> Result<String, String> {
    let mut b = Vec::new();
    for _ in 0..MAX_STRING {
        let mut c = [0];
        file.read_exact(&mut c).map_err(|e| e.to_string())?;
        if c[0] == 0 {
            return String::from_utf8(b).map_err(|e| e.to_string());
        }
        b.push(c[0]);
    }
    Err("ZIM string too long".into())
}
impl Zim {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let file_len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut h = [0u8; 80];
        read_at(&mut file, 0, &mut h)?;
        if u32le(&h[0..4]) != 0x044d495a || u16le(&h[4..6]) != 5 || u16le(&h[6..8]) != 0 {
            return Err("Only ZIM version 5.0 is supported".into());
        }
        let count = u32le(&h[24..28]) as usize;
        let cluster_count = u32le(&h[28..32]) as usize;
        let paths_pos = u64le(&h[32..40]);
        let clusters_pos = u64le(&h[48..56]);
        let mime_pos = u64le(&h[56..64]);
        for (pos, n) in [(paths_pos, count), (clusters_pos, cluster_count)] {
            if pos
                .checked_add((n as u64).saturating_mul(8))
                .filter(|&v| v <= file_len)
                .is_none()
            {
                return Err("Invalid ZIM pointer table".into());
            }
        }
        if mime_pos >= file_len {
            return Err("Invalid ZIM MIME table".into());
        }
        file.seek(SeekFrom::Start(mime_pos))
            .map_err(|e| e.to_string())?;
        let mut mimes = Vec::new();
        loop {
            let s = cstring(&mut file)?;
            if s.is_empty() {
                break;
            }
            mimes.push(s);
            if mimes.len() > 65535 {
                return Err("Invalid MIME table".into());
            }
        }
        let mut ptr = vec![0; count.checked_mul(8).ok_or("Too many entries")?];
        read_at(&mut file, paths_pos, &mut ptr)?;
        let mut entries = Vec::with_capacity(count);
        for chunk in ptr.chunks_exact(8) {
            let pos = u64le(chunk);
            if pos.checked_add(12).filter(|&v| v <= file_len).is_none() {
                return Err("Invalid ZIM entry offset".into());
            }
            let mut fixed = [0u8; 16];
            read_at(&mut file, pos, &mut fixed[..12])?;
            let mime_id = u16le(&fixed[..2]);
            let namespace = fixed[3];
            let redirect = if mime_id == 0xffff {
                Some(u32le(&fixed[8..12]))
            } else {
                None
            };
            let (cluster, blob) = if redirect.is_none() {
                file.read_exact(&mut fixed[12..16])
                    .map_err(|e| e.to_string())?;
                (u32le(&fixed[8..12]), u32le(&fixed[12..16]))
            } else {
                (0, 0)
            };
            let path = cstring(&mut file)?;
            let title = cstring(&mut file)?;
            let mime = if redirect.is_some() {
                None
            } else {
                Some(
                    mimes
                        .get(mime_id as usize)
                        .ok_or("Invalid MIME index")?
                        .clone(),
                )
            };
            entries.push(Entry {
                namespace,
                title: if title.is_empty() {
                    path.clone()
                } else {
                    title
                },
                path,
                mime,
                redirect,
                cluster,
                blob,
            });
        }
        let mut ptr = vec![0; cluster_count.checked_mul(8).ok_or("Too many clusters")?];
        read_at(&mut file, clusters_pos, &mut ptr)?;
        let clusters: Vec<u64> = ptr.chunks_exact(8).map(u64le).collect();
        if clusters.iter().any(|&p| p >= file_len) {
            return Err("Invalid ZIM cluster offset".into());
        }
        let paths = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (format!("{}/{}", e.namespace as char, e.path), i))
            .collect();
        Ok(Self {
            file,
            entries,
            clusters,
            file_len,
            cached: None,
            paths,
        })
    }
    pub fn article_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.namespace == b'A'
                    && e.mime
                        .as_deref()
                        .is_some_and(|m| m.starts_with("text/html"))
            })
            .map(|(i, _)| i)
    }
    /// Process entries in cluster order to avoid decompressing the same cluster repeatedly.
    pub fn article_jobs(&self) -> Vec<usize> {
        let mut jobs: Vec<_> = self.article_indices().collect();
        jobs.sort_unstable_by_key(|&i| (self.entries[i].cluster, self.entries[i].blob));
        jobs
    }
    pub fn entry_index(&self, path: &str) -> Option<usize> {
        self.paths.get(path).copied()
    }
    /// Find the article referenced by a Wikimedia URL in this archive.
    pub fn resolve_wiki_url(&self, href: &str) -> Result<(String, Option<String>), String> {
        let (_, path) = wiki_url(href).ok_or("不是可匹配的维基文章链接")?;
        self.resolve_link("A/", &path).or_else(|_| self.resolve_link("A/", &path.replace('_', "%20")))
    }
    /// Resolve a relative HTML link to a readable article and its fragment.
    pub fn resolve_link(&self, source: &str, href: &str) -> Result<(String, Option<String>), String> {
        if !is_internal_href(href) { return Err("不是包内链接".into()); }
        let (raw_path, fragment) = href.split_once('#').map_or((href, None), |(p, f)| (p, Some(f)));
        let raw_path = raw_path.split('?').next().unwrap_or("");
        let decoded = decode_url(raw_path)?;
        if !is_internal_href(&decoded) && !decoded.is_empty() { return Err("不是包内链接".into()); }
        let path = if decoded.is_empty() {
            source.to_string()
        } else {
            let mut parts: Vec<&str> = if decoded.starts_with('/') { Vec::new() } else { source.rsplit_once('/').map(|(d, _)| d.split('/').collect()).unwrap_or_default() };
            for part in decoded.split('/') {
                match part {
                    "" | "." => {},
                    ".." => { if parts.pop().is_none() { return Err("链接超出数据包".into()); } },
                    _ => parts.push(part),
                }
            }
            parts.join("/")
        };
        let mut i = self.entry_index(&path).ok_or_else(|| format!("数据包中没有词条：{path}"))?;
        for _ in 0..32 {
            let entry = self.entries.get(i).ok_or("无效的 ZIM 重定向")?;
            if let Some(next) = entry.redirect { i = next as usize; continue; }
            if entry.namespace != b'A' || !entry.mime.as_deref().is_some_and(|m| m.starts_with("text/html")) {
                return Err("链接目标不是可阅读的文章".into());
            }
            let target = format!("{}/{}", entry.namespace as char, entry.path);
            let fragment = fragment.filter(|f| !f.is_empty()).map(decode_url).transpose()?;
            return Ok((target, fragment));
        }
        Err("ZIM 重定向循环或层数过多".into())
    }
    pub fn article(
        &mut self,
        index: usize,
        render: Option<&crate::render::RenderOptions>,
    ) -> Result<crate::article::Article, String> {
        let entry = self.entries.get(index).ok_or("Invalid ZIM entry index")?;
        if entry.namespace != b'A'
            || !entry
                .mime
                .as_deref()
                .is_some_and(|m| m.starts_with("text/html"))
        {
            return Err("Not a ZIM HTML article".into());
        }
        let path = format!("{}/{}", entry.namespace as char, entry.path);
        let bytes = self.read(index)?;
        let html = std::str::from_utf8(&bytes).map_err(|e| format!("Invalid HTML UTF-8: {e}"))?;
        Ok(read_zim_article(html, &path, render))
    }
    pub fn index_doc(&mut self, index: usize) -> Result<crate::library::IndexDoc, String> {
        let entry = self.entries.get(index).ok_or("Invalid ZIM entry index")?;
        let title = entry.title.clone();
        let path = format!("{}/{}", entry.namespace as char, entry.path);
        let text = self.article(index, None)?.text.plain;
        Ok(crate::library::IndexDoc {
            path: path.clone(),
            book: title.clone(),
            juan: 0,
            title,
            text,
            facets: vec![],
            keys: vec![(String::new(), path)],
        })
    }
    pub fn read(&mut self, index: usize) -> Result<Vec<u8>, String> {
        let mut i = index;
        for _ in 0..32 {
            let e = self.entries.get(i).ok_or("Invalid ZIM entry index")?;
            if let Some(next) = e.redirect {
                i = next as usize;
                continue;
            }
            let (cluster, blob) = (e.cluster, e.blob as usize);
            if self.cached.as_ref().map(|c| c.0) != Some(cluster) {
                self.load_cluster(cluster)?;
            }
            let (_, data, extended) = self.cached.as_ref().unwrap();
            let width = if *extended { 8 } else { 4 };
            let read_offset = |n: usize| -> Option<usize> {
                let start = n.checked_mul(width)?;
                let b = data.get(start..start.checked_add(width)?)?;
                usize::try_from(if width == 8 {
                    u64le(b)
                } else {
                    u32le(b) as u64
                })
                .ok()
            };
            let header = read_offset(0).ok_or("Invalid ZIM blob table")?;
            if header < width * 2
                || header % width != 0
                || header > data.len()
                || blob >= header / width - 1
            {
                return Err("Invalid ZIM blob index".into());
            }
            let start = read_offset(blob).ok_or("Invalid ZIM blob offset")?;
            let end = read_offset(blob + 1).ok_or("Invalid ZIM blob offset")?;
            if start < header || start > end || end > data.len() {
                return Err("Invalid ZIM blob range".into());
            }
            return Ok(data[start..end].to_vec());
        }
        Err("ZIM redirect loop or chain too long".into())
    }
    fn load_cluster(&mut self, cluster: u32) -> Result<(), String> {
        let pos = *self
            .clusters
            .get(cluster as usize)
            .ok_or("Invalid cluster index")?;
        let end = self
            .clusters
            .iter()
            .copied()
            .filter(|&p| p > pos)
            .min()
            .unwrap_or(self.file_len);
        self.file
            .seek(SeekFrom::Start(pos))
            .map_err(|e| e.to_string())?;
        let mut info = [0];
        self.file.read_exact(&mut info).map_err(|e| e.to_string())?;
        let input = (&mut self.file).take(end - pos - 1);
        let mut reader: Box<dyn Read + '_> = match info[0] & 0x0f {
            1 => Box::new(input),
            5 => {
                Box::new(ruzstd::decoding::StreamingDecoder::new(input).map_err(|e| e.to_string())?)
            }
            n => return Err(format!("Unsupported ZIM compression method {n}")),
        };
        let mut data = Vec::new();
        reader
            .by_ref()
            .take(MAX_CLUSTER + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 > MAX_CLUSTER {
            return Err("ZIM cluster exceeds 256 MiB limit".into());
        }
        self.cached = Some((cluster, data, info[0] & 0x10 != 0));
        Ok(())
    }
}

/// Accept only local URL references; decoded paths are checked again on use.
pub fn is_internal_href(href: &str) -> bool {
    let href = href.trim();
    if href.is_empty() || href.starts_with("//") || href.starts_with('\\') || href.contains('\\') || href.chars().any(char::is_control) { return false; }
    let first = href.split(['/', '?', '#']).next().unwrap_or("");
    if let Some((scheme, _)) = first.split_once(':') {
        if !scheme.is_empty() && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) { return false; }
    }
    true
}

/// Accept HTTP(S) URLs only, without embedded credentials or control characters.
pub fn external_url(href: &str) -> Option<url::Url> {
    if href.chars().any(char::is_control) { return None; }
    let url = url::Url::parse(href).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() { return None; }
    Some(url)
}

/// Map Wikimedia article URLs to a project ID and an encoded ZIM path.
pub fn wiki_url(href: &str) -> Option<(String, String)> {
    let url = external_url(href)?;
    if url.query().is_some() { return None; }
    let host = url.host_str()?;
    let project = ["wikipedia", "wikiquote", "wikibooks", "wikisource", "wiktionary", "wikivoyage", "wikinews", "wikiversity"]
        .into_iter().find(|project| host == format!("{project}.org") || host.ends_with(&format!(".{project}.org")))?;
    let title = url.path().strip_prefix("/wiki/")?;
    let title = title.strip_prefix("Special:Search/").or_else(|| title.strip_prefix("Special%3ASearch/")).unwrap_or(title);
    if title.is_empty() || title.starts_with("Special:") || title.starts_with("Special%3A") { return None; }
    let mut path = format!("/A/{title}");
    if let Some(fragment) = url.fragment().filter(|f| !f.is_empty()) { path.push('#'); path.push_str(fragment); }
    Some((project.to_string(), path))
}

fn decode_url(s: &str) -> Result<String, String> {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() { return Err("链接编码不正确".into()); }
            let hex = |b: u8| (b as char).to_digit(16).ok_or("链接编码不正确");
            out.push((hex(bytes[i + 1])? * 16 + hex(bytes[i + 2])?) as u8);
            i += 3;
        } else { out.push(bytes[i]); i += 1; }
    }
    String::from_utf8(out).map_err(|_| "链接不是有效的 UTF-8".into())
}

/// Separate the archive generator's marked attribution, retaining the actual
/// per-page wording. Unknown or unmarked content remains part of the article.
fn read_zim_article(html: &str, path: &str, render: Option<&crate::render::RenderOptions>) -> crate::article::Article {
    const START: &str = "<!--htdig_noindex-->";
    const END: &str = "<!--/htdig_noindex-->";
    let mut search_from = 0;
    while let Some(relative) = html[search_from..].find(START) {
        let start = search_from + relative;
        let content = start + START.len();
        let Some(end_relative) = html[content..].find(END) else { break };
        let end = content + end_relative;
        let text = crate::markup::parse(&html[content..end]).text();
        if text.starts_with("This article is issued from ") && text.contains("The text is licensed under ") {
            let mut body = String::with_capacity(html.len() - (end + END.len() - start));
            body.push_str(&html[..start]);
            body.push_str(&html[end + END.len()..]);
            let mut article = crate::article::read_zim_html(&body, path, render);
            article.attribution = Some(text);
            return article;
        }
        search_from = end + END.len();
    }
    crate::article::read_zim_html(html, path, render)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn separates_only_the_marked_attribution_from_the_article() {
        let source = r#"<body><p>正文 This article is issued from 正文引用</p><!--htdig_noindex--><div><div>
            This article is issued from <a href="https://example.org/oldid">Wikiquote</a>.
            The text is licensed under <a href="https://example.org/license">Creative Commons - Attribution - Sharealike</a>.
            Additional terms may apply for the media files.
        </div></div><!--/htdig_noindex--></body>"#;
        let a = read_zim_article(source, "A/example", Some(&crate::render::RenderOptions::default()));
        assert!(a.text.text.contains("正文引用"));
        assert!(!a.text.text.contains("Additional terms may apply"));
        assert!(!a.html.unwrap().contains("Additional terms may apply"));
        let footer = a.attribution.unwrap();
        assert!(footer.contains("Wikiquote"));
        assert!(footer.contains("Additional terms may apply"));

        let other = "<body><p>正文</p><!--htdig_noindex--><div>保留的其他说明</div><!--/htdig_noindex--></body>";
        let a = read_zim_article(other, "A/example", None);
        assert!(a.text.text.contains("保留的其他说明"));
        assert!(a.attribution.is_none());
        let unmarked = "<body><p>This article is issued from a book. The text is licensed under a license.</p></body>";
        let a = read_zim_article(unmarked, "A/example", None);
        assert!(a.text.text.contains("This article is issued from"));
        assert!(a.attribution.is_none());
    }

    fn fixture() -> Vec<u8> {
        let mut b = vec![0u8; 80];
        b[0..4].copy_from_slice(&0x044d495au32.to_le_bytes());
        b[4..6].copy_from_slice(&5u16.to_le_bytes());
        b[24..28].copy_from_slice(&2u32.to_le_bytes());
        b[28..32].copy_from_slice(&1u32.to_le_bytes());
        let mime = b.len() as u64;
        b.extend_from_slice(b"text/html\0\0");
        let first = b.len() as u64;
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&[0, b'A']);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice("正文\0标题\0".as_bytes());
        let redirect = b.len() as u64;
        b.extend_from_slice(&0xffffu16.to_le_bytes());
        b.extend_from_slice(&[0, b'A']);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(b"redirect\0\0");
        let cluster = b.len() as u64;
        let html = "<html><body><p>中文内容</p></body></html>".as_bytes();
        b.push(1);
        b.extend_from_slice(&8u32.to_le_bytes());
        b.extend_from_slice(&(8u32 + html.len() as u32).to_le_bytes());
        b.extend_from_slice(html);
        let paths = b.len() as u64;
        b.extend_from_slice(&first.to_le_bytes());
        b.extend_from_slice(&redirect.to_le_bytes());
        let clusters = b.len() as u64;
        b.extend_from_slice(&cluster.to_le_bytes());
        b[32..40].copy_from_slice(&paths.to_le_bytes());
        b[48..56].copy_from_slice(&clusters.to_le_bytes());
        b[56..64].copy_from_slice(&mime.to_le_bytes());
        b
    }
    fn with_file(data: &[u8], f: impl FnOnce(&Path)) {
        let name = format!(
            "smart-library-zim-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, data).unwrap();
        f(&path);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn reads_article_and_redirect() {
        with_file(&fixture(), |p| {
            let mut z = Zim::open(p).unwrap();
            assert_eq!(z.entry_index("A/正文"), Some(0));
            assert_eq!(z.article_jobs(), vec![0]);
            assert!(z.article(0, None).unwrap().text.plain.contains("中文内容"));
            assert_eq!(z.read(0).unwrap(), z.read(1).unwrap());
            assert_eq!(z.index_doc(0).unwrap().path, "A/正文");
            assert_eq!(z.resolve_link("A/正文", "redirect#%E7%AB%A0%E8%8A%82").unwrap(), ("A/正文".to_string(), Some("章节".to_string())));
            assert_eq!(z.resolve_link("A/正文", "#章节").unwrap().0, "A/正文");
            assert!(z.resolve_link("A/正文", "../../escape").is_err());
            assert!(z.resolve_link("A/正文", "%6aavascript:alert(1)").is_err());
            assert!(z.resolve_link("A/正文", "https://example.com/").is_err());
            assert!(z.resolve_link("A/正文", "missing").is_err());
        });
    }

    #[test]
    fn preserves_safe_links_and_fragment_positions_without_changing_text() {
        let source = include_str!("../../samples/zim/links.html");
        let plain = read_zim_article(source, "A/1/2王子", None);
        let rendered = read_zim_article(source, "A/1/2王子", Some(&crate::render::RenderOptions::default()));
        assert_eq!(plain.text.plain, rendered.text.plain);
        assert_eq!(plain.text.plain, crate::article::read_article(source, "A/1/2王子", None).text.plain);
        assert_eq!(plain.anchors.get("章节"), plain.headings.iter().find(|h| h.text == "章节").map(|h| &h.lb));
        assert_eq!(plain.anchors.get("备用章节"), plain.anchors.get("章节"));
        let html = rendered.html.unwrap();
        assert!(html.contains("data-zim-href=\"../../A/%E5%BE%A1%E6%88%91\""));
        assert!(html.contains("data-zim-href=\"#章节\""));
        assert!(html.contains("data-zim-href=\"https://zh.wikipedia.org/wiki/亞瑟·叔本華\""));
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn recognizes_only_real_wikimedia_urls_and_safe_http_links() {
        let (id, path) = wiki_url("https://zh.wikipedia.org/wiki/%E5%BE%A1%E6%88%91#%E7%AB%A0").unwrap();
        assert_eq!(id, "wikipedia");
        assert_eq!(path, "/A/%E5%BE%A1%E6%88%91#%E7%AB%A0");
        assert!(wiki_url("https://zh.wikipedia.org.evil.test/wiki/x").is_none());
        assert_eq!(wiki_url("https://zh.wikipedia.org/wiki/Special:Search/test").unwrap().1, "/A/test");
        assert!(wiki_url("https://zh.wikipedia.org/wiki/x?oldid=1").is_none());
        assert!(external_url("javascript:alert(1)").is_none());
        assert!(external_url("https://user:password@example.com/").is_none());
    }

    #[test]
    #[ignore = "requires the local ZIM test packages"]
    fn real_archives_link_articles_and_sections() {
        let mut quote = Zim::open(Path::new(r"C:\Devx\smartLib.d\wikiquote_zh_all_maxi_2021-11.zim")).unwrap();
        let (target, _) = quote.resolve_link("A/1/2王子", "../../A/%E5%BE%A1%E6%88%91").unwrap();
        assert_eq!(target, "A/御我");
        assert_eq!(quote.resolve_wiki_url("https://zh.wikiquote.org/wiki/御我").unwrap().0, "A/御我");
        assert_eq!(quote.resolve_wiki_url("https://zh.wikiquote.org/wiki/Special:Search/御我").unwrap().0, "A/御我");
        let i = quote.entry_index("A/1/2王子").unwrap();
        let a = quote.article(i, Some(&crate::render::RenderOptions::default())).unwrap();
        assert!(a.html.unwrap().contains("data-zim-href=\"../../A/%E5%BE%A1%E6%88%91\""));

        let mut books = Zim::open(Path::new(r"C:\Devx\smartLib.d\wikibooks_zh_all_maxi_2023-01.zim")).unwrap();
        assert_eq!(books.resolve_wiki_url("https://zh.wikibooks.org/wiki/.htaccess").unwrap().0, "A/.htaccess");
        assert!(books.resolve_wiki_url("https://zh.wikibooks.org/wiki/不存在的测试词条").is_err());
        let i = books.entry_index("A/.htaccess").unwrap();
        let a = books.article(i, None).unwrap();
        let (path, fragment) = books.resolve_link("A/.htaccess", "#%E6%A6%82%E8%BF%B0").unwrap();
        assert_eq!(path, "A/.htaccess");
        assert_eq!(a.anchors.get(fragment.as_deref().unwrap()), a.headings.iter().find(|h| h.text == "概述").map(|h| &h.lb));
        assert!(a.anchors.contains_key("概述"));
    }
    #[test]
    fn rejects_corrupt_headers_and_blob_ranges() {
        let mut b = fixture();
        b[0] = 0;
        with_file(&b, |p| assert!(Zim::open(p).is_err()));
        let mut b = fixture();
        b[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        with_file(&b, |p| assert!(Zim::open(p).is_err()));
        let mut b = fixture();
        let cluster_table = u64le(&b[48..56]) as usize;
        let cluster = u64le(&b[cluster_table..cluster_table + 8]) as usize;
        b[cluster + 5..cluster + 9].copy_from_slice(&u32::MAX.to_le_bytes());
        with_file(&b, |p| assert!(Zim::open(p).unwrap().read(0).is_err()));
    }
}
