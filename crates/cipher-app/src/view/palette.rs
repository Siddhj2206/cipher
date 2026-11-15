//! The palette sheet: a scrim and a 600px card with a field and results.

use rune::tok::TextStyle;
use rune::{Color, Cursor, El, Key, col, el, row, text, text_input};

use super::widgets::{card, short};
use crate::palette::{self, Hit};
use crate::state::State;
use crate::theme::{Pal, SMALL, UI_MED};

pub fn palette_sheet(s: &State, p: &Pal) -> Option<El> {
    let q = s.palette.as_ref()?;
    let hits = palette::hits(s, &q.text);
    let pick = s.palette_pick.min(hits.len().saturating_sub(1));
    let mut list = col().w_full().padding(6.0).gap(1.0);
    for (i, h) in hits.iter().enumerate() {
        let (a, b) = match h {
            Hit::Chapter(ci) => (
                format!("Chapter {}", s.lib.chapters[*ci].number),
                short(&s.lib.chapters[*ci].opening),
            ),
            Hit::Term(ti) => (
                s.lib.terms[*ti].rendering.clone(),
                format!("{} · term", s.lib.terms[*ti].source),
            ),
            Hit::Action(a) => (palette::action_name(*a, s), palette::action_detail(*a, s)),
        };
        list = list.child(
            row()
                .key(format!("hit{i}"))
                .w_full()
                .h(38.0)
                .px(12.0)
                .gap(12.0)
                .center_y()
                .radius(6.0)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .fill(if i == pick {
                    p.hover
                } else {
                    Color::TRANSPARENT
                })
                .child(text(a).font(UI_MED).color(p.ink))
                .child(text(b).font(SMALL).color(p.ink3).grow(1.0).truncate())
                .on_press_tx(move |s: &mut State, tx| palette::run_nth(s, tx, i)),
        );
    }
    let count = hits.len();
    let sheet = card(p)
        .key("palette")
        .w(600.0)
        .modal(true, |s: &mut State| s.palette = None)
        .child(
            text_input(q, TextStyle::new(16.0, 400.0))
                .key("palette-input")
                .w_full()
                .px(18.0)
                .py(14.0)
                .single_line()
                .color(p.ink)
                .caret(p.red, p.highlight)
                .placeholder("Chapter number, a term, or an action", p.ink3)
                .on_edit(|s: &mut State| {
                    s.palette_pick = 0;
                    s.palette
                        .as_mut()
                        .expect("the palette is open while it is edited")
                })
                .on_input_key(move |s: &mut State, k, tx| match &k.key {
                    Key::Enter => {
                        let at = s.palette_pick;
                        palette::run_nth(s, tx, at);
                        true
                    }
                    Key::Down => {
                        s.palette_pick = (s.palette_pick + 1).min(count.saturating_sub(1));
                        true
                    }
                    Key::Up => {
                        s.palette_pick = s.palette_pick.saturating_sub(1);
                        true
                    }
                    Key::Escape => {
                        s.palette = None;
                        true
                    }
                    _ => false,
                }),
        )
        .child(el().w_full().h(1.0).fill(p.rule))
        .child(list);
    Some(
        col()
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .fill(p.scrim)
            .items_center()
            .pt(110.0)
            .child(sheet),
    )
}
