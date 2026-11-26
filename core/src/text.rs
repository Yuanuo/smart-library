//! Text decoding and reading position helpers.

/// Decode bytes as text, honouring a BOM:
/// UTF-8 (EF BB BF), UTF-16LE (FF FE), UTF-16BE (FE FF).
/// Without a BOM the bytes are treated as UTF-8 (invalid sequences are replaced).
pub fn decode_auto(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], false);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|&c| {
        if little_endian {
            u16::from_le_bytes(c)
        } else {
            u16::from_be_bytes(c)
        }
    });
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

/// Punctuation that CBETA added on top of the original text (the printed
/// books have none). Same set as `PUNCT` in `tools/ref_extract.py`.
/// `【】〖〗︵︶｀＇` were added after checking the `p` anchors of all volumes
/// (positions in the line do not count them); `○ △ □ 〃 ※ ～ 〇`, box-drawing
/// characters, digits and Latin letters *are* counted and stay.
const PUNCT: &str = "，。、；：？！「」『』（）〔〕［］《》〈〉…—·．﹑﹔﹕﹖﹗“”‘’,.;:?!()[]<>【】〖〗︵︶｀＇";

pub fn is_punct(c: char) -> bool {
    PUNCT.contains(c)
}

/// Characters dropped from `plain`: punctuation and whitespace (as Python's
/// `str.isspace`, which also counts U+001C..U+001F).
pub(crate) fn is_dropped(c: char) -> bool {
    is_punct(c) || c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

#[cfg(test)]
pub(crate) fn encode_utf16le_bom(s: &str) -> Vec<u8> {
    let mut v = vec![0xFF, 0xFE];
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16le_with_bom() {
        let text = "T,2,【大】,大正新脩大藏經,Taishō Tripiṭaka";
        assert_eq!(decode_auto(&encode_utf16le_bom(text)), text);
    }

    #[test]
    fn decodes_utf8_with_and_without_bom() {
        assert_eq!(decode_auto("漢".as_bytes()), "漢");
        let mut v = vec![0xEF, 0xBB, 0xBF];
        v.extend_from_slice("漢".as_bytes());
        assert_eq!(decode_auto(&v), "漢");
    }
}
