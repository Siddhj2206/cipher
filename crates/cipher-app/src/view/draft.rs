//! The new-Chapter page: set like a Chapter's opening page, with the source pasted or typed
//! straight onto it. What Cipher reads from the text (number, title, where it goes) shows in
//! the opener as it changes.

use rune::tok::TextStyle;
use rune::{Cursor, El, Key, ScrollSpec, col, el, row, text, text_input};

use super::widgets::{btn, kbd, plural};
use crate::fix::work::Status;
use crate::state::State;
use crate::theme::{GUTTER_W, LABEL, Pal, SERIF, SMALL, UI, src_body};

pub fn draft_page(s: &State, p: &Pal) -> El {
    let Some(d) = &s.draft else { return col() };
    let it = s.draft_intake();
    let width = s.text_width();
    let empty = it.is_none();
    let (number, read) = match it.as_ref().and_then(|i| i.number) {
        Some(n) => (n, true),
        None => (s.next_number(), false),
    };

    // What Cipher makes of it, under the number.
    let mut notes = col().gap(4.0).pt(18.0);
    match &it {
        None => {
            notes = notes.child(
                text("Paste the chapter onto the page, or type it. A first line such as “321화” or “Chapter 321” gives it its number.")
                    .font(SMALL)
                    .color(p.ink3)
                    .wrap(),
            )
        }
        Some(i) => {
            let place = match (i.number, i.after) {
                (_, Some(after)) => format!("goes after chapter {after}"),
                (Some(_), None) => "goes first".to_string(),
                (None, None) => "goes at the end".to_string(),
            };
            notes = notes.child(
                text(format!("{} · {place}", plural(i.blocks.len(), "paragraph")))
                    .key("draft-meta")
                    .font(SMALL)
                    .color(p.ink3),
            );
            if i.number.is_none() {
                notes = notes.child(
                    text(format!("No chapter number in the first lines, so it becomes chapter {number}."))
                        .font(SMALL)
                        .color(p.amber)
                        .wrap(),
                );
            }
            if i.exists {
                notes = notes.child(
                    text("This chapter is already in the book; adding it makes a second copy.")
                        .font(SMALL)
                        .color(p.red)
                        .wrap(),
                );
            }
        }
    }

    let mut opener = col()
        .key("draft-opener")
        .pl(GUTTER_W)
        .gap(2.0)
        .child(text("NEW CHAPTER").font(LABEL).color(p.red))
        .child(
            text(number.to_string())
                .font(TextStyle::new(60.0, 300.0).lh(72.0).family(SERIF))
                .color(if read { p.ink } else { p.ink4 }),
        );
    if let Some(title) = it.as_ref().and_then(|i| i.title.clone()) {
        opener = opener.child(
            text(title)
                .font(TextStyle::new(20.0, 400.0).italic().family(SERIF))
                .color(p.ink2)
                .pt(4.0),
        );
    }
    opener = opener
        .child(el().w(36.0).h(1.5).fill(p.ink).mt(14.0))
        .child(notes.w(width));

    // Translation needs a model; say so here, where it matters, without stopping anything.
    let connect = matches!(s.bg.status, Status::NoEndpoint);
    let mut page = col()
        .key("page")
        .w(width + GUTTER_W)
        .pt(56.0)
        .pb(160.0)
        .child(opener);
    if connect {
        page = page.child(
            row()
                .key("draft-connect")
                .ml(GUTTER_W)
                .w(width)
                .mt(22.0)
                .px(14.0)
                .py(10.0)
                .gap(12.0)
                .center_y()
                .radius(8.0)
                .fill(p.card)
                .border(1.0, p.rule)
                .child(
                    text("Chapters are kept as you add them and translated once a model is connected.")
                        .font(SMALL)
                        .color(p.ink2)
                        .grow(1.0)
                        .wrap(),
                )
                .child(
                    btn("Connect a model", false, p)
                        .key("draft-endpoint")
                        .on_press(|s: &mut State| s.open_settings()),
                ),
        );
    }

    let field = text_input(&d.text, src_body(&s.prefs.reading))
        .key("draft-text")
        .ml(GUTTER_W)
        .w(width)
        .min_h(380.0)
        .mt(30.0)
        .color(p.ink)
        .caret(p.red, p.highlight)
        .placeholder("그날 밤, 탑 위에서 바람이 불었다…", p.ink4)
        .on_edit(|s: &mut State| &mut s.draft.as_mut().expect("the draft is open").text)
        .on_input_key(|s: &mut State, k: &rune::KeyEvent, tx| {
            if k.key == Key::Enter && k.mods.cmd {
                s.add_draft(tx);
                return true;
            }
            false
        });
    page = page.child(field);

    let add_label = if read || !empty {
        format!("Add chapter {number}")
    } else {
        "Add chapter".to_string()
    };
    let mut add = btn(&add_label, true, p).key("draft-add");
    add = if empty {
        add.opacity(0.4)
    } else {
        add.on_press_tx(|s: &mut State, tx| s.add_draft(tx))
    };
    let mut actions = row()
        .key("draft-actions")
        .ml(GUTTER_W)
        .w(width)
        .mt(24.0)
        .pt(18.0)
        .gap(8.0)
        .center_y()
        .border_sides([1.0, 0.0, 0.0, 0.0], p.rule)
        .child(add)
        .child(kbd("Ctrl Enter", p));
    if !s.lib.chapters.is_empty() || !empty {
        actions = actions.child(
            btn(
                if s.lib.chapters.is_empty() {
                    "Clear"
                } else {
                    "Discard"
                },
                false,
                p,
            )
            .key("draft-discard")
            .on_press_tx(|s: &mut State, tx| s.leave_draft(tx, true)),
        );
    }
    actions = actions.child(el().grow(1.0)).child(
        text("Open a text file…")
            .key("draft-file")
            .font(UI)
            .color(p.ink3)
            .cursor(Cursor::Pointer)
            .on_press_tx(|s: &mut State, tx| s.draft_from_file(tx)),
    );
    if !empty {
        actions = actions.child(
            text("translated in order, after what is already queued")
                .font(SMALL)
                .color(p.ink4)
                .pl(10.0),
        );
    }
    page = page.child(actions);

    col()
        .key("draft-doc")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .items_center()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(page)
}
