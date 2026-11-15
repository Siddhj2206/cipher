//! Checks on a translated Chapter. They return **findings**, never fixes.
//!
//! Coverage (every source number appears exactly once) is not here: it is
//! [`crate::markers::Parsed::missing`] and friends.
//!
//! Inputs are plain structs: a [`PairText`] is the source and translation
//! text of one Pair, and a [`CheckTerm`] is a Term reduced to what the checks
//! need. Findings refer to Pairs and Terms by their index in the slices the
//! caller passed.
//!
//! Matching rules: source names are found with the precision matcher
//! ([`crate::matching::find`], all Terms at once, so a short name inside a
//! longer title is not counted twice) and renderings with
//! [`crate::matching::find_rendering`].

use std::collections::BTreeMap;
use std::ops::Range;

use crate::matching::{count_matches, find_rendering};
use crate::script::{Script, script_of};

/// The source and translation text of one Pair (a run of source Blocks and the
/// translated Blocks that render them, joined as the caller likes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairText<'a> {
    /// Source text.
    pub source: &'a str,
    /// Translation text.
    pub translation: &'a str,
}

/// A Term as the checks see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckTerm {
    /// Source form. One per Term; the precision matcher handles grammatical
    /// forms (particles) itself. An empty source never matches.
    pub source: String,
    /// The approved rendering. An empty rendering disables the missing-name check.
    pub rendering: String,
    /// Renderings that must not appear.
    pub forbidden: Vec<String>,
    /// The translation may leave it unnamed: the missing-name check skips it.
    pub may_omit: bool,
}

/// A run of source-script characters found in a translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRun {
    /// Byte range in the translation.
    pub range: Range<usize>,
    /// Which script (of the first character; runs are broken at script changes).
    pub script: Script,
}

/// Why a length ratio was flagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthProblem {
    /// The translation is empty.
    Empty,
    /// Much shorter than a translation of that source should be: an omission.
    TooShort,
    /// Much longer than it should be: a repetition loop or an addition.
    TooLong,
}

/// Thresholds for [`length_ratio`].
///
/// The ratio is *translation characters ÷ source characters* (Unicode scalar
/// values, after trimming), for Korean → English.
///
/// **Measured on the S-Class Transmigrator test Book (2026-11-11).** Only the
/// 22 of 94 Chapters whose source and translation had the same number of
/// paragraphs could be compared block by block; of their 82 Blocks with more
/// than 80 source characters, the ratio had:
/// median 2.02, quartiles 1.77 / 2.29, 95th percentile 2.57, 99th 2.77.
/// The few Blocks under 1.4 or over 2.8 were all paragraph misalignments
/// (a source paragraph holding several glued system lines), not real
/// omissions. Whole Chapters: median 2.22, range 1.01–4.70.
/// The defaults are about half and twice the median: a translation that has
/// lost half its text, or doubled it, is flagged; ordinary variation is not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LengthLimits {
    /// Only Pairs whose source has more characters than this are checked
    /// (short dialogue is too noisy). An empty translation of a non-empty
    /// source is always flagged.
    pub min_source_chars: usize,
    /// Ratios below this are [`LengthProblem::TooShort`].
    pub min_ratio: f64,
    /// Ratios above this are [`LengthProblem::TooLong`].
    pub max_ratio: f64,
}

impl Default for LengthLimits {
    fn default() -> Self {
        Self {
            min_source_chars: 80,
            min_ratio: 0.9,
            max_ratio: 4.0,
        }
    }
}

/// Options for [`check_chapter`].
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// Length thresholds.
    pub length: LengthLimits,
    /// Substrings that may legitimately contain source script (a deliberate
    /// `✠` sigil, a transliterated sound effect, ...). Source script inside
    /// one of these is not reported.
    pub allowed_script: Vec<String>,
}

/// Where a forbidden rendering was found in one Pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbiddenHit {
    /// Index of the Term in the slice passed to the check.
    pub term_index: usize,
    /// The Forbidden Rendering that was found (as spelled in the Term).
    pub rendering: String,
    /// Byte ranges in the translation (possessive `'s` excluded).
    pub ranges: Vec<Range<usize>>,
}

