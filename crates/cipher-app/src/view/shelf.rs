//! The Shelf: every Book as a cloth hardcover standing face-out on a quiet ledge, with a small
//! label under each. The last slot is an empty place for a new Book.
//!
//! The realism is held back on purpose: cloth colour, a hinge crease, soft light, a foil title
//! in a blind-stamped frame, and two shadows. No textures, no wood, no gloss.

use std::f32::consts::PI;

use rune::tok::TextStyle;
use rune::{
    Align, Color, Cursor, El, Key, Shadow, TextEdit, col, el, icon, oklch, row, text, text_input,
};

use super::widgets::{btn, card, icon_btn, plural, segmented, tool_btn};
use crate::home::{Home, LANGUAGES, NewBook, Prompt};
use crate::prefs::ShelfSort;
use crate::shelf::ShelfBook;
use crate::state::{Section, State};
use crate::theme::{
    CJK, HEADER_H, I_FILE, I_FOLDER, I_GEAR, I_MOON, I_PLUG, I_PLUS, I_SUN, I_X, LABEL, Pal, SERIF,
    SMALL, UI,
};

const COVER_W: f32 = 148.0;
const COVER_H: f32 = 212.0;
const SLOT_W: f32 = 176.0;
const PER_ROW: usize = 5;
const SHELF_W: f32 = SLOT_W * PER_ROW as f32;

fn home(s: &mut State) -> &mut Home {
    s.home.as_mut().expect("the Shelf is showing")
}

fn black(a: f32) -> Color {
    Color::BLACK.alpha(a)
}

fn white(a: f32) -> Color {
    Color::WHITE.alpha(a)
}

/// `t` of the way from `a` to `b` as the eye sees it (Oklab), unlike [`Color::mix`], which
/// mixes light and makes a little white look like a lot.
fn toward(a: Color, b: Color, t: f32) -> Color {
    let (x, y) = (a.to_oklab(), b.to_oklab());
    Color::from_oklab(std::array::from_fn(|i| x[i] + (y[i] - x[i]) * t))
}

/// Book-cloth colours: deep, a little dusty, never bright. A Book keeps its colour because it
/// comes from its file name.
fn cloth(b: &ShelfBook, dark: bool) -> Color {
    let name = b
        .path
        .file_name()
        .map(|n| n.as_encoded_bytes().to_vec())
        .unwrap_or_default();
    cloth_named(&name, dark)
}

/// The cloth for a Book file named `name`, so a new Book's cover shows the colour it will have.
fn cloth_named(name: &[u8], dark: bool) -> Color {
    // (lightness, chroma, hue) in OKLCH.
    const CLOTHS: [(f32, f32, f32); 8] = [
        (0.38, 0.085, 28.0),  // oxblood
        (0.37, 0.045, 155.0), // forest
        (0.36, 0.050, 250.0), // navy
        (0.52, 0.090, 70.0),  // ochre
        (0.35, 0.050, 330.0), // plum
        (0.40, 0.020, 240.0), // slate
        (0.40, 0.045, 195.0), // teal
        (0.40, 0.040, 55.0),  // umber
    ];
    let hash = name.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    let (l, c, h) = CLOTHS[(hash % CLOTHS.len() as u64) as usize];
    if dark {
        oklch(l - 0.08, c * 0.8, h)
    } else {
        oklch(l, c, h)
    }
}

fn foil(dark: bool) -> Color {
    if dark {
        oklch(0.80, 0.045, 85.0)
    } else {
        oklch(0.88, 0.050, 85.0)
    }
}

/// The mark stamped at a cover's foot for its Source Language.
fn language_mark(code: &str) -> &'static str {
    match code {
        "ko" => "한",
        "ja" => "日",
        "zh" => "中",
        _ => "",
    }
}

fn language(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or_else(|| code.to_uppercase(), |(_, name)| name.to_string())
}

