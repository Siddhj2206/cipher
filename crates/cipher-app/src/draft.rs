//! A new Chapter, set on its own page at the end of the Book (`docs/design/desktop-ux.md` ›
//! Adding chapters): paste or type the source onto the page, see what Cipher reads from it
//! (number, title, where it goes), and add it. Pasting anywhere in a Book opens it with the
//! text; an empty Book opens on it.

use rune::{FileDialogOptions, TextEdit, Tx};

use crate::library::{Intake, IntakeIssue, IntakePart};
use crate::state::{Screen, State};

pub fn batch_range(parts: &[IntakePart]) -> String {
    let selected = parts.iter().any(|p| p.include);
    let mut numbers = parts
        .iter()
        .filter(|p| !selected || p.include)
        .filter_map(|p| p.intake.number);
    let Some(first) = numbers.next() else {
        return String::new();
    };
    let last = numbers.last().unwrap_or(first);
    if first == last {
        first.to_string()
    } else {
        format!("{first}–{last}")
    }
}

pub struct Draft {
    /// The Chapter's source, as pasted or typed.
    pub text: TextEdit,
    /// The Chapter that was open, to go back to when the draft is put aside.
    pub back: usize,
    pub files: Vec<(String, String)>,
    pub file_name: Option<String>,
    pub included: std::collections::BTreeMap<(usize, usize), bool>,
    pub added: std::collections::BTreeSet<(usize, usize)>,
    pub numbers: std::collections::BTreeMap<(usize, usize), i64>,
}

impl Draft {
    pub fn reset_parts(&mut self) {
        self.included.clear();
        self.added.clear();
        self.numbers.clear();
    }

    pub fn pick_file(&mut self, name: String, text: String, append: bool) {
        if append {
            if self.files.is_empty() && !self.text.text.trim().is_empty() {
                self.files.push((
                    self.file_name.take().unwrap_or_else(|| "Draft".into()),
                    self.text.text.clone(),
                ));
                self.text = TextEdit::new("");
            } else if !self.text.text.trim().is_empty() {
                let source = self.files.len();
                let move_key = |(s, index)| (if s == source { s + 1 } else { s }, index);
                self.included = std::mem::take(&mut self.included)
                    .into_iter()
                    .map(|(key, value)| (move_key(key), value))
                    .collect();
                self.added = std::mem::take(&mut self.added)
                    .into_iter()
                    .map(move_key)
                    .collect();
                self.numbers = std::mem::take(&mut self.numbers)
                    .into_iter()
                    .map(|(key, value)| (move_key(key), value))
                    .collect();
            }
            self.files.push((name, text));
        } else {
            self.reset_parts();
            self.files.clear();
            self.file_name = Some(name);
            self.text = TextEdit::new(&text);
        }
    }
}

impl State {
    pub fn draft_plan(&self) -> Vec<IntakePart> {
        let Some(d) = &self.draft else {
            return Vec::new();
        };
        let mut plan = if d.files.is_empty()
            && let Some(name) = &d.file_name
        {
            self.lib
                .intake_plan(&[(name.clone(), d.text.text.clone())], "")
        } else {
            self.lib.intake_plan(&d.files, &d.text.text)
        };
        let mut next = self
            .lib
            .chapters
            .iter()
            .map(|c| c.number)
            .chain(d.numbers.values().copied())
            .chain(
                plan.iter()
                    .filter(|p| p.issue != Some(IntakeIssue::NoNumber))
                    .filter_map(|p| p.intake.number),
            )
            .max()
            .unwrap_or(0);
        for part in &mut plan {
            if let Some(&number) = d.numbers.get(&part.key) {
                part.intake.number = Some(number);
            } else if !d.numbers.is_empty() && part.issue == Some(IntakeIssue::NoNumber) {
                next += 1;
                part.intake.number = Some(next);
            }
            if let Some(&include) = d.included.get(&part.key) {
                part.include = include;
            }
            part.added = d.added.contains(&part.key);
            if part.added {
                part.include = false;
            }
        }
        plan.sort_by_key(|p| p.intake.number);
        plan
    }

