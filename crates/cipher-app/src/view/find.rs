//! A non-modal search bar above the reading page.
use crate::{
    state::{Screen, State},
    theme::{Pal, SMALL, UI_MED},
};
use cipher_book::Side;
use rune::tok::TextStyle;
use rune::{El, col, row, text, text_input};

pub fn bar(s: &State, p: &Pal) -> Option<El> {
    if s.screen != Screen::Reading {
        return None;
    }
    let f = s.find.as_ref()?;
    let count = if f.searching {
        "Searching…".to_string()
    } else if f.query.text.trim().is_empty() {
        "Find in book".into()
    } else if f.results.hits.is_empty() {
        "No matches".into()
    } else {
        let chapter = s
            .lib
            .chapter_index(f.results.hits[f.current].chapter)
            .map(|i| s.lib.chapters[i].number)
            .unwrap_or_default();
        format!(
            "{} of {}{} · Chapter {chapter}",
            f.current + 1,
            crate::view::thousands(f.results.hits.len() as i64),
            if f.results.more { "+" } else { "" }
        )
    };
    let mut scopes = row().gap(4.0);
    for (side, key, label) in [
        (Side::Translation, "find-scope-translation", "Translation"),
        (Side::Source, "find-scope-source", "Source"),
    ] {
        scopes = scopes.child(
            text(label)
                .key(key)
                .font(SMALL)
                .color(p.ink)
                .px(8.0)
                .py(5.0)
                .radius(4.0)
                .fill(if side == f.scope { p.hover } else { p.card })
                .on_press_tx(move |s: &mut State, tx| {
                    if let Some(f) = &mut s.find {
                        f.scope = side;
                    }
                    s.search_find();
                    s.poll_find(tx);
                }),
        );
    }
    Some(
        col()
            .key("find-bar")
            .pin(
                None,
                Some(66.0),
                Some(if s.context_shown() { 338.0 } else { 18.0 }),
                None,
            )
            .w(430.0)
            .padding(10.0)
            .gap(6.0)
            .radius(8.0)
            .fill(p.card)
            .border(1.0, p.rule)
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(
                        text_input(&f.query, TextStyle::new(15.0, 400.0))
                            .key("find-input")
                            .label("Find in book")
                            .grow(1.0)
                            .min_w(0.0)
                            .single_line()
                            .color(p.ink)
                            .caret(p.red, p.highlight)
                            .placeholder("Find in book", p.ink3)
                            .on_edit(|s: &mut State| {
                                &mut s.find.as_mut().expect("find is open").query
                            })
                            .on_input_change(|s: &mut State, tx| {
                                s.poll_find(tx);
                            })
                            .on_input_key(crate::find::key),
                    )
                    .child(
                        text("×")
                            .key("find-close")
                            .font(UI_MED)
                            .color(p.ink3)
                            .px(6.0)
                            .on_press_tx(|s: &mut State, tx| s.close_find(tx)),
                    ),
            )
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(scopes)
                    .child(
                        text("↑")
                            .key("find-prev")
                            .font(UI_MED)
                            .color(p.ink)
                            .on_press_tx(|s: &mut State, tx| s.step_find(tx, true)),
                    )
                    .child(
                        text("↓")
                            .key("find-next")
                            .font(UI_MED)
                            .color(p.ink)
                            .on_press_tx(|s: &mut State, tx| s.step_find(tx, false)),
                    ),
            )
            .child(text(count).key("find-count").font(SMALL).color(p.ink3)),
    )
}
