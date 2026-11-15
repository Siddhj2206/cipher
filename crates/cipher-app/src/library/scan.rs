//! Finding Terms in text: renderings in the Translation, source forms in the source.
//!
//! The matching rules live in `cipher_text::matching`. This module only adds what the app needs
//! on top: all Terms at once with no overlaps (so "Tesselid Argent" is not also "Argent"), and a
//! cheap per-Chapter prefilter so 190 Terms cost almost nothing on a Chapter that names 30.

use std::ops::Range;

use cipher_text::matching;

use super::TermInfo;

/// One Term occurrence in shown text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermHit {
    /// Byte range in the shown text.
    pub range: Range<usize>,
    /// Index into [`super::Library::terms`].
    pub term: usize,
}

/// The renderings that are worth scanning for in one body of text.
pub struct Candidates<'a> {
    terms: &'a [TermInfo],
    present: Vec<usize>,
}

pub fn fold(s: &str) -> String {
    s.to_lowercase().replace('’', "'")
}

impl<'a> Candidates<'a> {
    /// Candidates already found by a [`Prefilter`].
    pub fn from_present(terms: &'a [TermInfo], present: Vec<usize>) -> Self {
        Candidates { terms, present }
    }

    /// The Terms whose rendering appears somewhere in `text` (a superset of what the precise
    /// finder accepts, never a subset).
    pub fn in_text(terms: &'a [TermInfo], text: &str) -> Self {
        let hay = fold(text);
        let present = (0..terms.len())
            .filter(|&i| {
                let r = &terms[i].rendering;
                !r.is_empty() && hay.contains(&fold(r))
            })
            .collect();
        Candidates { terms, present }
    }

    /// Every rendering hit in `text`, longest first and never overlapping, sorted by position.
    pub fn renderings(&self, text: &str) -> Vec<TermHit> {
        let mut all: Vec<TermHit> = Vec::new();
        for &term in &self.present {
            for range in matching::find_rendering(text, &self.terms[term].rendering) {
                all.push(TermHit { range, term });
            }
        }
        non_overlapping(all)
    }
}

/// Which Terms a whole Chapter could name, found in one pass over its text for all Terms at
/// once. A superset of what the precise matchers accept, so the checks can run on these
/// Terms alone with the same results.
pub struct Prefilter {
    sources: Option<(aho_corasick::AhoCorasick, Vec<usize>)>,
    renderings: Option<(aho_corasick::AhoCorasick, Vec<usize>)>,
}

impl Prefilter {
    pub fn new(terms: &[TermInfo]) -> Self {
        let build = |pick: &dyn Fn(&TermInfo) -> String| {
            let (pats, ids): (Vec<String>, Vec<usize>) = terms
                .iter()
                .enumerate()
                .map(|(i, t)| (pick(t), i))
                .filter(|(p, _)| !p.trim().is_empty())
                .unzip();
            aho_corasick::AhoCorasick::new(&pats)
                .ok()
                .map(|ac| (ac, ids))
        };
        Prefilter {
            sources: build(&|t| t.source.clone()),
            renderings: build(&|t| fold(&t.rendering)),
        }
    }

    fn hits(set: &Option<(aho_corasick::AhoCorasick, Vec<usize>)>, text: &str) -> Vec<usize> {
        let Some((ac, ids)) = set else {
            return Vec::new();
        };
        let mut found: Vec<usize> = ac
            .find_overlapping_iter(text)
            .map(|m| ids[m.pattern().as_usize()])
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }

    /// Terms whose source form occurs in `text`, in Term order.
    pub fn sources_in(&self, text: &str) -> Vec<usize> {
        Self::hits(&self.sources, text)
    }

    /// Terms whose rendering occurs in `text`, ignoring case, in Term order.
    pub fn renderings_in(&self, text: &str) -> Vec<usize> {
        Self::hits(&self.renderings, &fold(text))
    }
}

/// Source-form hits in `text`, for every Term that has a source form, by the precision matcher.
pub struct SourceFinder {
    forms: Vec<String>,
    term_of: Vec<usize>,
}

impl SourceFinder {
    pub fn new(terms: &[TermInfo]) -> Self {
        let mut forms = Vec::new();
        let mut term_of = Vec::new();
        for (i, t) in terms.iter().enumerate() {
            if !t.source.trim().is_empty() {
                forms.push(t.source.clone());
                term_of.push(i);
            }
        }
        SourceFinder { forms, term_of }
    }

    pub fn find(&self, text: &str) -> Vec<TermHit> {
        let forms: Vec<&str> = self.forms.iter().map(String::as_str).collect();
        matching::find(text, &forms)
            .into_iter()
            .map(|m| TermHit {
                range: m.range,
                term: self.term_of[m.index],
            })
            .collect()
    }
}

fn non_overlapping(mut hits: Vec<TermHit>) -> Vec<TermHit> {
    hits.sort_by(|a, b| {
        (b.range.len())
            .cmp(&a.range.len())
            .then(a.range.start.cmp(&b.range.start))
            .then(a.term.cmp(&b.term))
    });
    let mut kept: Vec<TermHit> = Vec::new();
    for h in hits {
        if kept
            .iter()
            .all(|k| h.range.end <= k.range.start || h.range.start >= k.range.end)
        {
            kept.push(h);
        }
    }
    kept.sort_by_key(|h| h.range.start);
    kept
}
