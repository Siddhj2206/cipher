use cipher_book::{RevisionId, TermId, TermStatus};
use rune::TextEdit;

use crate::state::State;

pub struct TermEditor {
    pub source: TextEdit,
    pub rendering: TextEdit,
    pub note: TextEdit,
}

impl State {
    pub fn edit_cast_term(&mut self, index: usize) {
        let t = &self.lib.terms[index];
        self.term_editor = Some(TermEditor {
            source: TextEdit::new(&t.source),
            rendering: TextEdit::new(&t.rendering),
            note: TextEdit::new(t.note.as_deref().unwrap_or("")),
        });
    }

    fn restore_cast_term(&mut self, id: Option<TermId>) {
        self.cast_open = id.and_then(|id| self.lib.terms.iter().position(|t| t.id == id));
        self.look = None;
        if let Some(index) = self.cast_open {
            self.look_at(index);
        }
    }

    pub fn decide_term(&mut self, index: usize, accept: bool) {
        let id = self.lib.terms[index].id;
        let result = self.lib.set_term_status(
            index,
            if accept {
                TermStatus::Accepted
            } else {
                TermStatus::Dismissed
            },
        );
        self.finish_term(result.map(Some), Some(id));
    }

    pub fn save_term(&mut self, index: usize) {
        let Some(e) = &self.term_editor else { return };
        let id = self.lib.terms[index].id;
        let result = self
            .lib
            .edit_term(index, &e.source.text, &e.rendering.text, &e.note.text);
        self.finish_term(result, Some(id));
    }

    fn finish_term(
        &mut self,
        result: cipher_book::Result<Option<(RevisionId, String)>>,
        id: Option<TermId>,
    ) {
        match result {
            Ok(change) => {
                self.term_editor = None;
                self.restore_cast_term(id);
                if let Some((revision, label)) = change {
                    self.toast_undo = None;
                    self.term_toast_undo = Some(revision);
                    self.toast = Some(label);
                }
            }
            Err(e) => {
                self.toast = Some(e.to_string());
                self.toast_undo = None;
                self.term_toast_undo = None;
            }
        }
    }

    pub fn undo_cast_term(&mut self, revision: RevisionId) {
        let id = self.cast_open.map(|i| self.lib.terms[i].id);
        match self.lib.undo_term(revision) {
            Ok(()) => {
                self.restore_cast_term(id);
                self.toast = Some("Term change undone".into());
            }
            Err(e) => self.toast = Some(e.to_string()),
        }
        self.term_editor = None;
        self.term_toast_undo = None;
    }
}
