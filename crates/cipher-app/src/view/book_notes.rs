//! The Book's address guidance and generated continuity notes, separate from Style.

use rune::tok::TextStyle;
use rune::{El, ScrollSpec, col, row, text, text_input};

use super::widgets::btn;
use crate::state::State;
use crate::theme::{GUTTER_W, Pal, SERIF, SMALL, UI};

pub fn book_notes(s: &State, p: &Pal) -> El {
    let Some(edit) = &s.notes else { return col() };
    let mut body = col()
        .w(s.text_width())
        .ml(GUTTER_W)
        .pt(48.0)
        .pb(80.0)
        .gap(16.0)
        .child(
            text("Book notes")
                .font(TextStyle::new(30.0, 500.0).family(SERIF))
                .color(p.ink),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text("Style rules live on the Style page")
                        .font(SMALL)
                        .color(p.ink2),
                )
                .child(
                    btn("Open Style", false, p)
                        .key("notes-style")
                        .on_press(|s: &mut State| s.open_style()),
                ),
        )
        .child(
            text("Forms of Address")
                .font(TextStyle::new(22.0, 500.0).family(SERIF))
                .color(p.ink),
        )
        .child(
            text("Your guidance for how people address one another, sent with every Chapter.")
                .font(UI)
                .color(p.ink2)
                .wrap(),
        );
    let mut headings = row().w_full().gap(8.0);
    for label in ["Speaker", "Listener", "Source form", "Rendering"] {
        headings = headings.child(text(label).font(SMALL).color(p.ink2).grow(1.0).w(0.0));
    }
    body = body.child(headings.child(text("").w(70.0)));
    for (i, form) in edit.forms.iter().enumerate() {
        let mut fields = row().w_full().gap(8.0).center_y();
        for (j, name) in ["speaker", "listener", "source-form", "rendering"]
            .iter()
            .enumerate()
        {
            fields = fields.child(
                text_input(&form.fields[j], UI)
                    .key(format!("notes-form-{i}-{name}"))
                    .label(format!("Form {} {}", i + 1, name.replace('-', " ")))
                    .w(0.0)
                    .grow(1.0)
                    .min_h(36.0)
                    .px(6.0)
                    .fill(p.hover)
                    .color(p.ink)
                    .caret(p.red, p.highlight)
                    .on_edit(move |s: &mut State| {
                        &mut s.notes.as_mut().expect("notes open").forms[i].fields[j]
                    }),
            );
        }
        body = body.child(
            fields.child(
                btn("Remove", false, p)
                    .key(format!("notes-form-{i}-remove"))
                    .on_press(move |s: &mut State| {
                        s.notes.as_mut().expect("notes open").forms.remove(i);
                    }),
            ),
        );
    }
    body = body.child(
        btn("Add form", false, p)
            .key("notes-add-form")
            .on_press(|s: &mut State| s.notes.as_mut().expect("notes open").add_form()),
    );
    let compact = btn("Compact", false, p).key("notes-compact");
    let compact = if edit.notes.is_empty() || s.notes_compacting() {
        compact.opacity(0.4)
    } else {
        compact.on_press(|s: &mut State| s.compact_notes())
    };
    body = body
        .child(
            row()
                .w_full()
                .mt(24.0)
                .center_y()
                .child(
                    text("Story Notes")
                        .font(TextStyle::new(22.0, 500.0).family(SERIF))
                        .color(p.ink)
                        .grow(1.0),
                )
                .child(compact),
        )
        .child(
            text("Written by translation; the model reads the latest notes as the story so far.")
                .font(UI)
                .color(p.ink2)
                .wrap(),
        );
    if edit.notes.is_empty() {
        body = body.child(
            text("No Story Notes yet. Translating a Chapter writes its first note.")
                .font(SMALL)
                .color(p.ink2)
                .wrap(),
        );
    }
    for (i, note) in edit.notes.iter().enumerate() {
        body = body.child(
            col()
                .w_full()
                .gap(6.0)
                .child(
                    row()
                        .w_full()
                        .center_y()
                        .child(
                            text(note.origin.clone())
                                .font(SMALL)
                                .color(p.ink2)
                                .grow(1.0),
                        )
                        .child(
                            btn("Remove", false, p)
                                .key(format!("notes-note-{i}-remove"))
                                .on_press(move |s: &mut State| {
                                    s.notes.as_mut().expect("notes open").notes.remove(i);
                                }),
                        ),
                )
                .child(
                    text_input(
                        &note.text,
                        TextStyle::new(15.0, 400.0).lh(25.0).family(SERIF),
                    )
                    .key(format!("notes-note-{i}"))
                    .label(format!("Story Note: {}", note.origin))
                    .w_full()
                    .min_h(110.0)
                    .px(8.0)
                    .py(8.0)
                    .fill(p.hover)
                    .color(p.ink)
                    .caret(p.red, p.highlight)
                    .on_edit(move |s: &mut State| {
                        &mut s.notes.as_mut().expect("notes open").notes[i].text
                    }),
                ),
        );
    }
    if edit.changed() {
        body = body.child(
            row()
                .gap(8.0)
                .mt(12.0)
                .child(
                    btn("Save", true, p)
                        .key("notes-save")
                        .on_press(|s: &mut State| s.save_notes()),
                )
                .child(
                    btn("Revert", false, p)
                        .key("notes-revert")
                        .on_press(|s: &mut State| s.revert_notes()),
                )
                .child(text("Not saved · Ctrl S").font(SMALL).color(p.ink2)),
        );
    }
    col()
        .w_full()
        .items_center()
        .child(body)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .key("notes-page")
        .grow(1.0)
        .min_h(0.0)
}
