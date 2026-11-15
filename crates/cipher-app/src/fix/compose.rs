//! The selection and the fix box: what `docs/design/desktop-ux.md` calls steps 1–4.

use cipher_book::BlockId;
use cipher_engine::{Reach, Selection, SuggestionRequest};

use super::Picked;
use crate::state::{State, Tab};

impl State {
    fn selection_of(&self, block: BlockId, text: &str) -> Selection {
        Selection {
            chapter: self.lib.chapters[self.lib.current()].id,
            block,
            text: text.to_string(),
        }
    }

    /// The user selected `text` in a translated Block. A short selection starts as a Rename.
    pub fn select(&mut self, block: BlockId, text: String) {
        if self
            .fix
            .picked
            .as_ref()
            .is_some_and(|p| p.block == block && p.text == text)
        {
            return;
        }
        self.fix.rename = text.split_whitespace().count() <= 4;
        self.fix.scope = 0;
        self.fix.scope_chosen = false;
        self.fix.composing = false;
        self.fix.picked = Some(Picked {
            block,
            text,
            reach: None,
        });
        self.request_reach();
    }

    /// Works out Reach once per selection: inline when headless, else on a thread, one at a
    /// time, so dragging a selection never queues up whole-Book scans.
    fn request_reach(&mut self) {
        let Some(p) = &self.fix.picked else { return };
        let selection = self.selection_of(p.block, &p.text);
        if rune::is_headless() {
            if let Ok(reach) = self.lib.reach(&selection, cipher_book::Scope::Everywhere) {
                self.set_reach(reach);
            }
        } else if !self.fix.reach_busy {
            self.fix.reach_busy = true;
            self.bg.reach(self.lib.path(), selection);
        }
    }

    pub fn on_reach(&mut self, selection: Selection, reach: Reach) {
        self.fix.reach_busy = false;
        match &self.fix.picked {
            Some(p) if p.block == selection.block && p.text == selection.text => {
                self.set_reach(reach)
            }
            Some(_) => self.request_reach(),
            None => {}
        }
    }

    fn set_reach(&mut self, reach: Reach) {
        // A known Term starts on "Everywhere + glossary": renaming a character is one request.
        if reach.term.is_some() && !self.fix.scope_chosen {
            self.fix.scope = 2;
        }
        if let Some(p) = &mut self.fix.picked {
            p.reach = Some(reach);
        }
    }

    pub fn choose_scope(&mut self, at: usize) {
        self.fix.scope = at % super::SCOPES.len();
        self.fix.scope_chosen = true;
    }

    pub fn open_composer(&mut self, seed: &str) {
        self.fix.composing = true;
        self.fix.composer.replace_text(seed.to_string());
        self.fix.composer.focused = true;
        self.fix.composer.set_caret(seed.len());
    }

    pub fn close_fix(&mut self) {
        self.fix.composing = false;
        self.fix.picked = None;
        self.fix.composer.replace_text(String::new());
    }

    /// Sends the fix: the Suggestion becomes a note while Cipher works.
    pub fn submit(&mut self) {
        let Some(p) = self.fix.picked.clone() else {
            return;
        };
        let said = self.fix.composer.text.trim().to_string();
        if said.is_empty() {
            return;
        }
        let request = SuggestionRequest {
            selection: self.selection_of(p.block, &p.text),
            said,
            scope: self.fix.scope(),
            rename: self.fix.rename,
        };
        match self.lib.suggest(request) {
            Ok(_) => {
                self.term_card = None;
                self.close_fix();
                self.tab = Tab::Suggestions;
            }
            Err(e) => self.toast = Some(format!("Could not send the Suggestion: {e}")),
        }
    }
}