/// The cloth board every cover is made of: crease, light, blind-stamped frame, two shadows.
fn board(color: Color, s: &State) -> El {
    let gold = foil(s.dark());
    let hinge = 12.0 / COVER_W;
    // Left to right: the spine's roll, the hinge crease, the board, a darker fore-edge.
    let crease = [
        (0.0, black(0.34)),
        (hinge * 0.55, black(0.12)),
        (hinge * 0.95, white(0.10)),
        (hinge * 1.10, black(0.16)),
        (hinge * 1.45, black(0.0)),
        (0.93, black(0.0)),
        (1.0, black(0.18)),
    ];
    // Light from above: a faint lift at the head, falling off to the tail.
    let light = [(0.0, white(0.08)), (0.35, white(0.0)), (1.0, black(0.16))];
    el().size(COVER_W, COVER_H)
        .radius(3.0)
        .fill(color)
        .linear_gradient(PI / 2.0, crease)
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: black(if s.dark() { 0.55 } else { 0.28 }),
            inset: false,
        })
        .box_shadow(Shadow {
            x: 2.0,
            y: 10.0,
            blur: 22.0,
            spread: -6.0,
            color: black(if s.dark() { 0.5 } else { 0.26 }),
            inset: false,
        })
        .child(
            el().absolute(0.0, 0.0)
                .size(COVER_W, COVER_H)
                .radius(3.0)
                .linear_gradient(PI, light),
        )
        .child(
            el().absolute(22.0, 12.0)
                .size(COVER_W - 34.0, COVER_H - 24.0)
                .radius(1.0)
                .border(1.0, gold.alpha(0.28)),
        )
}

/// The Source Language stamped small at a cover's foot.
fn foot_mark(code: &str, s: &State) -> El {
    text(language_mark(code))
        .font(TextStyle::new(13.0, 500.0).family(CJK))
        .color(foil(s.dark()).alpha(0.55))
        .absolute(COVER_W / 2.0 + 3.0, COVER_H - 34.0)
}

const COVER_TITLE: TextStyle = TextStyle::new(15.0, 500.0).lh(20.0).family(SERIF);

/// A closed hardcover, seen from the front.
fn cover(b: &ShelfBook, i: usize, s: &State, p: &Pal) -> El {
    let gold = foil(s.dark());
    let title = col()
        .absolute(26.0, 34.0)
        .w(COVER_W - 42.0)
        .items_center()
        .gap(10.0)
        .child(
            text(b.title.clone())
                .font(COVER_TITLE)
                .color(gold)
                .wrap()
                .align(Align::Center),
        )
        .child(el().size(18.0, 1.0).fill(gold.alpha(0.55)));

    let path = b.path.clone();
    let menu_path = b.path.clone();
    let mut cover = board(cloth(b, s.dark()), s)
        .key(format!("cover{i}"))
        .on_secondary_press(move |s: &mut State, _, _| s.toggle_book_menu(menu_path.clone()))
        .child(title)
        .child(foot_mark(&b.source_lang, s))
        .child(menu_button(b, i, p));
    if b.problem.is_none() {
        cover = cover
            .cursor(Cursor::Pointer)
            .press_scale(0.985)
            .on_press(move |s: &mut State| s.open_book(&path));
    }
    cover.hover_group()
}

/// A new Book in the empty slot: a blank cover in the cloth its file will get, the title typed
/// onto it in foil, and its language stamped at the foot.
pub(super) fn blank_cover(h: &Home, form: &NewBook, s: &State) -> El {
    let gold = foil(s.dark());
    let name = h
        .shelf
        .new_path(if form.title.text.trim().is_empty() {
            "Untitled"
        } else {
            &form.title.text
        })
        .file_name()
        .map(|n| n.as_encoded_bytes().to_vec())
        .unwrap_or_default();
    let title = text_input(&form.title, COVER_TITLE)
        .key("new-title")
        .absolute(26.0, 34.0)
        .w(COVER_W - 42.0)
        .align(Align::Center)
        .color(gold)
        .caret(gold, gold.alpha(0.3))
        .on_edit(|s: &mut State| &mut home(s).new_book.as_mut().expect("open").title)
        .on_input_key(|s: &mut State, k, _| match &k.key {
            Key::Enter if !k.mods.shift => {
                s.create_book();
                true
            }
            Key::Escape => s.shelf_escape(),
            _ => false,
        });
    let mut cover = board(cloth_named(&name, s.dark()), s).key("new-cover");
    // A hint under the caret's line, so the caret never sits inside the word.
    if form.title.text.is_empty() {
        cover = cover.child(
            text("type a title")
                .font(TextStyle::new(12.0, 400.0).italic().family(SERIF))
                .color(gold.alpha(0.45))
                .absolute(26.0, 60.0)
                .w(COVER_W - 42.0)
                .align(Align::Center),
        );
    }
    cover.child(title).child(foot_mark(form.lang, s))
}

