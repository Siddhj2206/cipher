//! The Contents sidebar (left), and the scrubber that replaces it when it is closed.

use cipher_book::ChapterState;
use rune::tok::TextStyle;
use rune::{Color, Cursor, El, Icon, col, el, icon, row, text};

use super::widgets::{dot, icon_btn, plural, ring, scrolling, section};
use crate::library::ChapterRow;
use crate::state::{Screen, State};
use crate::theme::{
    CJK, HEADER_H, I_BOOK, I_LEFT_PANEL, I_PEN, I_PLUS, I_SHELF, I_USERS, LEFT_W, Pal, SCRUBBER_W,
    SERIF, SMALL, UI,
};

/// The state badge at the right of a Chapter row.
fn badge(ch: &ChapterRow, p: &Pal) -> El {
    let (state, outdated, edits) = (ch.state, ch.outdated, ch.pending_edits);
    let mut right = row().gap(6.0).center_y();
    if edits > 0 {
        right = right.child(
            text(edits.to_string())
                .font(TextStyle::new(10.5, 600.0))
                .color(p.on_red)
                .px(5.0)
                .radius(7.0)
                .fill(p.red)
                .help(format!("{edits} suggested edits waiting for a decision")),
        );
    }
    if outdated > 0 {
        right = right.child(
            text(outdated.to_string())
                .font(TextStyle::new(10.5, 600.0))
                .color(p.on_red)
                .px(5.0)
                .radius(7.0)
                .fill(p.red)
                .help(format!(
                    "{outdated} outdated: the source changed after translation"
                )),
        );
    }
    match state {
        ChapterState::Translated => right,
        ChapterState::Empty => right.child(
            text("empty")
                .font(TextStyle::new(11.0, 400.0))
                .color(p.ink4)
                .help("This chapter has no text"),
        ),
        ChapterState::Waiting => right.child(ring(p.amber, 7.0).help("Waiting to translate")),
        ChapterState::Queued => right.child(dot(p.amber, 7.0).help("Queued to translate")),
        ChapterState::Translating => right
            .child(text("translating").font(SMALL).color(p.amber))
            .child(dot(p.amber, 7.0)),
        ChapterState::Failed => right.child(dot(p.red, 7.0).help(match &ch.failure {
            Some(why) => format!("Translation failed: {why}. Open it to retry."),
            None => "Translation failed. Open it to retry.".to_string(),
        })),
    }
}

/// One Chapter in the list. Its handlers find the Chapter by id, since rows move.
fn chapter_row(s: &State, i: usize, p: &Pal) -> El {
    let lib = &s.lib;
    let Some(ch) = lib.chapters.get(i) else {
        return el();
    };
    let on = i == lib.current() && s.screen == Screen::Reading;
    let untranslated = ch.untranslated();
    let id = ch.id;
    let index = move |s: &State| s.lib.chapters.iter().position(|c| c.id == id);
    row()
        .w_full()
        .py(7.0)
        .pl(10.0)
        .pr(10.0)
        .gap(12.0)
        .radius(6.0)
        .cursor(Cursor::Pointer)
        .fill(if on { p.selected } else { Color::TRANSPARENT })
        .hover(p.hover)
        .child(
            text(ch.number.to_string())
                .font(TextStyle::new(13.0, if on { 600.0 } else { 450.0 }).features("tnum"))
                .color(if on { p.red } else { p.ink3 })
                .w(28.0)
                .pt(1.0),
        )
        .child(
            col().grow(1.0).min_w(0.0).gap(1.0).child(
                text(ch.label())
                    .font(
                        TextStyle::new(12.5, 400.0)
                            .lh(17.0)
                            .family(if untranslated { CJK } else { SERIF }),
                    )
                    .color(if on {
                        p.ink
                    } else if untranslated || ch.opening.is_empty() {
                        p.ink3
                    } else {
                        p.ink2
                    })
                    .wrap()
                    .max_lines(2),
            ),
        )
        .child(badge(ch, p))
        .on_press_tx(move |s: &mut State, tx| {
            if let Some(i) = index(s) {
                s.go(tx, i)
            }
        })
        .on_secondary_press(move |s: &mut State, at: rune::PointerPress, _| {
            if let Some(i) = index(s) {
                s.chapter_menu = Some((i, at.x, at.y))
            }
        })
}