/// One problem found by a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    /// Korean, Japanese or Chinese script remains in a translation.
    LeftoverScript {
        /// Pair index.
        pair: usize,
        /// Where, in the translation.
        run: ScriptRun,
    },
    /// A Forbidden Rendering is present in a Pair whose source names the Term.
    ForbiddenRendering {
        /// Pair index.
        pair: usize,
        /// What was found.
        hit: ForbiddenHit,
    },
    /// Chapter level: the source names the Term at least
    /// [`MISSING_NAME_MIN_COUNT`] times and the translation never renders it.
    MissingName {
        /// Term index.
        term_index: usize,
        /// How often the source names it.
        source_count: usize,
    },
    /// A long Pair whose translation length is implausible.
    LengthRatio {
        /// Pair index.
        pair: usize,
        /// Characters in the source.
        source_chars: usize,
        /// Characters in the translation.
        translation_chars: usize,
        /// What is wrong with it.
        problem: LengthProblem,
    },
}

/// The source must name a Term at least this many times (in the Chapter)
/// before an absent rendering is reported. Below that, a pronoun is normal.
pub const MISSING_NAME_MIN_COUNT: usize = 3;

/// Run every check on a Chapter's Pairs.
///
/// Findings are ordered: per Pair in order (leftover script, Forbidden
/// Renderings, length), then the Chapter-level missing names.
pub fn check_chapter(
    pairs: &[PairText],
    terms: &[CheckTerm],
    options: &CheckOptions,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let allowed: Vec<&str> = options.allowed_script.iter().map(String::as_str).collect();
    for (i, pair) in pairs.iter().enumerate() {
        for run in leftover_script(pair.translation, &allowed) {
            out.push(Finding::LeftoverScript { pair: i, run });
        }
        for hit in forbidden_in_pair(pair, terms) {
            out.push(Finding::ForbiddenRendering { pair: i, hit });
        }
        if let Some(f) = length_ratio(i, pair, &options.length) {
            out.push(f);
        }
    }
    out.extend(missing_names(pairs, terms));
    out
}

/// Runs of Hangul, kana or Han characters in `text`.
///
/// A run is a maximal sequence of characters of one script; a space or a
/// change of script ends it. Runs that fall entirely inside an occurrence of
/// one of the `allowed` substrings are skipped.
///
/// ```
/// use cipher_text::checks::leftover_script;
/// let runs = leftover_script("He said 안녕 twice.", &[]);
/// assert_eq!(runs.len(), 1);
/// assert_eq!(runs[0].range, 8..14);
/// ```
pub fn leftover_script(text: &str, allowed: &[&str]) -> Vec<ScriptRun> {
    let mut allowed_ranges: Vec<Range<usize>> = Vec::new();
    for a in allowed.iter().filter(|a| !a.is_empty()) {
        allowed_ranges.extend(text.match_indices(a).map(|(s, m)| s..s + m.len()));
    }
    let mut runs: Vec<ScriptRun> = Vec::new();
    let mut current: Option<(usize, Script)> = None;
    let close = |runs: &mut Vec<ScriptRun>, start: usize, end: usize, script: Script| {
        if !allowed_ranges
            .iter()
            .any(|a| a.start <= start && end <= a.end)
        {
            runs.push(ScriptRun {
                range: start..end,
                script,
            });
        }
    };
    for (i, c) in text.char_indices() {
        let s = script_of(c);
        match (current, s) {
            (Some((_, cur)), Some(s)) if cur == s => {}
            (Some((start, cur)), _) => {
                close(&mut runs, start, i, cur);
                current = s.map(|s| (i, s));
            }
            (None, Some(s)) => current = Some((i, s)),
            (None, None) => {}
        }
    }
    if let Some((start, cur)) = current {
        close(&mut runs, start, text.len(), cur);
    }
    runs
}

/// Forbidden Renderings present in `pair`, for Terms its source names.
///
/// This looks for something wrong being *present*, never for the right
/// rendering being absent: Korean repeats names where English says "he".
///
/// An occurrence that lies inside a legitimate rendering of a Term the source
/// names (the Term's own, or another's) is ignored: forbidding `Argent` does
/// not flag the `Argent` in `Tesselid Argent` when that is the approved
/// rendering of another named Term. A forbidden string equal to the Term's
/// own rendering (ignoring case) is ignored. Empty strings are ignored.
pub fn forbidden_in_pair(pair: &PairText, terms: &[CheckTerm]) -> Vec<ForbiddenHit> {
    if terms.iter().all(|t| t.forbidden.is_empty()) {
        return Vec::new();
    }
    let sources: Vec<&str> = terms.iter().map(|t| t.source.as_str()).collect();
    let counts = count_matches(pair.source, &sources);
    let named: Vec<usize> = (0..terms.len()).filter(|&i| counts[i] > 0).collect();
    if named.is_empty() {
        return Vec::new();
    }
    let legit: Vec<Range<usize>> = named
        .iter()
        .flat_map(|&i| find_rendering(pair.translation, &terms[i].rendering))
        .collect();
    let mut out = Vec::new();
    for &i in &named {
        let term = &terms[i];
        for forbidden in &term.forbidden {
            if forbidden.is_empty() || forbidden.to_lowercase() == term.rendering.to_lowercase() {
                continue;
            }
            let ranges: Vec<Range<usize>> = find_rendering(pair.translation, forbidden)
                .into_iter()
                .filter(|r| !legit.iter().any(|l| l.start <= r.start && r.end <= l.end))
                .collect();
            if !ranges.is_empty() {
                out.push(ForbiddenHit {
                    term_index: i,
                    rendering: forbidden.clone(),
                    ranges,
                });
            }
        }
    }
    out
}

