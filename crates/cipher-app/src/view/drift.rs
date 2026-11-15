//! What a Term's Drift is and what to do about it: shown in an open Cast row and on the Term
//! card.

use rune::tok::TextStyle;
use rune::{Cursor, El, col, row, text};

use super::widgets::{btn, plural};
use crate::state::State;
use crate::theme::{Pal, SMALL};

/// The most drifting Chapters listed by number.
const LISTED: usize = 12;

/// Summary lines, the drifting Chapters, and Fix / May go unnamed. `None` for a Term that
/// neither drifts nor was marked.
pub fn drift_panel(s: &State, term: usize, p: &Pal) -> Option<El> {
    let t = &s.lib.terms[term];
    let usage = s.lib.usage(term);
    if usage.drift_chapters.is_empty() && !t.may_omit {
        return None;
    }
    let sum = s.drift_summary(term);
    let mut c = col().w_full().gap(8.0);
    if sum.forbidden_chapters > 0 {
        let words = t
            .forbidden
            .iter()
            .map(|f| format!("\u{201c}{f}\u{201d}"))
            .collect::<Vec<_>>()
            .join(" or ");
        c = c.child(
            text(format!(
                "{words} appears {} in {}, where it should read \u{201c}{}\u{201d}.",
                plural(sum.forbidden_uses, "time"),
                plural(sum.forbidden_chapters, "chapter"),
                t.rendering
            ))
            .font(SMALL)
            .color(p.ink2)
            .wrap(),
        );
    }
    if sum.unnamed_chapters > 0 {
        c = c.child(
            text(format!(
                "Left unnamed in {}: the source names it three or more times and the translation never does.",
                plural(sum.unnamed_chapters, "chapter")
            ))
            .font(SMALL)
            .color(p.ink2)
            .wrap(),
        );
    }
    if t.may_omit {
        c = c.child(
            text("May go unnamed: chapters that leave it out aren't counted.")
                .font(SMALL)
                .color(p.ink3)
                .wrap(),
        );
    }

    if !usage.drift_chapters.is_empty() {
        let mut chips = row().gap(4.0).flex_wrap();
        for &i in usage.drift_chapters.iter().take(LISTED) {
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
        let more = usage.drift_chapters.len().saturating_sub(LISTED);
        if more > 0 {
            chips = chips.child(
                text(format!("+{more} more"))
                    .font(TextStyle::new(11.5, 400.0))
                    .color(p.ink3)
                    .py(2.0),
            );
        }
        c = c.child(chips);
    }

    let mut actions = row().gap(8.0).center_y().pt(2.0);
    let mut status: Option<String> = None;
    let n = usage.drift_chapters.len();
    if let Some(fix) = s.drift_fix(term) {
        let c = crate::library::counts(fix);
        let id = fix.id;
        let line = if fix.state == cipher_book::SuggestionState::Working {
            "A fix is on its way: the model is reading the paragraphs.".to_string()
        } else if c.pending > 0 {
            format!(
                "A fix is ready: {} to review{}.",
                plural(c.pending, "change"),
                if c.skipped > 0 {
                    format!(", {} the model left as they were", c.skipped)
                } else {
                    String::new()
                }
            )
        } else {
            "The fix changed nothing: the model left every paragraph as it was.".to_string()
        };
        status = Some(line);
        let working = fix.state == cipher_book::SuggestionState::Working;
        actions = actions.child(
            btn(
                if working {
                    "Show the fix"
                } else {
                    "Review the fix"
                },
                true,
                p,
            )
            .key(format!("fix-drift{term}"))
            .on_press_tx(move |s: &mut State, tx| {
                if working {
                    s.fix_drift(term)
                } else {
                    s.start_review(tx, id)
                }
            }),
        );
    } else if n > 0 {
        status = Some(
            "Fixing makes one Suggestion: a wrong wording is replaced word for word, and the model names it where it was left out. Nothing changes until you review it."
                .to_string(),
        );
        actions = actions.child(
            btn(&format!("Fix in {}", plural(n, "chapter")), true, p)
                .key(format!("fix-drift{term}"))
                .on_press(move |s: &mut State| s.fix_drift(term)),
        );
    }
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
                .help("For a name the translation often replaces with a pronoun: leaving it out stops counting as drift. Forbidden wordings are still checked.")
                .on_press(move |s: &mut State| s.set_may_omit(term, true)),
        );
    }
    if let Some(line) = status {
        c = c.child(text(line).font(SMALL).color(p.ink2).wrap());
    }
    Some(c.child(actions))
}
