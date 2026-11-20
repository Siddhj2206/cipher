//! The Shelf screen's state and what its buttons do: open, new, import, open from elsewhere,
//! rename and remove, and the way back from a Book.

use std::path::{Path, PathBuf};

use rune::{FileDialogOptions, TextEdit, Tx};

use crate::files::{self, Show};
use crate::fix::Fix;
use crate::fix::work::Msg;
use crate::library::Library;
use crate::shelf::{EXTENSION, Position, Shelf};
use crate::state::{Screen, State};

/// Source Languages a new Book can have.
pub const LANGUAGES: [(&str, &str); 3] = [("ko", "Korean"), ("ja", "Japanese"), ("zh", "Chinese")];

/// Typing a path, for when the desktop has no file chooser portal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prompt {
    Import,
    OpenFile,
}

#[derive(Clone, Copy)]
pub enum FilesSource {
    Shelf,
    Book,
    Settings,
}

pub struct NewBook {
    pub title: TextEdit,
    pub lang: &'static str,
}

pub struct Home {
    pub shelf: Shelf,
    pub new_book: Option<NewBook>,
    /// The Book whose title is being edited.
    pub renaming: Option<(PathBuf, TextEdit)>,
    /// The Book whose "⋯" menu is open.
    pub menu: Option<PathBuf>,
    /// The Book waiting for "Move to Trash" or "Remove" to be confirmed.
    pub confirm: Option<PathBuf>,
    pub prompt: Option<(Prompt, TextEdit)>,
    /// Something running, such as an import.
    pub busy: Option<String>,
    pub error: Option<String>,
    pub files_notice: Option<&'static str>,
    /// Filters the Shelf by title.
    pub search: TextEdit,
}

impl Home {
    pub fn new(shelf: Shelf) -> Home {
        Home {
            shelf,
            new_book: None,
            renaming: None,
            menu: None,
            confirm: None,
            prompt: None,
            busy: None,
            error: None,
            files_notice: None,
            search: TextEdit::new(""),
        }
    }

    /// The user's Shelf, or in a headless run one in `dir` that remembers nothing.
    pub fn load(headless_dir: Option<&Path>) -> Home {
        let shelf = match headless_dir {
            Some(dir) => Shelf::open(dir.to_path_buf(), None),
            None => Shelf::open(
                Shelf::default_dir().unwrap_or_else(|| PathBuf::from("Cipher")),
                Shelf::default_recents(),
            ),
        };
        Home::new(shelf)
    }

    fn closed_forms(&mut self) {
        self.new_book = None;
        self.renaming = None;
        self.menu = None;
        self.confirm = None;
        self.prompt = None;
        self.clear_message();
    }

    pub fn clear_message(&mut self) {
        self.error = None;
        self.files_notice = None;
    }
}

fn focused(text: &str) -> TextEdit {
    let mut t = TextEdit::new(text);
    t.focused = true;
    t
}

impl State {
    fn home(&mut self) -> Option<&mut Home> {
        self.home.as_mut()
    }

    fn home_error(&mut self, e: impl std::fmt::Display) {
        if let Some(h) = self.home() {
            h.busy = None;
            h.error = Some(e.to_string());
        }
    }

    /// Opens a Book from the Shelf and puts it on screen.
    pub fn open_book(&mut self, path: &Path) {
        let mut lib = match Library::open_quick(path) {
            Ok(lib) => lib,
            Err(e) => return self.home_error(format!("Could not open {}: {e}", path.display())),
        };
        if let Some(h) = self.home()
            && let Err(e) = h.shelf.opened(path)
        {
            h.error = Some(e);
        }
        let position = self
            .prefs_path
            .as_ref()
            .and(Shelf::default_recents())
            .and_then(|r| Shelf::position_in(&r, path));
        if let Some(pos) = position
            && let Some(i) = lib.chapters.iter().position(|c| c.number == pos.chapter)
        {
            let _ = lib.open_chapter(i);
        }
        self.show_book(lib);
        if let Some(Position {
            block: Some(block),
            offset,
            ..
        }) = position
        {
            let page = self.lib.page();
            if page.blocks(self.side()).iter().any(|b| b.id.0 == block) {
                self.pending_scroll = Some((format!("row{block}"), offset));
            }
        }
        if self.prefs_path.is_some() {
            self.prefs.last_book = Some(path.to_path_buf());
        }
        self.finish_welcome();
        self.pending_title = Some(format!("{} · Cipher", self.lib.meta.title));
    }

