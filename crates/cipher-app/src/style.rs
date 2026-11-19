//! The Style page (`docs/design/desktop-ux.md` › Style): the Book's Style Rules as one text,
//! a rule per paragraph, sent with every Chapter's translation.

use rune::TextEdit;

use crate::state::{Screen, State};

pub struct StyleEdit {
    pub text: TextEdit,
    /// What the Book holds, to tell whether there is anything to save.
    pub saved: String,
}

impl StyleEdit {
    pub fn changed(&self) -> bool {
        self.text.text.trim() != self.saved.trim()
    }
}

impl State {
    pub fn open_style(&mut self) {
        if self.screen == Screen::Draft || self.fix.review.is_some() {
            return;
        }
        // Text not saved yet is still there.
        if self.style.as_ref().is_some_and(StyleEdit::changed) {
            self.palette = None;
            self.screen = Screen::Style;
            return;
        }
        match cipher_engine::style_text(self.lib.book()) {
            Ok(saved) => {
                let mut text = TextEdit::new(&saved);
                text.focused = true;
                self.style = Some(StyleEdit { text, saved });
                self.palette = None;
                self.screen = Screen::Style;
            }
            Err(e) => self.toast = Some(format!("Could not read the style: {e}")),
        }
    }

    /// Ctrl+S: writes the text as the Book's Style Rules, one Revision.
    pub fn save_style(&mut self) {
        let Some(edit) = &self.style else { return };
        let text = edit.text.text.clone();
        match cipher_engine::set_style(self.lib.book_mut(), &text) {
            Ok(changed) => {
                let saved = cipher_engine::style_text(self.lib.book()).unwrap_or(text);
                if let Some(e) = &mut self.style {
                    e.text = TextEdit::new(&saved);
                    e.text.focused = true;
                    e.saved = saved;
                }
                self.toast = Some(if changed {
                    "Style saved: chapters translated from now on follow it.".into()
                } else {
                    "Nothing changed.".into()
                });
            }
            Err(e) => self.toast = Some(format!("Could not save the style: {e}")),
        }
    }

    /// Puts the text back to what is saved.
    pub fn revert_style(&mut self) {
        if let Some(e) = &mut self.style {
            e.text = TextEdit::new(&e.saved);
            e.text.focused = true;
        }
    }

    /// Esc: back to reading. Unsaved text stays for the next visit.
    pub fn leave_style(&mut self) {
        self.screen = Screen::Reading;
    }
}