pub fn contents(s: &State, p: &Pal) -> El {
    let lib = &s.lib;
    let total = lib.chapters.len();
    let done = lib.translated();
    let empty = lib.empty();
    let waiting = total - done - empty;
    let drift_n = lib.drifting_terms();
    let cur = lib.current();

    let header = row()
        .h(HEADER_H)
        .w_full()
        .pl(8.0)
        .pr(10.0)
        .center_y()
        .gap(6.0)
        .child(
            icon_btn(&I_SHELF, "to-shelf", false, p)
                .help("All books (Ctrl+O)")
                .on_press(|s: &mut State| s.go_shelf()),
        )
        .child(
            text(lib.meta.title.clone())
                .font(TextStyle::new(16.0, 500.0).italic().family(SERIF))
                .color(p.ink)
                .grow(1.0)
                .truncate(),
        )
        .child(
            icon_btn(&I_LEFT_PANEL, "hide-left", false, p)
                .help("Hide contents (Ctrl+\\)")
                .on_press(|s: &mut State| s.toggle_contents()),
        );

    // Progress: translated vs waiting, as one hairline.
    let frac = done as f32 / total.max(1) as f32;
    let progress = col()
        .w_full()
        .px(18.0)
        .pb(14.0)
        .gap(7.0)
        .child(
            row()
                .w_full()
                .h(3.0)
                .radius(2.0)
                .fill(p.rule)
                .child(
                    el().h_full()
                        .w(frac * (LEFT_W - 36.0))
                        .radius(2.0)
                        .fill(p.ink3),
                )
                .child(el().h_full().grow(1.0).radius(2.0).fill(if waiting > 0 {
                    p.amber
                } else {
                    p.rule
                })),
        )
        .child(
            text(if empty > 0 {
                format!("{done} translated · {waiting} waiting · {empty} empty")
            } else {
                format!(
                    "{done} translated · {waiting} waiting · {}",
                    plural(total, "chapter")
                )
            })
            .font(SMALL)
            .color(p.ink3),
        );

    let nav = |i: &'static Icon, label: &str, extra: String, key: &str, on: bool| {
        row()
            .key(key.to_string())
            .h(32.0)
            .mx(8.0)
            .px(10.0)
            .gap(10.0)
            .center_y()
            .radius(6.0)
            .fill(if on { p.selected } else { Color::TRANSPARENT })
            .hover(p.hover)
            .cursor(Cursor::Pointer)
            .child(icon(i, 15.0).color(if on { p.ink } else { p.ink3 }))
            .child(
                text(label.to_string())
                    .font(UI)
                    .color(if on { p.ink } else { p.ink2 })
                    .grow(1.0),
            )
            .child(text(extra).font(SMALL).color(p.ink3))
    };
    let navs = col()
        .w_full()
        .gap(1.0)
        .pb(6.0)
        .child(
            nav(
                &I_BOOK,
                "Reading",
                String::new(),
                "nav-read",
                s.screen == Screen::Reading,
            )
            .on_press_tx(|s: &mut State, tx| {
                if s.screen == Screen::Draft {
                    s.leave_draft(tx, false)
                } else {
                    s.screen = Screen::Reading
                }
            }),
        )
        .child(
            nav(
                &I_USERS,
                "Cast & terms",
                if drift_n > 0 {
                    format!("{drift_n} drifting")
                } else {
                    String::new()
                },
                "nav-cast",
                s.screen == Screen::Cast,
            )
            .on_press(|s: &mut State| s.screen = Screen::Cast),
        )
        .child(
            nav(
                &I_PEN,
                "Style",
                String::new(),
                "nav-style",
                s.screen == Screen::Style,
            )
            .help("How this book should read: sent with every chapter")
            .on_press(|s: &mut State| s.open_style()),
        )
        .child(
            nav(
                &I_PLUS,
                "New chapter",
                "Ctrl N".into(),
                "nav-add",
                s.screen == Screen::Draft,
            )
            .help("A page for the next chapter: paste or type it there")
            .on_press_tx(|s: &mut State, tx| s.new_chapter(tx)),
        );

    // Only the rows in view are built, so a Book of thousands of Chapters scrolls like ten.
    let rows = col()
        .key("chapter-rows")
        .w_full()
        .gap(1.0)
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            lib.chapters.len(),
            48.0,
            |_: &State, i| format!("ch{i}").into(),
            |s: &State, i| chapter_row(s, i, &crate::theme::pal(s.dark())),
        )
        .virtual_rows_version((
            lib.generation(),
            lib.chapters.len(),
            cur,
            s.screen == Screen::Reading,
            lib.scanned(),
            s.dark(),
        ));
    let mut list = col().w_full().px(8.0).pb(16.0).gap(1.0).child(rows);
    // A draft waiting to be added keeps its place at the end of the list.
    if s.draft.is_some() {
        let on = s.screen == Screen::Draft;
        let n = s
            .draft_intake()
            .and_then(|i| i.number)
            .unwrap_or_else(|| s.next_number());
        let label = format!("Chapter {n}");
        list = list.child(
            row()
                .key("ch-draft")
                .w_full()
                .py(7.0)
                .px(10.0)
                .gap(12.0)
                .radius(6.0)
                .cursor(Cursor::Pointer)
                .fill(if on { p.selected } else { Color::TRANSPARENT })
                .hover(p.hover)
                .child(icon(&I_PLUS, 13.0).color(p.red).w(28.0))
                .child(
                    text(label)
                        .font(TextStyle::new(12.5, 500.0))
                        .color(p.ink)
                        .grow(1.0),
                )
                .child(
                    text("draft")
                        .font(TextStyle::new(11.0, 400.0))
                        .color(p.ink3),
                )
                .on_press(|s: &mut State| s.open_draft("")),
        );
    }

    col()
        .key("contents")
        .w(LEFT_W)
        .h_full()
        .fill(p.side)
        .border_sides([0.0, 1.0, 0.0, 0.0], p.rule)
        .child(header)
        .child(progress)
        .child(navs)
        .child(section("CHAPTERS", String::new(), p).pt(10.0))
        .child(scrolling(list, "contents-list"))
}