    /// Opens the Book that was open when the app last closed, at launch (no window yet, so no
    /// `Tx` yet; `poll` sets the title). A Book that can't be opened leaves the Shelf showing.
    pub fn reopen(&mut self, path: &Path) {
        self.open_book(path);
        if self.home.is_some() {
            self.prefs.last_book = None;
        }
    }

    /// A `.cipher` file opened from the desktop while Cipher runs: it joins the Shelf if it is
    /// from elsewhere, and opens.
    pub fn open_from_desktop(&mut self, path: &Path) {
        if self.home.is_none() {
            self.go_shelf();
        }
        self.add_book_file(path);
    }

    /// Makes `lib` the Book on screen, from its first Chapter, with its own worker.
    pub fn show_book(&mut self, lib: Library) {
        self.notifier = crate::notify::Notifier::default();
        self.lib = lib;
        self.home = None;
        self.screen = Screen::Reading;
        self.source = false;
        self.interlinear = false;
        self.unfolded.clear();
        self.term_card = None;
        self.palette = None;
        self.toast = None;
        self.toast_undo = None;
        self.term_toast_undo = None;
        self.term_editor = None;
        self.term_merge = None;
        self.cast_open = None;
        self.fix = Fix::new();
        self.draft = None;
        self.notes = None;
        self.progress = None;
        self.paused = None;
        self.tops.borrow_mut().clear();
        let at = self.lib.current();
        self.load_strip(at);
        // A Book with no Chapters opens on the page for its first.
        if self.lib.chapters.is_empty() {
            self.open_draft("");
        }
        if crate::live() && self.lib.is_saved() {
            let path = self.lib.path().to_path_buf();
            self.bg.restart(&path);
        }
    }

    /// Closes the Book and shows the Shelf. Its worker stops after the call it is making.
    pub fn go_shelf(&mut self) {
        self.remember_position();
        self.prefs.last_book = None;
        self.show_shelf();
        self.pending_title = Some("Cipher".into());
    }

    pub fn show_shelf(&mut self) {
        self.notifier = crate::notify::Notifier::default();
        self.bg.stop();
        let dir = self.scratch.as_ref().map(|d| d.path().to_path_buf());
        self.lib = Library::blank();
        self.strip.clear();
        self.settings = None;
        self.draft = None;
        self.palette = None;
        self.toast = None;
        self.toast_undo = None;
        self.term_toast_undo = None;
        self.term_editor = None;
        self.term_merge = None;
        self.cast_open = None;
        self.fix = Fix::new();
        self.home = Some(Home::load(dir.as_deref()));
    }

    pub fn start_new_book(&mut self) {
        if let Some(h) = self.home() {
            h.closed_forms();
            h.new_book = Some(NewBook {
                title: focused(""),
                lang: "ko",
            });
        }
    }

    /// Creates the Book the form describes and opens it.
    pub fn create_book(&mut self) {
        let Some(h) = self.home() else { return };
        let Some(form) = &h.new_book else { return };
        let (title, lang) = (form.title.text.clone(), form.lang);
        match h.shelf.create(&title, lang) {
            Ok(path) => self.open_book(&path),
            Err(e) => h.error = Some(e),
        }
    }

    /// Asks for a folder of `raw/<n>.md` Chapters to import.
    pub fn pick_import(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions {
            title: "Import a folder of chapters".into(),
            ..Default::default()
        };
        tx.open_folder(options, |s: &mut State, answer, _| match answer {
            Ok(Some(folder)) => s.import_folder(&folder),
            Ok(None) => {}
            Err(_) => s.ask_path(Prompt::Import),
        });
    }

    /// Asks for a `.cipher` file anywhere.
    pub fn pick_book_file(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions {
            title: "Open a Cipher book".into(),
            ..Default::default()
        }
        .filter("Cipher books", &[EXTENSION]);
        tx.open_file(options, |s: &mut State, answer, _| match answer {
            Ok(Some(path)) => s.add_book_file(&path),
            Ok(None) => {}
            Err(_) => s.ask_path(Prompt::OpenFile),
        });
    }

    /// No file chooser: ask for the path as text instead.
    fn ask_path(&mut self, prompt: Prompt) {
        if let Some(h) = self.home() {
            h.closed_forms();
            h.prompt = Some((prompt, focused("")));
        }
    }

    /// The typed path, for the kind of prompt that asked.
    pub fn submit_path(&mut self) {
        let Some((prompt, text)) = self.home().and_then(|h| h.prompt.take()) else {
            return;
        };
        let raw = text.text.trim();
        let path = match raw.strip_prefix("~/") {
            Some(rest) => std::env::var_os("HOME")
                .map_or_else(|| PathBuf::from(raw), |h| Path::new(&h).join(rest)),
            None => PathBuf::from(raw),
        };
        match prompt {
            Prompt::Import => self.import_folder(&path),
            Prompt::OpenFile => self.add_book_file(&path),
        }
    }

