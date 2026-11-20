//! Other wordings: what a translation calls a Term where it doesn't use the Term's rendering.
//!
//! Evidence for the user to judge, never a fix by itself: on Genius Wizard (30 drifting Terms,
//! 2026-11-16) about 63% of the wordings shown are real alternative names, the rest are a second
//! meaning of the source word (수인 is also "hand seals") or phrases that sit near the name.
//! `docs/design/translation-pipeline.md` › Drift has the method and the measurements.
//!
//! A wording is a run of 1–4 English words, neither starting nor ending with a stopword, that
//! appears in the Pairs naming the Term without its rendering:
//! - in at least 3 of them and 5% of them;
//! - at least 10× as often there as in Pairs that don't name the Term;
//! - and at least 15% of all Pairs containing it name the Term (`began`, `level` fail).
//!
//! Variants are one wording: case, `'`/`’`, the plural or possessive of the last word. A wording
//! mostly inside longer wordings that are kept (`threads` in `mana threads`) is dropped, and so is
//! one that only extends a shorter kept wording (`Kebaek's body`).

use std::collections::{HashMap, HashSet};

use crate::checks::PairText;
use crate::matching::{SourceForms, find_rendering};

/// Wordings shown at most.
pub const SHOWN: usize = 5;

const STOP: &[&str] = &[
    "the", "a", "an", "of", "to", "and", "in", "on", "at", "by", "for", "with", "from", "as", "is",
    "was", "he", "she", "it", "his", "her", "its", "they", "them", "their", "that", "this", "i",
    "you", "we", "be", "had", "has", "have", "were", "are", "not", "but", "or", "so", "if", "into",
    "than", "then", "there", "what", "who", "which", "me", "my", "your", "our", "us", "him", "do",
    "did", "does", "been", "being", "am", "will", "would", "could", "can", "should", "may",
    "might", "shall", "just", "very", "also", "too", "no", "yes", "up", "down", "out", "off",
    "over", "under", "again", "here", "when", "where", "how", "all", "any", "each", "more", "most",
    "other", "some", "such", "only", "own", "same", "about", "after", "before", "while", "whom",
    "whose", "these", "those", "because", "until", "s", "t", "don", "didn", "won", "isn", "wasn",
];

/// What the Pairs say about one Term.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evidence {
    /// Pairs whose source names the Term (longest match: 마탑 inside 블레이버 마탑 doesn't count).
    pub naming: Vec<usize>,
    /// Of those, the Pairs whose translation renders it.
    pub rendered: Vec<usize>,
    /// Other wordings, most Pairs first, at most [`SHOWN`].
    pub wordings: Vec<Wording>,
    /// Pairs naming the Term that render neither it nor a shown wording.
    pub neither: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wording {
    /// The most frequent spelling.
    pub text: String,
    /// Pairs (naming the Term, not rendering it) that contain it.
    pub pairs: Vec<usize>,
    /// Another Term whose rendering this wording is, contains or sits in, by index.
    pub other_term: Option<usize>,
}

/// One Term to look for, with all Terms' source forms and renderings for comparison.
pub struct Query<'a> {
    pub term: usize,
    pub sources: &'a SourceForms<'a>,
    pub renderings: &'a [&'a str],
}

