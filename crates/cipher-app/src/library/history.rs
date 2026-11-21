use cipher_book::{
    Result, RevisionId, RevisionKind, RevisionSummary, UndoMode, UndoOutcome, UndoPreview,
};

use super::Library;

pub fn undo_mode(revision: &RevisionSummary) -> UndoMode {
    match revision.kind {
        RevisionKind::Translation
        | RevisionKind::Chapter
        | RevisionKind::Import
        | RevisionKind::TermMerge
        | RevisionKind::Undo => UndoMode::Whole,
        RevisionKind::Review
        | RevisionKind::HandEdit
        | RevisionKind::Glossary
        | RevisionKind::Suggestion
        | RevisionKind::Notes => UndoMode::SkipConflicts,
    }
}

impl Library {
    pub fn chapter_has_active_job(&self, index: usize) -> bool {
        let Some(chapter) = self.chapters.get(index) else {
            return true;
        };
        self.book.chapter_has_active_job(chapter.id).unwrap_or(true)
    }

    pub fn delete_chapter(&mut self, index: usize) -> Result<()> {
        let chapter = self
            .chapters
            .get(index)
            .ok_or_else(|| cipher_book::Error::Invalid("No chapter selected".into()))?
            .id;
        self.book
            .apply(cipher_book::Change::DeleteChapter { chapter })?;
        self.rebuild_after_history(None, index)?;
        self.refresh_suggestions()?;
        Ok(())
    }

    fn rebuild_after_history(
        &mut self,
        current: Option<cipher_book::ChapterId>,
        index: usize,
    ) -> Result<()> {
        self.chapters = self
            .book
            .chapters()?
            .into_iter()
            .enumerate()
            .map(|(i, c)| super::ChapterRow {
                id: c.id,
                title: c.title,
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
        self.scanned = self.chapters.is_empty();
        self.refresh_queue()?;
        if !self.chapters.is_empty() {
            let at = current
                .and_then(|id| self.chapters.iter().position(|c| c.id == id))
                .unwrap_or(index.min(self.chapters.len() - 1));
            self.open_chapter(at)?;
        }
        self.derive_usage();
        Ok(())
    }
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
            let rebuild = preview.changes.iter().any(|(entity, _)| {
                matches!(
                    *entity,
                    "chapter" | "term" | "term_alias" | "forbidden_rendering"
                )
            });
            if rebuild {
                self.rebuild_after_history(current, index)?;
            } else {
                let affected: Vec<_> = preview
                    .chapters
                    .iter()
                    .filter_map(|id| self.chapter_index(*id))
                    .collect();
                self.refresh_chapters(&affected)?;
                self.near.clear();
                self.open_chapter(index)?;
                // Invalidate an in-flight scan so it cannot overwrite the refreshed stats.
                self.generation =
                    super::GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            self.refresh_suggestions()?;
        }
        Ok(outcome)
    }
}
