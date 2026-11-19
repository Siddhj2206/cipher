//! What a Term's Drift is and what to do about it, shown in an open Cast row and on the Term
//! card: how the Book names it (the rendering and the other wordings, with counts, chapters
//! and examples), which wordings are the same name, and which one to keep.

use cipher_engine::{Evidence, Seen};
use rune::tok::TextStyle;
use rune::{Cursor, El, Span, col, rich, row, text};

use super::widgets::{btn, chip, plural};
use crate::state::State;
use crate::theme::{CJK, LABEL, Pal, SERIF, SMALL};

/// The most drifting Chapters listed by number.
const LISTED: usize = 12;

/// `None` for a Term that neither drifts nor was marked.
pub fn drift_panel(s: &State, term: usize, p: &Pal) -> Option<El> {
    let t = &s.lib.terms[term];
    let usage = s.lib.usage(term);
    if usage.drift_chapters.is_empty() && !t.may_omit {
        return None;
    }
    let sum = s.drift_summary(term);
    let mut c = col().w_full().gap(8.0);
    let mut lines: Vec<String> = Vec::new();
    if sum.forbidden_chapters > 0 {
        lines.push(format!(
            "A forbidden wording appears {} in {}.",
            plural(sum.forbidden_uses, "time"),
            plural(sum.forbidden_chapters, "chapter")
        ));
    }
    if sum.unnamed_chapters > 0 {
        lines.push(format!(
            "Left unnamed in {}: the source names it three or more times and the translation never says \u{201c}{}\u{201d}.",
            plural(sum.unnamed_chapters, "chapter"),
            t.rendering
        ));
    }
    if t.may_omit {
        lines.push("May go unnamed: chapters that leave it out aren't counted.".into());
    }
    for l in lines {
        c = c.child(text(l).font(SMALL).color(p.ink2).wrap());
    }
    if !usage.drift_chapters.is_empty() {
        c = c.child(chapter_chips(s, term, &usage.drift_chapters, p));
    }

    if let Some(fix) = s.drift_fix(term) {
        let n = crate::library::counts(fix);
        let id = fix.id;
        let working = fix.state == cipher_book::SuggestionState::Working;
        let line = if working {
            "The model is reading the paragraphs that leave it out.".to_string()
        } else if n.pending > 0 {
            format!(
                "{} to review under Suggestions.",
                plural(n.pending, "change")
            )
        } else {
            "Nothing left to review.".to_string()
        };
        c = c.child(text(line).font(SMALL).color(p.ink2).wrap()).child(
            row().gap(8.0).child(
                btn(if working { "Show it" } else { "Review" }, true, p)
                    .key(format!("fix-drift{term}"))
                    .on_press_tx(move |s: &mut State, tx| {
                        if working {
                            s.tab = crate::state::Tab::Suggestions;
                            s.show_context();
                        } else {
                            s.start_review(tx, id)
                        }
                    }),
            ),
        );
    }

    c = c.child(naming(s, term, p));
    let mut actions = row().gap(8.0).center_y().pt(2.0).flex_wrap();
    if t.may_omit {
        actions = actions.child(
            btn("Check it again", false, p)
                .key(format!("omit-off{term}"))
                .on_press(move |s: &mut State| s.set_may_omit(term, false)),
        );
    } else if sum.unnamed_chapters > 0 {
        actions = actions.child(
            btn("It can go unnamed", false, p)
                .key(format!("omit-on{term}"))
                .help("For a name the translation often replaces with a pronoun, or a source word with another meaning: leaving it out stops counting as drift. Forbidden wordings are still checked.")
                .on_press(move |s: &mut State| s.set_may_omit(term, true)),
        );
    }
    Some(c.child(actions))
}

