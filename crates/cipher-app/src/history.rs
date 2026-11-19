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
        if self.screen == Screen::Draft || self.fix.review.is_some() {
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
            return;
        };
        let id = p.revision.id;
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
                        self.style = None;
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
        Key::Escape => s.screen = Screen::Reading,
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
