//! A new Chapter, set on its own page at the end of the Book (`docs/design/desktop-ux.md` ›
//! Adding chapters): paste or type the source onto the page, see what Cipher reads from it
//! (number, title, where it goes), and add it. Pasting anywhere in a Book opens it with the
//! text; an empty Book opens on it.

use rune::{FileDialogOptions, TextEdit, Tx};

use crate::library::Intake;
use crate::state::{Screen, State};

pub struct Draft {
    /// The Chapter's source, as pasted or typed.
    pub text: TextEdit,
    /// The Chapter that was open, to go back to when the draft is put aside.
    pub back: usize,
}

impl State {
    /// What the page reads from the draft so far.
    pub fn draft_intake(&self) -> Option<Intake> {
        self.draft
            .as_ref()
            .and_then(|d| self.lib.intake(&d.text.text))
    }

    /// The new-Chapter page, with `text` on it (appended to a draft already started).
    pub fn open_draft(&mut self, text: &str) {
        if self.home.is_some() || self.fix.review.is_some() || self.settings_page.is_some() {
            return;
        }
        self.palette = None;
        self.term_card = None;
        self.fix.picked = None;
        let back = self.lib.current();
        let draft = self.draft.get_or_insert_with(|| Draft {
            text: TextEdit::new(""),
            back,
        });
        if !text.trim().is_empty() {
            let joined = if draft.text.text.trim().is_empty() {
                text.to_string()
            } else {
                format!("{}\n\n{text}", draft.text.text.trim_end())
            };
            draft.text = TextEdit::new(&joined);
        }
        draft.text.focused = true;
        self.screen = Screen::Draft;
    }

    /// "Add chapters" and Ctrl+N: the page, filled from the clipboard if it holds a Chapter.
    pub fn new_chapter(&mut self, tx: &mut Tx) {
        self.open_draft("");
        tx.read_clipboard(|s: &mut State, content, _| {
            let text = content.text();
            // Only text that reads as a Chapter (a few paragraphs, or a numbered heading) is
            // taken; a stray word on the clipboard isn't.
            let fresh = s
                .draft
                .as_ref()
                .is_some_and(|d| d.text.text.trim().is_empty());
            if fresh
                && let Some(it) = s.lib.intake(text)
                && (it.number.is_some() || it.blocks.len() >= 3)
            {
                s.open_draft(text);
            }
        });
    }

    /// Fills the page from a text file.
    pub fn draft_from_file(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions {
            title: "Add a chapter from a text file".into(),
            ..Default::default()
        }
        .filter("Text", &["txt", "md", "markdown"]);
        tx.open_file(options, |s: &mut State, answer, _| {
            if let Ok(Some(path)) = answer {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        if let Some(d) = &mut s.draft {
                            d.text = TextEdit::new("");
                        }
                        s.open_draft(&text);
                    }
                    Err(e) => s.toast = Some(format!("Could not read {}: {e}", path.display())),
                }
            }
        });
    }

    /// Adds the draft as a Chapter in number order, queues it and opens it.
    pub fn add_draft(&mut self, tx: &mut Tx) {
        let Some(intake) = self.draft_intake() else {
            return;
        };
        match self.lib.add_chapter(&intake) {
            Ok(at) => {
                self.draft = None;
                self.screen = Screen::Reading;
                self.go(tx, at);
            }
            Err(e) => self.fail(e),
        }
    }

    /// Back to reading. The draft stays (the contents list keeps a row for it) unless it was
    /// empty or `discard` is set.
    pub fn leave_draft(&mut self, tx: &mut Tx, discard: bool) {
        let Some(d) = &mut self.draft else { return };
        // An empty Book has nowhere else to be: the page stays, emptied if asked.
        if self.lib.chapters.is_empty() {
            if discard {
                d.text = TextEdit::new("");
            }
            return;
        }
        let back = d.back;
        if discard || d.text.text.trim().is_empty() {
            self.draft = None;
        }
        self.screen = Screen::Reading;
        self.go(tx, back.min(self.lib.chapters.len() - 1));
    }

    /// The number the next Chapter gets if its text names none.
    pub fn next_number(&self) -> i64 {
        self.lib.chapters.last().map_or(1, |c| c.number + 1)
    }
}