fn chapter_chips(s: &State, term: usize, chapters: &[usize], p: &Pal) -> El {
    let mut chips = row().gap(4.0).flex_wrap();
    for &i in chapters.iter().take(LISTED) {
        chips = chips.child(
            text(s.lib.chapters[i].number.to_string())
                .key(format!("dch{term}-{i}"))
                .font(TextStyle::new(11.5, 500.0).features("tnum"))
                .color(p.red)
                .px(7.0)
                .py(2.0)
                .radius(9.0)
                .fill(p.red_tint)
                .cursor(Cursor::Pointer)
                .help("Open this chapter")
                .on_press_tx(move |s: &mut State, tx| {
                    s.screen = crate::state::Screen::Reading;
                    s.go(tx, i)
                }),
        );
    }
    let more = chapters.len().saturating_sub(LISTED);
    if more > 0 {
        chips = chips.child(
            text(format!("+{more} more"))
                .font(TextStyle::new(11.5, 400.0))
                .color(p.ink3)
                .py(2.0),
        );
    }
    chips
}

/// "HOW THE BOOK NAMES IT": the rendering and each other wording, the user's marks, Keep.
fn naming(s: &State, term: usize, p: &Pal) -> El {
    let t = &s.lib.terms[term];
    let mut c = col().w_full().gap(10.0).pt(6.0);
    let Some(look) = s.look_for(term) else {
        return c;
    };
    let head = |extra: String| {
        row()
            .w_full()
            .gap(8.0)
            .child(
                text("HOW THE BOOK NAMES IT")
                    .font(LABEL)
                    .color(p.ink3)
                    .grow(1.0),
            )
            .child(text(extra).font(SMALL).color(p.ink4))
    };
    let Some(ev) = &look.evidence else {
        let line = match &look.error {
            Some(e) => format!("Could not read the Book: {e}"),
            None => "Reading the Book\u{2026}".to_string(),
        };
        return c
            .child(head(String::new()))
            .child(text(line).font(SMALL).color(p.ink3).wrap());
    };
    c = c.child(head(format!(
        "{} name {}",
        plural(ev.naming, "paragraph"),
        t.source
    )));
    c = c.child(wording_row(
        s,
        term,
        ev,
        None,
        &t.rendering,
        &ev.rendered,
        p,
    ));
    for (i, w) in ev.wordings.iter().enumerate() {
        c = c.child(wording_row(s, term, ev, Some(i), &w.text, &w.seen, p));
    }
    if ev.wordings.is_empty() {
        c = c.child(
            text("No other wording comes up often where the source names it.")
                .font(SMALL)
                .color(p.ink3)
                .wrap(),
        );
    }
    if ev.neither > 0 {
        c = c.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(format!(
                        "{} name it neither way, often with a pronoun.",
                        plural(ev.neither, "paragraph")
                    ))
                    .font(SMALL)
                    .color(p.ink3)
                    .wrap()
                    .grow(1.0)
                    .min_w(0.0),
                )
                .child(
                    btn("Ask the model", false, p)
                        .key(format!("ask{term}"))
                        .help("Sends only these paragraphs. Each answer must name it and change little else, or it is left as it was.")
                        .on_press(move |s: &mut State| s.ask_model(term)),
                ),
        );
    }
    c
}

