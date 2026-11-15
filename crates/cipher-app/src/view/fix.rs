//! The selection chip and the fix box, anchored under the selected paragraph.

use rune::tok::TextStyle;
use rune::{Align, Color, El, Key, Side as Edge, el, icon, row, text, text_input};

use super::widgets::{card, chip, kbd, short};
use crate::fix::SCOPES;
use crate::state::State;
use crate::theme::{I_ARROW, INTERLINEAR, Pal, SERIF, SMALL, UI_MED};

/// "Suggest a fix · or just start typing": small, so selecting to copy is never blocked.
pub fn chip_bar(s: &State, p: &Pal) -> Option<El> {
    let picked = s.fix.picked.as_ref()?;
    if s.fix.composing || s.fix.review.is_some() {
        return None;
    }
    Some(
        row()
            .key("fix-chip")
            .gap(8.0)
            .px(10.0)
            .h(30.0)
            .center_y()
            .radius(15.0)
            .fill(p.bar)
            .shadow(6.0, 16.0, p.shadow)
            .cursor(rune::Cursor::Pointer)
            .anchored(
                format!("b{}", picked.block.0),
                Edge::Bottom,
                Align::Start,
                8.0,
            )
            .child(text("Suggest a fix").font(UI_MED).color(p.bar_fg))
            .child(text("or just start typing").font(SMALL).color(p.bar_fg2))
            .on_press(|s: &mut State| s.open_composer("")),
    )
}

fn scope_labels(s: &State) -> [String; 3] {
    let reach = s.fix.picked.as_ref().and_then(|p| p.reach.as_ref());
    [
        "Just here".to_string(),
        match reach {
            Some(r) => format!("Everywhere · {} in {} ch", r.blocks, r.chapters),
            None => "Everywhere".to_string(),
        },
        if s.fix.is_term() {
            "Everywhere + glossary".to_string()
        } else {
            "From now on".to_string()
        },
    ]
}

/// The source line under the quote: the Term's source form, or the paragraph's source.
fn source_line(s: &State) -> String {
    let Some(p) = &s.fix.picked else {
        return String::new();
    };
    if s.fix.is_term()
        && let Some(src) = p.reach.as_ref().and_then(|r| r.source_expr.as_ref())
    {
        return format!("{src} · glossary term");
    }
    let page = s.lib.page();
    page.pair_of(cipher_book::Side::Translation, p.block)
        .and_then(|pair| page.pair(pair))
        .map(|pv| pv.source_text.clone())
        .unwrap_or_default()
}

pub fn composer(s: &State, p: &Pal) -> Option<El> {
    let picked = s.fix.picked.as_ref()?;
    if !s.fix.composing {
        return None;
    }
    let rename = s.fix.rename;
    let mut scopes = row()
        .gap(6.0)
        .center_y()
        .child(
            row()
                .key("rename")
                .gap(5.0)
                .center_y()
                .px(8.0)
                .py(3.0)
                .radius(12.0)
                .cursor(rune::Cursor::Pointer)
                .hover(p.hover)
                .border(1.0, if rename { p.ink2 } else { p.rule })
                .child(
                    el().size(10.0, 10.0)
                        .radius(2.0)
                        .border(1.0, p.ink2)
                        .fill(if rename { p.ink } else { Color::TRANSPARENT }),
                )
                .child(
                    text("Rename")
                        .font(SMALL)
                        .color(if rename { p.ink } else { p.ink3 }),
                )
                .help("Exact replace, no model call")
                .on_press(|s: &mut State| s.fix.rename = !s.fix.rename),
        )
        .child(el().w(1.0).h(16.0).fill(p.rule).mx(2.0));
    for (i, label) in scope_labels(s).iter().enumerate() {
        scopes = scopes.child(
            chip(label, s.fix.scope == i, p)
                .key(format!("scope{i}"))
                .on_press(move |s: &mut State| s.choose_scope(i)),
        );
    }
    debug_assert_eq!(SCOPES.len(), 3);
    Some(
        card(p)
            .key("composer")
            .w(640.0)
            .padding(14.0)
            .gap(10.0)
            .anchored(
                format!("b{}", picked.block.0),
                Edge::Bottom,
                Align::Start,
                10.0,
            )
            .child(
                row()
                    .w_full()
                    .gap(10.0)
                    .center_y()
                    .child(
                        text(short(&picked.text))
                            .font(TextStyle::new(14.0, 400.0).family(SERIF))
                            .color(p.ink)
                            .px(6.0)
                            .py(2.0)
                            .radius(3.0)
                            .fill(p.highlight),
                    )
                    .child(icon(&I_ARROW, 14.0).color(p.ink3))
                    .child(
                        text_input(&s.fix.composer, TextStyle::new(14.0, 400.0))
                            .key("composer-input")
                            .grow(1.0)
                            .min_w(0.0)
                            .single_line()
                            .color(p.ink)
                            .caret(p.red, p.highlight)
                            .placeholder("What should it say? Or say what's wrong.", p.ink3)
                            .on_edit(|s: &mut State| &mut s.fix.composer)
                            .on_input_key(|s: &mut State, k, _| match &k.key {
                                Key::Enter => {
                                    s.submit();
                                    true
                                }
                                Key::Tab => {
                                    s.choose_scope(s.fix.scope + 1);
                                    true
                                }
                                Key::Escape => {
                                    s.fix.composing = false;
                                    true
                                }
                                _ => false,
                            }),
                    ),
            )
            .child(
                text(source_line(s))
                    .font(INTERLINEAR)
                    .color(p.ink3)
                    .wrap()
                    .max_lines(3)
                    .pl(10.0)
                    .border_sides([0.0, 0.0, 0.0, 2.0], p.rule),
            )
            .child(
                row()
                    .w_full()
                    .center_y()
                    .child(scopes)
                    .child(el().grow(1.0))
                    .child(
                        row()
                            .gap(4.0)
                            .center_y()
                            .child(kbd("Tab", p))
                            .child(kbd("↵", p)),
                    ),
            ),
    )
}
