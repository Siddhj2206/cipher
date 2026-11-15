//! The Context sidebar (right): the Terms in this Chapter, and Suggestions.

use cipher_text::drift::DriftKind;
use rune::tok::TextStyle;
use rune::{Color, Cursor, El, col, el, icon, row, text};

use super::notes;
use super::widgets::{dot, icon_btn, plural, scrolling, section};
use crate::library::TermInfo;
use crate::state::{State, Tab, TermCard};
use crate::theme::{
    CJK, HEADER_H, I_RIGHT_PANEL, I_X, LABEL, Pal, RIGHT_W, SERIF, SMALL, UI, UI_MED,
};

/// "never Tower Master, Mage Lord"
fn never(t: &TermInfo) -> Option<String> {
    (!t.forbidden.is_empty()).then(|| format!("never {}", t.forbidden.join(", ")))
}

/// What Drift says about a Term in the current Chapter, as sentences.
fn drift_lines(s: &State, term: usize) -> Vec<String> {
    s.lib
        .stats(s.lib.current())
        .drift
        .iter()
        .filter(|d| d.term_index == term)
        .map(|d| match &d.kind {
            DriftKind::ForbiddenPresent { rendering, count } => format!(
                "“{rendering}” is forbidden and appears {} here",
                plural(*count, "time")
            ),
            DriftKind::NeverRendered { source_count } => format!(
                "The source names it {} here and the translation never renders it",
                plural(*source_count, "time")
            ),
        })
        .collect()
}

