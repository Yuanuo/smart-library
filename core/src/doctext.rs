//! The text of office documents and PDFs (for reading and for the index of a local folder):
//! `.docx`, `.pptx`, `.odt` (zip files of XML) and `.pdf`. Old binary `.doc` files are not read.

use std::io::Read;
use std::path::Path;

/// Suffixes this module can read.
pub const DOC_EXTS: &[&str] = &["pdf", "docx", "pptx", "odt"];

pub fn is_document(ext: &str) -> bool {
    DOC_EXTS.contains(&ext)
}

/// The text of a document, one paragraph per line.
pub fn extract(path: &Path, ext: &str) -> Result<String, String> {
    match ext {
        "pdf" => pdf(path),
        "docx" => zipped(path, |n| n == "word/document.xml", &["w:t"], &["w:p"], &["w:tab"]),
        "pptx" => {
            let mut text = zipped(path, |n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"), &["a:t"], &["a:p"], &[])?;
            text.truncate(text.trim_end().len());
            Ok(text)
        }
        "odt" => zipped(path, |n| n == "content.xml", &["text:p", "text:h"], &["text:p", "text:h"], &["text:tab", "text:s"]),
        _ => Err(format!("不能读取 .{} 文件的文字", ext)),
    }
}

fn pdf(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    // the library panics on some broken files: a document it cannot read is no reason to stop
    let out = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes));
    match out {
        Ok(Ok(text)) => Ok(clean_pdf(&text)),
        Ok(Err(e)) => Err(format!("PDF 读不了：{}", e)),
        Err(_) => Err("PDF 读不了：文件结构不支持".into()),
    }
}

/// Lines trimmed, empty lines dropped; PDFs often use the radical forms (⼤ ⽩ ⾄) of common characters,
/// which NFKC turns into the characters themselves.
fn clean_pdf(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let text: String = text.nfkc().collect();
    let mut out = String::with_capacity(text.len());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The text of the XML entries of a zip file that `pick` chooses (in name order): the content of the
/// `text` elements, a line break at the end of each `para` element, a space for `blank` elements.
fn zipped(path: &Path, pick: impl Fn(&str) -> bool, text: &[&str], para: &[&str], blank: &[&str]) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("文件打不开：{}", e))?;
    let mut names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).filter(|n| pick(n)).collect();
    names.sort_by_key(|n| (n.len(), n.clone())); // slide2 before slide10
    let mut out = String::new();
    for n in names {
        let mut xml = String::new();
        zip.by_name(&n).map_err(|e| e.to_string())?.read_to_string(&mut xml).map_err(|e| format!("{}: {}", n, e))?;
        xml_text(&xml, text, para, blank, &mut out);
    }
    Ok(out)
}

fn xml_text(xml: &str, text: &[&str], para: &[&str], blank: &[&str], out: &mut String) {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(xml);
    let mut inside = 0usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let name = name.as_ref();
                if text.contains(&name) {
                    inside += 1;
                } else if blank.contains(&name) {
                    out.push(' ');
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                if blank.contains(&name.as_ref()) {
                    out.push(' ');
                }
            }
            Ok(Event::Text(t)) if inside > 0 => out.push_str(t.as_ref()),
            Ok(Event::GeneralRef(g)) if inside > 0 => {
                if g.is_char_ref() {
                    if let Ok(Some(c)) = g.resolve_char_ref() {
                        out.push(c);
                    }
                } else {
                    out.push_str(match g.xml10_content().as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let name = name.as_ref();
                if text.contains(&name) {
                    inside = inside.saturating_sub(1);
                }
                if para.contains(&name) && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(path: &Path, files: &[(&str, &str)]) {
        let mut zw = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in files {
            zw.start_file(*n, o).unwrap();
            zw.write_all(c.as_bytes()).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn reads_docx_pptx_odt() {
        let d = std::env::temp_dir().join(format!("sl-doctext-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let docx = d.join("a.docx");
        zip_with(&docx, &[("word/document.xml", "<w:document><w:body><w:p><w:r><w:t>如是我聞</w:t></w:r><w:r><w:tab/><w:t>A &amp; B</w:t></w:r></w:p><w:p><w:r><w:t>第二段</w:t></w:r></w:p></w:body></w:document>")]);
        assert_eq!(extract(&docx, "docx").unwrap(), "如是我聞 A & B\n第二段\n");
        let pptx = d.join("a.pptx");
        zip_with(&pptx, &[("ppt/slides/slide10.xml", "<p:sld><a:p><a:r><a:t>十</a:t></a:r></a:p></p:sld>"), ("ppt/slides/slide2.xml", "<p:sld><a:p><a:r><a:t>二</a:t></a:r></a:p></p:sld>")]);
        assert_eq!(extract(&pptx, "pptx").unwrap(), "二\n十");
        let odt = d.join("a.odt");
        zip_with(&odt, &[("content.xml", "<office:text><text:h>标题</text:h><text:p>甲<text:s/>乙</text:p></office:text>")]);
        assert_eq!(extract(&odt, "odt").unwrap(), "标题\n甲 乙\n");
        assert!(extract(&d.join("none.pdf"), "pdf").is_err());
        std::fs::write(d.join("bad.pdf"), b"not a pdf").unwrap();
        assert!(extract(&d.join("bad.pdf"), "pdf").is_err());
        std::fs::remove_dir_all(&d).ok();
    }
}

#[cfg(test)]
mod real {
    /// Needs a PDF: `SL_PDF=<file> cargo test -p smart-library-core real_pdf -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_pdf() {
        let p = std::env::var("SL_PDF").unwrap();
        let t = std::time::Instant::now();
        let text = super::extract(std::path::Path::new(&p), "pdf").unwrap();
        println!("{} chars in {:?}\n{}", text.chars().count(), t.elapsed(), text.chars().take(600).collect::<String>());
    }
}
