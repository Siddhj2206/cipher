//! Acting on Drift (`docs/design/desktop-ux.md` › Cast & terms): fix a Term everywhere it
//! drifts, as one Suggestion to review, or mark it as one the Translation may leave unnamed.

use cipher_text::drift::DriftKind;

use crate::state::{State, Tab};

/// What a Term's Drift is made of, over the whole Book.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DriftSummary {
    /// Chapters where a Forbidden Rendering appears, and how many times in all.
    pub forbidden_chapters: usize,
    pub forbidden_uses: usize,
    /// Chapters whose source names the Term often and whose Translation never does.
    pub unnamed_chapters: usize,
}

impl State {
    pub fn drift_summary(&self, term: usize) -> DriftSummary {
        let mut out = DriftSummary::default();
        for &c in &self.lib.usage(term).drift_chapters {
            for d in self
                .lib
                .stats(c)
                .drift
                .iter()
                .filter(|d| d.term_index == term)
            {
                match d.kind {
                    DriftKind::ForbiddenPresent { count, .. } => {
                        out.forbidden_chapters += 1;
                        out.forbidden_uses += count;
                    }
                    DriftKind::NeverRendered { .. } => out.unnamed_chapters += 1,
                }
            }
        }
        out
    }

    /// The open Suggestion fixing this Term's Drift, if one was sent and isn't done.
    pub fn drift_fix(&self, term: usize) -> Option<&cipher_book::Suggestion> {
        let name = &self.lib.terms[term].rendering;
        self.lib.suggestions.iter().find(|x| {
            x.kind == cipher_book::SuggestionKind::Drift
                && x.quote == *name
                && x.state != cipher_book::SuggestionState::Done
        })
    }

    /// Opens or closes a Term's row on the Cast page.
    pub fn toggle_cast_term(&mut self, term: usize) {
        self.cast_open = if self.cast_open == Some(term) {
            None
        } else {
            Some(term)
        };
    }

    /// Sends one Suggestion that fixes the Term everywhere it drifts, and shows it under
    /// Suggestions, where its edits are reviewed.
    pub fn fix_drift(&mut self, term: usize) {
        if !self.lib.scanned() {
            self.toast = Some("Still counting Terms; try again in a moment.".into());
            return;
        }
        let name = self.lib.terms[term].rendering.clone();
        let n = self.lib.usage(term).drift_chapters.len();
        if self.drift_fix(term).is_some() {
            self.tab = Tab::Suggestions;
            self.show_context();
            return;
        }
        match self.lib.fix_drift(term) {
            Ok(Some(_)) => {
                self.tab = Tab::Suggestions;
                self.show_context();
                self.term_card = None;
                self.toast = Some(format!(
                    "Fixing \u{201c}{name}\u{201d} in {}: review the changes under Suggestions.",
                    crate::view::plural(n, "chapter")
                ));
            }
            Ok(None) => {
                self.toast = Some(format!(
                    "Nothing to change for \u{201c}{name}\u{201d}: where it drifts, the source no longer names it."
                ))
            }
            Err(e) => self.toast = Some(format!("Could not fix the drift: {e}")),
        }
    }

    /// Marks a Term as one the Translation may leave unnamed, or counts it again.
    pub fn set_may_omit(&mut self, term: usize, on: bool) {
        let name = self.lib.terms[term].rendering.clone();
        match self.lib.set_may_omit(term, on) {
            Ok(()) => {
                self.toast = Some(if on {
                    format!(
                        "\u{201c}{name}\u{201d} may go unnamed: chapters that leave it out no longer count as drift."
                    )
                } else {
                    format!("\u{201c}{name}\u{201d} is checked again in every chapter.")
                })
            }
            Err(e) => self.toast = Some(format!("Could not change the Term: {e}")),
        }
    }
}