/// A Term's card: what it is, how much it is used, what it must not be, and whether it drifts.
pub fn term_card(s: &State, tc: TermCard, p: &Pal) -> El {
    let t = &s.lib.terms[tc.term];
    let usage = s.lib.usage(tc.term);
    let mut label = String::from("TERM");
    if t.provisional {
        label.push_str(" · PROVISIONAL");
    }
    let mut c = col()
        .w_full()
        .gap(5.0)
        .pl(12.0)
        .border_sides(
            [0.0, 0.0, 0.0, 2.0],
            if t.provisional { p.ink3 } else { p.ink },
        )
        .child(
            row()
                .w_full()
                .child(text(label).font(LABEL).color(p.ink3).grow(1.0))
                .child(
                    el().key("term-x")
                        .size(18.0, 18.0)
                        .center()
                        .radius(4.0)
                        .hover(p.hover)
                        .cursor(Cursor::Pointer)
                        .child(icon(&I_X, 12.0).color(p.ink3))
                        .on_press(|s: &mut State| s.term_card = None),
                ),
        )
        .child(
            text(t.rendering.clone())
                .font(TextStyle::new(17.0, 600.0).family(SERIF))
                .color(p.ink)
                .wrap(),
        )
        .child(
            text(t.source.clone())
                .font(TextStyle::new(14.0, 400.0).family(CJK))
                .color(p.ink2),
        );
    let about: Vec<&str> = [t.kind.as_deref(), t.gender.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    if !about.is_empty() {
        c = c.child(text(about.join(" · ")).font(SMALL).color(p.ink3));
    }
    if let Some(note) = &t.note {
        c = c.child(
            text(note.clone())
                .font(SMALL.lh(17.0))
                .color(p.ink2)
                .wrap()
                .max_lines(5),
        );
    }
    c = c.child(
        text(format!(
            "{} in {}",
            plural(usage.total as usize, "use"),
            plural(usage.chapters, "chapter")
        ))
        .font(SMALL)
        .color(p.ink3),
    );
    if let Some(n) = never(t) {
        c = c.child(text(n).font(SMALL).color(p.ink2).wrap());
    }
    let here = drift_lines(s, tc.term);
    let elsewhere = usage
        .drift_chapters
        .iter()
        .filter(|&&c| c != s.lib.current())
        .count();
    for line in &here {
        c = c.child(text(line.clone()).font(SMALL).color(p.red).wrap());
    }
    if elsewhere > 0 {
        let also = if here.is_empty() {
            "Drifts"
        } else {
            "Also drifts"
        };
        c = c.child(
            text(format!("{also} in {}", plural(elsewhere, "other chapter")))
                .font(SMALL)
                .color(p.red)
                .wrap(),
        );
    }
    if let Some(panel) = super::drift::drift_panel(s, tc.term, p) {
        c = c.child(panel.pt(4.0));
    }
    c
}

/// Terms to list for the current Chapter: the ones it uses, in order of first appearance, then
/// ones that drift here by never being rendered.
fn listed(s: &State) -> Vec<(usize, u32)> {
    let stats = s.lib.stats(s.lib.current());
    if !stats.in_source.is_empty() {
        return stats.in_source.clone();
    }
    let mut v: Vec<(usize, u32)> = stats
        .present
        .iter()
        .map(|&t| (t, stats.counts[t]))
        .collect();
    for d in &stats.drift {
        if !v.iter().any(|(t, _)| *t == d.term_index) {
            v.push((d.term_index, 0));
        }
    }
    v
}

pub fn context(s: &State, p: &Pal) -> El {
    let cur = s.lib.current();
    let here = listed(s);
    let open = s.lib.open_suggestions;

    let tab = |label: String, on: bool, key: &str| {
        row()
            .key(key.to_string())
            .h(28.0)
            .px(10.0)
            .center_y()
            .radius(6.0)
            .cursor(Cursor::Pointer)
            .fill(if on { p.selected } else { Color::TRANSPARENT })
            .hover(p.hover)
            .child(text(label).font(if on { UI_MED } else { UI }).color(if on {
                p.ink
            } else {
                p.ink3
            }))
    };
    let header = row()
        .h(HEADER_H)
        .w_full()
        .pl(10.0)
        .pr(10.0)
        .gap(2.0)
        .center_y()
        .child(
            tab(
                format!("Terms · {}", here.len()),
                s.tab == Tab::Terms,
                "rt-terms",
            )
            .on_press(|s: &mut State| s.tab = Tab::Terms),
        )
        .child(
            tab(
                if open > 0 {
                    format!("Suggestions · {open}")
                } else {
                    "Suggestions".into()
                },
                s.tab == Tab::Suggestions,
                "rt-req",
            )
            .on_press(|s: &mut State| s.tab = Tab::Suggestions),
        )
        .child(el().grow(1.0))
        .child(
            icon_btn(&I_RIGHT_PANEL, "hide-right", false, p)
                .help("Hide context (Ctrl+J)")
                .on_press(|s: &mut State| s.toggle_context()),
        );

    let mut body = col().w_full().pb(24.0);
    match s.tab {
        Tab::Terms => {
            if let Some(tc) = s.term_card {
                body = body.child(
                    col()
                        .w_full()
                        .px(18.0)
                        .pt(14.0)
                        .pb(6.0)
                        .child(term_card(s, tc, p)),
                );
            }
            let untranslated = s.lib.chapters[cur].untranslated();
            let (label, note) = if untranslated {
                ("IN THE SOURCE", "how they will be translated")
            } else {
                ("IN THIS CHAPTER", "")
            };
            body = body.child(section(label, note.to_string(), p));
            for (ti, n) in here {
                let t = &s.lib.terms[ti];
                let on = s.term_card.is_some_and(|c| c.term == ti);
                let drifting = s.lib.stats(cur).drifts(ti);
                let mut second = row().gap(6.0).min_w(0.0).child(
                    text(t.source.clone())
                        .font(TextStyle::new(12.0, 400.0).family(CJK))
                        .color(p.ink3)
                        .truncate(),
                );
                if let Some(nv) = never(t) {
                    second = second.child(text(nv).font(SMALL).color(p.ink3).truncate());
                }
                if t.provisional {
                    second = second.child(
                        text("provisional")
                            .font(SMALL.italic())
                            .color(p.ink3)
                            .help("Proposed by the translation step; not yet accepted"),
                    );
                }
                body = body.child(
                    row()
                        .key(format!("tl{ti}"))
                        .mx(8.0)
                        .px(10.0)
                        .py(6.0)
                        .gap(10.0)
                        .center_y()
                        .radius(6.0)
                        .cursor(Cursor::Pointer)
                        .fill(if on { p.selected } else { Color::TRANSPARENT })
                        .hover(p.hover)
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(1.0)
                                .child(
                                    text(t.rendering.clone())
                                        .font(TextStyle::new(14.0, 500.0).family(SERIF))
                                        .color(p.ink)
                                        .truncate(),
                                )
                                .child(second),
                        )
                        .child(if drifting {
                            dot(p.red, 6.0).help("Drifting in this chapter")
                        } else {
                            el()
                        })
                        .child(
                            text(n.to_string())
                                .font(SMALL.features("tnum"))
                                .color(p.ink3),
                        )
                        .on_press_tx(move |s: &mut State, tx| s.show_term(Some(tx), ti)),
                );
            }
        }
        Tab::Suggestions => {
            let chapter = s.lib.chapters[cur].id;
            let here = |c: cipher_book::ChapterId| c == chapter;
            let (mut this, mut elsewhere) = (Vec::new(), Vec::new());
            for sug in &s.lib.suggestions {
                let n = notes::note(s, sug, p, !here(sug.chapter));
                if here(sug.chapter) {
                    this.push(n)
                } else {
                    elsewhere.push(n)
                }
            }
            for f in &s.fix.finished {
                let n = notes::finished_note(s, f, p, !here(f.chapter));
                if here(f.chapter) {
                    this.push(n)
                } else {
                    elsewhere.push(n)
                }
            }
            if this.is_empty() && elsewhere.is_empty() {
                body = body.child(
                    text("Nothing yet. Select text while reading and type what it should say. Suggestions wait here while you keep reading.")
                        .font(UI.lh(19.0))
                        .color(p.ink3)
                        .wrap()
                        .px(18.0)
                        .pt(18.0),
                );
            }
            for (label, list) in [("THIS CHAPTER", this), ("OTHER CHAPTERS", elsewhere)] {
                if list.is_empty() {
                    continue;
                }
                body = body.child(section(label, String::new(), p));
                let mut group = col().w_full().px(18.0).gap(22.0);
                for n in list {
                    group = group.child(n);
                }
                body = body.child(group);
            }
        }
    }
    col()
        .key("context")
        .w(RIGHT_W)
        .h_full()
        .fill(p.side)
        .border_sides([0.0, 0.0, 0.0, 1.0], p.rule)
        .child(header)
        .child(scrolling(body, "context-body"))
}
