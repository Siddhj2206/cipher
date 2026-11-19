use cipher_book::{
    Result, RevisionId, RevisionKind, RevisionSummary, UndoMode, UndoOutcome, UndoPreview,
};

use super::Library;

pub fn undo_mode(revision: &RevisionSummary) -> UndoMode {
    match revision.kind {
        RevisionKind::Translation
        | RevisionKind::Chapter
        | RevisionKind::Import
        | RevisionKind::Undo => UndoMode::Whole,
        RevisionKind::Review
        | RevisionKind::HandEdit
        | RevisionKind::Glossary
        | RevisionKind::Suggestion
        | RevisionKind::Notes => UndoMode::SkipConflicts,
    }
}

impl Library {
    pub fn revisions(&self) -> Result<Vec<RevisionSummary>> {
        self.book.revisions(200)
    }

    pub fn preview_undo(&mut self, id: RevisionId) -> Result<UndoPreview> {
        self.book.preview_undo(id)
    }

    pub fn undo_revision(&mut self, id: RevisionId) -> Result<UndoOutcome> {
        let preview = self.book.preview_undo(id)?;
        let index = self.current();
        let current = self.chapters.get(index).map(|c| c.id);
        let outcome = self.book.undo(id, undo_mode(&preview.revision))?;
        if matches!(outcome, UndoOutcome::Undone { .. }) {
            self.chapters = self
                .book
                .chapters()?
                .into_iter()
                .enumerate()
                .map(|(i, c)| super::ChapterRow {
                    id: c.id,
                    number: c.number.unwrap_or(i as i64 + 1),
                    state: c.state,
                    outdated: c.outdated,
                    opening: String::new(),
                    failure: None,
                    queue_place: None,
                    pending_edits: 0,
                    made_by: None,
                })
                .collect();
            self.near.clear();
            self.page = super::Page::default();
            self.stats = vec![super::ChapterStats::default(); self.chapters.len()];
            self.generation = super::GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let all: Vec<_> = (0..self.chapters.len()).collect();
            self.terms = self
                .book
                .glossary()?
                .terms
                .into_iter()
                .filter(|t| t.status != cipher_book::TermStatus::Dismissed)
                .map(super::term_info)
                .collect();
            self.check = super::stats::check_terms(&self.terms);
            self.finder = super::SourceFinder::new(&self.terms);
            self.pre = super::scan::Prefilter::new(&self.terms);
            self.refresh_chapters(&all)?;
            self.refresh_suggestions()?;
            self.scanned = true;
            if !self.chapters.is_empty() {
                let at = current
                    .and_then(|id| self.chapters.iter().position(|c| c.id == id))
                    .unwrap_or(index.min(self.chapters.len() - 1));
                self.open_chapter(at)?;
            }
        }
        Ok(outcome)
    }
}