/// Under the blank cover: the language as three stamps, and Create.
pub(super) fn new_book_label(form: &NewBook, cancel: bool, s: &State, p: &Pal) -> El {
    let mut stamps = row().gap(6.0);
    for (i, &(code, name)) in LANGUAGES.iter().enumerate() {
        let on = form.lang == code;
        stamps = stamps.child(
            col()
                .key(format!("lang{i}"))
                .w(46.0)
                .py(5.0)
                .gap(2.0)
                .items_center()
                .radius(5.0)
                .border(1.0, if on { p.ink2 } else { p.rule })
                .fill(if on { p.card } else { Color::TRANSPARENT })
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .help(format!("Written in {name}"))
                .child(
                    text(language_mark(code))
                        .font(TextStyle::new(15.0, 500.0).family(CJK))
                        .color(if on { p.ink } else { p.ink3 }),
                )
                .child(
                    text(name.to_string())
                        .font(TextStyle::new(9.0, 500.0))
                        .color(if on { p.ink2 } else { p.ink4 }),
                )
                .on_press(move |s: &mut State| {
                    if let Some(f) = &mut home(s).new_book {
                        f.lang = code;
                    }
                }),
        );
    }
    let _ = s;
    col()
        .key("new-label")
        .w(COVER_W)
        .pt(12.0)
        .gap(10.0)
        .child(text("WRITTEN IN").font(LABEL).color(p.ink3))
        .child(stamps)
        .child({
            let mut r = row().gap(6.0).child(
                btn("Create", true, p)
                    .key("new-create")
                    .on_press(|s: &mut State| s.create_book()),
            );
            if cancel {
                r = r.child(
                    btn("Cancel", false, p)
                        .key("new-cancel")
                        .on_press(|s: &mut State| home(s).new_book = None),
                );
            }
            r
        })
}

/// "⋯" at a cover's top right, shown on hover.
fn menu_button(b: &ShelfBook, i: usize, p: &Pal) -> El {
    let path = b.path.clone();
    text("⋯")
        .key(format!("menu{i}"))
        .font(TextStyle::new(15.0, 700.0))
        .color(p.ink)
        .absolute(COVER_W - 30.0, 8.0)
        .size(22.0, 22.0)
        .align(Align::Center)
        .radius(11.0)
        .fill(p.card.alpha(0.9))
        .cursor(Cursor::Pointer)
        .help("More (or right-click the cover)")
        .group_opacity(0.0, 1.0)
        .on_press(move |s: &mut State| s.toggle_book_menu(path.clone()))
}

