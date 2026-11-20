//! Acting on Drift (`docs/design/desktop-ux.md` › Cast & terms, `translation-pipeline.md` ›
//! Drift): see what the Book calls a Term where it doesn't use its rendering, mark which of
//! those wordings are the same name, and keep one; or let it go unnamed; or, by choice, ask
//! the model about paragraphs that name it neither way.

use cipher_book::{Suggestion, SuggestionState, TermId};
use cipher_engine::Evidence;
use cipher_text::drift::DriftKind;

use crate::fix::work::Msg;
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

/// The evidence for the Term whose drift panel is open, and what the user marked.
#[derive(Debug, Clone)]
pub struct Look {
    pub term: TermId,
    /// The Library generation it was read in; a later one reads it again.
    pub generation: u64,
    /// `None` while it is being read.
    pub evidence: Option<Evidence>,
    pub error: Option<String>,
    /// Per wording: the user says it is this Term's name. Forbidden Renderings start marked.
    pub same: Vec<bool>,
    /// The wording whose examples are open; `None` for none, `Some(None)` for the rendering.
    pub examples: Option<Option<usize>>,
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

    /// An open Suggestion about this Term's name: a wording kept, or the model asked.
    pub fn drift_fix(&self, term: usize) -> Option<&Suggestion> {
        let t = &self.lib.terms[term];
        self.lib.suggestions.iter().find(|x| {
            x.state != SuggestionState::Done
                && x.source_expr
                    .as_deref()
                    .is_some_and(|source| t.source_forms().any(|s| s == source))
                && (x.kind == cipher_book::SuggestionKind::Drift || x.is_rename)
        })
    }

    /// Opens or closes a Term's row on the Cast page.
    pub fn toggle_cast_term(&mut self, term: usize) {
        self.term_editor = None;
        self.term_merge = None;
        self.cast_open = if self.cast_open == Some(term) {
            None
        } else {
            self.look_at(term);
            Some(term)
        };
    }

    /// Reads how the Book names a drifting Term, on another thread, unless it is read already
    /// for this state of the Book.
    pub fn look_at(&mut self, term: usize) {
        let Some(t) = self.lib.terms.get(term) else {
            return;
        };
        let generation = self.lib.generation();
        if self.lib.usage(term).drift_chapters.is_empty() && t.forbidden.is_empty()
            || !self.lib.is_saved()
            || self
                .look
                .as_ref()
                .is_some_and(|l| l.term == t.id && l.generation == generation)
        {
            return;
        }
        let id = t.id;
        self.look = Some(Look {
            term: id,
            generation,
            evidence: None,
            error: None,
            same: Vec::new(),
            examples: None,
        });
        let job = self.lib.evidence(term);
        self.bg
            .spawn(move || Msg::Evidence(id, generation, job().map_err(|e| e.to_string())));
    }

    pub fn on_evidence(&mut self, term: TermId, generation: u64, result: Result<Evidence, String>) {
        let Some(look) = self
            .look
            .as_mut()
            .filter(|l| l.term == term && l.generation == generation)
        else {
            return;
        };
        match result {
            Ok(ev) => {
                look.same = ev.wordings.iter().map(|w| w.forbidden).collect();
                look.evidence = Some(ev);
            }
            Err(e) => look.error = Some(e),
        }
    }

    /// The evidence read for Term `term`, if it is the one open and has arrived.
    pub fn look_for(&self, term: usize) -> Option<&Look> {
        let id = self.lib.terms.get(term)?.id;
        self.look.as_ref().filter(|l| l.term == id)
    }

    pub fn mark_same(&mut self, wording: usize, same: bool) {
        if let Some(s) = self.look.as_mut().and_then(|l| l.same.get_mut(wording)) {
            *s = same;
        }
    }

    pub fn toggle_examples(&mut self, wording: Option<usize>) {
        if let Some(l) = self.look.as_mut() {
            l.examples = if l.examples == Some(wording) {
                None
            } else {
                Some(wording)
            };
        }
    }

    /// Keeps `winner` (a wording, or the rendering for `None`) as the Term's name and renames
    /// the wordings marked as the same name to it: one Suggestion, reviewed under Suggestions.
    pub fn keep_wording(&mut self, term: usize, winner: Option<usize>) {
        let Some(look) = self.look_for(term) else {
            return;
        };
        let Some(ev) = &look.evidence else { return };
        let rendering = self.lib.terms[term].rendering.clone();
        let name = match winner {
            Some(i) => ev.wordings[i].text.clone(),
            None => rendering.clone(),
        };
        let same: Vec<String> = ev
            .wordings
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != winner && look.same.get(*i).copied().unwrap_or(false))
            .map(|(_, w)| w.text.clone())
            .collect();
        match self.lib.use_wording(term, &name, &same) {
            Ok(Some(_)) => {
                self.look = None;
                self.tab = Tab::Suggestions;
                self.show_context();
                self.term_card = None;
                self.toast = Some(format!(
                    "Review \u{201c}{name}\u{201d} under Suggestions. Nothing has changed yet."
                ));
            }
            Ok(None) => {
                self.look = None;
                self.toast = Some(format!("\u{201c}{name}\u{201d} kept; nothing to rename."));
            }
            Err(e) => self.toast = Some(format!("Could not rename: {e}")),
        }
    }

    /// The opt-in model step for the paragraphs that name the Term neither way.
    pub fn ask_model(&mut self, term: usize) {
        if self.drift_fix(term).is_some() {
            self.tab = Tab::Suggestions;
            self.show_context();
            return;
        }
        match self.lib.ask_model(term) {
            Ok(Some(_)) => {
                self.tab = Tab::Suggestions;
                self.show_context();
                self.term_card = None;
                self.toast =
                    Some("Asked the model: its changes wait under Suggestions.".to_string());
            }
            Ok(None) => self.toast = Some("No paragraph leaves it out.".to_string()),
            Err(e) => self.toast = Some(format!("Could not ask the model: {e}")),
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
