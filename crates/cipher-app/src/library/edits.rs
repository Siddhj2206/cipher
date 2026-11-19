//! Suggestions and their Proposed Edits: reading them, deciding them, undoing a Review.
//!
//! Writes go through the Library's own Book handle with `apply` and `undo`; the worker has its
//! own handle on the same file.

use std::collections::BTreeSet;

use cipher_book::{
    BlockId, Change, ChapterId, EditState, ProposedEditId, Result, RevisionId, Scope, Suggestion,
    SuggestionFilter, SuggestionId, UndoMode, UndoOutcome,
};
use cipher_engine::{Reach, Selection, SuggestionRequest};

use super::Library;

/// What a Suggestion's Proposed Edits stand at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Edits with new text, waiting for a decision.
    pub pending: usize,
    /// Chapters those edits sit in.
    pub chapters: usize,
    /// Blocks the model chose to leave as they are, waiting to be dismissed.
    pub skipped: usize,
    /// Edits whose Block changed after they were proposed. Never applied.
    pub stale: usize,
}

pub fn counts(s: &Suggestion) -> Counts {
    let mut c = Counts::default();
    let mut chapters = BTreeSet::new();
    for e in &s.edits {
        match (e.state, &e.after) {
            (EditState::Pending, Some(_)) => {
                c.pending += 1;
                chapters.insert(e.chapter);
            }
            (EditState::Pending, None) => c.skipped += 1,
            (EditState::Stale, _) => c.stale += 1,
            _ => {}
        }
    }
    c.chapters = chapters.len();
    c
}

/// What one batch of decisions did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    pub revision: RevisionId,
    /// Edits written into their Blocks.
    pub applied: usize,
    /// Edits whose Block changed since they were proposed, so nothing was written.
    pub stale: usize,
    /// Chapters whose text changed, to re-read.
    pub chapters: Vec<ChapterId>,
}

impl Library {
    /// Re-reads the open Suggestions and what they mark on the page and the contents list.
    pub(super) fn refresh_suggestions(&mut self) -> Result<()> {
        self.suggestions = self.book.suggestions(SuggestionFilter {
            chapter: None,
            open_only: true,
        })?;
        self.open_suggestions = self.suggestions.len();
        self.edit_blocks.clear();
        let mut per_chapter = vec![0usize; self.chapters.len()];
        for s in &self.suggestions {
            for e in &s.edits {
                if e.state == EditState::Pending && e.after.is_some() {
                    self.edit_blocks.insert(e.block);
                    if let Some(at) = self.chapters.iter().position(|c| c.id == e.chapter) {
                        per_chapter[at] += 1;
                    }
                }
            }
        }
        for (row, n) in self.chapters.iter_mut().zip(per_chapter) {
            row.pending_edits = n;
        }
        Ok(())
    }

    pub fn suggestion(&self, id: SuggestionId) -> Option<&Suggestion> {
        self.suggestions.iter().find(|s| s.id == id)
    }

    /// Whether a pending Proposed Edit waits on this Block.
    pub fn has_edit(&self, block: BlockId) -> bool {
        self.edit_blocks.contains(&block)
    }

    pub fn chapter_index(&self, id: ChapterId) -> Option<usize> {
        self.chapters.iter().position(|c| c.id == id)
    }

    pub fn path(&self) -> &std::path::Path {
        self.book.path()
    }

    /// The Book, for reads the Library doesn't keep (the Style Rules).
    pub fn book(&self) -> &cipher_book::Book {
        &self.book
    }

    /// The Book handle for the app's own writes, such as seeding a scene.
    pub fn book_mut(&mut self) -> &mut cipher_book::Book {
        &mut self.book
    }

    /// What a Suggestion would reach. Local and free; reads the whole Book for Everywhere, so
    /// the app computes that on a thread of its own with its own Book handle.
    pub fn reach(&self, selection: &Selection, scope: Scope) -> Result<Reach> {
        cipher_engine::reach(&self.book, selection, scope).map_err(super::engine_error)
    }

    /// Records a Suggestion: renames become Proposed Edits at once, the rest become Jobs. From
    /// now on also writes the Term or Style Rule, so the Library reloads.
    pub fn suggest(&mut self, req: SuggestionRequest) -> Result<SuggestionId> {
        let glossary_changes = req.scope == Scope::FromNowOn;
        let id = cipher_engine::suggest(&mut self.book, req).map_err(super::engine_error)?;
        if glossary_changes {
            self.reload()?;
        } else {
            self.refresh_queue()?;
        }
        Ok(id)
    }

