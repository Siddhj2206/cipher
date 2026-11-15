//! One Chapter as the page shows it: Blocks rendered to text and spans, Terms found, Pairs
//! resolved to indices. Plain owned data; the views only lay it out.

use std::ops::Range;

use cipher_book::{BlockId, Chapter, PairId, Side};
use cipher_text::blocks::{self, BlockKind, StyledSpan};
use cipher_text::drift::DriftKind;
use cipher_text::matching;

use super::TermInfo;
use super::scan::{Candidates, SourceFinder, TermHit};
use super::stats::ChapterStats;

/// What `Pair::flagged` says, in the terms the page cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairFlag {
    /// Paired by position when an existing translation was imported. True of every Pair of
    /// such a Chapter, so the page keeps it quiet.
    Imported,
    /// Repair could not fix this Pair; the reason is shown on the page.
    Problem(String),
}

/// A Forbidden Rendering that is present in the shown text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbiddenMark {
    pub range: Range<usize>,
    pub term: usize,
    pub rendering: String,
}

#[derive(Debug, Clone)]
pub struct PageBlock {
    pub id: BlockId,
    pub pair: Option<PairId>,
    pub kind: BlockKind,
    /// Display text: inline markers and heading marks removed.
    pub text: String,
    pub spans: Vec<StyledSpan>,
    pub terms: Vec<TermHit>,
    pub forbidden: Vec<ForbiddenMark>,
    pub outdated: bool,
}

/// A Pair resolved to positions in [`Page::source`] and [`Page::translation`].
#[derive(Debug, Clone)]
pub struct PairView {
    pub id: PairId,
    pub flag: Option<PairFlag>,
    pub source: Vec<usize>,
    pub translation: Vec<usize>,
    /// The source Blocks' text, joined by line breaks: what unfolding the Pair shows.
    pub source_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    /// Index in [`super::Library::chapters`].
    pub index: usize,
    pub number: i64,
    pub source: Vec<PageBlock>,
    pub translation: Vec<PageBlock>,
    pub pairs: Vec<PairView>,
}

impl Page {
    pub fn blocks(&self, side: Side) -> &[PageBlock] {
        match side {
            Side::Source => &self.source,
            Side::Translation => &self.translation,
        }
    }

    pub fn pair(&self, id: PairId) -> Option<&PairView> {
        self.pairs.iter().find(|p| p.id == id)
    }

    /// The pair that `block` belongs to on `side`.
    pub fn pair_of(&self, side: Side, block: BlockId) -> Option<PairId> {
        self.blocks(side)
            .iter()
            .find(|b| b.id == block)
            .and_then(|b| b.pair)
    }

    /// The first Block of `pair` on `side`.
    pub fn first_block(&self, side: Side, pair: PairId) -> Option<BlockId> {
        let view = self.pair(pair)?;
        let at = match side {
            Side::Source => view.source.first(),
            Side::Translation => view.translation.first(),
        }?;
        Some(self.blocks(side)[*at].id)
    }

    /// Whether `index` is the first translated Block of its Pair.
    pub fn first_of_pair(&self, index: usize) -> bool {
        self.translation[index]
            .pair
            .and_then(|p| self.pair(p))
            .is_none_or(|p| p.translation.first() == Some(&index))
    }

    /// The Chapter's `# Chapter N:` heading is replaced by the page opener, so the first
    /// translated Block is hidden when it is a level-1 heading.
    pub fn hides_first_translated(&self) -> bool {
        self.translation
            .first()
            .is_some_and(|b| b.kind == BlockKind::Heading(1))
    }

    /// Translated Blocks marked Outdated.
    pub fn outdated(&self) -> usize {
        self.translation.iter().filter(|b| b.outdated).count()
    }

    /// Whether the Translation came with an import rather than from a model.
    pub fn imported(&self) -> bool {
        self.pairs
            .first()
            .is_some_and(|p| p.flag == Some(PairFlag::Imported))
    }

    /// Pairs with a reason worth showing (imported ones are the quiet default).
    pub fn problems(&self) -> usize {
        self.pairs
            .iter()
            .filter(|p| matches!(p.flag, Some(PairFlag::Problem(_))))
            .count()
    }
}

pub struct Context<'a> {
    pub terms: &'a [TermInfo],
    pub finder: &'a SourceFinder,
    pub stats: &'a ChapterStats,
}

pub fn build(index: usize, number: i64, ch: &Chapter, cx: &Context) -> Page {
    let translated_text = ch
        .translation
        .iter()
        .map(|b| blocks::render(&b.text).text)
        .collect::<Vec<_>>()
        .join("\n");
    let candidates = Candidates::in_text(cx.terms, &translated_text);
    let drifting: Vec<(usize, &str)> = cx
        .stats
        .drift
        .iter()
        .filter_map(|d| match &d.kind {
            DriftKind::ForbiddenPresent { rendering, .. } => {
                Some((d.term_index, rendering.as_str()))
            }
            DriftKind::NeverRendered { .. } => None,
        })
        .collect();

    let make = |b: &cipher_book::Block, side: Side| {
        let r = blocks::render(&b.text);
        let terms = match side {
            Side::Source => cx.finder.find(&r.text),
            Side::Translation => candidates.renderings(&r.text),
        };
        let forbidden = match side {
            Side::Source => Vec::new(),
            Side::Translation => forbidden_marks(&r.text, &terms, &drifting),
        };
        PageBlock {
            id: b.id,
            pair: b.pair,
            kind: blocks::kind(&b.text),
            terms,
            forbidden,
            text: r.text,
            spans: r.spans,
            outdated: b.outdated,
        }
    };
    let source: Vec<PageBlock> = ch.source.iter().map(|b| make(b, Side::Source)).collect();
    let translation: Vec<PageBlock> = ch
        .translation
        .iter()
        .map(|b| make(b, Side::Translation))
        .collect();

    let positions = |blocks: &[PageBlock], pair| -> Vec<usize> {
        (0..blocks.len())
            .filter(|&i| blocks[i].pair == Some(pair))
            .collect()
    };
    let pairs = ch
        .pairs
        .iter()
        .map(|p| {
            let source_at = positions(&source, p.id);
            let source_text = source_at
                .iter()
                .map(|&i| source[i].text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            PairView {
                id: p.id,
                flag: p.flagged.as_deref().map(|reason| match reason {
                    "imported" | cipher_engine::ALIGNED => PairFlag::Imported,
                    other => PairFlag::Problem(other.to_string()),
                }),
                translation: positions(&translation, p.id),
                source: source_at,
                source_text,
            }
        })
        .collect();
    Page {
        index,
        number,
        source,
        translation,
        pairs,
    }
}

/// Where a Forbidden Rendering that the Chapter's Drift names stands in `text`, outside the
/// rendering of any Term (forbidding "Argent" does not flag the one in "Tesselid Argent").
fn forbidden_marks(
    text: &str,
    terms: &[TermHit],
    drifting: &[(usize, &str)],
) -> Vec<ForbiddenMark> {
    let mut out = Vec::new();
    for &(term, rendering) in drifting {
        for range in matching::find_rendering(text, rendering) {
            let inside_term = terms
                .iter()
                .any(|t| t.range.start <= range.start && range.end <= t.range.end);
            if !inside_term {
                out.push(ForbiddenMark {
                    range,
                    term,
                    rendering: rendering.to_string(),
                });
            }
        }
    }
    out
}
