//! View state and the actions that change it. Nothing here is stored in the Book: the current
//! Chapter, sidebars, theme and scroll are the app's own.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cipher_book::{BlockId, PairId, Side};
use rune::{Key, KeyEvent, TextEdit, Tx};

use crate::draft::Draft;
use crate::fix::Fix;
use crate::fix::work::{Background, Msg, Paused, Progress};
use crate::home::Home;
use crate::library::Library;
use crate::prefs::{Appearance, Prefs};
use crate::settings::Settings;
use crate::shelf::{Position, Shelf};
use crate::welcome::Welcome;

/// Paragraphs of a translating Chapter as they arrive: (first, last, text), 1-based.
pub type Streamed = Vec<(u32, u32, String)>;

/// The narrowest reading column before a sidebar gives way to it.
const MIN_TEXT: f32 = 520.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Screen {
    Reading,
    Cast,
    /// The new-Chapter page (`draft.rs`).
    Draft,
    /// The Book's style, as one text (`style.rs`).
    Style,
}

/// The sections of the Settings window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    General,
    Reading,
    Translation,
    Shortcuts,
    About,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::General,
        Section::Reading,
        Section::Translation,
        Section::Shortcuts,
        Section::About,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Reading => "Reading",
            Section::Translation => "Translation",
            Section::Shortcuts => "Shortcuts",
            Section::About => "About",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Suggestions,
    Terms,
}

/// The Term shown in the context sidebar, anchored at the Block it was opened from so that the
/// margin note in the focus layout sits beside that paragraph.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TermCard {
    pub block: BlockId,
    pub term: usize,
}

pub struct State {
    pub lib: Library,
    pub screen: Screen,
    /// Read the source instead of the Translation (Ctrl+T).
    pub source: bool,
    pub interlinear: bool,
    /// What the user chose: appearance, reading type, sidebars, the window. `poll` writes it
    /// when it changes.
    pub prefs: Prefs,
    /// Where `prefs` is saved; `None` in a headless run.
    pub prefs_path: Option<std::path::PathBuf>,
    /// `prefs` as last written.
    pub saved_prefs: Prefs,
    /// The desktop's light or dark, as the view last saw it.
    pub system_dark: Cell<bool>,
    /// The window's size in points, as the view last laid it out.
    pub window_size: Cell<(f32, f32)>,
    pub tab: Tab,
    /// Pairs whose source is unfolded under their translated Block.
    pub unfolded: Vec<PairId>,
    pub term_card: Option<TermCard>,
    /// The Term whose row is open on the Cast page (an index into `lib.terms`).
    pub cast_open: Option<usize>,
    /// The Cast page's search, over renderings, source forms and forbidden wordings.
    pub cast_search: TextEdit,
    /// The Cast page shows only drifting Terms.
    pub cast_drifting: bool,
    /// How the Book names the Term whose drift panel is open (`drift.rs`).
    pub look: Option<crate::drift::Look>,
    pub palette: Option<TextEdit>,
    pub find: Option<crate::find::Find>,
    pub palette_pick: usize,
    pub toast: Option<String>,
    /// The toast offers Undo for this Suggestion's Review.
    pub toast_undo: Option<cipher_book::SuggestionId>,
    /// The toast showing and when it appeared, to let it go by itself.
    toast_seen: Option<(String, f64)>,
    pub fix: Fix,
    pub bg: Background,
    /// The Shelf, while it shows instead of a Book. `lib` is then a blank Book.
    pub home: Option<Home>,
    /// The first-run welcome, over the Shelf.
    pub welcome: Option<Welcome>,
    /// A new Chapter being pasted or typed onto its page.
    pub draft: Option<Draft>,
    /// The Style page's text, and what is saved, while it is open.
    pub style: Option<crate::style::StyleEdit>,
    /// The Settings window, open at this section.
    pub settings_page: Option<Section>,
    /// The Endpoint form in Settings › Translation, once it has shown.
    pub settings: Option<Settings>,
    /// The keyboard shortcuts sheet (Ctrl+/).
    pub shortcuts_open: bool,
    /// The reading type popover under the header's "Aa".
    pub type_open: bool,
    /// The header's model menu, with the Endpoints as saved when it opened, and the default.
    pub model_menu: Option<(Vec<cipher_llm::Endpoint>, Option<String>)>,
    /// A Chapter's right-click menu: the Chapter and where the pointer was.
    pub chapter_menu: Option<(usize, f32, f32)>,
    /// Files are being dragged over the window.
    pub drop_hover: bool,
    /// The Library load whose scan is running (`Library::generation`).
    scanning: u64,
    /// In a window too narrow for both sidebars, the context sidebar was opened last.
    context_first: bool,
    /// The Book whose old import Pairs were last checked for re-pairing.
    realigning: Option<std::path::PathBuf>,
    /// The running translation, from the worker's Events.
    pub progress: Option<Progress>,
    /// The translating Chapter's paragraphs finished so far: (first, last, text) over its
    /// source Block numbers, 1-based.
    pub streamed: Option<(cipher_book::ChapterId, Streamed)>,
    /// The queue is waiting for a usage limit to reset.
    pub paused: Option<Paused>,
    pub scroll_y: Rc<Cell<f32>>,
    /// The Chapters on the page, in order: the current one and its neighbours, so reading runs
    /// on from one Chapter into the next.
    pub strip: Vec<usize>,
    /// A scroll to make once the window can (`poll`), for the first Chapter shown.
    pub pending_scroll: Option<(String, f32)>,
    /// A window title to set once `poll` can.
    pub pending_title: Option<String>,
    /// When the page, having moved, counts as still (on the window clock).
    pub settle_at: Option<f64>,
    /// Top of each row of the current page, relative to the page, recorded each frame.
    pub tops: Rc<RefCell<Vec<(BlockId, f32)>>>,
    /// A headless run works on a copy of the Book in here, so it never changes the real one.
    /// Last, so the Book closes before the folder goes.
    pub scratch: Option<tempfile::TempDir>,
}