/// How often each Term's source is named across all `pairs`, by Term index.
pub fn source_counts(pairs: &[PairText], terms: &[CheckTerm]) -> Vec<usize> {
    let sources: Vec<&str> = terms.iter().map(|t| t.source.as_str()).collect();
    let mut totals = vec![0; terms.len()];
    for pair in pairs {
        for (t, n) in totals.iter_mut().zip(count_matches(pair.source, &sources)) {
            *t += n;
        }
    }
    totals
}

/// Terms the source names at least [`MISSING_NAME_MIN_COUNT`] times but no
/// translation renders. Returns `(term_index, source_count)` ascending by Term.
///
/// "Renders" means [`find_rendering`] finds the Term's rendering in any Pair's
/// translation. Skipped: Terms with an empty rendering, Terms marked `may_omit`, and Terms
/// whose source is one character (명, 문, 門): as words they are too often something else (명
/// also counts people), so their count says nothing about the name.
pub fn never_rendered(pairs: &[PairText], terms: &[CheckTerm]) -> Vec<(usize, usize)> {
    let counts = source_counts(pairs, terms);
    let mut out = Vec::new();
    for (i, term) in terms.iter().enumerate() {
        if term.rendering.is_empty()
            || term.may_omit
            || term.source.trim().chars().count() <= 1
            || counts[i] < MISSING_NAME_MIN_COUNT
        {
            continue;
        }
        let rendered = pairs
            .iter()
            .any(|p| !find_rendering(p.translation, &term.rendering).is_empty());
        if !rendered {
            out.push((i, counts[i]));
        }
    }
    out
}

/// The Chapter-level missing-name check as [`Finding`]s. See [`never_rendered`].
pub fn missing_names(pairs: &[PairText], terms: &[CheckTerm]) -> Vec<Finding> {
    never_rendered(pairs, terms)
        .into_iter()
        .map(|(term_index, source_count)| Finding::MissingName {
            term_index,
            source_count,
        })
        .collect()
}

/// Flag a Pair whose translation length is implausible for its source.
///
/// Returns `None` for short sources (see [`LengthLimits`]), except that an
/// empty translation of a non-empty source is always flagged. `pair_index` is
/// carried into the finding.
pub fn length_ratio(pair_index: usize, pair: &PairText, limits: &LengthLimits) -> Option<Finding> {
    let source_chars = pair.source.trim().chars().count();
    let translation_chars = pair.translation.trim().chars().count();
    let problem = if source_chars == 0 {
        return None;
    } else if translation_chars == 0 {
        LengthProblem::Empty
    } else if source_chars <= limits.min_source_chars {
        return None;
    } else {
        let ratio = translation_chars as f64 / source_chars as f64;
        if ratio < limits.min_ratio {
            LengthProblem::TooShort
        } else if ratio > limits.max_ratio {
            LengthProblem::TooLong
        } else {
            return None;
        }
    };
    Some(Finding::LengthRatio {
        pair: pair_index,
        source_chars,
        translation_chars,
        problem,
    })
}

/// Group findings by Pair index (Chapter-level findings are not included).
pub fn by_pair(findings: &[Finding]) -> BTreeMap<usize, Vec<&Finding>> {
    let mut map: BTreeMap<usize, Vec<&Finding>> = BTreeMap::new();
    for f in findings {
        let pair = match f {
            Finding::LeftoverScript { pair, .. }
            | Finding::ForbiddenRendering { pair, .. }
            | Finding::LengthRatio { pair, .. } => *pair,
            Finding::MissingName { .. } => continue,
        };
        map.entry(pair).or_default().push(f);
    }
    map
}
