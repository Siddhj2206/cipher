use cipher_book::UndoMode;
use rune::tok::TextStyle;
use rune::{El, ScrollSpec, col, row, text};

use super::widgets::{ago, btn};
use crate::history::{change_words, entity_words};
use crate::library::undo_mode;
use crate::state::State;
use crate::theme::{LABEL, Pal, SERIF, SMALL, UI};

pub fn history_page(s: &State, p: &Pal) -> El {
    let mut body = col()
        .w_full()
        .max_w(760.0)
        .gap(12.0)
        .child(text("HISTORY").font(LABEL).color(p.red))
        .child(
            text("What changed in this book")
                .font(TextStyle::new(30.0, 500.0).family(SERIF))
                .color(p.ink)
                .wrap(),
        );
    let mut expanded = None;
    if let Some(preview) = &s.history.preview {
        let mut detail = col()
            .w_full()
            .px(16.0)
            .py(16.0)
            .gap(10.0)
            .fill(p.card)
            .radius(6.0)
            .child(text(&preview.revision.label).font(UI).color(p.ink).wrap())
            .child(
                text(
                    preview
                        .changes
                        .iter()
                        .map(|(e, n)| entity_words(e, *n))
                        .collect::<Vec<_>>()
                        .join(", "),
                )
                .font(SMALL)
                .color(p.ink2)
                .wrap(),
            );
        let mut chapters = row().w_full().gap(8.0).flex_wrap();
        for id in &preview.chapters {
            if let Some(index) = s.lib.chapter_index(*id) {
                chapters = chapters.child(
                    btn(
                        &format!("Chapter {}", s.lib.chapters[index].number),
                        false,
                        p,
                    )
                    .key(format!("history-chapter-{}", id.0))
                    .on_press_tx(move |s: &mut State, tx| s.go(tx, index)),
                );
            } else {
                chapters = chapters.child(
                    text(format!("Chapter {} (not currently in this book)", id.0))
                        .font(SMALL)
                        .color(p.ink3)
                        .wrap(),
                );
            }
        }
        detail = detail.child(chapters);
        for id in &preview.chapters {
            let conflicts: Vec<_> = preview
                .conflict_chapters
                .iter()
                .filter(|(_, c)| c == id)
                .map(|(c, _)| c.entity)
                .collect();
            if !conflicts.is_empty() {
                let chapter = s
                    .lib
                    .chapter_index(*id)
                    .map(|i| s.lib.chapters[i].number)
                    .unwrap_or(id.0);
                detail = detail.child(
                    text(format!(
                        "{} in Chapter {chapter} changed since",
                        change_words(conflicts.into_iter())
                    ))
                    .font(SMALL)
                    .color(p.red)
                    .wrap(),
                );
            }
        }
        let unplaced: Vec<_> = preview
            .conflicts
            .iter()
            .filter(|c| {
                !preview
                    .conflict_chapters
                    .iter()
                    .any(|(other, _)| other == *c)
            })
            .map(|c| c.entity)
            .collect();
        if !unplaced.is_empty() {
            detail = detail.child(
                text(format!(
                    "{} changed since",
                    change_words(unplaced.into_iter())
                ))
                .font(SMALL)
                .color(p.red)
                .wrap(),
            );
        }
        let blocked =
            undo_mode(&preview.revision) == UndoMode::Whole && !preview.conflicts.is_empty();
        let undone = preview.already_undone_by.is_some();
        let label = if !blocked && !preview.conflicts.is_empty() {
            "Undo what's unchanged"
        } else {
            "Undo"
        };
        let button = btn(label, true, p)
            .key("history-undo")
            .focusable(!blocked && !undone)
            .focus_ring(2.0, 2.0, p.red);
        let actions = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(if blocked || undone {
                button.opacity(0.4)
            } else {
                button.on_press(|s: &mut State| s.undo_history())
            });
        detail = detail.child(actions);
        if blocked || undone {
            detail = detail.child(text(if undone { "Already undone. Select its Undo revision to redo." } else if preview.revision.kind == cipher_book::RevisionKind::TermMerge { "Nothing will be undone: later work changed this merge. Term merges must be undone as a whole." } else if preview.conflicts.iter().any(|c| c.entity == "job") { "Nothing will be undone: this chapter has started work, saved progress or dependent jobs. Finish that work before undoing the chapter." } else { "Nothing will be undone: later work changed these chapters. Structural changes must be undone as a whole." }).font(SMALL).color(p.ink3).wrap());
        }
        expanded = Some(detail);
    } else if let Some(error) = &s.history.preview_error {
        expanded = Some(text(error).font(SMALL).color(p.red).wrap());
    }
    if s.history.revisions.is_empty() {
        body = body.child(text("No changes yet.").font(UI).color(p.ink3));
    }
    for (i, revision) in s.history.revisions.iter().enumerate() {
        let selected = i == s.history.selected;
        let label = format!(
            "{}{}",
            revision.label,
            if revision.undone_by.is_some() {
                " · Undone"
            } else {
                ""
            }
        );
        body = body.child(
            col()
                .w_full()
                .px(12.0)
                .py(12.0)
                .gap(4.0)
                .radius(4.0)
                .fill(if selected { p.selected } else { p.paper })
                .child(
                    text(label)
                        .font(UI)
                        .color(if revision.undone_by.is_some() {
                            p.ink3
                        } else {
                            p.ink
                        })
                        .wrap(),
                )
                .child(
                    text(format!(
                        "{} · {}",
                        revision.kind.as_str().replace('_', " "),
                        ago(revision.created_at)
                    ))
                    .font(SMALL)
                    .color(p.ink3),
                )
                .key(format!("history-row-{i}"))
                .on_press(move |s: &mut State| s.select_history(i)),
        );
        if selected && let Some(detail) = expanded.take() {
            body = body.child(detail);
        }
    }
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(24.0)
        .py(32.0)
        .items_center()
        .child(body)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .key("history-page")
}