/// One wording: what it is, how much and where, its examples, Same name and Keep.
fn wording_row(
    s: &State,
    term: usize,
    ev: &Evidence,
    at: Option<usize>,
    wording: &str,
    seen: &Seen,
    p: &Pal,
) -> El {
    let look = s.look_for(term);
    let same = at.is_none_or(|i| look.and_then(|l| l.same.get(i).copied()).unwrap_or(false));
    let open = look.is_some_and(|l| l.examples == Some(at));
    let key = at.map_or("r".to_string(), |i| i.to_string());
    let w = at.map(|i| &ev.wordings[i]);

    let mut about: Vec<String> = Vec::new();
    if let Some((a, b)) = seen.chapters {
        about.push(if a == b {
            format!("ch {a}")
        } else {
            format!("ch {a}\u{2013}{b}")
        });
    }
    if at.is_none() {
        about.push("the glossary's".into());
    }
    if w.is_some_and(|w| w.forbidden) {
        about.push("forbidden".into());
    }
    if let Some(other) = w
        .and_then(|w| w.other_term)
        .and_then(|id| s.lib.terms.iter().find(|t| t.id == id))
    {
        about.push(format!("also {} = {}", other.source, other.rendering));
    }

    let mut top = row().w_full().gap(8.0).center_y().child(
        text(wording.to_string())
            .font(TextStyle::new(14.0, if at.is_none() { 600.0 } else { 500.0 }).family(SERIF))
            .color(p.ink)
            .truncate()
            .grow(1.0)
            .min_w(0.0),
    );
    top = top.child(
        text(seen.pairs.len().to_string())
            .font(SMALL.features("tnum"))
            .color(p.ink2),
    );
    let mut sub = row().w_full().gap(6.0).center_y().child(
        text(about.join(" \u{b7} "))
            .font(SMALL)
            .color(p.ink3)
            .truncate()
            .grow(1.0)
            .min_w(0.0),
    );
    if !seen.examples.is_empty() {
        sub = sub.child(
            text(if open { "Hide examples" } else { "Examples" })
                .font(SMALL)
                .color(p.ink2)
                .px(5.0)
                .radius(4.0)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .key(format!("ex{term}-{key}"))
                .on_press(move |s: &mut State| s.toggle_examples(at)),
        );
    }

    let mut actions = row().gap(6.0).center_y().flex_wrap();
    if let Some(i) = at {
        actions = actions.child(
            chip(
                if same {
                    "\u{2713} Same name"
                } else {
                    "Same name?"
                },
                same,
                p,
            )
            .key(format!("same{term}-{i}"))
            .help("Mark it if this wording is this Term. Only marked wordings are renamed.")
            .on_press(move |s: &mut State| s.mark_same(i, !same)),
        );
    }
    if same {
        let marks: Vec<bool> = look.map(|l| l.same.clone()).unwrap_or_default();
        let changes = ev.changes(at, &marks);
        if changes > 0 || at.is_some() {
            actions = actions.child(
                btn(&format!("Keep \u{b7} {}", plural(changes, "change")), false, p)
                    .key(format!("keep{term}-{key}"))
                    .help("Make this the Term's rendering and rename the wordings marked Same name to it. Nothing changes until you review the renames.")
                    .on_press(move |s: &mut State| s.keep_wording(term, at)),
            );
        }
    }

    let mut c = col()
        .w_full()
        .gap(3.0)
        .pl(10.0)
        .border_sides(
            [0.0, 0.0, 0.0, 2.0],
            if at.is_none() || same { p.ink3 } else { p.rule },
        )
        .child(top)
        .child(sub);
    if open {
        for e in &seen.examples {
            c = c.child(example(e, wording, p));
        }
    }
    c.child(actions.pt(3.0))
}

/// An example Pair: its Chapter, the source, the translation with the wording marked.
fn example(e: &cipher_engine::Example, wording: &str, p: &Pal) -> El {
    let spans: Vec<Span> = cipher_text::matching::find_rendering(&e.translation, wording)
        .into_iter()
        .map(|r| Span::new(r.start, r.end).bg(p.highlight, 1.0))
        .collect();
    col()
        .w_full()
        .gap(3.0)
        .py(4.0)
        .child(
            text(format!("Chapter {}", e.number.unwrap_or(0)))
                .font(SMALL)
                .color(p.ink4),
        )
        .child(
            text(e.source.clone())
                .font(TextStyle::new(12.5, 400.0).lh(19.0).family(CJK))
                .color(p.ink3)
                .wrap()
                .max_lines(3),
        )
        .child(
            rich(e.translation.clone(), spans)
                .font(TextStyle::new(13.0, 400.0).lh(19.0).family(SERIF))
                .color(p.ink2)
                .wrap()
                .max_lines(4),
        )
}
