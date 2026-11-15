//! A Suggestion as a note: its Scope in red, the quote, what the user said, a status line, and
//! what can be done with it. Shown in the context sidebar, or in the page margin when the
//! sidebar is closed.

use cipher_book::{EditOrigin, Scope, Suggestion, SuggestionState};
use rune::tok::TextStyle;
use rune::{El, col, row, text};

use super::diff::sample;
use super::widgets::{btn, plural, short};
use crate::fix::{Finished, work::Status};
use crate::library::counts;
use crate::state::State;
use crate::theme::{LABEL, Pal, SERIF, SMALL};

fn scope_label(scope: Scope, quote_is_term: bool) -> &'static str {
    match (scope, quote_is_term) {
        (Scope::Here, _) => "JUST HERE",
        (Scope::Everywhere, _) => "EVERYWHERE",
        (Scope::FromNowOn, true) => "EVERYWHERE + GLOSSARY",
        (Scope::FromNowOn, false) => "FROM NOW ON",
    }
}

fn is_term(s: &State, sug: &Suggestion) -> bool {
    let q = sug.quote.trim().to_lowercase();
    s.lib.terms.iter().any(|t| {
        t.rendering.to_lowercase() == q || t.forbidden.iter().any(|f| f.to_lowercase() == q)
    })
}

/// The frame every note shares.
fn frame(label: &str, live: bool, quote: &str, said: &str, p: &Pal, where_: Option<El>) -> El {
    let mut head = row().w_full().gap(6.0).center_y().child(
        text(label.to_string())
            .font(LABEL)
            .color(if live { p.red } else { p.ink3 })
            .grow(1.0),
    );
    if let Some(w) = where_ {
        head = head.child(w);
    }
    col()
        .w_full()
        .gap(4.0)
        .pl(12.0)
        .border_sides([0.0, 0.0, 0.0, 2.0], if live { p.red } else { p.ink4 })
        .child(head)
        .child(
            text(format!("“{quote}”"))
                .font(TextStyle::new(14.0, 400.0).italic().family(SERIF))
                .color(p.ink3)
                .max_lines(2),
        )
        .child(
            text(said.to_string())
                .font(TextStyle::new(14.0, 550.0))
                .color(p.ink)
                .wrap(),
        )
}

fn where_link(
    s: &State,
    ch: cipher_book::ChapterId,
    block: cipher_book::BlockId,
    key: String,
    p: &Pal,
) -> El {
    let number = s
        .lib
        .chapter_index(ch)
        .map_or(0, |i| s.lib.chapters[i].number);
    text(format!("ch {number}"))
        .font(SMALL)
        .color(p.ink3)
        .px(6.0)
        .radius(4.0)
        .hover(p.hover)
        .cursor(rune::Cursor::Pointer)
        .key(key)
        .on_press_tx(move |s: &mut State, tx| s.go_block(tx, ch, block))
}

/// A Suggestion that is still open.
pub fn note(s: &State, sug: &Suggestion, p: &Pal, show_where: bool) -> El {
    let c = counts(sug);
    let id = sug.id;
    let grouped = c.pending > 0
        && sug
            .edits
            .iter()
            .filter(|e| e.state == cipher_book::EditState::Pending && e.after.is_some())
            .all(|e| e.origin == EditOrigin::Rename);
    let live = c.pending > 0 || sug.state == SuggestionState::Working;
    let where_ =
        show_where.then(|| where_link(s, sug.chapter, sug.block, format!("where{}", id.0), p));
    let label = if sug.kind == cipher_book::SuggestionKind::Drift {
        "FIXING DRIFT"
    } else {
        scope_label(sug.scope, is_term(s, sug))
    };
    let mut n = frame(label, live, &sug.quote, &sug.said, p, where_);

    let mut lines: Vec<String> = Vec::new();
    if c.pending > 0 {
        let what = if grouped { "exact replacement" } else { "edit" };
        lines.push(format!(
            "{}{} in {}",
            if grouped { "" } else { "Ready · " },
            plural(c.pending, what),
            plural(c.chapters, "chapter")
        ));
    }
    if sug.state == SuggestionState::Working {
        lines.push(match &s.bg.status {
            Status::Running(_) => "Working · the model is reading the rest".to_string(),
            Status::NoEndpoint => "Waiting for an Endpoint".to_string(),
            Status::Unavailable(_) => "Waiting for an Endpoint · could not connect".to_string(),
        });
    }
    if c.stale > 0 {
        lines.push(format!(
            "{} changed since, so it is not applied",
            plural(c.stale, "paragraph")
        ));
    }
    if c.skipped > 0 {
        let why = sug
            .edits
            .iter()
            .find(|e| e.after.is_none())
            .and_then(|e| e.skip_reason.as_deref())
            .map(|r| format!(" · “{}”", short(r)))
            .unwrap_or_default();
        lines.push(format!("{} left as they are{why}", c.skipped));
    }
    if lines.is_empty() {
        lines.push("Nothing to change".to_string());
    }
    for l in lines {
        n = n.child(text(l).font(SMALL).color(p.ink3).wrap());
    }

    let mut actions = row().gap(6.0).pt(4.0);
    if grouped {
        for e in sug
            .edits
            .iter()
            .filter(|e| e.state == cipher_book::EditState::Pending && e.after.is_some())
            .take(2)
        {
            n = n.child(sample(&e.before, e.after.as_deref().unwrap_or(""), p));
        }
        actions = actions
            .child(
                btn(&format!("Accept all {}", c.pending), true, p)
                    .key(format!("all{}", id.0))
                    .on_press(move |s: &mut State| s.accept_all(id)),
            )
            .child(
                btn("One by one", false, p)
                    .key(format!("rev{}", id.0))
                    .on_press_tx(move |s: &mut State, tx| s.start_review(tx, id)),
            );
    } else if c.pending > 0 {
        actions = actions
            .child(
                btn("Review", true, p)
                    .key(format!("rev{}", id.0))
                    .on_press_tx(move |s: &mut State, tx| s.start_review(tx, id)),
            )
            .child(
                btn("Accept all", false, p)
                    .key(format!("all{}", id.0))
                    .on_press(move |s: &mut State| s.accept_all(id)),
            );
    } else if c.skipped > 0 {
        actions = actions.child(
            btn("Dismiss", false, p)
                .key(format!("all{}", id.0))
                .on_press(move |s: &mut State| s.accept_all(id)),
        );
    } else if c.stale > 0 {
        actions = actions.child(
            btn("Dismiss", false, p)
                .key(format!("dismiss{}", id.0))
                .on_press(move |s: &mut State| s.dismiss_stale(id)),
        );
    }
    n.child(actions)
}

/// A Suggestion whose Review was applied in this session: its result and Undo.
pub fn finished_note(s: &State, f: &Finished, p: &Pal, show_where: bool) -> El {
    let id = f.suggestion;
    let where_ = show_where.then(|| where_link(s, f.chapter, f.block, format!("where{}", id.0), p));
    let quote_is_term = s
        .lib
        .terms
        .iter()
        .any(|t| t.rendering.eq_ignore_ascii_case(f.said.trim()));
    frame(
        scope_label(f.scope, quote_is_term),
        false,
        &f.quote,
        &f.said,
        p,
        where_,
    )
    .child(text(f.summary()).font(SMALL).color(p.ink3).wrap())
    .child(
        row().pt(4.0).child(
            btn("Undo", false, p)
                .key(format!("undo{}", id.0))
                .on_press(move |s: &mut State| s.undo(id)),
        ),
    )
}