    /// How the Book names Term `term`, as a job for another thread: it reads the whole Book
    /// (`cipher_engine::evidence`).
    pub fn evidence(
        &self,
        term: usize,
    ) -> impl FnOnce() -> Result<cipher_engine::Evidence> + Send + 'static {
        let (path, id) = (self.book.path().to_path_buf(), self.terms[term].id);
        move || {
            cipher_engine::evidence(&cipher_book::Book::open(&path)?, id)
                .map_err(super::engine_error)
        }
    }

    /// Makes `winner` Term `term`'s rendering and renames the wordings in `same` to it, as one
    /// Suggestion to review (`cipher_engine::use_wording`). No Book content changes yet.
    pub fn use_wording(
        &mut self,
        term: usize,
        winner: &str,
        same: &[String],
    ) -> Result<Option<SuggestionId>> {
        let id = self.terms[term].id;
        let made = cipher_engine::use_wording(&mut self.book, id, winner, same)
            .map_err(super::engine_error)?;
        self.reload()?;
        Ok(made)
    }

    /// Sends the Pairs that name Term `term` neither way to the model
    /// (`cipher_engine::ask_model`).
    pub fn ask_model(&mut self, term: usize) -> Result<Option<SuggestionId>> {
        let id = self.terms[term].id;
        let made = cipher_engine::ask_model(&mut self.book, id).map_err(super::engine_error)?;
        self.refresh_queue()?;
        Ok(made)
    }

    /// Marks Term `term` as one the Translation may leave unnamed (or not), then counts again.
    pub fn set_may_omit(&mut self, term: usize, on: bool) -> Result<()> {
        let id = self.terms[term].id;
        let glossary = self.book.glossary()?;
        let t = glossary
            .terms
            .into_iter()
            .find(|t| t.id == id)
            .ok_or(cipher_book::Error::NotFound("term", id.0))?;
        self.book.apply(Change::UpsertTerm {
            id: Some(id),
            fields: cipher_book::TermFields {
                may_omit: on,
                ..t.fields
            },
            status: t.status,
            first_chapter: None,
        })?;
        self.reload()
    }

    /// Reads the whole Book again, staying on the Chapter that is open.
    pub fn reload(&mut self) -> Result<()> {
        let current = self.current();
        let fresh = Library::open_quick(self.book.path())?;
        *self = fresh;
        self.open_chapter(current)
    }

    /// Accepts or skips Proposed Edits in one Revision. An edit whose Block changed since is
    /// marked stale and never written over the newer text.
    pub fn decide(&mut self, decisions: Vec<(ProposedEditId, bool)>) -> Result<Decided> {
        let ids: Vec<ProposedEditId> = decisions.iter().map(|d| d.0).collect();
        let mut chapters = BTreeSet::new();
        for s in &self.suggestions {
            for e in &s.edits {
                if ids.contains(&e.id) {
                    chapters.insert(e.chapter);
                }
            }
        }
        let applied = self.book.apply(Change::DecideEdits { decisions })?;
        let after = self.book.suggestions(SuggestionFilter::default())?;
        let state_of = |id: &ProposedEditId| {
            after
                .iter()
                .flat_map(|s| &s.edits)
                .find(|e| e.id == *id)
                .map(|e| e.state)
        };
        let n_applied = ids
            .iter()
            .filter(|id| state_of(id) == Some(EditState::Accepted))
            .count();
        let n_stale = ids
            .iter()
            .filter(|id| state_of(id) == Some(EditState::Stale))
            .count();
        let chapters: Vec<ChapterId> = chapters.into_iter().collect();
        self.reread(&chapters)?;
        Ok(Decided {
            revision: applied.revision,
            applied: n_applied,
            stale: n_stale,
            chapters,
        })
    }

    pub fn decide_metadata(
        &mut self,
        decisions: Vec<(cipher_book::MetadataEditId, bool)>,
    ) -> Result<Decided> {
        let ids: Vec<_> = decisions.iter().map(|d| d.0).collect();
        let applied = self.book.apply(Change::DecideMetadata { decisions })?;
        let after = self.book.suggestions(SuggestionFilter::default())?;
        let edits: Vec<_> = after
            .iter()
            .flat_map(|s| &s.metadata)
            .filter(|e| ids.contains(&e.id))
            .collect();
        let decided = Decided {
            revision: applied.revision,
            applied: edits
                .iter()
                .filter(|e| e.state == EditState::Accepted)
                .count(),
            stale: edits.iter().filter(|e| e.state == EditState::Stale).count(),
            chapters: Vec::new(),
        };
        self.reload()?;
        Ok(decided)
    }

    /// Undoes a Review. Blocks edited since are left alone and reported as conflicts.
    pub fn undo_review(
        &mut self,
        revision: RevisionId,
        chapters: &[ChapterId],
    ) -> Result<UndoOutcome> {
        let out = self.book.undo(revision, UndoMode::SkipConflicts)?;
        self.reread(chapters)?;
        self.reload()?;
        Ok(out)
    }

    fn reread(&mut self, chapters: &[ChapterId]) -> Result<()> {
        let at: Vec<usize> = chapters
            .iter()
            .filter_map(|&c| self.chapter_index(c))
            .collect();
        self.refresh_chapters(&at)
    }
}
