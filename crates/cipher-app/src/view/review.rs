//! The Review bar at the bottom of the page, and the toast that follows a Review.

use rune::{Color, Cursor, El, Icon, el, icon, row, text};

use super::widgets::{card, short};
use crate::fix::queue;
use crate::state::State;
use crate::theme::{I_DOWN, I_UP, I_X, Pal, SMALL, UI, UI_MED};

/// Where the bar and toast sit: centred on the page, between the sidebars.
fn pinned(s: &State, bottom: f32, child: El) -> El {
    let (inset_l, inset_r) = s.sidebar_layout().insets();
    row()
        .pin(Some(inset_l), None, Some(inset_r), Some(bottom))
        .justify_center()
        .child(child)
}

pub fn bar(s: &State, p: &Pal) -> Option<El> {
    let review = s.fix.review.as_ref()?;
    let sug = s.lib.suggestion(review.suggestion)?;
    let q = queue(sug);
    let edit = q.iter().find(|e| e.id == review.edit)?;
    // "3 of 143": what is decided in this Review, then this one.
    let total = review.acc.applied + review.acc.skipped + review.acc.stale + q.len();
    let at = review.acc.applied + review.acc.skipped + review.acc.stale + 1;
    let chapter = s
        .lib
        .chapter_index(edit.chapter)
        .map_or(0, |i| s.lib.chapters[i].number);
    let skipped_edit = edit.after.is_none();
    let nav = |i: &'static Icon, k: &str| {
        el().key(k.to_string())
            .size(26.0, 26.0)
            .center()
            .radius(5.0)
            .hover(p.bar_hover)
            .cursor(Cursor::Pointer)
            .child(icon(i, 15.0).color(p.bar_fg))
    };
    let dbtn = |l: &str, k: &str, key: &str, primary: bool| {
        row()
            .key(key.to_string())
            .h(28.0)
            .px(10.0)
            .gap(6.0)
            .center_y()
            .radius(6.0)
            .cursor(Cursor::Pointer)
            .fill(if primary { p.red } else { Color::TRANSPARENT })
            .hover(if primary { p.red_soft } else { p.bar_hover })
            .child(text(l.to_string()).font(UI_MED).color(if primary {
                p.on_red
            } else {
                p.bar_fg
            }))
            .child(
                text(k.to_string())
                    .font(SMALL)
                    .color(if primary { p.on_red } else { p.bar_fg2 })
                    .opacity(if primary { 0.7 } else { 1.0 }),
            )
    };
    let title = if skipped_edit {
        match edit.skip_reason.as_deref().filter(|r| !r.is_empty()) {
            Some(why) => format!("Left as it is: {why}"),
            None => format!("Left as it is · “{}”", short(&sug.quote)),
        }
    } else if sug.kind == cipher_book::SuggestionKind::Drift {
        format!("Naming “{}”", short(&sug.quote))
    } else {
        format!("“{}” → “{}”", short(&sug.quote), short(&sug.said))
    };
    let mut b = row()
        .gap(10.0)
        .h(48.0)
        .px(14.0)
        .center_y()
        .radius(12.0)
        .fill(p.bar)
        .border(1.0, p.rule)
        .shadow(10.0, 30.0, p.shadow)
        .child(
            rune::col()
                .gap(1.0)
                .pr(6.0)
                .max_w(520.0)
                .child(text(title).font(UI_MED).color(p.bar_fg).max_lines(1))
                .child(
                    text(format!("{at} of {total} · Chapter {chapter}"))
                        .font(SMALL)
                        .color(p.bar_fg2),
                ),
        )
        .child(nav(&I_UP, "r-prev").on_press_tx(|s: &mut State, tx| s.step_edit(tx, -1)))
        .child(nav(&I_DOWN, "r-next").on_press_tx(|s: &mut State, tx| s.step_edit(tx, 1)))
        .child(el().w(1.0).h(22.0).fill(p.bar_hover))
        .child(
            dbtn("Skip", "X", "r-skip", false)
                .on_press_tx(|s: &mut State, tx| s.decide_current(tx, false)),
        );
    if !skipped_edit {
        b = b.child(
            dbtn("Accept rest", "⇧A", "r-all", false)
                .on_press_tx(|s: &mut State, tx| s.accept_rest(tx)),
        );
    }
    b = b
        .child(
            dbtn(
                if skipped_edit { "Dismiss" } else { "Accept" },
                "A",
                "r-accept",
                true,
            )
            .on_press_tx(|s: &mut State, tx| s.decide_current(tx, true)),
        )
        .child(
            el().key("r-close")
                .size(26.0, 26.0)
                .center()
                .radius(5.0)
                .hover(p.bar_hover)
                .cursor(Cursor::Pointer)
                .child(icon(&I_X, 14.0).color(p.bar_fg2))
                .on_press_tx(|s: &mut State, tx| s.leave_review(tx)),
        );
    Some(pinned(s, 26.0, b))
}

/// A short message that stays until it is closed; after a Review it offers Undo.
pub fn toast(s: &State, p: &Pal) -> Option<El> {
    let t = s.toast.as_ref()?;
    let mut r = row()
        .gap(10.0)
        .px(14.0)
        .h(36.0)
        .center_y()
        .child(text(t.clone()).font(UI).color(p.ink2));
    if let Some(id) = s.toast_undo {
        r = r.child(
            text("Undo")
                .font(UI_MED)
                .color(p.red)
                .px(6.0)
                .radius(4.0)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .key("toast-undo")
                .on_press(move |s: &mut State| s.undo(id)),
        );
    }
    if let Some(revision) = s.term_toast_undo {
        r = r.child(
            text("Undo")
                .font(UI_MED)
                .color(p.red)
                .cursor(Cursor::Pointer)
                .key("term-toast-undo")
                .on_press(move |s: &mut State| s.undo_cast_term(revision)),
        );
    }
    r = r.child(
        el().size(18.0, 18.0)
            .center()
            .cursor(Cursor::Pointer)
            .child(icon(&I_X, 12.0).color(p.ink3))
            .key("toast-x")
            .on_press(|s: &mut State| {
                s.toast = None;
                s.toast_undo = None;
                s.term_toast_undo = None;
            }),
    );
    let lift = if s.fix.review.is_some() { 86.0 } else { 26.0 };
    Some(pinned(
        s,
        lift,
        card(p).key("toast").shadow(6.0, 18.0, p.shadow).child(r),
    ))
}
