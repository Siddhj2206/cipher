//! The new-Chapter page: set like a Chapter's opening page, with the source pasted or typed
//! straight onto it. What Cipher reads from the text (number, title, where it goes) shows in
//! the opener as it changes.

use rune::tok::TextStyle;
use rune::{Cursor, El, Key, ScrollSpec, col, el, row, text, text_input};

use super::widgets::{btn, kbd, plural};
use crate::fix::work::Status;
use crate::library::IntakeIssue;
use crate::state::State;
use crate::theme::{GUTTER_W, LABEL, Pal, SERIF, SMALL, UI, src_body};

pub fn draft_page(s: &State, p: &Pal) -> El {
    let Some(d) = &s.draft else { return col() };
    let it = s.draft_intake();
    let plan = s.draft_plan();
    let batch = plan.len() > 1 || !d.files.is_empty();
    let included = plan.iter().filter(|part| part.include).count();
    let width = s.text_width();
    let empty = if batch { included == 0 } else { it.is_none() };
    let (number, read) = match it.as_ref().and_then(|i| i.number) {
        Some(n) => (n, true),
        None => (s.next_number(), false),
    };

    // What Cipher makes of it, under the number.
    let mut notes = col().gap(4.0).pt(18.0);
    match &it {
        None => {
            notes = notes.child(
                text("Paste the chapter onto the page, or type it. A first line such as “321화” or “Chapter 321” gives it its number.")
                    .font(SMALL)
                    .color(p.ink3)
                    .wrap(),
            )
        }
        Some(i) => {
            let place = match (i.number, i.after) {
                (_, Some(after)) => format!("goes after chapter {after}"),
                (Some(_), None) => "goes first".to_string(),
                (None, None) => "goes at the end".to_string(),
            };
            notes = notes.child(
                text(format!("{} · {place}", plural(i.blocks.len(), "paragraph")))
                    .key("draft-meta")
                    .font(SMALL)
                    .color(p.ink3),
            );
            if i.number.is_none() {
                notes = notes.child(
                    text(format!("No chapter number in the first lines, so it becomes chapter {number}."))
                        .font(SMALL)
                        .color(p.amber)
                        .wrap(),
                );
            }
            if i.exists {
                notes = notes.child(
                    text("This chapter is already in the book; adding it makes a second copy.")
                        .font(SMALL)
                        .color(p.red)
                        .wrap(),
                );
            }
        }
    }

    if batch {
        notes = col().gap(4.0).pt(18.0);
        let first = plan
            .iter()
            .find(|part| part.include)
            .and_then(|part| part.intake.number);
        let after = first.and_then(|n| {
            s.lib
                .chapters
                .iter()
                .filter(|c| c.number < n)
                .map(|c| c.number)
                .max()
        });
        let place = match after {
            Some(after) => format!("go after chapter {after}"),
            None => "go first".into(),
        };
        notes = notes.child(
            text(if included == 0 {
                "nothing selected".into()
            } else {
                format!("{} · {place}", plural(included, "chapter"))
            })
            .key("draft-meta")
            .font(SMALL)
            .color(p.ink3),
        );
    }
    let number_label = if batch {
        crate::draft::batch_range(&plan)
    } else {
        number.to_string()
    };
    let mut opener = col()
        .key("draft-opener")
        .pl(GUTTER_W)
        .gap(2.0)
        .child(
            text(if batch { "NEW CHAPTERS" } else { "NEW CHAPTER" })
                .font(LABEL)
                .color(p.red),
        )
        .child(
            text(number_label)
                .font(TextStyle::new(60.0, 300.0).lh(72.0).family(SERIF))
                .color(if if batch { included > 0 } else { read } {
                    p.ink
                } else {
                    p.ink4
                }),
        );
    if let Some(title) = it.as_ref().filter(|_| !batch).and_then(|i| i.title.clone()) {
        opener = opener.child(
            text(title)
                .font(TextStyle::new(20.0, 400.0).italic().family(SERIF))
                .color(p.ink2)
                .pt(4.0),
        );
    }
    opener = opener
        .child(el().w(36.0).h(1.5).fill(p.ink).mt(14.0))
        .child(notes.w(width));

    // Translation needs a model; say so here, where it matters, without stopping anything.
    let connect = matches!(s.bg.status, Status::NoEndpoint);
    let mut page = col()
        .key("page")
        .w(width + GUTTER_W)
        .pt(56.0)
        .pb(160.0)
        .child(opener);
    if connect {
        page = page.child(
            row()
                .key("draft-connect")
                .ml(GUTTER_W)
                .w(width)
                .mt(22.0)
                .px(14.0)
                .py(10.0)
                .gap(12.0)
                .center_y()
                .radius(8.0)
                .fill(p.card)
                .border(1.0, p.rule)
                .child(
                    text("Chapters are kept as you add them and translated once a model is connected.")
                        .font(SMALL)
                        .color(p.ink2)
                        .grow(1.0)
                        .wrap(),
                )
                .child(
                    btn("Connect a model", false, p)
                        .key("draft-endpoint")
                        .on_press(|s: &mut State| s.open_settings()),
                ),
        );
    }

    if batch {
        let mut preview = col().key("draft-preview").ml(GUTTER_W).w(width).mt(24.0);
        for part in &plan {
            let key = part.key;
            let intake = &part.intake;
            let title = intake
                .title
                .as_deref()
                .or_else(|| intake.blocks.first().map(String::as_str))
                .unwrap_or("No paragraphs")
                .lines()
                .next()
                .unwrap_or("");
            let source = part
                .source
                .as_ref()
                .map(|name| format!(" · {name}"))
                .unwrap_or_default();
            let mut detail = col()
                .grow(1.0)
                .gap(4.0)
                .child(
                    text(title)
                        .font(UI)
                        .color(if part.include { p.ink } else { p.ink4 })
                        .wrap(),
                )
                .child(
                    text(format!(
                        "{}{source}",
                        plural(intake.blocks.len(), "paragraph")
                    ))
                    .font(SMALL)
                    .color(if part.include { p.ink3 } else { p.ink4 }),
                );
            if part.added {
                detail = detail.child(text("Already added to the book.").font(SMALL).color(p.ink4));
            } else if let Some(issue) = part.issue {
                let (note, color) = match issue {
                    IntakeIssue::NoNumber => (
                        "No chapter number read; assigned the next free number.",
                        p.amber,
                    ),
                    IntakeIssue::Duplicate => {
                        ("Duplicate number in this batch; skipped by default.", p.red)
                    }
                    IntakeIssue::InBook => ("Already in the book; skipped by default.", p.red),
                };
                detail = detail.child(
                    text(note)
                        .font(SMALL)
                        .color(color)
                        .opacity(if part.include { 1.0 } else { 0.45 })
                        .wrap(),
                );
            }
            preview = preview.child(
                row()
                    .key(format!("draft-part-{}-{}", key.0, key.1))
                    .py(12.0)
                    .gap(14.0)
                    .center_y()
                    .border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
                    .child(
                        text(intake.number.unwrap().to_string())
                            .font(UI)
                            .color(if part.include { p.ink2 } else { p.ink4 })
                            .w(50.0),
                    )
                    .child(detail)
                    .child(if part.added {
                        text("Added").font(SMALL).color(p.ink4)
                    } else {
                        btn(if part.include { "Skip" } else { "Include" }, false, p)
                            .key(format!("draft-include-{}-{}", key.0, key.1))
                            .on_press(move |s: &mut State| s.toggle_draft_part(key))
                    }),
            );
        }
        page = page.child(preview);
    }

    let source = d.text.text.clone();
    let field = text_input(&d.text, src_body(&s.prefs.reading))
        .key("draft-text")
        .ml(GUTTER_W)
        .w(width)
        .min_h(if batch { 160.0 } else { 380.0 })
        .mt(30.0)
        .color(p.ink)
        .caret(p.red, p.highlight)
        .placeholder("그날 밤, 탑 위에서 바람이 불었다…", p.ink4)
        .on_edit(move |s: &mut State| {
            let d = s.draft.as_mut().expect("the draft is open");
            if d.text.text != source {
                d.reset_parts();
            }
            &mut d.text
        })
        .on_input_change(|s: &mut State, _| {
            if let Some(d) = &mut s.draft {
                d.reset_parts();
            }
        })
        .on_input_key(|s: &mut State, k: &rune::KeyEvent, tx| {
            if k.key == Key::Enter && k.mods.cmd {
                s.add_draft(tx);
                return true;
            }
            false
        });
    let add_label = if batch {
        format!("Add {}", plural(included, "chapter"))
    } else if read || !empty {
        format!("Add chapter {number}")
    } else {
        "Add chapter".to_string()
    };
    let mut add = btn(&add_label, true, p).key("draft-add");
    add = if empty {
        add.opacity(0.4)
    } else {
        add.on_press_tx(|s: &mut State, tx| s.add_draft(tx))
    };
    let mut actions = row()
        .key("draft-actions")
        .ml(GUTTER_W)
        .w(width)
        .mt(24.0)
        .pt(18.0)
        .gap(8.0)
        .center_y()
        .border_sides([1.0, 0.0, 0.0, 0.0], p.rule)
        .child(add)
        .child(kbd("Ctrl Enter", p));
    if !s.lib.chapters.is_empty() || !empty {
        actions = actions.child(
            btn(
                if s.lib.chapters.is_empty() {
                    "Clear"
                } else {
                    "Discard"
                },
                false,
                p,
            )
            .key("draft-discard")
            .on_press_tx(|s: &mut State, tx| s.leave_draft(tx, true)),
        );
    }
    actions = actions.child(el().grow(1.0)).child(
        text(if batch && !d.files.is_empty() {
            "Add another file…"
        } else {
            "Open a text file…"
        })
        .key("draft-file")
        .font(UI)
        .color(p.ink3)
        .cursor(Cursor::Pointer)
        .on_press_tx(move |s: &mut State, tx| {
            if batch && s.draft.as_ref().is_some_and(|d| !d.files.is_empty()) {
                s.draft_more_files(tx);
            } else {
                s.draft_from_file(tx);
            }
        }),
    );
    if d.file_name.is_some() && d.files.is_empty() {
        actions = actions.child(
            text("Add another file…")
                .key("draft-more-files")
                .font(UI)
                .color(p.ink3)
                .cursor(Cursor::Pointer)
                .on_press_tx(|s: &mut State, tx| s.draft_more_files(tx)),
        );
    }
    if !empty && !batch {
        actions = actions.child(
            text("translated in order, after what is already queued")
                .font(SMALL)
                .color(p.ink4)
                .pl(10.0),
        );
    }
    if batch {
        page = page.child(actions);
        if !empty {
            page = page.child(
                text("translated in order, after what is already queued")
                    .ml(GUTTER_W)
                    .pt(10.0)
                    .font(SMALL)
                    .color(p.ink4),
            );
        }
        if d.files.is_empty() || !d.text.text.trim().is_empty() {
            page = page.child(field);
        }
    } else {
        page = page.child(field).child(actions);
    }

    col()
        .key("draft-doc")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .items_center()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(page)
}