    fn add_book_file(&mut self, path: &Path) {
        let Some(h) = self.home() else { return };
        match h.shelf.add(path) {
            Ok(()) => {
                let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                self.open_book(&path);
            }
            Err(e) => h.error = Some(e),
        }
    }

    /// Imports on a thread; the Shelf shows it when done.
    pub fn import_folder(&mut self, folder: &Path) {
        let Some(h) = self.home() else { return };
        let out = match h.shelf.import_path(folder) {
            Ok(out) => out,
            Err(e) => return h.error = Some(e),
        };
        h.closed_forms();
        h.busy = Some(format!(
            "Importing {}…",
            folder.file_name().unwrap_or_default().to_string_lossy()
        ));
        let folder = folder.to_path_buf();
        self.bg.spawn(move || {
            Msg::Imported(
                cipher_engine::import(&folder, &out)
                    .map(|r| (out, r))
                    .map_err(|e| e.to_string()),
            )
        });
    }

    pub fn on_imported(&mut self, result: Result<(PathBuf, cipher_engine::ImportReport), String>) {
        let Some(h) = self.home() else { return };
        h.busy = None;
        match result {
            Ok((_, r)) => {
                h.shelf.scan();
                self.finish_welcome();
                let mut line = format!(
                    "Imported {} chapters ({} translated), {} terms",
                    r.chapters, r.translated, r.terms
                );
                if r.skipped_terms > 0 {
                    line.push_str(&format!(
                        "; {} glossary entries had no source form",
                        r.skipped_terms
                    ));
                }
                self.toast = Some(line);
                self.toast_undo = None;
                self.term_toast_undo = None;
            }
            Err(e) => h.error = Some(e),
        }
    }

    pub fn start_rename(&mut self, path: PathBuf, title: &str) {
        if let Some(h) = self.home() {
            h.closed_forms();
            h.renaming = Some((path, focused(title)));
        }
    }

    pub fn finish_rename(&mut self) {
        let Some(h) = self.home() else { return };
        let Some((path, title)) = h.renaming.take() else {
            return;
        };
        if let Err(e) = h.shelf.rename(&path, &title.text) {
            h.error = Some(e);
        }
    }

    /// Opens or closes a Book's "⋯" menu.
    pub fn toggle_book_menu(&mut self, path: PathBuf) {
        if let Some(h) = self.home() {
            let open = h.menu.as_ref() == Some(&path);
            h.closed_forms();
            if !open {
                h.menu = Some(path);
            }
        }
    }

    pub fn confirm_remove(&mut self, path: PathBuf) {
        if let Some(h) = self.home() {
            h.closed_forms();
            h.confirm = Some(path);
        }
    }

    pub fn remove_book(&mut self) {
        let Some(h) = self.home() else { return };
        let Some(path) = h.confirm.take() else { return };
        if let Err(e) = h.shelf.remove(&path) {
            h.error = Some(e);
        }
    }

    /// Opens the folder Cipher keeps Books in, from Settings.
    pub fn show_books_folder(&mut self) {
        self.settings_books_notice = None;
        if let Some(dir) = self
            .home
            .as_ref()
            .map(|h| h.shelf.dir.clone())
            .or_else(Shelf::default_dir)
        {
            self.show_in_files(dir, FilesSource::Settings);
        } else {
            self.on_show_in_files(
                FilesSource::Settings,
                Err("Could not locate the Books folder".into()),
            );
        }
    }

    /// Shows a Book in the file manager.
    pub fn reveal_book(&mut self, path: &Path) {
        if let Some(h) = self.home() {
            h.menu = None;
            h.clear_message();
            self.show_in_files(path.to_path_buf(), FilesSource::Book);
        }
    }

    /// Opens the Shelf's folder in the file manager.
    pub fn show_folder(&mut self) {
        let Some(h) = self.home() else { return };
        h.clear_message();
        let dir = h.shelf.dir.clone();
        self.show_in_files(dir, FilesSource::Shelf);
    }

    fn show_in_files(&self, path: PathBuf, source: FilesSource) {
        if !rune::is_headless() {
            let target = match source {
                FilesSource::Book => Show::Book,
                FilesSource::Shelf | FilesSource::Settings => Show::Folder,
            };
            self.bg.spawn(move || Msg::ShowInFiles {
                source,
                result: files::show(&path, target),
            });
        }
    }

