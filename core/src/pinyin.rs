//! Chinese -> Pinyin conversion with word/phrase-level polyphone disambiguation.
//!
//! Dictionary data (`data/pinyin.txt`) is from `appxi-smartcn-pinyin`.
//! Greedy longest-match is used so multi-character words (e.g. `般若`, `阿弥陀佛`,
//! `银行`, `重新`) resolve their context-dependent pronunciations instead of
//! falling back to single-character defaults.

use crate::analyzer::to_simplified;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

const DATA: &str = include_str!("../../data/pinyin.txt");

struct Dict {
    map: HashMap<Cow<'static, str>, Box<[Cow<'static, str>]>>,
    max_len: usize,
}

fn load_preset(
    map: &mut HashMap<Cow<'static, str>, Box<[Cow<'static, str>]>>,
    max_len: &mut usize,
) {
    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((word, rhs)) = line.split_once('=') else {
            continue;
        };
        let word = word.trim();
        let syllables: Box<[Cow<'static, str>]> = rhs
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(Cow::Borrowed)
            .collect();
        if word.is_empty() || syllables.is_empty() {
            continue;
        }
        let len = word.chars().count();
        if len > *max_len {
            *max_len = len;
        }
        map.insert(Cow::Borrowed(word), syllables);
    }
}

fn load_user(
    text: &str,
    map: &mut HashMap<Cow<'static, str>, Box<[Cow<'static, str>]>>,
    max_len: &mut usize,
) -> usize {
    let mut count = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((word, rhs)) = line.split_once('=') else {
            continue;
        };
        let word = word.trim();
        let syllables: Box<[Cow<'static, str>]> = rhs
            .split([',', '，'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| Cow::Owned(s.to_ascii_lowercase()))
            .collect();
        if word.is_empty() || syllables.is_empty() {
            continue;
        }
        let len = word.chars().count();
        if len > *max_len {
            *max_len = len;
        }
        map.insert(Cow::Owned(word.to_string()), syllables);
        count += 1;
    }
    count
}

fn build_dict(user: &str) -> Dict {
    let mut map = HashMap::with_capacity(32_000);
    let mut max_len = 1usize;
    load_preset(&mut map, &mut max_len);
    if !user.trim().is_empty() {
        load_user(user, &mut map, &mut max_len);
    }
    Dict { map, max_len }
}

fn store() -> &'static RwLock<Arc<Dict>> {
    static STORE: OnceLock<RwLock<Arc<Dict>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(Arc::new(build_dict(""))))
}

fn dict() -> Arc<Dict> {
    store().read().unwrap().clone()
}

/// Replace the active user overrides/additions on top of the embedded default dictionary.
/// Returns the number of valid user entries loaded.
pub fn set_user(text: &str) -> usize {
    let mut d = build_dict("");
    let count = if text.trim().is_empty() {
        0
    } else {
        load_user(text, &mut d.map, &mut d.max_len)
    };
    *store().write().unwrap() = Arc::new(d);
    count
}

/// Count valid entries in a user dictionary text without modifying the active dictionary.
pub fn count_user(text: &str) -> usize {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| {
            l.split_once('=').is_some_and(|(w, r)| {
                !w.trim().is_empty() && r.split([',', '，']).any(|s| !s.trim().is_empty())
            })
        })
        .count()
}

fn lookup<'a>(
    d: &'a Dict,
    text: &str,
    chars: &[char],
    offsets: &[usize],
    start: usize,
    len: usize,
) -> Option<&'a [Cow<'static, str>]> {
    let slice = &text[offsets[start]..offsets[start + len]];
    if let Some(v) = d.map.get(slice) {
        return Some(v);
    }
    let mut changed = false;
    let mut simp = String::with_capacity(slice.len());
    for &c in &chars[start..start + len] {
        let sc = to_simplified(c);
        if sc != c {
            changed = true;
        }
        simp.push(sc);
    }
    if changed {
        if let Some(v) = d.map.get(simp.as_str()) {
            return Some(v);
        }
    }
    None
}