impl State {
    pub fn new(lib: Library) -> State {
        let mut state = State {
            lib,
            screen: Screen::Reading,
            source: false,
            interlinear: false,
            prefs: Prefs::default(),
            prefs_path: None,
            saved_prefs: Prefs::default(),
            system_dark: Cell::new(false),
            window_size: Cell::new((0.0, 0.0)),
            tab: Tab::Terms,
            unfolded: Vec::new(),
            term_card: None,
            cast_open: None,
            cast_search: TextEdit::new(""),
            cast_drifting: false,
            look: None,
            palette: None,
            find: None,
            palette_pick: 0,
            toast: None,
            toast_undo: None,
            toast_seen: None,
            fix: Fix::new(),
            bg: Background::new(),
            home: None,
            draft: None,
            style: None,
            welcome: None,
            settings_page: None,
            settings: None,
            shortcuts_open: false,
            type_open: false,
            chapter_menu: None,
            model_menu: None,
            drop_hover: false,
            scanning: 0,
            context_first: false,
            realigning: None,
            progress: None,
            streamed: None,
            paused: None,
            scroll_y: Rc::default(),
            strip: Vec::new(),
            pending_scroll: None,
            settle_at: None,
            pending_title: None,
            tops: Rc::default(),
            scratch: None,
        };
        let at = state.lib.current();
        state.load_strip(at);
        state
    }

    /// The Chapters to keep on the page around `at`: one before and one after.
    fn window(&self, at: usize) -> Vec<usize> {
        let n = self.lib.chapters.len();
        (at.saturating_sub(1)..(at + 2).min(n)).collect()
    }

    /// Puts Chapter `at` and its neighbours on the page, scrolled to `at`'s opener.
    pub fn load_strip(&mut self, at: usize) {
        self.strip = self.window(at);
        if let Err(e) = self.lib.keep_pages(&self.strip) {
            self.fail(e);
        }
        self.pending_scroll = (!self.strip.is_empty()).then(|| (format!("sec{at}"), 0.0));
    }