/// The small card under a cover: counts and a progress hairline, or a form in its place.
fn label(h: &Home, b: &ShelfBook, i: usize, p: &Pal) -> El {
    let mut c = col().w(COVER_W).pt(12.0).gap(6.0);
    if let Some((_, edit)) = h.renaming.as_ref().filter(|(rp, _)| *rp == b.path) {
        return c
            .child(field(
                edit,
                "rename-input",
                "Title",
                |s| &mut home(s).renaming.as_mut().expect("open").1,
                |s, _| s.finish_rename(),
                p,
            ))
            .child(
                row()
                    .gap(6.0)
                    .child(
                        btn("Cancel", false, p)
                            .key("rename-cancel")
                            .on_press(|s: &mut State| home(s).renaming = None),
                    )
                    .child(
                        btn("Save", true, p)
                            .key("rename-save")
                            .on_press(|s: &mut State| s.finish_rename()),
                    ),
            );
    }
    if h.confirm.as_ref() == Some(&b.path) {
        let (ask, act) = if b.elsewhere {
            ("Remove from the shelf? The file stays.", "Remove")
        } else {
            ("Move to the trash?", "Move to Trash")
        };
        return c.child(text(ask).font(SMALL).color(p.ink2).wrap()).child(
            row()
                .gap(6.0)
                .child(
                    btn("Keep", false, p)
                        .key(format!("keep{i}"))
                        .on_press(|s: &mut State| home(s).confirm = None),
                )
                .child(
                    btn(act, true, p)
                        .key(format!("remove{i}"))
                        .on_press(|s: &mut State| s.remove_book()),
                ),
        );
    }
    if h.menu.as_ref() == Some(&b.path) {
        let (rp, dp, title) = (b.path.clone(), b.path.clone(), b.title.clone());
        let (op, fp) = (b.path.clone(), b.path.clone());
        let item = |label: &str, key: String| {
            text(label.to_string())
                .key(key)
                .font(UI)
                .color(p.ink)
                .w_full()
                .px(10.0)
                .py(6.0)
                .radius(5.0)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
        };
        return c.child(
            card(p)
                .w_full()
                .padding(4.0)
                .child(
                    item("Open", format!("open{i}"))
                        .on_press(move |s: &mut State| s.open_book(&op)),
                )
                .child(
                    item("Show in Files", format!("reveal{i}"))
                        .on_press_tx(move |s: &mut State, tx| s.reveal_book(tx, &fp)),
                )
                .child(
                    item("Rename…", format!("rename{i}"))
                        .on_press(move |s: &mut State| s.start_rename(rp.clone(), &title)),
                )
                .child(
                    item(
                        if b.elsewhere {
                            "Remove from shelf"
                        } else {
                            "Move to Trash…"
                        },
                        format!("delete{i}"),
                    )
                    .on_press(move |s: &mut State| s.confirm_remove(dp.clone())),
                ),
        );
    }

    if let Some(problem) = &b.problem {
        return c.child(
            text(format!("Can't be read: {problem}"))
                .font(SMALL)
                .color(p.red)
                .wrap(),
        );
    }
    // Two short lines that fit under the cover: where you are, then what is left.
    let first = match b.at_chapter {
        Some(n) if b.chapters > 0 => format!("Chapter {n} of {}", b.chapters),
        _ if b.chapters == 0 => "No chapters yet".to_string(),
        _ => plural(b.chapters, "chapter"),
    };
    let second = if b.chapters == 0 {
        language(&b.source_lang).to_string()
    } else if b.translated + b.empty >= b.chapters {
        "All translated".to_string()
    } else {
        format!("{} to translate", b.chapters - b.translated - b.empty)
    };
    let frac = b.translated as f32 / b.chapters.max(1) as f32;
    c = c
        .child(text(first).font(SMALL).color(p.ink2).truncate())
        .child(text(second).font(SMALL).color(p.ink3).truncate())
        .child(
            row()
                .w_full()
                .h(2.0)
                .radius(1.0)
                .fill(p.rule)
                .child(el().h_full().w(frac * COVER_W).radius(1.0).fill(p.ink3)),
        );
    if b.elsewhere {
        c = c.child(
            text(b.path.display().to_string())
                .font(TextStyle::new(11.0, 400.0))
                .color(p.ink4)
                .truncate(),
        );
    }
    c
}

/// The empty place at the end of the shelf: where a new Book goes.
fn new_slot(p: &Pal) -> El {
    col()
        .key("new-slot")
        .size(COVER_W, COVER_H)
        .radius(3.0)
        .items_center()
        .justify_center()
        .gap(10.0)
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 6.0,
            spread: 0.0,
            color: black(0.10),
            inset: true,
        })
        .fill(black(0.03))
        .border(1.0, p.rule)
        .hover(p.hover)
        .cursor(Cursor::Pointer)
        .child(icon(&I_PLUS, 18.0).color(p.ink3))
        .child(text("New book").font(UI).color(p.ink3))
        .on_press(|s: &mut State| s.start_new_book())
}

