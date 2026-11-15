//! Matchers: which Terms occur in a text.
//!
//! Three tools with different trade-offs:
//! - [`terms_in`], the **recall matcher**. Selecting Terms for a prompt. A
//!   wrongly included Term costs about 40 characters, so it never misses.
//! - [`find`], the **precision matcher**. Checks and Drift. Non-overlapping,
//!   longest match wins, and short Hangul source forms must sit on a word
//!   boundary so that `한` does not match inside `한국`.
//! - [`find_rendering`], the **English-side finder**. Looks for a rendering
//!   (or a Forbidden Rendering) in translated text.

use std::ops::Range;

use crate::script::is_hangul;

/// Korean particles (조사) and copula endings that may follow a noun.
///
/// Used by the precision matcher to accept a short Hangul source form that is
/// followed by a particle (`한은`, `한에게는`), and by
/// [`crate::resolve`] to strip particles from a source expression. Verb
/// endings are not included on purpose: only things that attach to nouns.
pub const PARTICLES: &[&str] = &[
    "은",
    "는",
    "이",
    "가",
    "을",
    "를",
    "의",
    "에",
    "도",
    "만",
    "로",
    "와",
    "과",
    "께",
    "아",
    "야",
    "여",
    "님",
    "씨",
    "에게",
    "에서",
    "으로",
    "에는",
    "에도",
    "로는",
    "로도",
    "과는",
    "와는",
    "께서",
    "이나",
    "이랑",
    "랑",
    "하고",
    "보다",
    "처럼",
    "까지",
    "부터",
    "마저",
    "조차",
    "밖에",
    "한테",
    "이라",
    "이란",
    "이여",
    "이야",
    "이다",
    "이며",
    "이든",
    "라고",
    "라는",
    "에게는",
    "에게도",
    "에서는",
    "에서도",
    "에게서",
    "으로는",
    "으로도",
    "한테서",
    "이라고",
    "이라는",
    "이었다",
    "였다",
    "입니다",
    "께서는",
    "과의",
    "와의",
    "에게의",
    "에서의",
    "으로의",
    "로의",
];

/// A [`find`] result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Index into the `sources` slice that matched.
    pub index: usize,
    /// Byte range of the match in the searched text.
    pub range: Range<usize>,
}

/// Recall matcher: indices of `sources` that occur anywhere in `text`.
///
/// Plain substring search, case-insensitive for scripts with case, with no
/// boundary rules at all. Empty (or all-whitespace) sources never match.
/// Indices are returned in ascending order, each at most once.
///
/// ```
/// use cipher_text::matching::terms_in;
/// // 한 matches inside 한국 here: this matcher favours recall.
/// assert_eq!(terms_in("한국에서", &["한", "미국"]), [0]);
/// ```
pub fn terms_in(text: &str, sources: &[&str]) -> Vec<usize> {
    let hay = text.to_lowercase();
    sources
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            let s = s.trim();
            !s.is_empty() && hay.contains(&s.to_lowercase())
        })
        .map(|(i, _)| i)
        .collect()
}

/// Precision matcher: every occurrence of any source, without overlaps.
///
/// - Matching is exact (case-sensitive substring); empty sources are ignored.
/// - **Longest match wins.** Candidates are taken longest first (ties go to
///   the leftmost, then the lowest index), and a candidate that overlaps an
///   accepted one is dropped. So with sources `아르젠트` and `아르젠트 공작`,
///   the text `아르젠트 공작이` yields one match, of the second source.
/// - **Short Hangul sources** (1–2 Hangul syllables in the source) must be
///   whole words: the character before the match must not be Hangul (when the
///   source starts with Hangul), and the text after it (when the source ends
///   with Hangul) must be the end, a non-Hangul character, or a chain of
///   Korean [`PARTICLES`] that itself ends at a non-Hangul character or the
///   end. So `한은 말했다` and `한에게는 말했다` match `한`, but `한국`,
///   `한의사` and `대한` do not.
/// - Sources with three or more Hangul syllables, or none, are plain
///   substring matches.
///
/// The result is sorted by start offset.
///
/// ```
/// use cipher_text::matching::find;
/// let m = find("한국의 한은 웃었다", &["한"]);
/// assert_eq!(m.len(), 1);
/// assert_eq!(&"한국의 한은 웃었다"[m[0].range.clone()], "한");
/// ```
pub fn find(text: &str, sources: &[&str]) -> Vec<Match> {
    let mut candidates: Vec<Match> = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        if source.is_empty() {
            continue;
        }
        for (start, _) in text.match_indices(source) {
            let end = start + source.len();
            if hangul_boundary_ok(text, start, end, source) {
                candidates.push(Match {
                    index,
                    range: start..end,
                });
            }
        }
    }
    candidates.sort_by(|a, b| {
        (b.range.end - b.range.start)
            .cmp(&(a.range.end - a.range.start))
            .then(a.range.start.cmp(&b.range.start))
            .then(a.index.cmp(&b.index))
    });
    let mut taken = vec![false; text.len()];
    let mut accepted = Vec::new();
    for m in candidates {
        if taken[m.range.clone()].iter().any(|&t| t) {
            continue;
        }
        taken[m.range.clone()].fill(true);
        accepted.push(m);
    }
    accepted.sort_by_key(|m| m.range.start);
    accepted
}

