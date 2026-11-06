//! The `aio` analyzer (design-spec §7): the same text analysis for indexing
//! and for queries. Pure Rust, no Tantivy dependency; `search.rs` wraps it
//! as a Tantivy tokenizer.
//!
//! Port of the old Solr `text_aio` chain, plus single characters:
//!
//! 1. fold every char: NFKC (full width -> half width, compatibility
//!    ideographs -> unified), lower case, and traditional -> simplified
//!    (`data/t2s.txt`, one char to one char, so that a simplified query finds
//!    the traditional text and the other way round);
//! 2. classify: CJK (ideographs, kana, hangul, PUA glyphs, 〇 々 〡 ...),
//!    word chars (letters, digits of other scripts), transparent chars
//!    (punctuation and whitespace, see `volume::is_punct`) and breakers
//!    (every other symbol, e.g. ○ △ ─);
//! 3. a run of CJK chars gives **bigrams** (`如是` `是妙` ...); a run of a
//!    single CJK char gives that char; a run of word chars gives one word.
//!    Transparent chars do not end a CJK run, so bigrams cross commas, the
//!    same as the punctuation-free `plain` text of a volume; they do end a word;
//! 4. every CJK char is also given as a **unigram**, in a separate list
//!    (indexed without positions: a one-char query never needs them).
//!
//! Positions: one per emitted term, so the bigrams of a run are consecutive
//! and a phrase is a sequence of consecutive bigrams.

use crate::volume::is_punct;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// Traditional -> simplified pairs, from fast2s (MIT, see data/t2s.LICENSE.md).
/// All of them are inside U+3400..U+9FFF, and no target is itself a source.
const T2S: &str = include_str!("../../data/t2s.txt");
const T2S_FIRST: u32 = 0x3400;
const T2S_LAST: u32 = 0x9FFF;

fn t2s_table() -> &'static [char] {
    static TABLE: OnceLock<Vec<char>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t: Vec<char> = (T2S_FIRST..=T2S_LAST).map(|u| char::from_u32(u).unwrap_or(' ')).collect();
        for line in T2S.lines() {
            let mut it = line.chars();
            if let (Some(a), Some(' '), Some(b)) = (it.next(), it.next(), it.next()) {
                let u = a as u32;
                if (T2S_FIRST..=T2S_LAST).contains(&u) {
                    t[(u - T2S_FIRST) as usize] = b;
                }
            }
        }
        t
    })
}

/// Traditional -> simplified for one char (identity for anything else).
#[inline]
pub fn to_simplified(c: char) -> char {
    let u = c as u32;
    if (T2S_FIRST..=T2S_LAST).contains(&u) {
        t2s_table()[(u - T2S_FIRST) as usize]
    } else {
        c
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKind {
    /// two CJK chars
    Bigram,
    /// a CJK run of one char
    Single,
    /// letters / digits
    Word,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub text: String,
    pub kind: TermKind,
    /// position in the term stream (phrase queries)
    pub position: usize,
    /// byte range in the input text
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unigram {
    pub text: String,
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Cjk,
    Word,
    Transparent,
    Break,
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF      // CJK and Kangxi radicals
        | 0x3005 | 0x3007    // 々 〇
        | 0x3021..=0x3029    // Suzhou numerals 〡
        | 0x3041..=0x309F    // hiragana
        | 0x30A0..=0x30FA | 0x30FC..=0x30FF // katakana (not ・)
        | 0x31F0..=0x31FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF    // hangul
        | 0xE000..=0xF8FF    // PUA: CBETA missing glyphs
        | 0xF900..=0xFAFF
        | 0x20000..=0x3FFFF  // ext B..H
        | 0xF0000..=0x10FFFF // supplementary PUA: CBETA glyphs, Siddham
    )
}

fn classify(c: char) -> Class {
    if is_cjk(c) {
        Class::Cjk
    } else if c.is_alphanumeric() || is_combining(c) {
        Class::Word
    } else if is_punct(c) || c.is_whitespace() || c.is_control() {
        Class::Transparent
    } else {
        Class::Break
    }
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF)
}

/// Fold one char (NFKC + lower case). Usually one char; a few compatibility
/// chars expand (`㈠` -> `(一)`).
pub fn fold(c: char, out: &mut String) {
    let u = c as u32;
    if u < 0x80 {
        out.push(c.to_ascii_lowercase());
        return;
    }
    // fast path: unified ideographs, PUA and kana are stable under NFKC
    if (0x3400..=0x9FFF).contains(&u) {
        out.push(to_simplified(c));
        return;
    }
    if (0x20000..=0x2FFFF).contains(&u) || u >= 0xF0000 || (0xE000..=0xF8FF).contains(&u) {
        out.push(c);
        return;
    }
    for n in std::iter::once(c).nfkc() {
        for l in n.to_lowercase() {
            out.push(to_simplified(l));
        }
    }
}

/// Folded chars of `text`, each with the byte range of the source char.
fn folded_chars(text: &str) -> Vec<(char, usize, usize)> {
    let mut out = Vec::with_capacity(text.len() / 3 + 1);
    let mut buf = String::new();
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        buf.clear();
        fold(c, &mut buf);
        for f in buf.chars() {
            out.push((f, i, end));
        }
    }
    out
}