/// The board the books stand on: a lit front edge and the shadow it throws.
fn ledge(s: &State, p: &Pal) -> El {
    let (top, lift, shade) = if s.dark() {
        (toward(p.side, white(1.0), 0.07), 0.06, 0.30)
    } else {
        (toward(p.side, black(1.0), 0.05), 0.35, 0.12)
    };
    el().w(SHELF_W + 40.0)
        .h(12.0)
        .radius(2.0)
        .fill(top)
        .gradient(
            toward(top, white(1.0), lift),
            toward(top, black(1.0), shade),
        )
        .box_shadow(Shadow {
            x: 0.0,
            y: 8.0,
            blur: 16.0,
            spread: -4.0,
            color: black(if s.dark() { 0.5 } else { 0.16 }),
            inset: false,
        })
}

/// A one-line field bound to one of the Shelf's [`TextEdit`]s. Enter runs `submit`.
fn field(
    edit: &TextEdit,
    key: &str,
    placeholder: &str,
    bind: fn(&mut State) -> &mut TextEdit,
    submit: fn(&mut State, &mut rune::Tx),
    p: &Pal,
) -> El {
    text_input(edit, UI)
        .key(key.to_string())
        .w_full()
        .px(10.0)
        .py(7.0)
        .radius(6.0)
        .border(1.0, p.rule)
        .fill(p.paper)
        .single_line()
        .color(p.ink)
        .caret(p.red, p.highlight)
        .placeholder(placeholder.to_string(), p.ink4)
        .on_edit(bind)
        .on_input_key(move |s: &mut State, k, tx| match &k.key {
            Key::Enter => {
                submit(s, tx);
                true
            }
            Key::Escape => s.shelf_escape(),
            _ => false,
        })
}

fn sheet(key: &str, label: &str, body: El, close: fn(&mut State), p: &Pal) -> El {
    col()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(p.scrim)
        .items_center()
        .pt(150.0)
        .child(
            card(p)
                .key(key.to_string())
                .w(460.0)
                .modal(true, close)
                .child(
                    col()
                        .w_full()
                        .padding(22.0)
                        .gap(14.0)
                        .child(text(label.to_string()).font(LABEL).color(p.red))
                        .child(body),
                ),
        )
}

/// New book, as a sheet over the Shelf.
/// Typing a path, when the desktop has no file chooser.
pub fn prompt_sheet(h: &Home, p: &Pal) -> Option<El> {
    let (kind, edit) = h.prompt.as_ref()?;
    let (label, hint, action) = match kind {
        Prompt::Import => (
            "IMPORT A FOLDER",
            "A folder with raw/1.md, raw/2.md, …",
            "Import",
        ),
        Prompt::OpenFile => ("OPEN A BOOK", "A .cipher file", "Open"),
    };
    let body = col()
        .w_full()
        .gap(12.0)
        .child(
            text("This desktop has no file chooser, so type the path.")
                .font(SMALL)
                .color(p.ink3),
        )
        .child(field(
            edit,
            "path-input",
            hint,
            |s| &mut home(s).prompt.as_mut().expect("open").1,
            |s, _| s.submit_path(),
            p,
        ))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(el().grow(1.0))
                .child(
                    btn("Cancel", false, p)
                        .key("path-cancel")
                        .on_press(|s: &mut State| home(s).prompt = None),
                )
                .child(
                    btn(action, true, p)
                        .key("path-go")
                        .on_press(|s: &mut State| s.submit_path()),
                ),
        );
    Some(sheet(
        "path-prompt",
        label,
        body,
        |s| home(s).prompt = None,
        p,
    ))
}

/// The search field in the Shelf's header.
fn search_box(h: &Home, p: &Pal) -> El {
    text_input(&h.search, UI)
        .key("shelf-search")
        .w(200.0)
        .px(10.0)
        .py(5.0)
        .radius(6.0)
        .border(1.0, p.rule)
        .fill(p.card)
        .single_line()
        .color(p.ink)
        .caret(p.red, p.highlight)
        .placeholder("Search books".to_string(), p.ink4)
        .on_edit(|s: &mut State| &mut home(s).search)
}