    pub fn toggle_draft_part(&mut self, key: (usize, usize)) {
        let Some(include) = self
            .draft_plan()
            .iter()
            .find(|p| p.key == key && !p.added)
            .map(|p| p.include)
        else {
            return;
        };
        if let Some(d) = &mut self.draft {
            d.included.insert(key, !include);
        }
    }

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
            files: Vec::new(),
            file_name: None,
            included: Default::default(),
            added: Default::default(),
            numbers: Default::default(),
        });
        if !text.trim().is_empty() {
            draft.reset_parts();
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
                .is_some_and(|d| d.text.text.trim().is_empty() && d.files.is_empty());
            if fresh
                && let Some(it) = s.lib.intake(text)
                && (it.number.is_some() || it.blocks.len() >= 3 || s.lib.intakes(text).len() >= 2)
            {
                s.open_draft(text);
            }
        });
    }

    /// Fills the page from a text file.
    pub fn draft_from_file(&mut self, tx: &mut Tx) {
        self.pick_draft_file(tx, false);
    }

    pub fn draft_more_files(&mut self, tx: &mut Tx) {
        self.pick_draft_file(tx, true);
    }

    fn pick_draft_file(&mut self, tx: &mut Tx, append: bool) {
        let options = FileDialogOptions {
            title: "Add a chapter from a text file".into(),
            ..Default::default()
        }
        .filter("Text", &["txt", "md", "markdown"]);
        tx.open_file(options, move |s: &mut State, answer, _| {
            if let Ok(Some(path)) = answer {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        if let Some(d) = &mut s.draft {
                            d.pick_file(name, text, append);
                        }
                        s.open_draft("");
                    }
                    Err(e) => s.toast = Some(format!("Could not read {}: {e}", path.display())),
                }
            }
        });
    }

    /// Adds the draft as a Chapter in number order, queues it and opens it.
    pub fn add_draft(&mut self, tx: &mut Tx) {
        self.add_draft_with(tx, crate::library::Library::add_intakes);
    }

    pub(crate) fn add_draft_with(
        &mut self,
        tx: &mut Tx,
        commit: impl FnOnce(
            &mut crate::library::Library,
            &[IntakePart],
        ) -> cipher_book::Result<Option<usize>>,
    ) {
        let plan = self.draft_plan();
        let batch = self.draft.as_ref().is_some_and(|d| !d.files.is_empty()) || plan.len() > 1;
        let previous: std::collections::HashSet<_> =
            self.lib.chapters.iter().map(|c| c.id).collect();
        let total = plan.iter().filter(|p| p.include).count();
        if batch && let Some(d) = &mut self.draft {
            d.numbers
                .extend(plan.iter().filter_map(|p| Some((p.key, p.intake.number?))));
        }
        let result = if batch {
            commit(&mut self.lib, &plan)
        } else {
            let Some(intake) = self.draft_intake() else {
                return;
            };
            self.lib.add_chapter(&intake).map(Some)
        };
        match result {
            Ok(Some(at)) => {
                self.draft = None;
                self.screen = Screen::Reading;
                self.go(tx, at);
                if batch {
                    self.toast = None;
                    self.toast_undo = None;
                }
            }
            Ok(None) => {}
            Err(e) => {
                let added: Vec<_> = self
                    .lib
                    .chapters
                    .iter()
                    .enumerate()
                    .filter_map(|(at, c)| (!previous.contains(&c.id)).then_some(at))
                    .collect();
                if let Some(&at) = added.first() {
                    let mut saved: Vec<_> =
                        added.iter().map(|&i| self.lib.chapters[i].number).collect();
                    if let Some(d) = &mut self.draft {
                        for part in plan.iter().filter(|p| p.include) {
                            if let Some(i) =
                                saved.iter().position(|&n| Some(n) == part.intake.number)
                            {
                                saved.remove(i);
                                d.added.insert(part.key);
                            }
                        }
                    }
                    if added.len() == total {
                        self.draft = None;
                        self.screen = Screen::Reading;
                        self.go(tx, at);
                    }
                    self.toast = Some(format!("Added {} of {total} chapters: {e}", added.len()));
                    self.toast_undo = None;
                } else {
                    self.fail(e);
                }
            }
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
                d.files.clear();
                d.file_name = None;
                d.reset_parts();
            }
            return;
        }
        let back = d.back;
        if discard || (d.text.text.trim().is_empty() && d.files.is_empty()) {
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
