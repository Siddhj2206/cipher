use cipher_book::{Change, RevisionId, TermId, TermMerge, TermStatus};
use rune::TextEdit;

use crate::state::State;

pub struct TermEditor {
    pub source: TextEdit,
    pub rendering: TextEdit,
    pub note: TextEdit,
}

pub struct TermMergeEditor {
    pub anchor: TermId,
    pub search: TextEdit,
    pub preview: Option<TermMerge>,
}

impl State {
    pub fn start_term_merge(&mut self, index: usize) {
        self.term_editor = None;
        self.term_merge = Some(TermMergeEditor {
            anchor: self.lib.terms[index].id,
            search: TextEdit::new(""),
            preview: None,
        });
    }

    pub fn preview_term_merge(&mut self, partner: TermId) {
        let Some(editor) = &mut self.term_merge else {
            return;
        };
        match self.lib.book().preview_term_merge(editor.anchor, partner) {
            Ok(preview) => editor.preview = Some(preview),
            Err(e) => self.toast = Some(e.to_string()),
        }
    }

    pub fn confirm_term_merge(&mut self) -> bool {
        let Some(preview) = self.term_merge.as_mut().and_then(|e| e.preview.take()) else {
            return false;
        };
        let id = preview.survivor.id;
        let card = self.term_card_id().map(|card| {
            if card == preview.duplicate.id {
                id
            } else {
                card
            }
        });
        let label = format!("Merged into {}", preview.survivor.fields.rendering);
        let result = self
            .lib
            .book_mut()
            .apply(Change::MergeTerms(Box::new(preview)))
            .and_then(|applied| {
                self.lib.reload()?;
                Ok(Some((applied.revision, label)))
            });
        let merged = result.is_ok();
        self.finish_term(result, Some(id), card);
        if merged {
            self.term_merge = None;
            self.cast_pending = false;
            self.cast_drifting = false;
            if let Some(t) = self.cast_open.and_then(|i| self.lib.terms.get(i))
                && !t.matches_query(&self.cast_search.text.trim().to_lowercase())
            {
                self.cast_search = TextEdit::new("");
            }
        }
        merged
    }

    pub fn edit_cast_term(&mut self, index: usize) {
        let t = &self.lib.terms[index];
        self.term_editor = Some(TermEditor {
            source: TextEdit::new(&t.source),
            rendering: TextEdit::new(&t.rendering),
            note: TextEdit::new(t.note.as_deref().unwrap_or("")),
        });
    }

    fn term_card_id(&self) -> Option<TermId> {
        self.term_card
            .and_then(|card| self.lib.terms.get(card.term).map(|t| t.id))
    }

    fn restore_cast_term(&mut self, id: Option<TermId>, card: Option<TermId>) {
        if let Err(e) = self.lib.keep_pages(&self.strip) {
            self.fail(e);
        }
        self.term_card = self.term_card.and_then(|mut selected| {
            selected.term = self.lib.terms.iter().position(|t| Some(t.id) == card)?;
            Some(selected)
        });
        self.cast_open = id.and_then(|id| self.lib.terms.iter().position(|t| t.id == id));
        self.look = None;
        if let Some(index) = self.visible_term() {
            self.look_at(index);
        }
    }

    pub fn decide_term(&mut self, index: usize, accept: bool) {
        let card = self.term_card_id();
        let id = self.lib.terms[index].id;
        let result = self.lib.set_term_status(
            index,
            if accept {
                TermStatus::Accepted
            } else {
                TermStatus::Dismissed
            },
        );
        self.finish_term(result.map(Some), Some(id), card);
    }

    pub fn save_term(&mut self, index: usize) {
        let card = self.term_card_id();
        let Some(e) = &self.term_editor else { return };
        let id = self.lib.terms[index].id;
        let result = self
            .lib
            .edit_term(index, &e.source.text, &e.rendering.text, &e.note.text);
        self.finish_term(result, Some(id), card);
    }