/// Number of [`find`] matches per source: `result[i]` counts source `i`.
///
/// Longest-match-wins is applied across all `sources` together, so a short
/// name inside a longer title is not double counted.
pub fn count_matches(text: &str, sources: &[&str]) -> Vec<usize> {
    let mut counts = vec![0; sources.len()];
    for m in find(text, sources) {
        counts[m.index] += 1;
    }
    counts
}

fn hangul_syllables(s: &str) -> usize {
    s.chars().filter(|&c| is_hangul(c)).count()
}

fn hangul_boundary_ok(text: &str, start: usize, end: usize, source: &str) -> bool {
    let n = hangul_syllables(source);
    if n == 0 || n > 2 {
        return true;
    }
    if source.chars().next().is_some_and(is_hangul)
        && text[..start].chars().next_back().is_some_and(is_hangul)
    {
        return false;
    }
    if source.chars().next_back().is_some_and(is_hangul) {
        return particles_then_boundary(&text[end..], 4);
    }
    true
}

/// True when `rest` starts with a non-Hangul char (or is empty), or with a
/// particle chain that is followed by one.
fn particles_then_boundary(rest: &str, depth: u8) -> bool {
    match rest.chars().next() {
        None => return true,
        Some(c) if !is_hangul(c) => return true,
        _ => {}
    }
    if depth == 0 {
        return false;
    }
    PARTICLES
        .iter()
        .filter(|p| rest.starts_with(**p))
        .any(|p| particles_then_boundary(&rest[p.len()..], depth - 1))
}

/// English-side finder: occurrences of `rendering` in translated `text`.
///
/// - **Whole word(s)**: the character before must not be a letter or digit
///   (and not the second half of `O'Brien`); the character after must not be
///   a letter or digit. A following possessive (`'s`, `’s`, `s'`) is therefore
///   allowed, and the returned range **excludes** it.
/// - **First letter is case-insensitive**: `Argent` finds `argent`, but the
///   rest must match exactly (`ARGENT` does not match). Apostrophes `'` and
///   `’` are interchangeable.
/// - Occurrences do not overlap. An empty rendering finds nothing.
///
/// ```
/// use cipher_text::matching::find_rendering;
/// let t = "Argent's sword; the argent coin; Argental.";
/// let r: Vec<_> = find_rendering(t, "Argent").into_iter().map(|r| &t[r]).collect();
/// assert_eq!(r, ["Argent", "argent"]);
/// ```
pub fn find_rendering(text: &str, rendering: &str) -> Vec<Range<usize>> {
    let Some(first) = rendering.chars().next() else {
        return Vec::new();
    };
    let rest = &rendering[first.len_utf8()..];
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut skip_until = 0;
    for (i, c) in text.char_indices() {
        if i < skip_until || !chars_eq_ci(c, first) {
            continue;
        }
        let after_first = i + c.len_utf8();
        let Some(rest_len) = prefix_len_exact(&text[after_first..], rest) else {
            continue;
        };
        let end = after_first + rest_len;
        if word_boundary_before(text, i) && word_boundary_after(text, end) {
            out.push(i..end);
            skip_until = end;
        }
    }
    out
}

/// Length in bytes of the prefix of `hay` that equals `pat` (apostrophe
/// variants treated as equal), or `None`.
fn prefix_len_exact(hay: &str, pat: &str) -> Option<usize> {
    let mut h = hay.char_indices();
    let mut consumed = 0;
    for pc in pat.chars() {
        let (i, hc) = h.next()?;
        if hc != pc && !(is_apostrophe(hc) && is_apostrophe(pc)) {
            return None;
        }
        consumed = i + hc.len_utf8();
    }
    Some(consumed)
}

pub(crate) fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '’'
}

/// Equality ignoring case, with `'` and `’` equal.
pub(crate) fn chars_eq_ci(a: char, b: char) -> bool {
    a == b || (is_apostrophe(a) && is_apostrophe(b)) || a.to_lowercase().eq(b.to_lowercase())
}

/// The character at `start` begins a word: the previous char is not
/// alphanumeric and is not an apostrophe glued to a letter (`O'Brien`).
pub(crate) fn word_boundary_before(text: &str, start: usize) -> bool {
    let mut before = text[..start].chars().rev();
    match before.next() {
        None => true,
        Some(c) if c.is_alphanumeric() => false,
        Some(c) if is_apostrophe(c) => !before.next().is_some_and(char::is_alphanumeric),
        Some(_) => true,
    }
}

/// The match ending at `end` ends a word: the next char is not alphanumeric.
pub(crate) fn word_boundary_after(text: &str, end: usize) -> bool {
    !text[end..]
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric)
}

/// Strip one trailing particle, longest first, keeping at least two
/// characters of stem. Returns `None` when no particle can be stripped.
pub(crate) fn strip_one_particle(word: &str) -> Option<&str> {
    PARTICLES
        .iter()
        .filter(|p| word.ends_with(**p))
        .max_by_key(|p| p.len())
        .map(|p| &word[..word.len() - p.len()])
        .filter(|stem| stem.chars().count() >= 2)
}