/// The most ticks the scrubber draws.
const MAX_TICKS: usize = 160;

/// With the contents closed, the window edge keeps a scrubber: one tick per Chapter, a longer
/// tick and label every ten. The current Chapter is red, earlier ones ink3, Chapters still
/// waiting amber, failed ones soft red.
pub fn scrubber(s: &State, p: &Pal) -> El {
    let cur = s.lib.current();
    let mut bar = col().w(SCRUBBER_W).h_full().py(14.0);
    // A long Book shows every `step`th Chapter, and always the current one.
    let step = s.lib.chapters.len().div_ceil(MAX_TICKS).max(1);
    let labelled = 10 * step;
    for (i, ch) in s.lib.chapters.iter().enumerate() {
        let is_cur = i == cur;
        if !is_cur && i % step != 0 {
            continue;
        }
        let tens = ch.number % labelled as i64 == 0;
        let color = if is_cur {
            p.red
        } else if ch.state == ChapterState::Failed {
            p.red_soft
        } else if ch.state == ChapterState::Empty {
            p.ink4
        } else if ch.untranslated() {
            p.amber
        } else if i < cur {
            p.ink3
        } else {
            p.ink4
        };
        let w = if is_cur {
            20.0
        } else if tens {
            12.0
        } else {
            7.0
        };
        let label = if is_cur || tens {
            text(ch.number.to_string())
                .font(TextStyle::new(9.5, if is_cur { 650.0 } else { 400.0 }))
                .color(if is_cur { p.red } else { p.ink4 })
        } else {
            el()
        };
        let note = match ch.state {
            ChapterState::Translated => String::new(),
            ChapterState::Waiting => " · waiting to translate".into(),
            ChapterState::Empty => " · empty".into(),
            ChapterState::Queued => " · queued".into(),
            ChapterState::Translating => " · translating".into(),
            ChapterState::Failed => " · translation failed".into(),
        };
        bar = bar.child(
            row()
                .key(format!("tick{i}"))
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .center_y()
                .justify_start()
                .gap(4.0)
                .pl(10.0)
                .cursor(Cursor::Pointer)
                .hover(p.hover)
                .help(format!("Chapter {}{note}", ch.number))
                .child(el().w(w).h(if is_cur { 2.0 } else { 1.5 }).fill(color))
                .child(label)
                .on_press_tx(move |s: &mut State, tx| s.go(tx, i)),
        );
    }
    bar
}

/// A Chapter's right-click menu, where the pointer was.
pub fn chapter_menu(s: &State, p: &Pal) -> Option<El> {
    let (i, x, y) = s.chapter_menu?;
    let ch = s.lib.chapters.get(i)?;
    let item = |label: &str, key: &str| {
        text(label.to_string())
            .key(key.to_string())
            .font(crate::theme::UI)
            .color(p.ink)
            .w_full()
            .px(10.0)
            .py(6.0)
            .radius(5.0)
            .hover(p.hover)
            .cursor(Cursor::Pointer)
    };
    let mut menu = super::widgets::card(p)
        .key("chapter-menu")
        .w(230.0)
        .padding(4.0)
        .child(
            item(&format!("Open chapter {}", ch.number), "cm-open").on_press_tx(
                move |s: &mut State, tx| {
                    s.chapter_menu = None;
                    s.go(tx, i)
                },
            ),
        );
    let later = s.lib.chapters[i + 1..]
        .iter()
        .filter(|c| c.state == ChapterState::Waiting)
        .count();
    match ch.state {
        ChapterState::Waiting => {
            menu = menu.child(
                item("Translate", "cm-translate").on_press(move |s: &mut State| {
                    s.chapter_menu = None;
                    s.translate_chapter(i)
                }),
            );
            if later > 0 {
                menu = menu.child(
                    item(
                        &format!("Translate all {} from here", later + 1),
                        "cm-translate-from",
                    )
                    .on_press(move |s: &mut State| {
                        s.chapter_menu = None;
                        s.translate_from(i)
                    }),
                );
            }
        }
        ChapterState::Translated => {
            menu = menu.child(
                item("Translate again", "cm-retranslate")
                    .help("Replaces this chapter's translation with a new one from the model.")
                    .on_press(move |s: &mut State| {
                        s.chapter_menu = None;
                        s.translate_chapter(i)
                    }),
            )
        }
        ChapterState::Failed => {
            menu = menu.child(item("Retry", "cm-retry").on_press(move |s: &mut State| {
                s.chapter_menu = None;
                s.retry_chapter(i)
            }))
        }
        _ => {}
    }
    Some(
        menu.pin(Some(x / s.prefs.zoom), Some(y / s.prefs.zoom), None, None)
            .modal(true, |s: &mut State| s.chapter_menu = None),
    )
}
