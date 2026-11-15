//! Drift: a Term rendered inconsistently in a Chapter.
//!
//! Drift is found locally, with the same rules as [`crate::checks`]:
//! - a Forbidden Rendering is still present in a Pair whose source names the Term;
//! - or the source names the Term at least
//!   [`MISSING_NAME_MIN_COUNT`](crate::checks::MISSING_NAME_MIN_COUNT) times
//!   and the Chapter never renders it.

use crate::checks::{CheckTerm, PairText, forbidden_in_pair, never_rendered};

/// What kind of Drift a Term shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    /// A Forbidden Rendering is still present.
    ForbiddenPresent {
        /// The Forbidden Rendering, as spelled in the Term.
        rendering: String,
        /// Total occurrences in the Chapter's Pairs that name the Term.
        count: usize,
    },
    /// Named in the source three or more times, never rendered.
    NeverRendered {
        /// How often the source names the Term in this Chapter.
        source_count: usize,
    },
}

/// Drift of one Term in one Chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    /// Index of the Term in the slice passed to [`drift`].
    pub term_index: usize,
    /// What is wrong.
    pub kind: DriftKind,
}

/// Drift per Term for one Chapter's Pairs.
///
/// Results are ordered by Term index; for one Term the
/// [`ForbiddenPresent`](DriftKind::ForbiddenPresent) entries (in the order of
/// the Term's `forbidden` list) come before
/// [`NeverRendered`](DriftKind::NeverRendered). A Term with no Drift does not
/// appear. Counts are occurrences, not Pairs.
pub fn drift(pairs: &[PairText], terms: &[CheckTerm]) -> Vec<Drift> {
    let mut forbidden: Vec<(usize, String, usize)> = Vec::new();
    for pair in pairs {
        for hit in forbidden_in_pair(pair, terms) {
            let n = hit.ranges.len();
            match forbidden
                .iter_mut()
                .find(|(t, r, _)| *t == hit.term_index && *r == hit.rendering)
            {
                Some(entry) => entry.2 += n,
                None => forbidden.push((hit.term_index, hit.rendering, n)),
            }
        }
    }
    let mut out: Vec<Drift> = Vec::new();
    for (term_index, rendering, count) in forbidden {
        out.push(Drift {
            term_index,
            kind: DriftKind::ForbiddenPresent { rendering, count },
        });
    }
    for (term_index, source_count) in never_rendered(pairs, terms) {
        out.push(Drift {
            term_index,
            kind: DriftKind::NeverRendered { source_count },
        });
    }
    let order = |d: &Drift| {
        (
            d.term_index,
            matches!(d.kind, DriftKind::NeverRendered { .. }),
        )
    };
    // Stable: keeps the forbidden-list order within a Term.
    out.sort_by_key(order);
    out
}