/// Terms for the positional field (`body`).
pub fn terms(text: &str) -> Vec<Term> {
    let chars = folded_chars(text);
    let mut out = Vec::new();
    let mut pos = 0usize;
    // current CJK run: (char, from, to); kept across transparent chars
    let mut run: Vec<(char, usize, usize)> = Vec::new();
    let mut word = String::new();
    let mut word_from = 0usize;
    let mut word_to = 0usize;

    fn flush_run(run: &mut Vec<(char, usize, usize)>, out: &mut Vec<Term>, pos: &mut usize) {
        if run.len() == 1 {
            out.push(Term {
                text: run[0].0.to_string(),
                kind: TermKind::Single,
                position: *pos,
                from: run[0].1,
                to: run[0].2,
            });
            *pos += 1;
        } else {
            for w in run.windows(2) {
                let mut t = String::with_capacity(8);
                t.push(w[0].0);
                t.push(w[1].0);
                out.push(Term {
                    text: t,
                    kind: TermKind::Bigram,
                    position: *pos,
                    from: w[0].1,
                    to: w[1].2,
                });
                *pos += 1;
            }
        }
        run.clear();
    }

    for &(c, from, to) in &chars {
        let class = classify(c);
        if class != Class::Word && !word.is_empty() {
            out.push(Term {
                text: std::mem::take(&mut word),
                kind: TermKind::Word,
                position: pos,
                from: word_from,
                to: word_to,
            });
            pos += 1;
        }
        match class {
            Class::Cjk => run.push((c, from, to)),
            Class::Word => {
                if !run.is_empty() {
                    flush_run(&mut run, &mut out, &mut pos);
                }
                if word.is_empty() {
                    word_from = from;
                }
                word.push(c);
                word_to = to;
            }
            Class::Transparent => {}
            Class::Break => {
                if !run.is_empty() {
                    flush_run(&mut run, &mut out, &mut pos);
                }
            }
        }
    }
    if !word.is_empty() {
        out.push(Term {
            text: word,
            kind: TermKind::Word,
            position: pos,
            from: word_from,
            to: word_to,
        });
        pos += 1;
    }
    if !run.is_empty() {
        flush_run(&mut run, &mut out, &mut pos);
    }
    out
}

/// Single CJK chars for the non-positional field (`body1`).
pub fn unigrams(text: &str) -> Vec<Unigram> {
    folded_chars(text)
        .into_iter()
        .filter(|(c, _, _)| is_cjk(*c))
        .map(|(c, from, to)| Unigram {
            text: c.to_string(),
            from,
            to,
        })
        .collect()
}

/// The query-side view of a text: the folded chars that matter for matching
/// (CJK and word chars), used to find hit positions inside a volume.
pub fn match_key(text: &str) -> Vec<char> {
    folded_chars(text)
        .into_iter()
        .map(|(c, _, _)| c)
        .filter(|c| matches!(classify(*c), Class::Cjk | Class::Word))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(ts: &[Term]) -> Vec<&str> {
        ts.iter().map(|t| t.text.as_str()).collect()
    }

    #[test]
    fn bigrams_cross_punctuation_and_have_consecutive_positions() {
        let t = terms("如是，妙行");
        assert_eq!(texts(&t), vec!["如是", "是妙", "妙行"]);
        assert_eq!(t.iter().map(|t| t.position).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert!(t.iter().all(|t| t.kind == TermKind::Bigram));
        // byte ranges point into the input
        assert_eq!(&"如是，妙行"[t[1].from..t[1].to], "是，妙");
    }

    #[test]
    fn symbols_break_runs_and_single_chars_stay() {
        let t = terms("為三○初標");
        assert_eq!(texts(&t), vec!["为三", "初标"]);
        let t = terms("佛○");
        assert_eq!(texts(&t), vec!["佛"]);
        assert_eq!(t[0].kind, TermKind::Single);
    }

    #[test]
    fn latin_words_are_folded_and_split() {
        let t = terms("No. 4 [Ｎｏ. 1(1)] Śāriputra舍利弗");
        assert_eq!(texts(&t), vec!["no", "4", "no", "1", "1", "śāriputra", "舍利", "利弗"]);
        assert_eq!(t[5].kind, TermKind::Word);
    }

    #[test]
    fn folds_full_width_and_compatibility_ideographs() {
        // U+F900 (豈, compatibility) -> U+8C48 (豈) -> 岂
        assert_eq!(texts(&terms("\u{F900}佛")), vec!["岂佛"]);
        assert_eq!(texts(&terms("ＡＢＣ")), vec!["abc"]);
    }

    #[test]
    fn keeps_missing_glyphs_in_runs() {
        // CBETA PUA glyph between ideographs
        assert_eq!(texts(&terms("波羅\u{F00B2}國")), vec!["波罗", "罗\u{F00B2}", "\u{F00B2}国"]);
    }

    #[test]
    fn folds_traditional_and_simplified_together() {
        assert_eq!(terms("他念攝之"), terms("他念摄之"));
        assert_eq!(texts(&terms("說經")), vec!["说经"]);
        assert_eq!(to_simplified('攝'), '摄');
        assert_eq!(to_simplified('摄'), '摄');
        assert_eq!(to_simplified('佛'), '佛');
    }

    #[test]
    fn unigrams_and_match_key() {
        let u: Vec<String> = unigrams("如是，No妙").into_iter().map(|u| u.text).collect();
        assert_eq!(u, vec!["如", "是", "妙"]);
        assert_eq!(match_key("如是，Ｎｏ 妙"), vec!['如', '是', 'n', 'o', '妙']);
    }
}
