//! Structured reading view of a volume (design-spec A.6, "渲染的结构映射").
//!
//! The HTML is produced by the **same walk** as the text extraction
//! (`volume.rs`): every char that the extraction emits is written here too,
//! and counted with the same rule as `plain`. So a hit found in `plain`
//! (`search::locate`) can be highlighted in the HTML without any second
//! mapping, and `data-lb` markers sit exactly where `lines` says.
//!
//! What is shown but not counted (it is not reading text): collation notes
//! (a numbered marker with the note in a hidden bubble), variant readings
//! (in the `title` of the reading), verse caesuras, illegible-char marks,
//! the other language of a Sanskrit/Chinese pair, images.

use crate::volume::extract_with_html;
pub use crate::html_out::{figure_path, RenderOptions};

#[derive(Debug, Clone)]
pub struct RenderedVolume {
    pub html: String,
    /// number of collation notes shown as markers
    pub notes: usize,
    /// number of `plain` chars written (must equal the extraction's `plain`)
    pub plain_chars: usize,
}

/// Render a volume XML to an HTML fragment (no `<html>`/`<body>`).
pub fn render_volume(xml: &str, opts: &RenderOptions) -> Result<RenderedVolume, String> {
    let (_vt, out) = extract_with_html(xml, opts)?;
    Ok(RenderedVolume {
        html: out.buf,
        notes: out.notes,
        plain_chars: out.plain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_out::safe_style;

    use crate::volume::extract_volume;

    const T0004: &str = include_str!("../../samples/cbeta/T01n0004_001.xml");

    fn wrap(body: &str) -> String {
        format!(
            r#"<TEI xmlns="http://www.tei-c.org/ns/1.0" xmlns:cb="http://www.cbeta.org/ns/1.0"><teiHeader><idno type="canon">T</idno>
<charDecl><char xml:id="CB01197"><charName>CBETA CHARACTER CB01197</charName>
<charProp><localName>composition</localName><value>[臊-品+巳]</value></charProp>
<mapping type="PUA">U+F04AD</mapping></char></charDecl></teiHeader><text><body>{}</body></text></TEI>"#,
            body
        )
    }

    fn marked(html: &str) -> Vec<String> {
        html.split("<mark>").skip(1).map(|s| s.split("</mark>").next().unwrap().to_string()).collect()
    }

    #[test]
    fn counts_exactly_the_plain_text_of_the_extraction() {
        let r = render_volume(T0004, &RenderOptions::default()).unwrap();
        let v = extract_volume(T0004).unwrap();
        assert_eq!(r.plain_chars, v.plain.chars().count());
        // every line of the extraction has its marker
        for l in &v.lines {
            assert!(r.html.contains(&format!("data-lb=\"{}\"", l.lb.as_deref().unwrap())));
        }
        // T0004 has Taisho/CBETA note pairs: only one marker per pair
        assert!(r.notes > 0);
        assert!(r.html.contains("class=\"note\""));
        assert!(r.html.contains("<p class=\"p\" id=\"pT01p0159a2701\""));
    }

    #[test]
    fn highlights_plain_ranges_with_inner_punctuation() {
        let xml = wrap(r#"<p><lb n="0001a01" ed="T"/>甲當念，<lb n="0001a02" ed="T"/>他念乙</p>"#);
        let r = render_volume(&xml, &RenderOptions { highlights: vec![(1, 5)], ..Default::default() }).unwrap();
        // the line marker splits the mark; the comma inside the hit is marked too
        assert_eq!(marked(&r.html).concat(), "當念，他念");
        assert!(!r.html.contains("<mark>甲"));
    }

    #[test]
    fn shows_notes_variants_glyphs_and_diagrams() {
        let xml = wrap(concat!(
            r#"<p><lb n="0001a01" ed="T"/>生<note n="1" type="orig" place="foot text">〔死〕－【三】</note>"#,
            r#"<note n="1" type="mod">〔－〕【CB】，死【大】</note><app n="1"><lem wit="【CB】">無</lem><rdg wit="【大】">死</rdg></app>"#,
            r##"<g ref="#CB01197"/></p><p cb:type="pre"><lb n="0001a02" ed="T"/>┌初<lb n="0001a03" ed="T"/>└二</p>"##,
            r#"<lg><l>佛昔修習<caesura/>於諸境界</l></lg>"#,
        ));
        let r = render_volume(&xml, &RenderOptions::default()).unwrap();
        assert_eq!(r.notes, 1, "orig + mod with the same n -> one marker");
        assert!(r.html.contains(concat!(
            "<span class=\"note\" tabindex=\"0\" data-note=\"1\" data-n=\"1\">",
            "<span class=\"nb-orig\">〔死〕－【三】</span><span class=\"nb-cb\">〔－〕【CB】，死【大】</span></span>"
        )));
        assert!(!r.html.contains("<sup>"), "the note number is CSS content, not text");
        assert!(r.html.contains("<span class=\"app v o\" data-n=\"1\" data-orig=\"死\" title=\"【CB】無；【大】死\">"));
        assert!(r.html.contains("[臊-品+巳]</span>"));
        assert!(r.html.contains("┌初<br><span class=\"lb\" data-lb=\"0001a03\"></span>└二"));
        assert!(r.html.contains("<span class=\"caesura\">　</span>"));
        let v = extract_volume(&xml).unwrap();
        assert_eq!(r.plain_chars, v.plain.chars().count());
    }

    #[test]
    fn a_reading_that_is_a_whole_block_stays_plain_text() {
        // T16n0663_004: the 明 edition omits the whole chapter, so the `lem`
        // (the 大 reading) is block-level. It must not become one inline
        // variant around the chapter, with the chapter text as its title.
        let xml = wrap(concat!(
            r#"<cb:div type="pin"><note n="1" type="orig" place="foot text">〔金光…而去〕一品四百二十八字－【明】</note>"#,
            r#"<note n="1" type="mod">〔金光…而去〕一品四百二十八字－【明】</note>"#,
            r#"<app n="1"><lem wit="【大】"><head><title>金光明經</title>囑累品第十九</head>"#,
            r#"<lb n="0001a03" ed="T"/><p xml:id="pT16p0001a0301">爾時釋迦牟尼佛，從三昧起現大神力。</p></lem>"#,
            r#"<rdg resp="Taisho" wit="【明】"><space quantity="0"/></rdg></app></cb:div>"#,
        ));
        let r = render_volume(&xml, &RenderOptions::default()).unwrap();
        assert!(
            r.html.contains("<div class=\"head\"><span class=\"x-title\">金光明經</span>囑累品第十九</div>"),
            "{}",
            r.html
        );
        assert!(
            r.html.contains("<p class=\"p\" id=\"pT16p0001a0301\">爾時釋迦牟尼佛，從三昧起現大神力。</p>"),
            "{}",
            r.html
        );
        assert!(!r.html.contains("title=\"【大】"), "{}", r.html);
        assert!(!r.html.contains("<span class=\"app"), "{}", r.html);
        assert!(r.html.contains("<span class=\"note\""), "{}", r.html);
        let v = extract_volume(&xml).unwrap();
        assert_eq!(r.plain_chars, v.plain.chars().count());
    }

    #[test]
    fn keeps_only_safe_styles() {
        assert_eq!(safe_style("margin-left:1em;text-indent:-1em;"), "margin-left:1em;text-indent:-1em");
        assert_eq!(safe_style("color:red;margin-left:expression(x)"), "");
    }

    #[test]
    fn finds_figures_at_the_bookcase_root() {
        assert_eq!(figure_path("../figures/B/B06p0447_01.gif").as_deref(), Some("figures/B/B06p0447_01.gif"));
        assert_eq!(figure_path("../../etc/passwd"), None);
        assert_eq!(figure_path("figures/../../x"), None);
    }
}
