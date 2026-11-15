//! Rename: deterministic replacement of one rendering by another.
//!
//! A Rename is a Suggestion the user marks as a plain change of rendering
//! (old words → new words). It is applied by exact replacement with no LLM
//! call, with care for word boundaries, case, possessives, "a"/"an" and
//! partial names. [`rename`] works on one Block's text; the caller decides
//! which Blocks to run it on and what to do with Blocks where it returns
//! `None`. It is meant for Target Language (English) text; word boundaries use
//! Unicode alphanumerics, so it never splits a word in any script.

use std::ops::Range;

use crate::matching::{chars_eq_ci, is_apostrophe, word_boundary_after, word_boundary_before};

/// What to rename.
///
/// All pairs (the main one and the partials) are applied in **one pass over
/// the original text**, longest match first, so a partial name never re-edits
/// text that the full name already replaced, and a replacement is never
/// searched for again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameSpec {
    /// The rendering to replace.
    pub old: String,
    /// What to put instead. Must not be blank.
    pub new: String,
    /// Extra `(old, new)` pairs for partial names: the full name `Tesselid
    /// Argent` renamed to `Reed Argent` should also turn a lone `Tesselid`
    /// into `Reed`. Use [`RenameSpec::with_derived_partials`] or add your own.
    pub partials: Vec<(String, String)>,
}

impl RenameSpec {
    /// A rename with no partial names.
    pub fn new(old: impl Into<String>, new: impl Into<String>) -> Self {
        Self {
            old: old.into(),
            new: new.into(),
            partials: Vec::new(),
        }
    }

    /// Add an extra `(old, new)` pair.
    #[must_use]
    pub fn with_partial(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.partials.push((old.into(), new.into()));
        self
    }

    /// Add the pairs [`derive_partials`] finds for `old` → `new`.
    ///
    /// Only appropriate for names of people: for places or items a single
    /// word such as `Magic` is a common word on its own.
    #[must_use]
    pub fn with_derived_partials(mut self) -> Self {
        let derived = derive_partials(&self.old, &self.new);
        self.partials.extend(derived);
        self
    }
}

/// Derive partial-name pairs from a multi-word rename.
///
/// When `old` and `new` have the same number of words (two or more) and
/// **exactly one** word differs, that word pair is the partial name:
/// `Tesselid Argent` → `Reed Argent` gives `[("Tesselid", "Reed")]`, and
/// `Duke Tesselid` → `Duke Reed` gives the same. Nothing is derived when the
/// word counts differ, more than one word differs, or the differing old word
/// is shorter than three letters or does not start with an uppercase letter.
///
/// The caller should only use this for people's names, and should show the
/// derived pairs to the user: `Duke` → `Marquess` would also rewrite every
/// other duke.
pub fn derive_partials(old: &str, new: &str) -> Vec<(String, String)> {
    let o: Vec<&str> = old.split_whitespace().collect();
    let n: Vec<&str> = new.split_whitespace().collect();
    if o.len() < 2 || o.len() != n.len() {
        return Vec::new();
    }
    let differing: Vec<usize> = (0..o.len()).filter(|&i| o[i] != n[i]).collect();
    let [i] = differing[..] else {
        return Vec::new();
    };
    let word = o[i];
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 3 || !word.chars().next().is_some_and(char::is_uppercase) {
        return Vec::new();
    }
    vec![(word.to_string(), n[i].to_string())]
}

/// The outcome of a [`rename`] that changed something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    /// The new text.
    pub text: String,
    /// Byte ranges **in the new `text`** of everything that was written:
    /// each replaced name (with its possessive ending) and each "a"/"an" that
    /// was corrected. Sorted, non-overlapping.
    pub changed: Vec<Range<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CaseKind {
    Exact,
    Upper,
    CapFirst,
    Title,
}

struct Edit {
    range: Range<usize>,
    replacement: String,
}

