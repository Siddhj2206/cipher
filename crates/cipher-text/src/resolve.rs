//! Local source resolution: which source expression does a Selection render?
//!
//! The user selects translated text such as `Duke Tesselid`. For an
//! "Everywhere" Suggestion Cipher needs the **source expression** behind it.
//! If the Selection is a Term's rendering the source form is known. Otherwise
//! take every Pair whose translation contains the Selection (the **hits**) and
//! look for a source substring that is frequent in the hits and rare in all the
//! other Pairs (the **others**). This works when the phrase occurs three or
//! more times; one-offs need a (tiny) model call, which is not this module.
//!
//! Korean words carry particles (`테셀리드가`, `테셀리드를`), so candidates are
//! character n-grams inside runs of letters and single spaces, scored by how
//! well they separate hits from others, and trailing particles are stripped
//! from the winner when that does not make it worse.

use std::collections::{HashMap, HashSet};

use crate::matching::strip_one_particle;

/// Fewer hits than this and [`resolve`] gives up.
pub const MIN_HITS: usize = 3;
/// The winner must occur in at least this fraction of the hits.
pub const MIN_RECALL: f64 = 0.6;
/// At least this fraction of all Blocks that contain the winner must be hits.
pub const MIN_PRECISION: f64 = 0.5;
/// The winner's score must beat every unrelated candidate by this much.
pub const MIN_MARGIN: f64 = 0.1;
/// Shortest candidate, in characters.
pub const MIN_NGRAM: usize = 2;
/// Longest candidate, in characters (spaces count).
pub const MAX_NGRAM: usize = 14;
/// At most this many candidates are returned by [`candidates`].
pub const MAX_CANDIDATES: usize = 8;
/// Candidates are generated from at most this many hits (an even sample);
/// frequency is then counted over all of them.
const SAMPLE: usize = 40;

/// A possible source expression with its evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The expression, a literal substring of the hit sources.
    pub text: String,
    /// How many hit sources contain it.
    pub hit_blocks: usize,
    /// How many other sources contain it.
    pub other_blocks: usize,
    /// `hit_blocks / hits`.
    pub recall: f64,
    /// `hit_blocks / (hit_blocks + other_blocks)`.
    pub precision: f64,
    /// Harmonic mean of recall and precision, 0–1.
    pub score: f64,
}

/// Ranked candidates, best first (for showing in the UI).
///
/// `hits` are the source texts of Pairs whose translation contains the
/// Selection; `others` are the source texts of every other Pair in scope.
/// Candidates must occur in at least [`MIN_HITS`] hits and in
/// [`MIN_RECALL`] of them. Among candidates with the same hit set the longest
/// is kept. Ties on score go to the longer text. At most [`MAX_CANDIDATES`]
/// are returned; fewer than [`MIN_HITS`] hits gives an empty list.
///
/// Cost is roughly `candidates × (hits + others)` substring searches, where
/// the number of surviving candidates is in the tens to low hundreds.
pub fn candidates(hits: &[impl AsRef<str>], others: &[impl AsRef<str>]) -> Vec<Candidate> {
    let h = hits.len();
    if h < MIN_HITS {
        return Vec::new();
    }
    let hit_texts: Vec<&str> = hits.iter().map(AsRef::as_ref).collect();
    let other_texts: Vec<&str> = others.iter().map(AsRef::as_ref).collect();
    let need = MIN_HITS.max((MIN_RECALL * h as f64).ceil() as usize);

    // 1. Generate n-grams from an even sample and keep those frequent in it.
    let step = h.div_ceil(SAMPLE);
    let sample: Vec<&str> = hit_texts.iter().step_by(step).copied().collect();
    let mut sample_df: HashMap<String, usize> = HashMap::new();
    for text in &sample {
        for gram in ngrams(text) {
            *sample_df.entry(gram).or_insert(0) += 1;
        }
    }
    let sample_need = ((MIN_RECALL * sample.len() as f64).ceil() as usize).max(1);

    // 2. Exact frequency over all hits.
    let mut scored: Vec<(String, usize)> = sample_df
        .into_iter()
        .filter(|(_, n)| *n >= sample_need)
        .filter_map(|(gram, _)| {
            let n = hit_texts.iter().filter(|t| t.contains(&gram)).count();
            (n >= need).then_some((gram, n))
        })
        .collect();

    // 3. Same hit set: keep only the longest (it can only have fewer "others").
    scored.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then(b.0.len().cmp(&a.0.len()))
            .then(a.0.cmp(&b.0))
    });
    let mut kept: Vec<(String, usize)> = Vec::new();
    for (gram, n) in scored {
        let dominated = kept
            .iter()
            .any(|(k, kn)| *kn == n && k.len() > gram.len() && k.contains(&gram));
        if !dominated {
            kept.push((gram, n));
        }
    }

    // 4. Score against the others, then try stripping particles.
    let score = |text: &str, hit_blocks: usize| -> Candidate {
        let other_blocks = other_texts.iter().filter(|t| t.contains(text)).count();
        let recall = hit_blocks as f64 / h as f64;
        let precision = hit_blocks as f64 / (hit_blocks + other_blocks) as f64;
        Candidate {
            text: text.to_string(),
            hit_blocks,
            other_blocks,
            recall,
            precision,
            score: 2.0 * recall * precision / (recall + precision),
        }
    };
    let mut out: Vec<Candidate> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (gram, n) in kept {
        let mut cand = score(&gram, n);
        while let Some(stem) = strip_one_particle(&cand.text) {
            let hits_with_stem = hit_texts.iter().filter(|t| t.contains(stem)).count();
            let stripped = score(stem, hits_with_stem);
            if stripped.score >= cand.score {
                cand = stripped;
            } else {
                break;
            }
        }
        if seen.insert(cand.text.clone()) {
            out.push(cand);
        }
    }
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.text.chars().count().cmp(&a.text.chars().count()))
            .then(a.text.cmp(&b.text))
    });
    out.truncate(MAX_CANDIDATES);
    out
}

