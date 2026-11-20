use cipher_book::TermMerge;
use rune::{Cursor, El, col, row, text, text_input};

use super::widgets::btn;
use crate::state::State;
use crate::terms::TermMergeEditor;
use crate::theme::{CJK, LABEL, Pal, SMALL, UI, UI_MED};

const RESULTS: usize = 6;

fn action(label: &str, key: &str, primary: bool, p: &Pal) -> El {
    btn(label, primary, p)
        .key(key.to_string())
        .focusable(true)
        .focus_ring(2.0, 2.0, p.red)
}

pub fn panel(s: &State, editor: &TermMergeEditor, index: usize, p: &Pal) -> El {
    let mut card = col()
        .w_full()
        .padding(14.0)
        .gap(10.0)
        .radius(8.0)
        .border(1.0, p.rule)
        .fill(p.card)
        .child(text("MERGE DUPLICATE TERMS").font(LABEL).color(p.red));
    if let Some(preview) = &editor.preview {
        return card.child(review(preview, p));
    }

    let anchor = &s.lib.terms[index];
    let query = editor.search.text.trim().to_lowercase();
    let mut results: Vec<_> = s
        .lib
        .terms
        .iter()
        .filter(|t| t.id != editor.anchor && t.matches_query(&query))
        .collect();
    results.sort_by(|a, b| {
        let similar = |t: &crate::library::TermInfo| {
            t.rendering.eq_ignore_ascii_case(&anchor.rendering)
                || t.source_forms()
                    .any(|f| anchor.source_forms().any(|other| other == f))
        };
        similar(b)
            .cmp(&similar(a))
            .then(a.rendering.cmp(&b.rendering))
    });
    card = card.child(text(format!(
        "Find the Term that refers to the same name as “{}”. You’ll choose which one to keep before merging.",
        anchor.rendering
    )).font(SMALL).color(p.ink2).wrap())
        .child(text_input(&editor.search, UI)
            .key("term-merge-search").label("Find a duplicate Term").w_full()
            .px(10.0).py(6.0).border(1.0, p.rule).radius(5.0)
            .single_line().color(p.ink).caret(p.red, p.highlight)
            .placeholder("Search by name or source form", p.ink4)
            .on_edit(|s: &mut State| &mut s.term_merge.as_mut().expect("merge open").search));
    for t in results.iter().take(RESULTS) {
        let id = t.id;
        let forms = t.source_forms().collect::<Vec<_>>().join(" / ");
        let status = if t.provisional { "Pending" } else { "Accepted" };
        card = card.child(
            col()
                .key(format!("term-merge-partner-{}", id.0))
                .w_full()
                .px(10.0)
                .py(5.0)
                .gap(2.0)
                .radius(5.0)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .focusable(true)
                .focus_ring(2.0, 2.0, p.red)
                .on_press(move |s: &mut State| s.preview_term_merge(id))
                .child(text(&t.rendering).font(UI_MED).color(p.ink).wrap())
                .child(
                    text(format!("{forms}  ·  {status}"))
                        .font(SMALL.family(CJK))
                        .color(p.ink3)
                        .wrap(),
                ),
        );
    }
    if results.is_empty() || results.len() > RESULTS {
        let hint = if results.is_empty() {
            "No other Term matches.".into()
        } else {
            format!(
                "Showing {RESULTS} of {} matches. Type to narrow the list.",
                results.len()
            )
        };
        card = card.child(text(hint).font(SMALL).color(p.ink3).wrap());
    }
    card.child(
        row().child(
            action("Cancel", "term-merge-cancel", false, p)
                .on_press(|s: &mut State| s.term_merge = None),
        ),
    )
}