    fn finish_term(
        &mut self,
        result: cipher_book::Result<Option<(RevisionId, String)>>,
        id: Option<TermId>,
        card: Option<TermId>,
    ) {
        match result {
            Ok(change) => {
                self.term_editor = None;
                self.restore_cast_term(id, card);
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
        let card = self.term_card_id();
        let id = self.cast_open.map(|i| self.lib.terms[i].id);
        match self.lib.undo_term(revision) {
            Ok(()) => {
                self.restore_cast_term(id, card);
                self.toast = Some("Term change undone".into());
            }
            Err(e) => self.toast = Some(e.to_string()),
        }
        self.term_editor = None;
        self.term_merge = None;
        self.term_toast_undo = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        library::Library,
        state::{Screen, TermCard},
    };
    use cipher_book::{Book, BookMeta, Change, TermFields};

    fn state() -> (tempfile::TempDir, State) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("terms.cipher");
        let mut book = Book::create(
            &path,
            BookMeta {
                title: "Test".into(),
                source_lang: "ko".into(),
                target_lang: "en".into(),
            },
        )
        .unwrap();
        for source in ["마탑주", "B", "C"] {
            book.apply(Change::UpsertTerm {
                id: None,
                fields: TermFields {
                    source: source.into(),
                    rendering: source.into(),
                    ..Default::default()
                },
                status: TermStatus::Pending,
                first_chapter: None,
            })
            .unwrap();
        }
        drop(book);
        (dir, State::new(Library::open(&path).unwrap()))
    }

    #[test]
    fn dismissal_preserves_card_identity_or_clears_removed_term() {
        let (_dir, mut s) = state();
        let id = s.lib.terms[2].id;
        s.term_card = Some(TermCard {
            block: cipher_book::BlockId(1),
            term: 2,
        });
        s.decide_term(0, false);
        assert_eq!(s.lib.terms[s.term_card.unwrap().term].id, id);
        s.undo_cast_term(s.term_toast_undo.unwrap());
        assert_eq!(s.lib.terms[s.term_card.unwrap().term].id, id);
        s.decide_term(s.term_card.unwrap().term, false);
        assert!(s.term_card.is_none());
    }

    #[test]
    fn term_reload_restores_neighboring_pages() {
        let (_dir, mut s) = state();
        for number in 1..=3 {
            s.lib
                .book_mut()
                .apply(Change::AddChapter {
                    number: Some(number),
                    title: None,
                    source: vec!["A".into()],
                })
                .unwrap();
        }
        s.lib.reload().unwrap();
        s.open_chapter(1);
        s.pending_scroll = None;
        s.decide_term(0, true);
        assert_eq!(s.strip, vec![0, 1, 2]);
        assert!(s.strip.iter().all(|&i| s.lib.page_at(i).is_some()));
        assert!(s.pending_scroll.is_none());
        s.undo_cast_term(s.term_toast_undo.unwrap());
        assert!(s.strip.iter().all(|&i| s.lib.page_at(i).is_some()));
        assert!(s.pending_scroll.is_none());
        s.pending_scroll = Some(("row42".into(), 160.0));
        s.decide_term(0, true);
        assert_eq!(s.pending_scroll, Some(("row42".into(), 160.0)));
    }

    #[test]
    fn cast_merge_updates_drift_without_editing_prose_and_history_undo_reverts_it() {
        let (_dir, mut s) = state();
        let chapter = cipher_book::ChapterId(
            s.lib
                .book_mut()
                .apply(Change::AddChapter {
                    number: Some(1),
                    title: None,
                    source: vec!["마탑주가 왔다.".into()],
                })
                .unwrap()
                .created[0],
        );
        let source = s.lib.book().chapter(chapter).unwrap().source[0].id;
        s.lib
            .book_mut()
            .apply(Change::SetTranslation {
                chapter,
                pairs: vec![cipher_book::NewPair {
                    source: vec![source],
                    translation: vec!["마탑주 arrived.".into()],
                    flagged: None,
                }],
            })
            .unwrap();
        s.lib.reload().unwrap();
        s.lib.apply_scan(s.lib.scan()().unwrap());
        assert_eq!(s.lib.drifting_terms(), 0);
        let before = s.lib.book().chapter(chapter).unwrap();
        let kept = s.lib.terms[1].id;
        let removed = s.lib.terms[0].id;
        s.screen = Screen::Cast;
        s.cast_open = Some(1);
        s.start_term_merge(1);
        s.preview_term_merge(removed);
        assert_eq!(s.lib.terms.len(), 3, "preview must not apply changes");
        assert!(s.confirm_term_merge());
        let index = s.cast_open.unwrap();
        assert_eq!(s.lib.terms[index].id, kept);
        assert_eq!(s.lib.terms[index].aliases, ["마탑주"]);
        s.lib.apply_scan(s.lib.scan()().unwrap());
        assert_eq!(s.lib.usage(index).drift_chapters, [0]);
        assert_eq!(
            s.lib.book().chapter(chapter).unwrap().translation,
            before.translation
        );
        assert_eq!(s.lib.book().chapter(chapter).unwrap().source, before.source);
        s.open_history();
        assert_eq!(
            s.history.revisions[0].kind,
            cipher_book::RevisionKind::TermMerge
        );
        s.undo_history();
        assert_eq!(s.lib.terms.len(), 3);
        assert!(s.lib.terms.iter().any(|t| t.id == removed));
        s.lib.apply_scan(s.lib.scan()().unwrap());
        assert_eq!(s.lib.drifting_terms(), 0);
    }

    #[test]
    fn scanned_usage_retries_evidence_for_the_open_term() {
        let (_dir, mut s) = state();
        let applied = s
            .lib
            .book_mut()
            .apply(Change::AddChapter {
                number: Some(1),
                title: None,
                source: vec!["마탑주, 마탑주, 마탑주, 마탑주.".into()],
            })
            .unwrap();
        let chapter = cipher_book::ChapterId(applied.created[0]);
        let source = s.lib.book().chapter(chapter).unwrap().source[0].id;
        s.lib
            .book_mut()
            .apply(Change::SetTranslation {
                chapter,
                pairs: vec![cipher_book::NewPair {
                    source: vec![source],
                    translation: vec!["He.".into()],
                    flagged: None,
                }],
            })
            .unwrap();
        s.lib.reload().unwrap();
        s.screen = Screen::Cast;
        s.cast_open = Some(0);
        s.decide_term(0, true);
        assert!(s.lib.terms[0].forbidden.is_empty());
        assert!(s.look.is_none());
        s.lib.apply_scan(s.lib.scan()().unwrap());
        s.start_scan();
        assert_eq!(s.look.as_ref().unwrap().term, s.lib.terms[0].id);
        assert!(s.look.as_ref().unwrap().evidence.is_none());
        s.start_scan();
        assert_eq!(s.look.as_ref().unwrap().generation, s.lib.generation());
    }

    #[test]
    fn evidence_retry_tracks_the_visible_surface() {
        let (_dir, mut s) = state();
        for t in &mut s.lib.terms {
            t.forbidden.push("old name".into());
        }
        s.cast_open = Some(0);
        s.term_card = Some(TermCard {
            block: cipher_book::BlockId(1),
            term: 1,
        });
        s.screen = Screen::Cast;
        s.start_scan();
        assert_eq!(s.look.as_ref().unwrap().term, s.lib.terms[0].id);
        s.open_chapter(0);
        s.term_card = Some(TermCard {
            block: cipher_book::BlockId(1),
            term: 1,
        });
        s.look_at(1);
        s.start_scan();
        assert_eq!(s.look.as_ref().unwrap().term, s.lib.terms[1].id);
        s.term_card = None;
        s.look = None;
        s.start_scan();
        assert!(s.look.is_none());
        s.screen = Screen::Cast;
        s.cast_open = None;
        s.term_card = Some(TermCard {
            block: cipher_book::BlockId(1),
            term: 1,
        });
        s.start_scan();
        assert!(s.look.is_none());
    }
}
