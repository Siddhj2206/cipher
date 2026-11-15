//! Character classes for the source scripts Cipher cares about.

/// A source script that must not remain in a translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    /// Korean: syllables, jamo and compatibility jamo.
    Hangul,
    /// Japanese hiragana and katakana, including the half-width forms.
    Kana,
    /// CJK ideographs (kanji / hanzi / hanja), including compatibility ideographs.
    Han,
}

/// Which [`Script`] a character belongs to, if any.
///
/// CJK punctuation (`、`, `「`, `・`) and the iteration mark `々` are
/// deliberately *not* scripts: they are not words that went untranslated.
pub fn script_of(c: char) -> Option<Script> {
    if is_hangul(c) {
        Some(Script::Hangul)
    } else if is_kana(c) {
        Some(Script::Kana)
    } else if is_han(c) {
        Some(Script::Han)
    } else {
        None
    }
}

/// True for any Hangul character: syllables, jamo, compatibility jamo.
pub fn is_hangul(c: char) -> bool {
    matches!(c,
        '\u{AC00}'..='\u{D7A3}'
        | '\u{1100}'..='\u{11FF}'
        | '\u{3131}'..='\u{318E}'
        | '\u{A960}'..='\u{A97F}'
        | '\u{D7B0}'..='\u{D7FF}'
        | '\u{FFA1}'..='\u{FFDC}')
}

/// True for a precomposed Hangul syllable only (가…힣).
pub fn is_hangul_syllable(c: char) -> bool {
    matches!(c, '\u{AC00}'..='\u{D7A3}')
}

/// True for hiragana and katakana, excluding the middle dot `・`.
pub fn is_kana(c: char) -> bool {
    matches!(c,
        '\u{3041}'..='\u{3096}'
        | '\u{309D}'..='\u{309F}'
        | '\u{30A1}'..='\u{30FA}'
        | '\u{30FC}'..='\u{30FF}'
        | '\u{31F0}'..='\u{31FF}'
        | '\u{FF66}'..='\u{FF9F}')
}

/// True for CJK ideographs.
pub fn is_han(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{20000}'..='\u{2FA1F}')
}