/// Split `"yi1"` -> `("yi", 1)`. If no trailing tone digit `1..=5`, tone is `5`.
fn split_tone(s: &str) -> (&str, u8) {
    let bytes = s.as_bytes();
    if let Some(&last) = bytes.last() {
        if (b'1'..=b'5').contains(&last) {
            return (&s[..s.len() - 1], last - b'0');
        }
    }
    (s, 5)
}

fn tone_char(vowel: char, tone: u8) -> char {
    match (vowel, tone) {
        ('a', 1) => 'ā',
        ('a', 2) => 'á',
        ('a', 3) => 'ǎ',
        ('a', 4) => 'à',
        ('o', 1) => 'ō',
        ('o', 2) => 'ó',
        ('o', 3) => 'ǒ',
        ('o', 4) => 'ò',
        ('e', 1) => 'ē',
        ('e', 2) => 'é',
        ('e', 3) => 'ě',
        ('e', 4) => 'è',
        ('i', 1) => 'ī',
        ('i', 2) => 'í',
        ('i', 3) => 'ǐ',
        ('i', 4) => 'ì',
        ('u', 1) => 'ū',
        ('u', 2) => 'ú',
        ('u', 3) => 'ǔ',
        ('u', 4) => 'ù',
        ('v', 1) => 'ǖ',
        ('v', 2) => 'ǘ',
        ('v', 3) => 'ǚ',
        ('v', 4) => 'ǜ',
        ('v', _) => 'ü',
        _ => vowel,
    }
}

/// Format a raw syllable like `"yi1"` or `"lv3"` with Unicode tone marks (`"yī"`, `"lǚ"`).
fn format_with_tone(raw: &str) -> String {
    let (base, tone) = split_tone(raw);
    if !(1..=4).contains(&tone) {
        return base.replace('v', "ü");
    }
    // Standard Mandarin tone placement: a > o > e > iu (u) / ui (i) > i / u / v.
    let target_idx = base
        .find('a')
        .or_else(|| base.find('o'))
        .or_else(|| base.find('e'))
        .or_else(|| base.find("iu").map(|i| i + 1))
        .or_else(|| base.find("ui").map(|i| i + 1))
        .or_else(|| base.find(['i', 'u', 'v']));

    let mut out = String::with_capacity(base.len() + 2);
    for (idx, ch) in base.char_indices() {
        if Some(idx) == target_idx {
            out.push(tone_char(ch, tone));
        } else if ch == 'v' {
            out.push('ü');
        } else {
            out.push(ch);
        }
    }
    out
}

enum Token<'a> {
    Pinyin(&'a str),
    Other(char),
}