fn review(preview: &TermMerge, p: &Pal) -> El {
    let keep = &preview.survivor;
    let remove = &preview.duplicate;
    let kept = &keep.fields;
    let dropped = &remove.fields;
    let swap =
        action("Swap which Term to keep", "term-merge-swap", false, p).on_press(|s: &mut State| {
            if let Some(preview) = s.term_merge.as_mut().and_then(|e| e.preview.as_mut()) {
                std::mem::swap(&mut preview.survivor, &mut preview.duplicate);
            }
        });
    let detail = |label: &str, value: String| {
        col()
            .w_full()
            .gap(2.0)
            .child(text(label.to_string()).font(LABEL).color(p.ink3))
            .child(text(value).font(UI).color(p.ink).wrap())
    };
    let aliases = preview.aliases();
    let forbidden = preview.forbidden();
    let mut body = col()
        .w_full()
        .gap(10.0)
        .child(detail(
            "KEEP",
            format!(
                "{} · {} ({})",
                kept.rendering,
                kept.source,
                keep.status.as_str()
            ),
        ))
        .child(detail(
            "MERGE INTO IT",
            format!(
                "{} · {} ({})",
                dropped.rendering,
                dropped.source,
                remove.status.as_str()
            ),
        ))
        .child(row().child(swap))
        .child(detail(
            "SOURCE FORMS KEPT",
            keep.source_forms()
                .take(1)
                .chain(aliases)
                .collect::<Vec<_>>()
                .join(" · "),
        ))
        .child(detail("PREFERRED RENDERING", kept.rendering.clone()))
        .child(detail(
            "FORBIDDEN RENDERINGS AFTER MERGE",
            if forbidden.is_empty() {
                "None".into()
            } else {
                forbidden.join(" · ")
            },
        ));

    let mut differences = Vec::new();
    if kept.rendering != dropped.rendering {
        differences.push(format!(
            "Preferred wording: keep “{}”, not “{}”.",
            kept.rendering, dropped.rendering
        ));
    }
    let duplicates: Vec<_> = remove
        .source_forms()
        .filter(|f| keep.source_forms().any(|other| f == &other))
        .collect();
    if !duplicates.is_empty() {
        differences.push(format!(
            "Shared source forms are kept once: {}.",
            duplicates.join(" · ")
        ));
    }
    for (label, a, b) in [
        ("Note", kept.note.as_deref(), dropped.note.as_deref()),
        ("Kind", kept.kind.as_deref(), dropped.kind.as_deref()),
        ("Gender", kept.gender.as_deref(), dropped.gender.as_deref()),
    ] {
        if a != b {
            differences.push(format!(
                "{label}: keep {}; discard {}.",
                quoted(a),
                quoted(b)
            ));
        }
    }
    if kept.may_omit != dropped.may_omit {
        differences.push(if kept.may_omit {
            "Drift rule: the kept Term may go unnamed.".into()
        } else {
            "Drift rule: the kept Term is checked for missing names.".into()
        });
    }
    if keep.status != remove.status {
        differences.push(format!("Status: stays {}.", keep.status.as_str()));
    }
    if !differences.is_empty() {
        body = body.child(detail("DIFFERENCES TO REVIEW", differences.join("\n")));
    }
    body.child(text("Chapter text won’t change. Drift may flag old wording for review. Undo restores both Terms in one History step.")
        .font(SMALL).color(p.ink3).wrap())
        .child(row().w_full().gap(8.0).flex_wrap()
            .child(action("Merge Terms", "term-merge-confirm", true, p)
                .on_press_tx(|s: &mut State, tx| {
                    if s.confirm_term_merge() && let Some(i) = s.cast_open {
                        tx.scroll_key_to(format!("term{i}"), 120.0);
                    }
                }))
            .child(action("Choose another", "term-merge-back", false, p)
                .on_press(|s: &mut State| {
                    if let Some(editor) = &mut s.term_merge {
                        editor.preview = None;
                    }
                }))
            .child(action("Cancel", "term-merge-cancel", false, p)
                .on_press(|s: &mut State| s.term_merge = None)))
}

fn quoted(value: Option<&str>) -> String {
    value.map_or_else(|| "none".into(), |s| format!("“{s}”"))
}