/// The source expression behind a Selection, or `None` when there is no clear winner.
///
/// Takes the best of [`candidates`] and accepts it only if all hold:
/// - there are at least [`MIN_HITS`] hits;
/// - recall ≥ [`MIN_RECALL`] and precision ≥ [`MIN_PRECISION`];
/// - its score beats every *unrelated* candidate (one that neither contains
///   nor is contained in it) by at least [`MIN_MARGIN`]: two unrelated
///   expressions that fit equally well mean the evidence cannot say which one
///   is meant.
///
/// A trailing particle is stripped from the winner when that does not lower
/// its score. This is evidence-based, not blind: a noun that merely ends in
/// `이` keeps it when the stem is worse. Returning `None` means "ask the model
/// (a one-off), or fall back to the Selection text alone".
///
/// ```
/// use cipher_text::resolve::resolve;
/// let hits = ["테셀리드가 웃었다.", "그는 테셀리드를 보았다.", "테셀리드는 말이 없었다."];
/// let others = ["비가 내렸다.", "그는 문을 열었다."];
/// assert_eq!(resolve(&hits, &others).unwrap().text, "테셀리드");
/// ```
pub fn resolve(hits: &[impl AsRef<str>], others: &[impl AsRef<str>]) -> Option<Candidate> {
    let cands = candidates(hits, others);
    let best = cands.first()?;
    if best.hit_blocks < MIN_HITS || best.recall < MIN_RECALL || best.precision < MIN_PRECISION {
        return None;
    }
    let rival = cands
        .iter()
        .skip(1)
        .find(|c| !c.text.contains(&best.text) && !best.text.contains(&c.text));
    if rival.is_some_and(|r| best.score - r.score < MIN_MARGIN) {
        return None;
    }
    Some(best.clone())
}

/// All character n-grams of `text` inside runs of letters/digits and single
/// spaces, from [`MIN_NGRAM`] to [`MAX_NGRAM`] characters, never starting or
/// ending with a space. Unique per call.
fn ngrams(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Find the next run.
        if !chars[i].is_alphanumeric() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len()
            && (chars[j].is_alphanumeric()
                || (chars[j] == ' ' && chars.get(j + 1).is_some_and(|c| c.is_alphanumeric())))
        {
            j += 1;
        }
        let run = &chars[i..j];
        for s in 0..run.len() {
            if run[s] == ' ' {
                continue;
            }
            for e in (s + MIN_NGRAM)..=(s + MAX_NGRAM).min(run.len()) {
                if run[e - 1] != ' ' {
                    out.insert(run[s..e].iter().collect());
                }
            }
        }
        i = j;
    }
    out
}
