use cipher_book::{RevisionId, RevisionSummary, UndoOutcome, UndoPreview};
use rune::{Key, KeyEvent, Tx};

use crate::state::{Screen, State};

#[derive(Default)]
pub struct History {
    pub revisions: Vec<RevisionSummary>,
    pub selected: usize,
    pub preview: Option<UndoPreview>,
    pub preview_error: Option<String>,
}

impl State {
    pub fn open_history(&mut self) {
        if (self.screen == Screen::Draft && !self.lib.chapters.is_empty())
            || self.fix.review.is_some()
        {
            return;
        }
        self.palette = None;
        self.screen = Screen::History;
        self.refresh_history(None);
    }

    fn refresh_history(&mut self, selected: Option<RevisionId>) {
        match self.lib.revisions() {
            Ok(revisions) => {
                self.history.selected = selected
                    .and_then(|id| revisions.iter().position(|r| r.id == id))
                    .unwrap_or(0);
                self.history.revisions = revisions;
                self.select_history(self.history.selected);
            }
            Err(e) => self.toast = Some(format!("Could not read history: {e}")),
        }
    }

    pub fn select_history(&mut self, index: usize) {
        self.history.selected = index;
        self.history.preview = None;
        self.history.preview_error = None;
        if let Some(r) = self.history.revisions.get(index) {
            match self.lib.preview_undo(r.id) {
                Ok(preview) => self.history.preview = Some(preview),
                Err(e) => {
                    self.history.preview_error = Some(format!(
                        "This change can't be undone here. It may need a newer version of Cipher. Details: {e}"
                    ));
                }
            }
        }
    }

    pub fn undo_history(&mut self) {
        let Some(p) = &self.history.preview else {
            self.toast = self.history.preview_error.clone();
            return;
        };
        if p.already_undone_by.is_some() {
            self.toast = Some("Already undone. Select its Undo revision to redo.".into());
            return;
        }
        if crate::library::undo_mode(&p.revision) == cipher_book::UndoMode::Whole
            && !p.conflicts.is_empty()
        {
            self.toast = Some(if p.conflicts.iter().any(|c| c.entity == "job") {
                "Not undone: this chapter has started work, saved progress or dependent jobs. Finish that work first.".into()
            } else {
                "Not undone: later work changed these chapters. Structural changes must be undone as a whole.".into()
            });
            return;
        }
        let id = p.revision.id;
        let style_changed = p.changes.iter().any(|(entity, _)| *entity == "style_rule");
        match self.lib.undo_revision(id) {
            Ok(outcome) => {
                self.toast = Some(match outcome {
                    UndoOutcome::Undone { skipped, .. } => {
                        self.fix.finished.retain(|f| !f.revisions.contains(&id));
                        if self
                            .toast_undo
                            .is_some_and(|s| !self.fix.finished.iter().any(|f| f.suggestion == s))
                        {
                            self.toast_undo = None;
                        }
                        self.fix.picked = None;
                        self.term_card = None;
                        self.load_strip(self.lib.current());
                        if style_changed {
                            if self
                                .style
                                .as_ref()
                                .is_some_and(crate::style::StyleEdit::changed)
                            {
                                if let Ok(saved) = cipher_engine::style_text(self.lib.book()) {
                                    self.style.as_mut().unwrap().saved = saved;
                                }
                            } else {
                                self.style = None;
                            }
                        }
                        if skipped.is_empty() {
                            "Undone".into()
                        } else {
                            format!(
                                "Undone, except {} changed since",
                                change_words(skipped.iter().map(|c| c.entity))
                            )
                        }
                    }
                    UndoOutcome::Conflict { conflicts } => format!(
                        "Not undone: {} changed since. See affected chapters below.",
                        change_words(conflicts.iter().map(|c| c.entity))
                    ),
                    UndoOutcome::AlreadyUndone { .. } => "Already undone".into(),
                });
                self.refresh_history(Some(id));
            }
            Err(e) => self.toast = Some(format!("Could not undo: {e}")),
        }
    }
}

