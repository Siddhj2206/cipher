// A release build on Windows is a window app: no console opens behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! The Cipher desktop app. See docs/design/desktop-ux.md.
//!
//! `cipher` opens the Shelf. For development, `cipher <book.cipher>` (or `CIPHER_BOOK`) opens a
//! Book directly, and `CIPHER_CHAPTER=N` opens it at the Chapter numbered N.

mod draft;
mod drift;
mod files;
mod fix;
mod history;
mod home;
mod keys;
mod library;
mod palette;
mod prefs;
mod scenes;
mod settings;
mod shelf;
mod sidebars;
mod state;
mod style;
mod theme;
mod view;
mod welcome;

use std::path::{Path, PathBuf};

use rune::{AppSpec, Cx, El, Tx, Waker};

use home::Home;
use library::Library;
use state::{State, on_key};

/// A Book to open at once instead of the Shelf: the first argument ending in `.cipher` (what a
/// file manager's "Open with" passes), else `CIPHER_BOOK`. Rune's own flags and their values
/// never end that way.
fn book_path() -> Option<String> {
    std::env::args()
        .skip(1)
        .find_map(|a| book_arg(&a))
        .or_else(|| std::env::var("CIPHER_BOOK").ok())
}

/// A launch argument naming a Book: a path, or a `file://` link (the desktop entry's `%U`).
fn book_arg(arg: &str) -> Option<String> {
    if arg.starts_with("--") || !arg.ends_with(".cipher") {
        return None;
    }
    match arg.strip_prefix("file://") {
        Some(rest) => Some(percent_decode(rest)),
        None => Some(arg.to_string()),
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The saved preferences, unless this is a headless run.
fn load_prefs() -> (prefs::Prefs, Option<PathBuf>) {
    if rune::is_headless() {
        return (prefs::Prefs::default(), None);
    }
    let path = prefs::Prefs::default_path();
    (
        path.as_deref().map(prefs::Prefs::load).unwrap_or_default(),
        path,
    )
}

fn init() -> State {
    let (prefs, prefs_path) = load_prefs();
    let Some(path) = book_path() else {
        // No Book given: the Shelf. A headless run's Shelf is an empty temporary folder.
        let scratch = rune::is_headless().then(|| tempfile::tempdir().expect("a temporary folder"));
        let mut state = State::new(Library::blank());
        state.home = Some(Home::load(scratch.as_ref().map(|d| d.path())));
        state.scratch = scratch;
        state.with_prefs(prefs, prefs_path);
        // The Book open when the app last closed opens again, where reading stopped.
        if let Some(last) = state.prefs.last_book.clone().filter(|p| p.exists())
            && state.prefs.reopen_last
        {
            state.reopen(&last);
        }
        // A first run, with nothing on the Shelf, starts at the welcome. Someone who already
        // has Books has started.
        if state
            .home
            .as_ref()
            .is_some_and(|h| !h.shelf.books.is_empty())
        {
            state.prefs.onboarded = true;
        } else if state.home.is_some() && !state.prefs.onboarded && state.prefs_path.is_some() {
            state.show_welcome();
        }
        return state;
    };
    let (path, scratch) = if rune::is_headless() {
        scratch_copy(Path::new(&path))
    } else {
        (PathBuf::from(&path), None)
    };
    let mut lib = match Library::open_quick(&path) {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("cipher: cannot open {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    if let Some(n) = std::env::var("CIPHER_CHAPTER")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        && let Some(i) = lib.chapters.iter().position(|c| c.number == n)
    {
        let _ = lib.open_chapter(i);
    }
    // Snapshots and scripts see a Book whole, as the app does a moment after opening.
    if rune::is_headless() {
        // An old import's Pairs are re-paired first, as the app does in the background.
        if lib.realign()().is_ok_and(|n| n > 0) {
            let _ = lib.reload();
        }
        if let Ok(scan) = lib.scan()() {
            lib.apply_scan(scan);
        }
    }
    // `State::new` puts the current Chapter and its neighbours on the page.
    let mut state = State::new(lib);
    state.scratch = scratch;
    state.with_prefs(prefs, prefs_path);
    state
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    view::view(s, cx)
}

/// A headless run (a scene, a script, a tree) works on a copy of the Book, so nothing it does
/// can change the real one: the file and its write-ahead log go into a temporary folder.
/// `CIPHER_SCRATCH=DIR` puts the copy in DIR and keeps it, to inspect after the run.
fn scratch_copy(path: &Path) -> (PathBuf, Option<tempfile::TempDir>) {
    let kept = std::env::var_os("CIPHER_SCRATCH").map(PathBuf::from);
    let dir = match &kept {
        Some(d) => tempfile::Builder::new()
            .prefix("run-")
            .tempdir_in(d)
            .expect("a folder in CIPHER_SCRATCH"),
        None => tempfile::tempdir().expect("a temporary folder"),
    };
    let name = path.file_name().expect("a Book file name");
    let to = dir.path().join(name);
    std::fs::copy(path, &to).unwrap_or_else(|e| {
        eprintln!("cipher: cannot copy {}: {e}", path.display());
        std::process::exit(1);
    });
    let wal = |p: &Path| {
        let mut w = p.as_os_str().to_owned();
        w.push("-wal");
        PathBuf::from(w)
    };
    if wal(path).exists() {
        let _ = std::fs::copy(wal(path), wal(&to));
    }
    if kept.is_some() {
        eprintln!("cipher: working on {}", to.display());
        let _ = dir.keep();
        return (to, None);
    }
    (to, Some(dir))
}

/// Whether this run uses the real Endpoints and runs the worker: always in a window; in a
/// headless run only with `CIPHER_LIVE=1`, for driving the app against a real model. The Book
/// is still a scratch copy, and nothing is written to `endpoints.json` or the keychain except
/// a ChatGPT session's own token refresh.
pub(crate) fn live() -> bool {
    !rune::is_headless() || std::env::var("CIPHER_LIVE").is_ok_and(|v| v == "1")
}

fn setup(s: &mut State, waker: Waker) {
    s.bg.set_waker(waker);
    if live() && s.lib.is_saved() {
        let path = s.lib.path().to_path_buf();
        s.bg.restart(&path);
    }
}

fn poll(s: &mut State, _t: f64, tx: &mut Tx) -> bool {
    let scrolled = match s.pending_scroll.take() {
        Some((key, y)) => {
            tx.scroll_key_to(key, y);
            true
        }
        None => false,
    };
    if let Some(title) = s.pending_title.take() {
        tx.set_title(title);
    }
    let settled = s.settle_strip(tx);
    s.start_scan();
    let changed = s.drain(tx) || scrolled || settled;
    let now = tx.clock();
    let changed = s.settle_toast(now, tx) || changed;
    s.save_prefs();
    changed
}

/// Inter for the interface and Noto Serif for book text, in the binary so every platform reads
/// the same (`assets/fonts/README.md`). CJK text falls back to the system's fonts.
const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/NotoSerif-VF.ttf"),
    include_bytes!("../assets/fonts/NotoSerif-Italic-VF.ttf"),
];

/// The desktop entry's name, the Wayland app id and the single-instance name.
pub const APP_ID: &str = "app.cipher.Cipher";

fn main() {
    let headless = rune::is_headless();
    if !headless && !rune::single_instance(APP_ID) {
        // Cipher is already running: the launch went to it.
        return;
    }
    rune::set_app_id(APP_ID);
    let (prefs, _) = load_prefs();
    rune::App::new(AppSpec {
        on_key: Some(on_key),
        setup: Some(setup),
        poll: Some(poll),
        scenes: scenes::SCENES,
        min_size: Some(prefs::WindowState::MIN),
        fonts: FONTS,
        ..AppSpec::new("Cipher", prefs.window.size(), init, view)
    })
    .maximized(prefs.window.maximized && !headless)
    .on_close(|s: &mut State, _| s.on_close())
    .on_open_url(|s: &mut State, url: &str, _: &mut Tx| {
        if let Some(path) = book_arg(url) {
            s.open_from_desktop(std::path::Path::new(&path));
        }
    })
    .run();
    rune::release_instance();
}