    pub fn on_show_in_files(&mut self, source: FilesSource, result: Result<(), String>) {
        let Err(e) = result else { return };
        eprintln!("cipher: {e}");
        let notice = match source {
            FilesSource::Book => "Try opening the book’s folder manually.",
            FilesSource::Shelf | FilesSource::Settings => "Try opening the Books folder manually.",
        };
        if matches!(source, FilesSource::Settings) {
            if self.settings_page.is_some() {
                self.settings_books_notice = Some(notice);
            }
        } else if let Some(h) = self.home() {
            h.files_notice = Some(notice);
        }
    }

    /// Esc on the Shelf: closes the open form, if any.
    pub fn shelf_escape(&mut self) -> bool {
        // The welcome keeps its cover open; Esc there does nothing.
        if self.welcome.is_some() {
            return false;
        }
        let Some(h) = self.home() else { return false };
        let open = h.new_book.is_some()
            || h.renaming.is_some()
            || h.menu.is_some()
            || h.confirm.is_some()
            || h.prompt.is_some()
            || h.error.is_some()
            || h.files_notice.is_some();
        h.closed_forms();
        open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Section;

    #[test]
    fn empty_file_books_open_on_the_new_chapter_page_through_every_entry_point() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.cipher");
        drop(
            cipher_book::Book::create(
                &path,
                cipher_book::BookMeta {
                    title: "Empty".into(),
                    source_lang: "ko".into(),
                    target_lang: "en".into(),
                },
            )
            .unwrap(),
        );
        let check = |state: State| {
            assert!(state.home.is_none());
            assert!(state.lib.chapters.is_empty());
            assert!(state.strip.is_empty());
            assert_eq!(state.screen, Screen::Draft);
            assert!(state.draft.as_ref().unwrap().text.text.is_empty());
            let mut runtime = rune::Runtime::new(
                state,
                crate::view::view,
                (1280.0, 900.0),
                rune::Env::new(false, 1.0),
            );
            runtime.frame(1.0 / 60.0);
            assert!(
                runtime
                    .elements()
                    .iter()
                    .any(|e| e.key.as_deref() == Some("draft-text"))
            );
            assert!(
                !runtime
                    .elements()
                    .iter()
                    .any(|e| e.text.as_deref() == Some("Chapter 0"))
            );
        };
        for deleted in [false, true] {
            if deleted {
                let mut book = cipher_book::Book::open(&path).unwrap();
                let chapter = book
                    .apply(cipher_book::Change::AddChapter {
                        number: Some(1),
                        title: None,
                        source: vec!["A chapter".into()],
                    })
                    .unwrap()
                    .created[0];
                book.apply(cipher_book::Change::DeleteChapter {
                    chapter: cipher_book::ChapterId(chapter),
                })
                .unwrap();
            }
            check(State::new(Library::open_quick(&path).unwrap()));
            for open in [State::reopen, State::open_book, State::open_from_desktop] {
                let mut state = State::new(Library::blank());
                state.home = Some(Home::load(Some(dir.path())));
                open(&mut state, &path);
                check(state);
            }
        }
    }

    #[test]
    fn show_in_files_reports_errors_at_the_entry_point() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("Missing Book é.cipher");
        let error = files::show(&path, Show::Book).unwrap_err();
        assert!(error.starts_with("Could not show the Book in the file manager:"));
        assert!(error.contains("Missing Book é.cipher"));

        let mut state = State::new(Library::blank());
        state.home = Some(Home::load(Some(scratch.path())));
        state.on_show_in_files(FilesSource::Book, Err(error.clone()));
        let home = state.home.as_ref().unwrap();
        assert_eq!(
            home.files_notice,
            Some("Try opening the book’s folder manually.")
        );
        assert!(home.error.is_none());
        assert!(state.settings_books_notice.is_none());

        assert!(state.shelf_escape());
        assert!(state.home.as_ref().unwrap().files_notice.is_none());
        state.on_show_in_files(FilesSource::Shelf, Err(error.clone()));
        assert_eq!(
            state.home.as_ref().unwrap().files_notice,
            Some("Try opening the Books folder manually.")
        );
        state.home.as_mut().unwrap().clear_message();

        state.open_settings_at(Section::General);
        state.on_show_in_files(FilesSource::Settings, Err(error.clone()));
        assert_eq!(
            state.settings_books_notice,
            Some("Try opening the Books folder manually.")
        );
        assert!(state.home.as_ref().unwrap().files_notice.is_none());

        state.close_settings();
        state.on_show_in_files(FilesSource::Settings, Err(error));
        assert!(state.settings_books_notice.is_none());
    }
}