    /// Reading has moved into Chapter `at` (it crosses the reading line): it becomes current,
    /// the next Chapter loads below, and the contents list follows. Nothing above changes while
    /// the page moves: wheel input beats `keep_on_screen`, so a Chapter dropped or added above
    /// mid-scroll would jump the page. [`State::settle_strip`] trims and fills in once it stops.
    pub fn follow(&mut self, tx: &mut Tx, at: usize) {
        if self.pending_scroll.is_some() || self.lib.current() == at || self.fix.review.is_some() {
            return;
        }
        if let Err(e) = self.lib.set_current(at) {
            return self.fail(e);
        }
        if at + 1 < self.lib.chapters.len() && !self.strip.contains(&(at + 1)) {
            self.strip.push(at + 1);
            if let Err(e) = self.lib.keep_pages(&self.strip) {
                self.fail(e);
            }
        }
        self.fix.picked = None;
        tx.scroll_into_view_in("contents-list", format!("ch{at}"));
    }

    /// The page moved; the strip settles a moment after it last does.
    pub fn moved(&mut self, tx: &mut Tx) {
        let due = tx.clock() + 0.25;
        self.settle_at = Some(due);
        tx.wake_at(due);
    }

    /// The page has been still for a moment: the strip becomes the current Chapter and its
    /// neighbours, holding the current one where it is on screen. Returns whether it changed.
    pub fn settle_strip(&mut self, tx: &mut Tx) -> bool {
        match self.settle_at {
            Some(due) if tx.clock() >= due => self.settle_at = None,
            _ => return false,
        }
        let at = self.lib.current();
        let want = self.window(at);
        if self.strip == want || self.pending_scroll.is_some() {
            return false;
        }
        tx.keep_on_screen(format!("sec{at}"));
        self.remember_position();
        self.strip = want;
        if let Err(e) = self.lib.keep_pages(&self.strip) {
            self.fail(e);
        }
        true
    }

    /// Which text the page shows: a Chapter without a Translation can only show its source.
    pub fn side(&self) -> Side {
        if self.source || self.lib.page().translation.is_empty() {
            Side::Source
        } else {
            Side::Translation
        }
    }

    pub(crate) fn fail(&mut self, e: impl std::fmt::Display) {
        self.toast = Some(format!("Could not read the Book: {e}"));
    }

    /// Shows Chapter `i`, at its top, without moving the contents list.
    pub fn open_chapter(&mut self, i: usize) {
        if let Err(e) = self.lib.open_chapter(i) {
            return self.fail(e);
        }
        self.screen = Screen::Reading;
        self.term_card = None;
        let at = self.lib.current();
        self.load_strip(at);
    }

    /// Files dropped on the window: a `.cipher` Book opens (joining the Shelf), a folder of
    /// Chapters is imported onto the Shelf. Anything else is ignored, saying so.
    pub fn drop_files(&mut self, paths: &[std::path::PathBuf]) {
        self.drop_hover = false;
        let Some(path) = paths.first() else { return };
        if path
            .extension()
            .is_some_and(|x| x == crate::shelf::EXTENSION)
        {
            self.open_from_desktop(path);
        } else if path.is_dir() {
            if self.home.is_none() {
                self.go_shelf();
            }
            self.import_folder(path);
        } else {
            self.toast = Some("Drop a .cipher book, or a folder of chapters to import.".into());
            self.toast_undo = None;
        }
    }

    /// The next (`by` 1) or previous (-1) Chapter, from its top.
    pub fn step_chapter(&mut self, tx: &mut Tx, by: isize) {
        let at = self.lib.current() as isize + by;
        if at >= 0 && (at as usize) < self.lib.chapters.len() {
            self.go(tx, at as usize);
        }
    }

    /// Jumps to Chapter `i`: the page to its top and the contents list to the row.
    pub fn go(&mut self, tx: &mut Tx, i: usize) {
        self.open_chapter(i);
        let at = self.lib.current();
        if let Some((key, y)) = self.pending_scroll.take() {
            tx.scroll_key_to(key, y);
        }
        tx.scroll_key_to(format!("ch{at}"), 200.0);
    }