/// Walk `text` with greedy longest-match, calling `on_token` for each matched
/// Chinese character syllable or unmatched character.
fn walk_pinyin(text: &str, mut on_token: impl FnMut(Token<'_>)) {
    if text.is_empty() {
        return;
    }
    let d = dict();
    let mut chars = Vec::with_capacity(text.chars().count());
    let mut offsets = Vec::with_capacity(chars.capacity() + 1);
    for (off, ch) in text.char_indices() {
        chars.push(ch);
        offsets.push(off);
    }
    offsets.push(text.len());

    let n = chars.len();
    let mut i = 0;
    while i < n {
        let max_try = d.max_len.min(n - i);
        let mut matched = None;
        for len in (1..=max_try).rev() {
            if let Some(syls) = lookup(&d, text, &chars, &offsets, i, len) {
                if len == 1 || syls.len() == len {
                    matched = Some((len, syls));
                    break;
                }
            }
        }
        if let Some((len, syls)) = matched {
            if len == 1 {
                on_token(Token::Pinyin(syls[0].as_ref()));
            } else {
                for s in syls {
                    on_token(Token::Pinyin(s.as_ref()));
                }
            }
            i += len;
        } else {
            on_token(Token::Other(chars[i]));
            i += 1;
        }
    }
}

/// Convert Chinese characters in `text` to toned pinyin, keeping all other
/// characters unchanged. Spaces are inserted between adjacent pinyin syllables
/// and between pinyin and ASCII alphanumeric characters.
pub fn to_pinyin(text: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Prev {
        None,
        Pinyin,
        Alnum,
        Other,
    }

    let mut out = String::with_capacity(text.len() * 2);
    let mut prev = Prev::None;
    walk_pinyin(text, |tok| match tok {
        Token::Pinyin(raw) => {
            if prev == Prev::Pinyin || prev == Prev::Alnum {
                out.push(' ');
            }
            out.push_str(&format_with_tone(raw));
            prev = Prev::Pinyin;
        }
        Token::Other(ch) => {
            if ch.is_ascii_alphanumeric() {
                if prev == Prev::Pinyin {
                    out.push(' ');
                }
                out.push(ch);
                prev = Prev::Alnum;
            } else {
                out.push(ch);
                prev = Prev::Other;
            }
        }
    });
    out
}

/// Build `(full_toneless_pinyin, pinyin_initials)` for title quick-search matching.
/// ASCII alphanumeric characters are kept in lowercase; punctuation and spaces are dropped.
pub fn pinyin_key(text: &str) -> (String, String) {
    let mut full = String::with_capacity(text.len() * 3);
    let mut initials = String::with_capacity(text.len());
    walk_pinyin(text, |tok| match tok {
        Token::Pinyin(raw) => {
            let (base, _) = split_tone(raw);
            full.push_str(base);
            if let Some(first) = base.chars().next() {
                initials.push(first);
            }
        }
        Token::Other(ch) => {
            if ch.is_ascii_alphanumeric() {
                let lc = ch.to_ascii_lowercase();
                full.push(lc);
                initials.push(lc);
            }
        }
    });
    (full, initials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_phrases_and_polyphones() {
        assert_eq!(to_pinyin("中文"), "zhōng wén");
        assert_eq!(to_pinyin("中文abc123"), "zhōng wén abc123");
        assert_eq!(to_pinyin("银行重新"), "yín háng chóng xīn");
        assert_eq!(to_pinyin("般若波羅蜜多心經"), "bō rě bō luó mì duō xīn jīng");
        assert_eq!(to_pinyin("阿彌陀佛"), "ē mí tuó fó");
        assert_eq!(to_pinyin("南無阿彌陀佛"), "nā mó ē mí tuó fó");
        assert_eq!(to_pinyin("伽藍"), "qié lán");
    }

    #[test]
    fn preserves_punctuation_whitespace_and_non_chinese() {
        assert_eq!(to_pinyin("如是我聞，一時佛在！"), "rú shì wǒ wén，yī shí fó zài！");
        assert_eq!(to_pinyin("第一行\n第二行"), "dì yī xíng\ndì èr xíng");
        assert_eq!(to_pinyin("T0262 妙法蓮華經"), "T0262 miào fǎ lián huá jīng");
        assert_eq!(to_pinyin("略履"), "lüè lǚ");
    }

    #[test]
    fn builds_search_keys() {
        assert_eq!(
            pinyin_key("妙法蓮華經"),
            ("miaofalianhuajing".to_string(), "mflhj".to_string())
        );
        assert_eq!(
            pinyin_key("t0262妙法莲华经"),
            ("t0262miaofalianhuajing".to_string(), "t0262mflhj".to_string())
        );
        assert_eq!(
            pinyin_key("般若波罗蜜多"),
            ("boreboluomiduo".to_string(), "brblmd".to_string())
        );
    }

    #[test]
    fn user_dictionary_overrides_and_adds_entries() {
        let _ = set_user("# comment\n阿弥陀经=e1,mi2,tuo2,jing1\n");
        assert_eq!(to_pinyin("阿彌陀經"), "ē mí tuó jīng");
        let _ = set_user("");
        assert_eq!(to_pinyin("阿彌陀經"), "ā mí tuó jīng");
    }
}