/// Apply a [`RenameSpec`] to `text`. Returns `None` when nothing changed.
///
/// **Matching** is case-insensitive and whole-word (`Reed` is not found in
/// `Reeds` or `Breed`; `'` and `’` are the same). What counts as a match
/// depends on the old rendering's own case:
/// - *Name-like* (starts uppercase, `Tesselid`, `Magic Tower`): the exact
///   spelling, or ALL CAPS (`TESSELID`). A lowercase `magic tower` is left
///   alone: lowercase variants of names are usually common words.
/// - *Phrase-like* (starts lowercase, `let out a breath`): the exact
///   spelling, a sentence-initial capital (`Let out a breath`), Title Case
///   (`Let Out A Breath`) or ALL CAPS.
///
/// **Replacement case** follows the match: the new rendering is written as
/// given, uppercased, with its first letter capitalised, or in Title Case.
///
/// **Possessives**: `'s`, `’s` and the bare `'` after an `s` are consumed
/// with the name and re-written for the new name: `Tesselid's` → `Reed's`,
/// `Moriphas'` → `Reed's`, `Reed's` → `Moriphas'`. When the old name already
/// ends in `s` and the text writes `'s` after it (`Moriphas's`), that style
/// is kept (`Moriphas's` → `Gus's`). An apostrophe after an `s` name is
/// treated as a closing quote, not a possessive, when the character before the
/// name is a quote.
///
/// **Articles**: an `a`/`an` right before a replaced name is corrected to fit
/// the new first sound (`an Argent` → `a Reed`), keeping its capital. Only a
/// directly adjacent article is touched (`a famous Argent` is left alone).
/// The sound rules are a small heuristic (vowel letters, `hour`/`honest`,
/// `university`/`one`, spelled-out initials like `an SS-rank`).
///
/// **Overlaps**: matches are taken longest first, so the full name wins over
/// its partials. Returns `None` for a blank `old` or `new`, or when the result
/// equals the input.
///
/// ```
/// use cipher_text::rename::{rename, RenameSpec};
/// let spec = RenameSpec::new("Moriphas", "Reed");
/// let r = rename("An angry Moriphas' sword, and Moriphas's cloak.", &spec).unwrap();
/// assert_eq!(r.text, "An angry Reed's sword, and Reed's cloak.");
/// ```
pub fn rename(text: &str, spec: &RenameSpec) -> Option<Renamed> {
    let mut pairs: Vec<(&str, &str)> = vec![(spec.old.as_str(), spec.new.as_str())];
    pairs.extend(spec.partials.iter().map(|(o, n)| (o.as_str(), n.as_str())));
    pairs.retain(|(o, n)| !o.trim().is_empty() && !n.trim().is_empty());
    if pairs.is_empty() || spec.old.trim().is_empty() || spec.new.trim().is_empty() {
        return None;
    }

    // Candidates: (range, pair index, case kind).
    let mut candidates: Vec<(Range<usize>, usize, CaseKind)> = Vec::new();
    for (pi, (old, _)) in pairs.iter().enumerate() {
        for (i, _) in text.char_indices() {
            let Some(len) = ci_prefix_len(&text[i..], old) else {
                continue;
            };
            let end = i + len;
            if !word_boundary_before(text, i) || !word_boundary_after(text, end) {
                continue;
            }
            if let Some(kind) = classify(&text[i..end], old) {
                candidates.push((i..end, pi, kind));
            }
        }
    }
    // Longest first (ties: leftmost, then main pair before partials).
    candidates.sort_by(|a, b| {
        (b.0.end - b.0.start)
            .cmp(&(a.0.end - a.0.start))
            .then(a.0.start.cmp(&b.0.start))
            .then(a.1.cmp(&b.1))
    });
    let mut taken = vec![false; text.len()];
    let mut chosen: Vec<(Range<usize>, usize, CaseKind)> = Vec::new();
    for c in candidates {
        if taken[c.0.clone()].iter().any(|&t| t) {
            continue;
        }
        taken[c.0.clone()].fill(true);
        chosen.push(c);
    }
    chosen.sort_by_key(|c| c.0.start);
    if chosen.is_empty() {
        return None;
    }

    let mut edits: Vec<Edit> = Vec::new();
    for (range, pi, kind) in chosen {
        let (old, new) = pairs[pi];
        let core = match kind {
            CaseKind::Exact => new.to_string(),
            CaseKind::Upper => new.to_uppercase(),
            CaseKind::CapFirst => capitalize_first(new),
            CaseKind::Title => title_case(new),
        };
        let (suffix_len, suffix) = possessive(text, &range, old, &core, kind);
        let mut replacement = core;
        replacement.push_str(&suffix);
        let full = range.start..range.end + suffix_len;

        if let Some(article) = fix_article(text, range.start, &replacement) {
            edits.push(article);
        }
        edits.push(Edit {
            range: full,
            replacement,
        });
    }
    edits.sort_by_key(|e| e.range.start);

    let mut out = String::with_capacity(text.len());
    let mut changed = Vec::new();
    let mut pos = 0;
    let mut any = false;
    for e in &edits {
        if e.range.start < pos {
            continue; // an article correction that collided with a previous edit
        }
        out.push_str(&text[pos..e.range.start]);
        let start = out.len();
        out.push_str(&e.replacement);
        if text[e.range.clone()] != e.replacement {
            any = true;
        }
        changed.push(start..out.len());
        pos = e.range.end;
    }
    out.push_str(&text[pos..]);
    (any && out != text).then_some(Renamed { text: out, changed })
}