    /// Jumps to a Block of any Chapter.
    pub fn go_block(&mut self, tx: &mut Tx, chapter: cipher_book::ChapterId, block: BlockId) {
        if let Some(at) = self.lib.chapter_index(chapter) {
            self.source = false;
            self.open_chapter(at);
            self.pending_scroll = None;
            tx.scroll_key_to(format!("row{}", block.0), 160.0);
            tx.scroll_key_to(format!("ch{at}"), 200.0);
        }
    }

    /// Opens a Term's card, anchored at its first use in this Chapter.
    pub fn show_term(&mut self, tx: Option<&mut Tx>, term: usize) {
        let Some(block) = self.lib.first_use(term) else {
            return;
        };
        self.term_card = Some(TermCard { block, term });
        self.tab = Tab::Terms;
        self.screen = Screen::Reading;
        self.look_at(term);
        if let Some(tx) = tx {
            tx.scroll_key_to(format!("row{}", block.0), 160.0);
        }
    }

    /// Switches between the Translation and the source, keeping the reading position by Pair.
    pub fn toggle_source(&mut self, tx: &mut Tx) {
        if self.lib.page().translation.is_empty() {
            return;
        }
        let y = self.scroll_y.get();
        let tops = self.tops.borrow().clone();
        let top = tops
            .iter()
            .rfind(|(_, t)| *t <= y + 4.0)
            .or(tops.first())
            .copied();
        let from = self.side();
        let pair = top.and_then(|(id, _)| self.lib.page().pair_of(from, id));
        self.source = !self.source;
        let to = self.side();
        if let (Some(pair), Some((_, t))) = (pair, top)
            && let Some(id) = self.lib.page().first_block(to, pair)
        {
            tx.scroll_key_to(format!("row{}", id.0), t - y);
        }
    }

    pub fn toggle_unfolded(&mut self, pair: PairId) {
        match self.unfolded.iter().position(|p| *p == pair) {
            Some(at) => {
                self.unfolded.remove(at);
            }
            None => self.unfolded.push(pair),
        }
    }

    pub fn open_palette(&mut self) {
        let mut q = TextEdit::new("");
        q.focused = true;
        self.palette = Some(q);
        self.palette_pick = 0;
    }

    /// Queues an untranslated Chapter.
    pub fn translate_chapter(&mut self, at: usize) {
        let again = self.lib.chapters[at].state == cipher_book::ChapterState::Translated;
        if let Err(e) = self.lib.translate(at) {
            return self.fail(e);
        }
        if again {
            self.toast = Some(format!(
                "Chapter {} will be translated again. The current translation stays until the new one is ready.",
                self.lib.chapters[at].number
            ));
        }
    }

    /// Queues this Chapter and the waiting ones after it.
    pub fn translate_from(&mut self, at: usize) {
        if let Err(e) = self.lib.translate_from(at) {
            self.fail(e);
        }
    }

    /// Puts a failed Chapter back in the queue.
    pub fn retry_chapter(&mut self, at: usize) {
        if let Err(e) = self.lib.retry(at) {
            self.fail(e);
        }
    }

    /// Takes the saved preferences; `path` is where they go (`None` in a headless run).
    pub fn with_prefs(&mut self, prefs: Prefs, path: Option<std::path::PathBuf>) {
        self.saved_prefs = prefs.clone();
        self.prefs = prefs;
        self.prefs_path = path;
    }