/// The evidence for `q.term` over a Book's Pairs.
pub fn evidence(pairs: &[PairText], q: &Query) -> Evidence {
    let rendering = q.renderings[q.term];
    let names = |text: &str| q.sources.names_term(text, q.term);
    let mut out = Evidence::default();
    let mut naming = vec![false; pairs.len()];
    let mut missing: Vec<usize> = Vec::new();
    for (i, p) in pairs.iter().enumerate() {
        if !names(p.source) {
            continue;
        }
        naming[i] = true;
        out.naming.push(i);
        if find_rendering(p.translation, rendering).is_empty() {
            missing.push(i);
        } else {
            out.rendered.push(i);
        }
    }
    if missing.is_empty() {
        return out;
    }

    // Candidates: keys in enough Pairs of `missing`.
    let mut sets: HashMap<Key, Vec<usize>> = HashMap::new();
    let mut surfaces: HashMap<Key, HashMap<String, usize>> = HashMap::new();
    for &i in &missing {
        for (key, surface) in grams(pairs[i].translation, None) {
            *surfaces
                .entry(key.clone())
                .or_default()
                .entry(surface)
                .or_default() += 1;
            sets.entry(key).or_default().push(i);
        }
    }
    let threshold = (missing.len() as f64 * 0.05).max(3.0);
    let own = key_of(&words(rendering));
    let mut candidates: Vec<Key> = sets
        .iter()
        .filter(|(k, v)| {
            v.len() as f64 >= threshold && **k != own && !(!own.is_empty() && contains(k, &own))
        })
        .map(|(k, _)| k.clone())
        .collect();
    if candidates.is_empty() {
        out.neither = missing;
        return out;
    }

    // How often each candidate appears in all Pairs, and in those naming the Term.
    let wanted: HashSet<&Key> = candidates.iter().collect();
    let firsts: HashSet<String> = candidates.iter().map(|k| k[0].clone()).collect();
    let mut total: HashMap<Key, usize> = HashMap::new();
    let mut in_naming: HashMap<Key, usize> = HashMap::new();
    for (i, p) in pairs.iter().enumerate() {
        for (key, _) in grams(p.translation, Some(&firsts)) {
            if wanted.contains(&key) {
                if naming[i] {
                    *in_naming.entry(key.clone()).or_default() += 1;
                }
                *total.entry(key).or_default() += 1;
            }
        }
    }
    let elsewhere_pairs = (pairs.len() - out.naming.len()).max(1) as f64;
    candidates.retain(|k| {
        let c = sets[k].len() as f64;
        let t = total.get(k).copied().unwrap_or(0);
        let n = in_naming.get(k).copied().unwrap_or(0);
        let other = t.saturating_sub(n) as f64;
        let enriched =
            other == 0.0 || (c / missing.len() as f64) / (other / elsewhere_pairs) >= 10.0;
        enriched && n as f64 >= 0.15 * t as f64
    });

    // Collapse: a key mostly inside longer kept keys goes; a key that only extends a shorter
    // kept key goes.
    let set_of = |k: &Key| -> HashSet<usize> { sets[k].iter().copied().collect() };
    let mut kept: Vec<Key> = Vec::new();
    for k in &candidates {
        let mine = set_of(k);
        let mut covered: HashSet<usize> = HashSet::new();
        for longer in candidates
            .iter()
            .filter(|l| l.len() > k.len() && contains(l, k))
        {
            covered.extend(sets[longer].iter().copied());
        }
        if !covered.is_empty() && mine.intersection(&covered).count() * 10 >= mine.len() * 7 {
            continue;
        }
        kept.push(k.clone());
    }
    let extensions: Vec<Key> = kept
        .iter()
        .filter(|k| {
            let mine = set_of(k);
            kept.iter()
                .filter(|s| s.len() < k.len() && contains(k, s))
                .any(|s| {
                    let theirs = set_of(s);
                    mine.intersection(&theirs).count() * 10 >= mine.len() * 9
                        && mine.len() * 2 < theirs.len()
                })
        })
        .cloned()
        .collect();
    kept.retain(|k| !extensions.contains(k));

    // Another Term's rendering: dropped when that Term is named where the wording is (it is
    // that Term's own name there), else labelled.
    let other_keys: Vec<(usize, Key)> = q
        .renderings
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != q.term)
        .map(|(i, r)| (i, key_of(&words(r))))
        .filter(|(_, k)| !k.is_empty())
        .collect();
    let mut shown: Vec<Wording> = Vec::new();
    for k in kept {
        let other = other_keys
            .iter()
            .find(|(_, ok)| contains(ok, &k) || contains(&k, ok))
            .map(|(i, _)| *i);
        if let Some(t) = other {
            let named_there = sets[&k]
                .iter()
                .filter(|&&p| q.sources.names_term(pairs[p].source, t))
                .count();
            if named_there * 2 >= sets[&k].len() {
                continue;
            }
        }
        let text = surfaces[&k]
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(s, _)| s.clone())
            .unwrap_or_default();
        shown.push(Wording {
            text,
            pairs: sets[&k].clone(),
            other_term: other,
        });
    }
    shown.sort_by(|a, b| b.pairs.len().cmp(&a.pairs.len()).then(a.text.cmp(&b.text)));
    shown.truncate(SHOWN);
    let covered: HashSet<usize> = shown.iter().flat_map(|w| w.pairs.iter().copied()).collect();
    out.neither = missing
        .into_iter()
        .filter(|i| !covered.contains(i))
        .collect();
    out.wordings = shown;
    out
}