pub fn change_words<'a>(entities: impl Iterator<Item = &'a str>) -> String {
    let mut counts = std::collections::BTreeMap::new();
    for entity in entities {
        *counts.entry(entity).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .map(|(e, n)| entity_words(e, n))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn entity_words(entity: &str, count: usize) -> String {
    let word = match entity {
        "block" => "paragraph",
        "pair" => "alignment",
        "forbidden_rendering" => "forbidden rendering",
        "style_rule" => "style rule",
        "form_of_address" => "form of address",
        "story_note" => "story note",
        "proposed_edit" => "proposed edit",
        "proposed_metadata" => "proposed Book change",
        other => other,
    };
    crate::view::plural(count, word)
}

pub fn on_key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> bool {
    match k.key {
        Key::Enter => s.undo_history(),
        Key::Escape => {
            s.screen = Screen::Reading;
            if s.lib.chapters.is_empty() {
                s.open_draft("");
            }
        }
        Key::Up | Key::Down if !s.history.revisions.is_empty() => {
            let at = s.history.selected;
            let next = if k.key == Key::Up {
                at.saturating_sub(1)
            } else {
                (at + 1).min(s.history.revisions.len() - 1)
            };
            s.select_history(next);
            tx.scroll_key_to(format!("history-row-{next}"), 80.0);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_final_chapter_renders_empty_book_and_history_undo_restores_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quiet.cipher");
        cipher_engine::import(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/the-quiet-tower"),
            &path,
        )
        .unwrap();
        let state = State::new(crate::library::Library::open(&path).unwrap());
        let mut runtime = rune::Runtime::new(
            state,
            crate::view::view,
            (1280.0, 900.0),
            rune::Env::new(false, 1.0),
        );
        runtime.on_key = Some(crate::state::on_key);
        while !runtime.state.lib.chapters.is_empty() {
            runtime
                .state
                .delete_chapter(runtime.state.lib.chapters.len() - 1);
            runtime.invalidate();
            runtime.frame(1.0 / 60.0);
        }
        assert_eq!(runtime.state.screen, Screen::Draft);
        assert!(runtime.state.draft.as_ref().unwrap().text.text.is_empty());
        assert!(runtime.state.strip.is_empty());
        assert!(
            runtime
                .elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("draft-text"))
        );
        // Reading can also be reached from History or Contents in an empty Book.
        runtime.state.screen = Screen::Reading;
        runtime.invalidate();
        runtime.frame(1.0 / 60.0);
        runtime.state.screen = Screen::Draft;
        runtime.key(KeyEvent {
            key: Key::Command('h'),
            mods: Default::default(),
        });
        runtime.invalidate();
        runtime.frame(1.0 / 60.0);
        assert_eq!(runtime.state.screen, Screen::History);
        runtime.state.undo_history();
        runtime.invalidate();
        runtime.frame(1.0 / 60.0);
        assert_eq!(runtime.state.lib.chapters.len(), 1);
        assert_eq!(runtime.state.strip, vec![0]);
        runtime.state.screen = Screen::Reading;
        runtime.invalidate();
        runtime.frame(1.0 / 60.0);
        assert!(!runtime.state.lib.page().translation.is_empty());
    }

    #[test]
    fn enter_undoes_the_selected_revision_and_explains_disabled_actions() {
        for blocked in [false, true] {
            let mut lib = crate::library::Library::blank();
            if blocked {
                let intake = lib.intake("# Chapter 1\n\nSource").unwrap();
                lib.add_chapter(&intake).unwrap();
                lib.book_mut().claim_next_job().unwrap().unwrap();
            } else {
                lib.book_mut()
                    .apply(cipher_book::Change::AddStoryNote {
                        chapter: None,
                        text: "Undo me".into(),
                    })
                    .unwrap();
            }
            let mut s = State::new(lib);
            s.open_history();
            let revisions = s.lib.revisions().unwrap().len();
            let mut runtime = rune::Runtime::new(
                s,
                |_, _| rune::col(),
                (800.0, 600.0),
                rune::Env::new(false, 1.0),
            );
            runtime.on_key = Some(crate::state::on_key);
            runtime.key(KeyEvent {
                key: Key::Enter,
                mods: Default::default(),
            });
            runtime.frame(1.0 / 60.0);
            if blocked {
                assert_eq!(runtime.state.lib.revisions().unwrap().len(), revisions);
                assert!(
                    runtime
                        .state
                        .toast
                        .as_ref()
                        .unwrap()
                        .contains("started work")
                );
            } else {
                assert_eq!(runtime.state.lib.revisions().unwrap().len(), revisions + 1);
                assert!(runtime.state.lib.book().story_notes().unwrap().is_empty());
                runtime.key(KeyEvent {
                    key: Key::Enter,
                    mods: Default::default(),
                });
                runtime.frame(1.0 / 60.0);
                assert_eq!(runtime.state.lib.revisions().unwrap().len(), revisions + 1);
                assert!(
                    runtime
                        .state
                        .toast
                        .as_ref()
                        .unwrap()
                        .contains("Already undone")
                );
            }
        }
    }

    #[test]
    fn history_undo_reloads_neighbor_pages_in_the_reading_strip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quiet.cipher");
        cipher_engine::import(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/the-quiet-tower"),
            &path,
        )
        .unwrap();
        let mut s = State::new(crate::library::Library::open(&path).unwrap());
        assert!(s.lib.page_at(1).is_some());
        let block = s
            .lib
            .book()
            .chapter(s.lib.chapters[0].id)
            .unwrap()
            .translation[0]
            .id;
        s.lib
            .book_mut()
            .apply(cipher_book::Change::EditBlock {
                block,
                text: "Edited".into(),
                hand: true,
            })
            .unwrap();
        s.open_history();
        s.undo_history();
        assert_eq!(s.strip, vec![0, 1]);
        assert!(s.lib.page_at(1).is_some());
    }

    #[test]
    fn history_undo_preserves_unsaved_style_and_only_invalidates_changed_rules() {
        for touched_style in [false, true] {
            for unsaved in [false, true] {
                let mut s = State::new(crate::library::Library::blank());
                s.open_style();
                if unsaved {
                    s.style.as_mut().unwrap().text.text = "My unsaved draft".into();
                }
                if touched_style {
                    cipher_engine::set_style(s.lib.book_mut(), "New saved rules").unwrap();
                } else {
                    s.lib
                        .book_mut()
                        .apply(cipher_book::Change::AddStoryNote {
                            chapter: None,
                            text: "Unrelated note".into(),
                        })
                        .unwrap();
                }
                s.open_history();
                s.undo_history();
                if unsaved {
                    assert_eq!(s.style.as_ref().unwrap().text.text, "My unsaved draft");
                } else {
                    assert_eq!(s.style.is_none(), touched_style);
                }
            }
        }
    }

    #[test]
    fn unreadable_preview_keeps_the_row_and_clears_stale_preview() {
        let mut s = State::new(crate::library::Library::blank());
        s.history.revisions.push(RevisionSummary {
            id: RevisionId(i64::MAX),
            kind: cipher_book::RevisionKind::Review,
            label: "Unsupported change".into(),
            created_at: 0,
            undone_by: None,
        });
        s.select_history(0);
        assert_eq!(s.history.revisions.len(), 1);
        assert!(s.history.preview.is_none());
        assert!(
            s.history
                .preview_error
                .as_ref()
                .unwrap()
                .contains("can't be undone here")
        );
        s.undo_history();
        s.select_history(1);
        assert!(s.history.preview_error.is_none());
    }
}