    /// Light or dark, as chosen or as the desktop has it.
    /// Counts the open Book's Terms on another thread, once per load (`Library::scan`).
    pub fn start_scan(&mut self) {
        // Once the Book is counted, re-pair what an old import paired by position: once per
        // opened Book, after the counts, so they never wait on it.
        if self.lib.scanned()
            && self.lib.is_saved()
            && self.realigning != Some(self.lib.path().to_path_buf())
        {
            self.realigning = Some(self.lib.path().to_path_buf());
            let job = self.lib.realign();
            self.bg
                .spawn(move || Msg::Realigned(job().map_err(|e| e.to_string())));
        }
        if self.lib.scanned() || self.scanning == self.lib.generation() {
            return;
        }
        self.scanning = self.lib.generation();
        let job = self.lib.scan();
        self.bg
            .spawn(move || Msg::Scanned(job().map_err(|e| e.to_string())));
    }

    /// The window's width in layout points, or unbounded before the first frame says.
    fn avail_w(&self) -> f32 {
        let (w, _) = self.window_size.get();
        if w <= 0.0 {
            f32::INFINITY
        } else {
            w / self.prefs.zoom
        }
    }

    /// Room the page needs beside the sidebars before one of them gives way.
    fn fits(&self, contents: bool, context: bool) -> bool {
        use crate::theme::{GUTTER_W, LEFT_W, RIGHT_W, SCRUBBER_W};
        let left = if contents { LEFT_W } else { SCRUBBER_W };
        let right = if context { RIGHT_W } else { 0.0 };
        self.avail_w() - left - right >= MIN_TEXT + GUTTER_W + 48.0
    }

    /// Whether the contents sidebar shows: open, and the window wide enough beside what else
    /// shows. In a narrow window the sidebar opened last wins.
    pub fn contents_shown(&self) -> bool {
        let (a, b) = (
            self.prefs.contents_open,
            self.prefs.context_open && self.screen_has_context(),
        );
        match (a, b) {
            (false, _) => false,
            (true, false) => true,
            (true, true) => {
                self.fits(true, true) || (!self.context_first && self.fits(true, false))
            }
        }
    }

    /// Whether the context sidebar shows; see [`State::contents_shown`].
    pub fn context_shown(&self) -> bool {
        if !self.prefs.context_open || !self.screen_has_context() {
            return false;
        }
        if !self.prefs.contents_open {
            return true;
        }
        self.fits(true, true) || (self.context_first && self.fits(false, true))
    }

    /// The new-Chapter page has no Terms or Suggestions yet: it gets the room.
    fn screen_has_context(&self) -> bool {
        !matches!(self.screen, Screen::Draft | Screen::Style)
    }

    /// The reading column: the chosen width, narrowed to what the window leaves.
    pub fn text_width(&self) -> f32 {
        use crate::theme::{GUTTER_W, LEFT_W, RIGHT_W, SCRUBBER_W};
        let left = if self.contents_shown() {
            LEFT_W
        } else {
            SCRUBBER_W
        };
        let right = if self.context_shown() { RIGHT_W } else { 0.0 };
        let room = self.avail_w() - left - right - GUTTER_W - 48.0;
        self.prefs
            .reading
            .width
            .min(room.max(MIN_TEXT.min(self.prefs.reading.width)))
    }

    /// Whether the page margin has room for notes beside the column.
    pub fn margin_room(&self) -> bool {
        use crate::theme::{GUTTER_W, LEFT_W, SCRUBBER_W};
        let left = if self.contents_shown() {
            LEFT_W
        } else {
            SCRUBBER_W
        };
        self.avail_w() - left - GUTTER_W - self.text_width() >= 316.0
    }

    pub fn toggle_contents(&mut self) {
        if self.contents_shown() {
            self.prefs.contents_open = false;
        } else {
            self.prefs.contents_open = true;
            self.context_first = false;
        }
    }

    /// Lets a toast go after long enough to read it: a few seconds plus its length, longer
    /// when it offers Undo. Returns whether it went.
    pub fn settle_toast(&mut self, t: f64, tx: &mut Tx) -> bool {
        let Some(msg) = &self.toast else {
            self.toast_seen = None;
            return false;
        };
        if self.toast_seen.as_ref().is_none_or(|(m, _)| m != msg) {
            self.toast_seen = Some((msg.clone(), t));
        }
        let since = self.toast_seen.as_ref().map_or(t, |x| x.1);
        let life =
            if self.toast_undo.is_some() { 10.0 } else { 5.0 } + msg.chars().count() as f64 / 25.0;
        if t - since >= life {
            self.toast = None;
            self.toast_undo = None;
            self.toast_seen = None;
            true
        } else {
            tx.wake_at(since + life);
            false
        }
    }