/// Lowercase words, the last one stemmed.
type Key = Vec<String>;

/// `inner` is a run of `outer`'s words, each compared stemmed (`hand seal` is in
/// `hand seals with both`).
fn contains(outer: &Key, inner: &Key) -> bool {
    let n = inner.len();
    let o: Vec<String> = outer.iter().map(|w| stem(w)).collect();
    let i: Vec<String> = inner.iter().map(|w| stem(w)).collect();
    n > 0 && n <= o.len() && o.windows(n).any(|w| w == i.as_slice())
}

/// Words: letters and digits, with `'`, `’` or `-` inside, and a trailing apostrophe.
fn words(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let alnum = |i: usize| i < b.len() && b[i].is_ascii_alphanumeric();
    let mut i = 0;
    while i < text.len() {
        if !alnum(i) {
            i += 1;
            continue;
        }
        let start = i;
        loop {
            while alnum(i) {
                i += 1;
            }
            let joiner = if text[i..].starts_with('’') {
                3
            } else if text[i..].starts_with(['\'', '-']) {
                1
            } else {
                0
            };
            if joiner > 0 && alnum(i + joiner) {
                i += joiner;
            } else {
                if joiner > 0 && text[i..].starts_with(['\'', '’']) {
                    i += joiner;
                }
                break;
            }
        }
        out.push(&text[start..i]);
    }
    out
}

fn stem(word: &str) -> String {
    let mut w = word.to_lowercase().replace('’', "'");
    if let Some(s) = w.strip_suffix("'s") {
        w = s.to_string();
    } else if w.ends_with("s'") {
        w.pop();
    }
    let w = w.trim_end_matches('\'').to_string();
    if w.len() > 4 && w.ends_with("ies") {
        return format!("{}y", &w[..w.len() - 3]);
    }
    if ["ss", "us", "is"].iter().any(|e| w.ends_with(e)) {
        return w;
    }
    if ["sses", "xes", "ches", "shes", "zes"]
        .iter()
        .any(|e| w.ends_with(e))
    {
        return w[..w.len() - 2].to_string();
    }
    if w.len() > 3 && w.ends_with('s') {
        return w[..w.len() - 1].to_string();
    }
    w
}

fn key_of(ws: &[&str]) -> Key {
    let Some((last, rest)) = ws.split_last() else {
        return Vec::new();
    };
    let mut k: Key = rest
        .iter()
        .map(|w| w.to_lowercase().replace('’', "'"))
        .collect();
    k.push(stem(last));
    k
}

/// Every 1–4 word run of a text, by key, with its spelling; each key once. With `firsts`, only
/// runs whose key starts with one of them.
fn grams(text: &str, firsts: Option<&HashSet<String>>) -> Vec<(Key, String)> {
    let ws = words(text);
    let stop = |w: &str| STOP.contains(&w.to_lowercase().as_str());
    let starts: Vec<bool> = ws
        .iter()
        .map(|w| {
            firsts.is_none_or(|f| {
                f.contains(&w.to_lowercase().replace('’', "'")) || f.contains(&stem(w))
            })
        })
        .collect();
    let mut seen: HashSet<Key> = HashSet::new();
    let mut out = Vec::new();
    for n in 1..=4 {
        for (at, g) in ws.windows(n).enumerate() {
            if !starts[at] || stop(g[0]) || stop(g[n - 1]) {
                continue;
            }
            let k = key_of(g);
            if seen.insert(k.clone()) {
                out.push((k, g.join(" ")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_keep_digits_apostrophes_and_hyphens() {
        assert_eq!(
            words("the 7th Legion’s Arch-Wizard, Lennok's threads' ends"),
            [
                "the",
                "7th",
                "Legion’s",
                "Arch-Wizard",
                "Lennok's",
                "threads'",
                "ends"
            ]
        );
    }

    #[test]
    fn variants_share_a_key() {
        assert_eq!(key_of(&["Mana", "Threads’"]), key_of(&["mana", "thread"]));
        assert_eq!(key_of(&["Witches"]), key_of(&["witch"]));
    }
}
