use rune::{El, col, row, text, text_input};

use super::widgets::btn;
use crate::{
    state::State,
    theme::{Pal, SMALL, UI},
};

pub fn panel(s: &State, index: usize, p: &Pal) -> El {
    let t = &s.lib.terms[index];
    let mut panel = col().w_full().gap(8.0).pb(16.0);
    if let Some(e) = &s.term_editor {
        for (n, label, value) in [
            (0, "Source form", &e.source),
            (1, "Rendering", &e.rendering),
            (2, "Note", &e.note),
        ] {
            panel = panel.child(text(label).font(SMALL).color(p.ink3)).child(
                text_input(value, UI)
                    .label(label)
                    .key(format!("term-field-{n}"))
                    .w_full()
                    .px(10.0)
                    .py(6.0)
                    .border(1.0, p.rule)
                    .radius(6.0)
                    .fill(p.card)
                    .color(p.ink)
                    .caret(p.red, p.highlight)
                    .single_line()
                    .on_edit(move |s: &mut State| {
                        let e = s.term_editor.as_mut().expect("term editor open");
                        match n {
                            0 => &mut e.source,
                            1 => &mut e.rendering,
                            _ => &mut e.note,
                        }
                    }),
            );
        }
        return panel.child(text("Saving changes what future chapters are told. Chapters already translated keep their wording; a new rendering makes the old one a forbidden wording, so Drift shows where it remains.").font(SMALL).color(p.ink3).wrap()).child(
            row().gap(8.0).child(btn("Save", true, p).key("term-save").on_press(move |s: &mut State| s.save_term(index)))
                .child(btn("Cancel", false, p).on_press(|s: &mut State| s.term_editor = None))
        );
    }
    if let Some(note) = &t.note {
        panel = panel.child(text(note.clone()).font(UI).color(p.ink2).wrap());
    }
    panel = panel.child(
        text(if t.provisional {
            "Pending: future chapters send it to the model as a provisional name.".into()
        } else {
            format!(
                "Accepted: future chapters are told to render 「{}」 as “{}” exactly.",
                t.source, t.rendering
            )
        })
        .font(SMALL)
        .color(p.ink3)
        .wrap(),
    );
    let mut actions = row().gap(8.0);
    if t.provisional {
        actions = actions.child(
            btn("Accept", true, p)
                .key("term-accept")
                .on_press(move |s: &mut State| s.decide_term(index, true)),
        );
    }
    panel.child(actions.child(btn("Dismiss", false, p).key("term-dismiss").help("Dismissed terms are no longer sent with chapters and no longer counted for Drift.").on_press(move |s: &mut State| s.decide_term(index, false)))
        .child(btn("Edit", false, p).key("term-edit").on_press(move |s: &mut State| s.edit_cast_term(index))))
}