    /// Opens the context sidebar, giving it the room first in a narrow window.
    pub fn show_context(&mut self) {
        self.prefs.context_open = true;
        self.context_first = true;
    }

    pub fn toggle_context(&mut self) {
        if self.context_shown() {
            self.prefs.context_open = false;
        } else {
            self.prefs.context_open = true;
            self.context_first = true;
        }
    }

    pub fn dark(&self) -> bool {
        self.prefs.appearance.dark(self.system_dark.get())
    }

    /// The quick switch (Ctrl+D, the header's sun or moon): the other one of what shows now.
    /// Choosing the one the desktop already has goes back to following it.
    pub fn flip_dark(&mut self) {
        let want_dark = !self.dark();
        self.prefs.appearance = if want_dark == self.system_dark.get() {
            Appearance::System
        } else if want_dark {
            Appearance::Dark
        } else {
            Appearance::Light
        };
    }

    /// Where reading is: the current Chapter and the row at the top of the window.
    pub fn reading_position(&self) -> Option<Position> {
        let chapter = self.lib.chapters.get(self.lib.current())?.number;
        let y = self.scroll_y.get();
        let top = self
            .tops
            .borrow()
            .iter()
            .rfind(|(_, t)| *t <= y + 4.0)
            .copied();
        Some(Position {
            chapter,
            block: top.map(|(id, _)| id.0),
            offset: top.map_or(0.0, |(_, t)| t - y),
        })
    }

    /// Notes where reading is in `shelf.json`, so the Book opens there next time.
    pub fn remember_position(&self) {
        if self.prefs_path.is_none() || !self.lib.is_saved() || self.lib.chapters.is_empty() {
            return;
        }
        if let (Some(recents), Some(pos)) = (Shelf::default_recents(), self.reading_position()) {
            let _ = Shelf::remember_position(&recents, self.lib.path(), pos);
        }
    }

    /// The window is closing: keep its size, the preferences and the reading position.
    pub fn on_close(&mut self) {
        let (w, h) = self.window_size.get();
        if w > 0.0 && h > 0.0 {
            self.prefs.window.width = w;
            self.prefs.window.height = h;
        }
        self.remember_position();
        self.save_prefs();
    }

    /// Writes the preferences if they changed since they were last written.
    pub fn save_prefs(&mut self) {
        if self.prefs == self.saved_prefs {
            return;
        }
        self.saved_prefs = self.prefs.clone();
        if let Some(path) = &self.prefs_path
            && let Err(e) = self.prefs.save(path)
        {
            self.toast = Some(e);
        }
    }

    pub fn toggle_cast(&mut self) {
        self.screen = if self.screen == Screen::Cast {
            Screen::Reading
        } else {
            Screen::Cast
        };
    }
}

pub fn on_key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> bool {
    // Anywhere: the window, Settings and the shortcuts sheet.
    match &k.key {
        Key::Command('q') => tx.quit(),
        Key::Function(11) => tx.toggle_full_screen(),
        Key::Command(',') => match s.settings_page {
            Some(_) => s.close_settings(),
            None => s.open_settings_at(Section::General),
        },
        Key::Command('/') | Key::Function(1) => {
            s.shortcuts_open = !s.shortcuts_open;
            if s.shortcuts_open {
                s.palette = None;
            }
        }
        Key::Command('d') => s.flip_dark(),
        _ => return screen_key(s, k, tx),
    }
    true
}

