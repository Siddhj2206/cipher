//! Cast edits are ordinary, undoable Book revisions.
use cipher_book::{Change, Error, Result, RevisionId, TermStatus, UndoMode};

use super::Library;

impl Library {
    pub fn pending_terms(&self) -> usize {
        self.terms.iter().filter(|t| t.provisional).count()
    }

    pub fn set_term_status(
        &mut self,
        index: usize,
        status: TermStatus,
    ) -> Result<(RevisionId, String)> {
        let t = self
            .terms
            .get(index)
            .ok_or_else(|| Error::Invalid("No such term".into()))?;
        if status == TermStatus::Accepted && !t.provisional {
            return Err(Error::Invalid("Term is already accepted".into()));
        }
        let label = format!(
            "{} {}",
            if status == TermStatus::Accepted {
                "Accepted"
            } else {
                "Dismissed"
            },
            t.rendering
        );
        let revision = self
            .book
            .apply(Change::SetTermStatus { term: t.id, status })?
            .revision;
        self.reload()?;
        Ok((revision, label))
    }

    pub fn edit_term(
        &mut self,
        index: usize,
        source: &str,
        rendering: &str,
        note: &str,
    ) -> Result<Option<(RevisionId, String)>> {
        let (source, rendering, note) = (source.trim(), rendering.trim(), note.trim());
        if source.is_empty() || rendering.is_empty() {
            return Err(Error::Invalid(
                "Source form and rendering must not be empty".into(),
            ));
        }
        let id = self
            .terms
            .get(index)
            .ok_or_else(|| Error::Invalid("No such term".into()))?
            .id;
        let t = self
            .book
            .glossary()?
            .terms
            .into_iter()
            .find(|t| t.id == id)
            .ok_or(Error::NotFound("term", id.0))?;
        let mut fields = t.fields.clone();
        fields.source = source.into();
        fields.rendering = rendering.into();
        fields.note = (!note.is_empty()).then(|| note.to_string());
        if fields == t.fields {
            return Ok(None);
        }
        let label = if rendering != t.fields.rendering {
            format!("Renamed {} → {}", t.fields.rendering, rendering)
        } else {
            format!("Edited {rendering}")
        };
        let revision = self
            .book
            .apply(Change::UpsertTerm {
                id: Some(id),
                fields,
                status: t.status,
                first_chapter: None,
            })?
            .revision;
        self.reload()?;
        Ok(Some((revision, label)))
    }

    pub fn undo_term(&mut self, revision: RevisionId) -> Result<()> {
        if let cipher_book::UndoOutcome::Conflict { .. } =
            self.book.undo(revision, UndoMode::Whole)?
        {
            return Err(Error::Invalid(
                "Term changed since this edit; cannot undo".into(),
            ));
        }
        self.reload()
    }
}
