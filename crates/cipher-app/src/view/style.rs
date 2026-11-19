//! The Style page: how this Book should read, as one text sent with every Chapter.

use rune::tok::TextStyle;
use rune::{El, ScrollSpec, col, row, text, text_input};

use super::widgets::{btn, kbd, plural};
use crate::state::State;
use crate::theme::{GUTTER_W, LABEL, Pal, SERIF, SMALL, UI};

pub fn style_page(s: &State, p: &Pal) -> El {
    let Some(edit) = &s.style else { return col() };
    let width = s.text_width();
    let rules = cipher_engine::style_rules(&edit.text.text).len();
    let changed = edit.changed();

    let head = col()
        .ml(GUTTER_W)
        .w(width)
        .pt(56.0)
        .gap(10.0)
        .child(text("STYLE").font(LABEL).color(p.red))
        .child(
            text("How this book should read")
                .font(TextStyle::new(30.0, 500.0).family(SERIF))
                .color(p.ink),
        )
        .child(
            text("Sent with every chapter's translation, before the glossary. Write it as you would brief a translator: tone, voice, how to handle honorifics or names. Each paragraph or list item is one rule. Fixes you send “From now on” add their rule here.")
                .font(UI.lh(20.0))
                .color(p.ink2)
                .wrap(),
        );

    let field = text_input(&edit.text, TextStyle::new(15.0, 400.0).lh(25.0).family(SERIF))
        .key("style-text")
        .ml(GUTTER_W)
        .w(width)
        .min_h(420.0)
        .mt(26.0)
        .color(p.ink)
        .caret(p.red, p.highlight)
        .placeholder(
            "Tense and atmospheric; keep short sentences short.\n\nDialogue should sound natural to an English reader…",
            p.ink4,
        )
        .on_edit(|s: &mut State| &mut s.style.as_mut().expect("the Style page is open").text);

    let mut save = btn("Save", true, p).key("style-save");
    save = if changed {
        save.on_press(|s: &mut State| s.save_style())
    } else {
        save.opacity(0.4)
    };
    let mut actions = row()
        .ml(GUTTER_W)
        .w(width)
        .mt(20.0)
        .gap(8.0)
        .center_y()
        .child(save);
    if changed {
        actions = actions.child(
            btn("Revert", false, p)
                .key("style-revert")
                .on_press(|s: &mut State| s.revert_style()),
        );
    }
    actions = actions
        .child(
            text(if changed {
                format!("{} · not saved", plural(rules, "rule"))
            } else {
                plural(rules, "rule")
            })
            .font(SMALL)
            .color(p.ink3)
            .grow(1.0),
        )
        .child(kbd("Ctrl S", p));

    col()
        .w_full()
        .items_center()
        .pb(80.0)
        .child(col().child(head).child(field).child(actions))
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .key("style-page")
        .grow(1.0)
        .min_h(0.0)
}