/// Keys for what is on screen: a sheet, Settings, the Shelf or a Book.
fn screen_key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> bool {
    if s.home.is_none() && s.screen == Screen::Reading && s.settings_page.is_none() {
        if k.key == Key::Command('f') {
            s.open_find(tx);
            return true;
        }
        if s.find.is_some() && s.palette.is_none() && crate::find::key(s, k, tx) {
            return true;
        }
    }
    if s.model_menu.is_some() && k.key == Key::Escape {
        s.model_menu = None;
        return true;
    }
    if s.chapter_menu.is_some() && k.key == Key::Escape {
        s.chapter_menu = None;
        return true;
    }
    if s.type_open && k.key == Key::Escape {
        s.type_open = false;
        return true;
    }
    if s.shortcuts_open {
        if k.key == Key::Escape {
            s.shortcuts_open = false;
            return true;
        }
        return false;
    }
    if s.screen == Screen::Style {
        match &k.key {
            Key::Command('s') => s.save_style(),
            Key::Escape => s.leave_style(),
            _ => return false,
        }
        return true;
    }
    if s.screen == Screen::Draft && s.draft.is_some() {
        match &k.key {
            Key::Enter if k.mods.cmd => s.add_draft(tx),
            Key::Escape => s.leave_draft(tx, false),
            _ => return false,
        }
        return true;
    }
    if s.settings_page.is_some() {
        if k.key == Key::Escape {
            s.close_settings();
            return true;
        }
        return false;
    }
    if s.home.is_some() {
        return match &k.key {
            Key::Command('n') if s.welcome.is_none() => {
                s.start_new_book();
                true
            }
            Key::Escape => s.shelf_escape(),
            _ => false,
        };
    }
    let reading = s.screen == Screen::Reading && s.fix.review.is_none() && s.palette.is_none();
    match &k.key {
        Key::Command('k') => s.open_palette(),
        Key::Command('n') => s.new_chapter(tx),
        Key::Command('o') | Key::Command('w') => s.go_shelf(),
        Key::Command('\\') => s.toggle_contents(),
        Key::Command('j') => s.toggle_context(),
        Key::Command('g') => s.toggle_cast(),
        Key::Command('i') => s.interlinear = !s.interlinear,
        Key::Command('t') => s.toggle_source(tx),
        Key::Command('=') | Key::Command('+') => s.prefs.reading.step_size(true),
        Key::Command('-') => s.prefs.reading.step_size(false),
        Key::Command('0') => s.prefs.reading.size = crate::prefs::Reading::default().size,
        Key::Text(t) if reading && t == " " && !k.mods.cmd => {
            tx.scroll_page("doc", if k.mods.shift { -1 } else { 1 })
        }
        Key::Right if reading && k.mods.alt => s.step_chapter(tx, 1),
        Key::Left if reading && k.mods.alt => s.step_chapter(tx, -1),
        _ => return fix_key(s, k, tx),
    }
    true
}

/// Keys for fixing while reading: Review's J K A X Shift+A Esc, and typing over a selection.
fn fix_key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> bool {
    if s.fix.review.is_some() {
        match &k.key {
            Key::Text(t) if t == "j" => s.step_edit(tx, 1),
            Key::Text(t) if t == "k" => s.step_edit(tx, -1),
            Key::Text(t) if t == "a" => s.decide_current(tx, true),
            Key::Text(t) if t == "A" => s.accept_rest(tx),
            Key::Text(t) if t == "x" => s.decide_current(tx, false),
            Key::Escape => s.leave_review(tx),
            _ => return false,
        }
        return true;
    }
    match &k.key {
        // With a selection, typing starts a fix; Ctrl+C still copies.
        Key::Text(t)
            if s.fix.picked.is_some()
                && !s.fix.composing
                && s.screen == Screen::Reading
                && !t.trim().is_empty() =>
        {
            s.open_composer(t);
        }
        Key::Escape => {
            if s.fix.composing {
                s.fix.composing = false;
            } else if s.fix.picked.is_some() {
                s.fix.picked = None;
            } else if s.term_card.is_some() {
                s.term_card = None;
            } else if s.screen == Screen::Cast {
                s.screen = Screen::Reading;
            } else {
                s.toast = None;
                s.toast_undo = None;
            }
        }
        _ => return false,
    }
    true
}