/// Byte length of the prefix of `hay` that equals `pat` ignoring case.
fn ci_prefix_len(hay: &str, pat: &str) -> Option<usize> {
    let mut h = hay.char_indices();
    let mut consumed = 0;
    for pc in pat.chars() {
        let (i, hc) = h.next()?;
        if !chars_eq_ci(hc, pc) {
            return None;
        }
        consumed = i + hc.len_utf8();
    }
    Some(consumed)
}

fn classify(found: &str, old: &str) -> Option<CaseKind> {
    if found == old || found.replace('’', "'") == old.replace('’', "'") {
        return Some(CaseKind::Exact);
    }
    let letters = found.chars().filter(|c| c.is_alphabetic()).count();
    if letters >= 2
        && found
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(char::is_uppercase)
    {
        return Some(CaseKind::Upper);
    }
    let name_like = old
        .chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(char::is_uppercase);
    if name_like {
        return None;
    }
    let norm = |s: &str| s.replace('’', "'");
    if norm(found) == norm(&capitalize_first(old)) {
        Some(CaseKind::CapFirst)
    } else if norm(found) == norm(&title_case(old)) {
        Some(CaseKind::Title)
    } else {
        None
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_start = true;
    for c in s.chars() {
        if at_start {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        at_start = c.is_whitespace() || c == '-';
    }
    out
}

fn ends_with_s(s: &str) -> bool {
    s.chars().next_back().is_some_and(|c| c == 's' || c == 'S')
}

/// The possessive after a matched name: (bytes consumed in the original,
/// text to write after the new name).
fn possessive(
    text: &str,
    name: &Range<usize>,
    old: &str,
    new_core: &str,
    kind: CaseKind,
) -> (usize, String) {
    let rest = &text[name.end..];
    let mut chars = rest.chars();
    let Some(apos) = chars.next().filter(|c| is_apostrophe(*c)) else {
        return (0, String::new());
    };
    let after = chars.as_str();
    let s_char = after.chars().next().filter(|c| *c == 's' || *c == 'S');
    let alnum_after = |s: &str| s.chars().next().is_some_and(char::is_alphanumeric);
    let upper = kind == CaseKind::Upper;
    let s_out = if upper { 'S' } else { 's' };

    if let Some(sc) = s_char {
        if !alnum_after(&after[sc.len_utf8()..]) {
            let consumed = apos.len_utf8() + sc.len_utf8();
            // Keep the 's style when the old name already ended in s.
            let suffix = if ends_with_s(new_core) && !ends_with_s(old) {
                apos.to_string()
            } else {
                format!("{apos}{sc}")
            };
            return (consumed, suffix);
        }
        return (0, String::new());
    }
    if ends_with_s(old) && !alnum_after(after) {
        let opening_quote = text[..name.start]
            .chars()
            .next_back()
            .is_some_and(|c| is_apostrophe(c) || c == '‘');
        if opening_quote {
            return (0, String::new());
        }
        let suffix = if ends_with_s(new_core) {
            apos.to_string()
        } else {
            format!("{apos}{s_out}")
        };
        return (apos.len_utf8(), suffix);
    }
    (0, String::new())
}

/// An edit that corrects the "a"/"an" right before `start`, if it disagrees
/// with the first sound of `replacement`.
fn fix_article(text: &str, start: usize, replacement: &str) -> Option<Edit> {
    let before = text[..start].strip_suffix(' ')?;
    let word_len = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic())
        .map(char::len_utf8)
        .sum::<usize>();
    let word_start = before.len() - word_len;
    let word = &before[word_start..];
    if !word.eq_ignore_ascii_case("a") && !word.eq_ignore_ascii_case("an") {
        return None;
    }
    if !word_boundary_before(text, word_start) {
        return None;
    }
    let desired = article_for(replacement);
    if word.eq_ignore_ascii_case(desired) {
        return None;
    }
    let all_upper = word.len() > 1 && word.chars().all(char::is_uppercase);
    let cased = if all_upper {
        desired.to_uppercase()
    } else if word.chars().next().is_some_and(char::is_uppercase) {
        capitalize_first(desired)
    } else {
        desired.to_string()
    };
    Some(Edit {
        range: word_start..word_start + word.len(),
        replacement: cased,
    })
}

/// `"a"` or `"an"` for the first word of `phrase`, by sound (heuristic).
///
/// Vowel letters take `an`, except words that start with a consonant sound
/// (`university`, `one`, `unicorn`, ...); `hour`, `honest`, `honor` and `heir`
/// take `an`; short all-caps initials and single capital letters are judged by
/// the letter's name (`an SS-rank`, `an F-rank`, `a D-rank`); digits by the
/// number's name (`an 8`, `a 7`).
pub(crate) fn article_for(phrase: &str) -> &'static str {
    let word: String = phrase
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .collect();
    let Some(first) = word.chars().next() else {
        return "a";
    };
    let n = word.chars().count();
    let lower = word.to_lowercase();
    if first.is_ascii_digit() {
        return if lower.starts_with('8') || lower.starts_with("11") || lower.starts_with("18") {
            "an"
        } else {
            "a"
        };
    }
    let initialism = word.chars().all(char::is_uppercase) && (n == 1 || (2..=3).contains(&n));
    if initialism {
        return if "aefhilmnorsx".contains(first.to_ascii_lowercase()) {
            "an"
        } else {
            "a"
        };
    }
    const AN_WORDS: &[&str] = &["hour", "honest", "honor", "honour", "heir"];
    const A_WORDS: &[&str] = &[
        "univers", "unicorn", "unifor", "union", "uniqu", "unit", "unan", "use", "usual", "usurp",
        "utensil", "utilit", "utop", "ukrain", "ubiq", "uran", "eu", "one", "once", "ewe",
    ];
    if AN_WORDS.iter().any(|w| lower.starts_with(w)) {
        "an"
    } else if A_WORDS.iter().any(|w| lower.starts_with(w)) {
        "a"
    } else if "aeiou".contains(first.to_ascii_lowercase()) {
        "an"
    } else {
        "a"
    }
}
