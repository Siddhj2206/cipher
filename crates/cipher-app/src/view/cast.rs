//! Cast & terms: the Glossary as a page the Book fills in itself, with real use and Drift.

use rune::tok::TextStyle;
use rune::{Cursor, El, ScrollSpec, col, row, text, text_input};

use super::page::page_width;
use super::widgets::plural;
use crate::state::State;
use crate::theme::{CJK, LABEL, Pal, SERIF, SMALL, SPARK, UI};

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The widest a sparkline gets, in characters; longer Books add Chapters into each one.
const SPARK_MAX: usize = 48;

/// Use across the Book as block characters, one per Chapter or per run of Chapters in a long
/// Book; a stretch that does not use the Term is a space.
fn sparkline(counts: &[u32]) -> String {
    let per = counts.len().div_ceil(SPARK_MAX).max(1);
    let counts: Vec<u32> = counts.chunks(per).map(|c| c.iter().sum()).collect();
    let max = counts.iter().copied().max().unwrap_or(0).max(1);
    counts
        .iter()
        .map(|&n| {
            if n == 0 {
                ' '
            } else {
                BARS[((n * 7) / max) as usize]
            }
        })
        .collect()
}

pub fn cast(s: &State, p: &Pal) -> El {
    let lib = &s.lib;
    let drifting = lib.drifting_terms();
    let mut list = col()
        .w(page_width(s))
        .pt(48.0)
        .pb(160.0)
        .px(56.0)
        .child(text("CAST & TERMS").font(LABEL).color(p.red))
        .child(
            text(format!("{} terms", lib.terms.len()))
                .font(TextStyle::new(34.0, 300.0).family(SERIF))
                .color(p.ink)
                .pt(4.0),
        )
        .child(
            text(format!(
                "{drifting} drifting: a forbidden wording appears, or a chapter names the term often in the source and never in the translation. Counted locally, no tokens. Open a term to fix it."
            ))
            .font(SMALL)
            .color(p.ink3)
            .pt(6.0)
            .pb(18.0)
            .wrap(),
        );

    // Search and the drifting filter, then labels over the columns.
    let query = s.cast_search.text.trim().to_lowercase();
    let mut tools = row().w_full().gap(10.0).center_y().pb(14.0).child(
        text_input(&s.cast_search, UI)
            .key("cast-search")
            .w(260.0)
            .px(10.0)
            .py(5.0)
            .radius(6.0)
            .border(1.0, p.rule)
            .fill(p.card)
            .single_line()
            .color(p.ink)
            .caret(p.red, p.highlight)
            .placeholder("Find a term".to_string(), p.ink4)
            .on_edit(|s: &mut State| &mut s.cast_search),
    );
    if drifting > 0 {
        tools = tools.child(
            text(format!("Drifting only · {drifting}"))
                .key("cast-drifting")
                .font(SMALL)
                .color(if s.cast_drifting { p.red } else { p.ink2 })
                .px(10.0)
                .py(5.0)
                .radius(6.0)
                .border(1.0, if s.cast_drifting { p.red } else { p.rule })
                .fill(if s.cast_drifting {
                    p.red_tint
                } else {
                    rune::Color::TRANSPARENT
                })
                .cursor(Cursor::Pointer)
                .on_press(|s: &mut State| s.cast_drifting = !s.cast_drifting),
        );
    }
    tools = tools.child(
        text(format!("Pending · {}", lib.pending_terms()))
            .key("cast-pending")
            .font(SMALL)
            .color(if s.cast_pending { p.red } else { p.ink2 })
            .px(10.0)
            .py(5.0)
            .radius(6.0)
            .border(1.0, if s.cast_pending { p.red } else { p.rule })
            .fill(if s.cast_pending {
                p.red_tint
            } else {
                rune::Color::TRANSPARENT
            })
            .cursor(Cursor::Pointer)
            .on_press(|s: &mut State| s.cast_pending = !s.cast_pending),
    );
    list = list.child(tools).child(
        row()
            .w_full()
            .px(8.0)
            .gap(16.0)
            .pb(4.0)
            .border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
            .child(text("TERM").font(LABEL).color(p.ink4).w(200.0))
            .child(text("USES").font(LABEL).color(p.ink4).w(36.0))
            .child(text("ACROSS THE BOOK").font(LABEL).color(p.ink4).grow(1.0))
            .child(text("").w(110.0)),
    );
    let order: Vec<usize> = lib
        .cast_order()
        .into_iter()
        .filter(|&i| !s.cast_drifting || !lib.usage(i).drift_chapters.is_empty())
        .filter(|&i| !s.cast_pending || lib.terms[i].provisional)
        .filter(|&i| {
            let t = &lib.terms[i];
            query.is_empty()
                || t.rendering.to_lowercase().contains(&query)
                || t.source.contains(&query)
                || t.forbidden
                    .iter()
                    .any(|f| f.to_lowercase().contains(&query))
        })
        .collect();
    if order.is_empty() {
        list = list.child(
            text("No term matches.")
                .font(SMALL)
                .color(p.ink3)
                .px(8.0)
                .pt(14.0),
        );
    }
    for i in order {
        let t = &lib.terms[i];
        let usage = lib.usage(i);
        let open = s.cast_open == Some(i);
        let mut right = row().w(110.0).justify_end();
        if !usage.drift_chapters.is_empty() {
            right = right.child(
                text(format!("drift · {} ch", usage.drift_chapters.len()))
                    .font(SMALL)
                    .color(p.red)
                    .px(8.0)
                    .py(2.0)
                    .radius(10.0)
                    .fill(p.red_tint)
                    .key(format!("drift{i}")),
            );
        } else if t.provisional {
            right = right.child(
                text("pending")
                    .font(SMALL)
                    .color(p.ink3)
                    .px(8.0)
                    .py(2.0)
                    .radius(10.0)
                    .fill(p.hover),
            );
        } else if t.may_omit {
            right = right.child(text("may go unnamed").font(SMALL).color(p.ink4));
        }
        let mut second = vec![t.source.clone()];
        if !t.forbidden.is_empty() {
            second.push(format!("never {}", t.forbidden.join(", ")));
        }
        if t.provisional {
            second.push("pending".into());
        }
        let line = row()
            .key(format!("term{i}"))
            .w_full()
            .py(9.0)
            .px(8.0)
            .gap(16.0)
            .center_y()
            .radius(6.0)
            .hover(p.hover)
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut State| s.toggle_cast_term(i))
            .child(
                col()
                    .w(200.0)
                    .gap(1.0)
                    .child(
                        text(t.rendering.clone())
                            .font(TextStyle::new(16.0, 500.0).family(SERIF))
                            .color(p.ink)
                            .truncate(),
                    )
                    .child(
                        text(second.join(" · "))
                            .font(TextStyle::new(12.5, 400.0).family(CJK))
                            .color(p.ink3)
                            .truncate(),
                    ),
            )
            .child(
                text(if usage.total == 0 {
                    "–".to_string()
                } else {
                    usage.total.to_string()
                })
                .font(SMALL.features("tnum"))
                .color(p.ink2)
                .w(36.0)
                .help(if usage.total == 0 {
                    "The translation never uses this rendering".to_string()
                } else {
                    format!(
                        "{} in {}",
                        plural(usage.total as usize, "use"),
                        plural(usage.chapters, "chapter")
                    )
                }),
            )
            .child(
                text(sparkline(&lib.counts(i).collect::<Vec<_>>()))
                    .font(SPARK)
                    .color(if usage.drift_chapters.is_empty() {
                        p.ink3
                    } else {
                        p.red_soft
                    })
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate(),
            )
            .child(right);
        let mut item = col()
            .w_full()
            .border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
            .child(line);
        if open {
            let detail = super::drift::drift_panel(s, i, p).unwrap_or_else(|| {
                text(format!(
                    "No drift flagged for \u{201c}{}\u{201d}.",
                    t.rendering
                ))
                .font(SMALL)
                .color(p.ink3)
                .wrap()
            });
            item = item.child(
                col()
                    .w_full()
                    .pl(8.0)
                    .pr(8.0)
                    .pb(14.0)
                    .child(super::term_editor::panel(s, i, p))
                    .child(detail),
            );
        }
        list = list.child(item);
    }
    col()
        .key("cast-doc")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .items_center()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(list)
}
