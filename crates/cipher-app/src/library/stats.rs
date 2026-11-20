//! Per-Chapter numbers: how often each Term is used, and where it drifts.
//!
//! Computed once per Chapter at load and again for a single Chapter when it changes. Drift uses
//! `cipher_text::drift` over the Chapter's Pairs, so the rule (a Forbidden Rendering present, or
//! the source names the Term at least three times and the Translation never renders it) is the
//! engine's own.

use cipher_book::Chapter;
use cipher_text::blocks;
use cipher_text::checks::{CheckTerm, PairText};
use cipher_text::drift::{self, Drift};
use cipher_text::matching::SourceForms;

use super::TermInfo;
use super::scan::{Candidates, Prefilter};

#[derive(Debug, Clone, Default)]
pub struct ChapterStats {
    /// Uses of each Term's rendering in this Chapter's Translation, by Term index.
    pub counts: Vec<u32>,
    /// Terms used here, in order of first appearance.
    pub present: Vec<usize>,
    /// Drift in this Chapter. `term_index` is a Term index. Empty when not translated.
    pub drift: Vec<Drift>,
    /// For a Chapter with no Translation yet: the Terms its source names, with how often, in
    /// order of first appearance. Kept apart from `counts`, which are uses in a Translation.
    pub in_source: Vec<(usize, u32)>,
}

impl ChapterStats {
    pub fn drifts(&self, term: usize) -> bool {
        self.drift.iter().any(|d| d.term_index == term)
    }
}

pub fn check_terms(terms: &[TermInfo]) -> Vec<CheckTerm> {
    terms
        .iter()
        .map(|t| CheckTerm {
            source: t.source.clone(),
            aliases: t.aliases.clone(),
            rendering: t.rendering.clone(),
            forbidden: t.forbidden.clone(),
            may_omit: t.may_omit,
        })
        .collect()
}

/// A Block's text as read: inline markers and heading marks removed.
pub fn shown(markdown: &str) -> String {
    blocks::render(markdown).text
}

pub fn compute(
    chapter: &Chapter,
    terms: &[TermInfo],
    check: &[CheckTerm],
    pre: &Prefilter,
) -> ChapterStats {
    let mut stats = ChapterStats {
        counts: vec![0; terms.len()],
        ..ChapterStats::default()
    };
    if chapter.translation.is_empty() {
        let all = chapter
            .source
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let cand = pre.sources_in(&all);
        let forms = SourceForms::new(cand.iter().map(|&i| terms[i].source_forms()));
        let mut counts = vec![0; cand.len()];
        let mut order = Vec::new();
        for hit in forms.find(&all) {
            if counts[hit.index] == 0 {
                order.push(hit.index);
            }
            counts[hit.index] += 1;
        }
        stats.in_source = order.into_iter().map(|i| (cand[i], counts[i])).collect();
        return stats;
    }
    let texts: Vec<String> = chapter.translation.iter().map(|b| shown(&b.text)).collect();
    let all = texts.join("\n");
    let candidates = Candidates::from_present(terms, pre.renderings_in(&all));
    let mut first_seen: Vec<(usize, usize, usize)> = Vec::new();
    for (bi, text) in texts.iter().enumerate() {
        for hit in candidates.renderings(text) {
            stats.counts[hit.term] += 1;
            if !first_seen.iter().any(|f| f.2 == hit.term) {
                first_seen.push((bi, hit.range.start, hit.term));
            }
        }
    }
    first_seen.sort();
    stats.present = first_seen.into_iter().map(|f| f.2).collect();

    // Each Pair's text on both sides, grouping Blocks by Pair in one pass.
    let slot: std::collections::HashMap<_, usize> = chapter
        .pairs
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id, i))
        .collect();
    let mut sides: Vec<[Vec<String>; 2]> = vec![[Vec::new(), Vec::new()]; chapter.pairs.len()];
    for (blocks, side) in [(&chapter.source, 0), (&chapter.translation, 1)] {
        for b in blocks.iter() {
            if let Some(&i) = b.pair.as_ref().and_then(|p| slot.get(p)) {
                sides[i][side].push(shown(&b.text));
            }
        }
    }
    let pair_texts: Vec<(String, String)> = sides
        .into_iter()
        .map(|[s, t]| (s.join("\n"), t.join("\n")))
        .collect();
    let pairs: Vec<PairText> = pair_texts
        .iter()
        .map(|(s, t)| PairText {
            source: s,
            translation: t,
        })
        .collect();
    // Drift needs the source to name a Term, so only the Terms it names are checked.
    let all_source: String = pair_texts
        .iter()
        .map(|(s, _)| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let named = pre.sources_in(&all_source);
    let subset: Vec<CheckTerm> = named.iter().map(|&i| check[i].clone()).collect();
    stats.drift = drift::drift(&pairs, &subset)
        .into_iter()
        .map(|mut d| {
            d.term_index = named[d.term_index];
            d
        })
        .collect();
    stats
}
