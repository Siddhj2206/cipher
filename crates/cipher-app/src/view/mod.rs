//! The views: each takes the state and a palette and returns Rune elements. Nothing in here
//! reads the Book.

mod cast;
mod contents;
mod context;
mod diff;
mod draft;
mod drift;
mod fix;
mod header;
mod notes;
mod page;
mod palette;
mod preferences;
mod review;
mod sheets;
mod shelf;
mod sidebar;
mod style;
mod welcome;
mod widgets;

use rune::{Cx, El, col, row};

use crate::sidebars::Sidebar;
use crate::state::{Screen, State};
use crate::theme::pal;
pub use widgets::{ago, plural, thousands};

/// The whole window: the screen showing, at the chosen zoom. It notes the desktop's light or
/// dark and the window's size for the State, which saves the size when the window closes.
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    s.system_dark.set(cx.system_dark());
    cx.zoom(s.prefs.zoom);
    if let Some([_, _, w, h]) = cx.layout("app-root") {
        s.window_size.set((w * s.prefs.zoom, h * s.prefs.zoom));
    }
    let mut root = col()
        .key("app-root")
        .w_full()
        .h_full()
        .child(screen(s, cx))
        .on_drop_files(|s: &mut State, paths: &[std::path::PathBuf], _| s.drop_files(paths))
        .on_drop_hover(|s: &mut State, over: bool, _| s.drop_hover = over)
        // Ctrl+V with no text field focused: a Chapter to add.
        .on_paste(|s: &mut State, text: &str, _| s.open_draft(text));
    if s.drop_hover {
        let p = pal(s.dark());
        root = root.child(
            col()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .fill(p.scrim)
                .items_center()
                .justify_center()
                .child(
                    rune::text("Drop a book to open it, or a folder of chapters to import")
                        .font(crate::theme::UI_MED)
                        .color(p.ink)
                        .px(18.0)
                        .py(12.0)
                        .radius(8.0)
                        .fill(p.card)
                        .border(1.0, p.rule),
                ),
        );
    }
    root
}

fn screen(s: &State, cx: &mut Cx<State>) -> El {
    let p = pal(s.dark());
    if let Some(h) = &s.home {
        let mut root = col().w_full().h_full().fill(p.paper).child(
            if s.welcome.is_some() {
                welcome::welcome(s, h, &p)
            } else {
                shelf::shelf(s, h, &p)
            }
            // Pages change in place; only menus and sheets fade.
            .instant_changes(),
        );
        for layer in [
            review::toast(s, &p),
            shelf::prompt_sheet(h, &p),
            preferences::settings(s, &p),
            preferences::shortcuts_sheet(s, &p),
        ]
        .into_iter()
        .flatten()
        {
            root = root.child(layer);
        }
        return root;
    }
    let content = match s.screen {
        Screen::Cast => cast::cast(s, &p),
        Screen::Draft => draft::draft_page(s, &p),
        Screen::Style => style::style_page(s, &p),
        Screen::Reading => page::reading(s, cx, &p),
    };
    let panes = s.sidebar_layout();
    let mut middle = row().w_full().grow(1.0).min_h(0.0);
    if panes.contents.is_none() {
        middle = middle.child(contents::scrubber(s, &p));
    }
    middle = middle.child(
        col()
            .key("page-slot")
            .grow(1.0)
            .h_full()
            .min_w(0.0)
            .child(content),
    );
    let main = col()
        .key("book-main")
        .grow(1.0)
        .h_full()
        .min_w(0.0)
        .fill(p.paper)
        .child(header::header(s, &p))
        .child(middle);

    let mut shell = row().w_full().h_full();
    if let Some(width) = panes.contents {
        shell = shell.child(contents::contents(s, &p, width));
    }
    shell = shell.child(main);
    // The new-Chapter page has no Terms or Suggestions yet: it gets the room.
    if let Some(width) = panes.context {
        shell = shell.child(context::context(s, &p, width));
    }
    for side in [Sidebar::Contents, Sidebar::Context] {
        if let Some(grip) = sidebar::grip(s, &p, side, panes.width(side)) {
            shell = shell.child(grip);
        }
    }

    // Pages change in place: switching screens, Chapters or panels never fades or slides.
    // Menus, sheets and toasts above keep their fades.
    let mut root = col()
        .w_full()
        .h_full()
        .fill(p.paper)
        .child(shell.instant_changes());
    for layer in [
        fix::chip_bar(s, &p),
        fix::composer(s, &p),
        review::bar(s, &p),
        review::toast(s, &p),
    ]
    .into_iter()
    .flatten()
    {
        root = root.child(layer);
    }
    for sheet in [
        preferences::type_popover(s, &p),
        contents::chapter_menu(s, &p),
        header::model_menu(s, &p),
        palette::palette_sheet(s, &p),
        preferences::settings(s, &p),
        preferences::shortcuts_sheet(s, &p),
    ]
    .into_iter()
    .flatten()
    {
        root = root.child(sheet);
    }
    root
}