/// "Continue *Title*, chapter N": the Book read last, one click back into it.
fn continue_line(b: &ShelfBook, p: &Pal) -> El {
    let path = b.path.clone();
    let place = b
        .at_chapter
        .map_or(String::new(), |n| format!("Chapter {n}"));
    // One baseline: the title in the book face, the rest in the UI face sized to sit with it.
    let line = TextStyle::new(14.0, 400.0).lh(22.0);
    let mut r = row()
        .key("continue")
        .gap(10.0)
        .center_y()
        .mb(28.0)
        .px(16.0)
        .py(8.0)
        .radius(8.0)
        .hover(p.hover)
        .cursor(Cursor::Pointer)
        .child(text("Continue").font(line).color(p.ink3))
        .child(
            text(b.title.clone())
                .font(TextStyle::new(17.0, 400.0).lh(22.0).italic().family(SERIF))
                .color(p.ink),
        );
    if !place.is_empty() {
        r = r
            .child(el().w(1.0).h(14.0).fill(p.rule))
            .child(text(place).font(line).color(p.ink2));
    }
    r.child(text("→").font(line).color(p.ink3))
        .on_press(move |s: &mut State| s.open_book(&path))
}

/// Recent or by title, over the right end of the shelf.
fn sort_row(s: &State, h: &Home, p: &Pal) -> El {
    let n = h.shelf.books.len();
    row()
        .w(SHELF_W)
        .px((SLOT_W - COVER_W) / 2.0)
        .pb(18.0)
        .center_y()
        .child(text(plural(n, "book")).font(SMALL).color(p.ink3).grow(1.0))
        .child(segmented(
            &[(ShelfSort::Recent, "Recent"), (ShelfSort::Title, "Title")],
            s.prefs.shelf_sort,
            "shelf-sort",
            |s, v| s.prefs.shelf_sort = v,
            p,
        ))
}

pub fn shelf(s: &State, h: &Home, p: &Pal) -> El {
    let header = row()
        .h(HEADER_H)
        .w_full()
        .px(20.0)
        .center_y()
        .gap(2.0)
        .window_drag()
        .child(
            text("Cipher")
                .font(TextStyle::new(17.0, 500.0).italic().family(SERIF))
                .color(p.ink),
        )
        .child(el().grow(1.0))
        .child(search_box(h, p))
        .child(el().w(10.0))
        .child(
            tool_btn(&I_FOLDER, "Import", false, p)
                .key("import")
                .help("Import a folder of chapters: raw/1.md, raw/2.md, … with optional tl/ and glossary.json")
                .on_press_tx(|s: &mut State, tx| s.pick_import(tx)),
        )
        .child(el().w(6.0))
        .child(
            tool_btn(&I_FILE, "Open", false, p)
                .key("open-file")
                .help("Open a .cipher book from anywhere; it stays where it is")
                .on_press_tx(|s: &mut State, tx| s.pick_book_file(tx)),
        )
        .child(el().w(6.0))
        .child(
            tool_btn(&I_PLUS, "New book", true, p)
                .key("new-book")
                .help("New book (Ctrl+N)")
                .on_press(|s: &mut State| s.start_new_book()),
        )
        .child(el().w(1.0).h(16.0).fill(p.rule).mx(8.0))
        .child(
            icon_btn(&I_PLUG, "shelf-endpoint", false, p)
                .help("Endpoints and models")
                .on_press(|s: &mut State| s.open_settings()),
        )
        .child(
            icon_btn(&I_GEAR, "shelf-settings", false, p)
                .help("Settings (Ctrl+,)")
                .on_press(|s: &mut State| s.open_settings_at(Section::General)),
        )
        .child(
            icon_btn(
                if s.dark() { &I_SUN } else { &I_MOON },
                "shelf-theme",
                false,
                p,
            )
            .help("Dark / light (Ctrl+D)")
            .on_press(|s: &mut State| s.flip_dark()),
        );

    let mut column = col().w(SHELF_W + 40.0).pt(64.0).pb(96.0).items_center();
    if let Some(busy) = &h.busy {
        column = column.child(
            text(busy.clone())
                .key("shelf-busy")
                .font(UI)
                .color(p.ink2)
                .pb(24.0),
        );
    }
    if let Some(e) = &h.error {
        column = column.child(
            row()
                .key("shelf-error")
                .gap(8.0)
                .pb(24.0)
                .center_y()
                .child(text(e.clone()).font(UI).color(p.red).wrap())
                .child(
                    el().size(22.0, 22.0)
                        .center()
                        .cursor(Cursor::Pointer)
                        .child(icon(&I_X, 12.0).color(p.ink3))
                        .on_press(|s: &mut State| home(s).error = None),
                ),
        );
    }

    if let Some(b) = h
        .shelf
        .books
        .iter()
        .find(|b| b.opened.is_some() && b.problem.is_none())
        && h.search.text.trim().is_empty()
    {
        column = column.child(continue_line(b, p));
    }
    column = column.child(sort_row(s, h, p));

    // Books in rows of five, then the empty slot; each row stands on its own ledge. A search
    // shows only the Books it matches, and no empty slot.
    let query = h.search.text.trim().to_lowercase();
    let mut books: Vec<(usize, &ShelfBook)> = h
        .shelf
        .books
        .iter()
        .enumerate()
        .filter(|(_, b)| query.is_empty() || b.title.to_lowercase().contains(&query))
        .collect();
    if s.prefs.shelf_sort == ShelfSort::Title {
        books.sort_by_key(|(_, b)| b.title.to_lowercase());
    }
    let slots = books.len() + usize::from(query.is_empty());
    if !query.is_empty() && books.is_empty() {
        column = column.child(
            text(format!(
                "No book's title has “{}” in it.",
                h.search.text.trim()
            ))
            .key("shelf-no-match")
            .font(TextStyle::new(16.0, 400.0).italic().family(SERIF))
            .color(p.ink3)
            .pb(24.0),
        );
    }
    for start in (0..slots).step_by(PER_ROW) {
        let mut covers = row().w(SHELF_W).items_end();
        let mut labels = row().w(SHELF_W).items_start();
        for at in start..(start + PER_ROW).min(slots) {
            let (front, under) = match books.get(at) {
                Some(&(i, b)) => (cover(b, i, s, p), label(h, b, i, p)),
                None => match &h.new_book {
                    Some(form) => (blank_cover(h, form, s), new_book_label(form, true, s, p)),
                    None => (new_slot(p), col().w(COVER_W)),
                },
            };
            covers = covers.child(row().w(SLOT_W).justify_center().child(front));
            labels = labels.child(row().w(SLOT_W).justify_center().child(under));
        }
        column = column
            .child(covers)
            .child(ledge(s, p))
            .child(labels.pb(44.0));
    }
    if h.shelf.books.is_empty() {
        column = column.child(
            text("Your shelf is empty. Start a book, or import a folder of chapters.")
                .key("shelf-empty")
                .font(TextStyle::new(16.0, 400.0).italic().family(SERIF))
                .color(p.ink3)
                .pb(24.0),
        );
    }
    column = column.child(
        row()
            .gap(6.0)
            .center_y()
            .child(
                text(format!("Kept in {}", tilde(&h.shelf.dir)))
                    .font(SMALL)
                    .color(p.ink4),
            )
            .child(
                text("Show folder")
                    .font(SMALL)
                    .color(p.ink3)
                    .key("show-folder")
                    .cursor(Cursor::Pointer)
                    .on_press_tx(|s: &mut State, tx| s.show_folder(tx)),
            ),
    );

    col()
        .w_full()
        .h_full()
        .fill(p.paper)
        // A soft pool of light over the shelf.
        .radial_gradient(
            [0.5, 0.0],
            [900.0, 700.0],
            [
                (0.0, white(if s.dark() { 0.03 } else { 0.30 })),
                (1.0, white(0.0)),
            ],
        )
        .child(header)
        .child(super::widgets::scrolling(
            col().w_full().items_center().child(column),
            "shelf-scroll",
        ))
}

/// `/home/u/Documents/Cipher` as `~/Documents/Cipher`.
fn tilde(path: &std::path::Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && shown.starts_with(&h) => format!("~{}", &shown[h.len()..]),
        _ => shown,
    }
}
