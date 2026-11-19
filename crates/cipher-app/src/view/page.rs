//! The page: one Chapter on paper.

use cipher_book::{BlockId, ChapterState, Side};
use cipher_text::blocks::{BlockKind, Style};
use rune::Cursor;
use rune::tok::TextStyle;
use rune::{Cx, El, Link, LinkPaint, ScrollSpec, Span, col, el, icon, rich, row, text};

use super::context::term_card;
use super::diff;
use super::notes;
use super::widgets::{btn, dot, plural};
use crate::fix::edit_in_review;
use crate::fix::work::Status;
use crate::library::{Library, Page, PageBlock, PairFlag};
use crate::state::{ReadingRow, State, Tab, TermCard};
use crate::theme::{
    GUTTER_W, I_LANG, INTERLINEAR, LABEL, Pal, SERIF, SMALL, UI_MED, body, heading, src_body,
};

/// "ko" is read as 한국어 on the opener of a source page.
fn language(code: &str) -> String {
    match code {
        "ko" => "한국어".into(),
        "ja" => "日本語".into(),
        "zh" => "中文".into(),
        other => other.to_uppercase(),
    }
}

pub fn page_width(s: &State) -> f32 {
    // The gutter and a little air; closed, the context sidebar's notes move into the margin.
    s.text_width()
        + if s.context_shown() || !s.margin_room() {
            96.0
        } else {
            316.0
        }
}

/// What the gutter dot says about a Block, if anything: a problem Pair or an Outdated Block.
fn warning(page: &Page, b: &PageBlock) -> Option<String> {
    let flag = b
        .pair
        .and_then(|p| page.pair(p))
        .and_then(|p| match &p.flag {
            Some(PairFlag::Problem(reason)) => Some(format!("Flagged: {reason}")),
            _ => None,
        });
    let outdated = b
        .outdated
        .then(|| "Outdated: the source changed after this was translated".to_string());
    match (flag, outdated) {
        (Some(a), Some(b)) => Some(format!("{a}. {b}")),
        (a, b) => a.or(b),
    }
}

fn body_text(s: &State, b: &PageBlock, side: Side, p: &Pal) -> El {
    let style = match (side, b.kind) {
        (_, BlockKind::Heading(_)) => heading(&s.prefs.reading),
        (Side::Source, _) => src_body(&s.prefs.reading),
        (Side::Translation, _) => body(&s.prefs.reading),
    };
    let picked = s.term_card.map(|c| c.term);
    let id = b.id;
    // Under Review the Proposed Edit shows in place: deleted words red, inserted words green.
    if side == Side::Translation
        && let Some((e, current)) = edit_in_review(s, id)
        && let Some(after) = &e.after
    {
        if !current {
            let shown = diff::shown(&e.before, after, p);
            return rich(shown.text, shown.spans)
                .font(style)
                .color(p.ink)
                .wrap()
                .key(format!("b{}", id.0))
                .w_full();
        }
        // The one under review, in full twice, like source above translation: as it is, then as proposed.
        let ((old, old_spans), (new, new_spans)) = diff::sides(&e.before, after, p);
        let label = |t: &str| text(t.to_string()).font(LABEL).color(p.ink4);
        return col()
            .w_full()
            .gap(4.0)
            .child(label("NOW"))
            .child(
                rich(old, old_spans)
                    .font(style)
                    .color(p.ink3)
                    .wrap()
                    .key(format!("b{}-now", id.0))
                    .w_full(),
            )
            .child(label("PROPOSED").pt(8.0))
            .child(
                rich(new, new_spans)
                    .font(style)
                    .color(p.ink)
                    .wrap()
                    .key(format!("b{}", id.0))
                    .w_full(),
            );
    }

    let mut spans: Vec<Span> = b
        .spans
        .iter()
        .map(|sp| {
            let span = Span::new(sp.range.start, sp.range.end);
            match sp.style {
                Style::Strong => span.bold(),
                Style::Emphasis => span.italic(),
            }
        })
        .collect();
    // The fix box takes the keyboard, so the quote keeps its highlight by itself.
    if let Some(sel) = s.fix.picked.as_ref().filter(|sel| sel.block == id)
        && let Some(at) = b.text.find(&sel.text)
    {
        spans.push(Span::new(at, at + sel.text.len()).bg(p.highlight, 1.0));
    }
    for h in &b.terms {
        let on = Some(h.term) == picked;
        let sp = Span::new(h.range.start, h.range.end).underline(if on { p.ink2 } else { p.ink4 });
        spans.push(if on { sp.bg(p.term_bg, 1.0) } else { sp });
    }
    for f in &b.forbidden {
        spans.push(
            Span::new(f.range.start, f.range.end)
                .color(p.red)
                .underline(p.red)
                .bg(p.red_tint, 1.5),
        );
    }

    let mut t = rich(b.text.clone(), spans)
        .font(style)
        .color(p.ink)
        .wrap()
        .selectable(true)
        .selection_color(p.highlight)
        .key(format!("b{}", id.0))
        .w_full();
    for h in &b.terms {
        let term = &s.lib.terms[h.term];
        let (other, note) = match side {
            Side::Translation => (&term.source, term.note.as_deref().unwrap_or("")),
            Side::Source => (&term.rendering, term.note.as_deref().unwrap_or("")),
        };
        let help = if note.is_empty() {
            other.clone()
        } else {
            format!("{other} · {note}")
        };
        t = t.link(
            Link::new(h.range.clone(), h.term.to_string())
                .hover(LinkPaint::new().bg(p.term_bg))
                .help(help),
        );
    }
    for f in &b.forbidden {
        let term = &s.lib.terms[f.term];
        t = t.link(
            Link::new(f.range.clone(), f.term.to_string())
                .hover(LinkPaint::new().bg(p.red_tint))
                .help(format!(
                    "Never “{}”: it should read “{}”",
                    f.rendering, term.rendering
                )),
        );
    }
    t.on_link(move |s: &mut State, tid, _| {
        if let Ok(term) = tid.parse() {
            s.term_card = Some(TermCard { block: id, term });
            s.tab = Tab::Terms;
            s.fix.picked = None;
            s.look_at(term);
        }
    })
}

/// For a source paragraph of the Chapter translating now: `Some(Some(text))` when its
/// translation has arrived, `Some(None)` when it was merged into the paragraph above, `None`
/// when it hasn't arrived (or nothing is translating here).
fn streamed_text<'a>(s: &'a State, page: &Page, index: usize) -> Option<Option<&'a str>> {
    let (chapter, entries) = s.streamed.as_ref()?;
    if s.lib.chapters.get(page.index).map(|c| c.id) != Some(*chapter) {
        return None;
    }
    let n = index as u32 + 1;
    let e = entries.iter().find(|e| e.0 <= n && n <= e.1)?;
    Some((e.0 == n).then_some(e.2.as_str()))
}

/// One Block as a row: gutter, text, optional interlinear source, optional margin note.
fn paragraph(s: &State, page: &Page, side: Side, index: usize, p: &Pal) -> El {
    let b = &page.blocks(side)[index];
    let id = b.id;
    let unfolded = b.pair.is_some_and(|pair| s.unfolded.contains(&pair));
    let warn = warning(page, b);
    let term_here = s.term_card.is_some_and(|c| c.block == id);
    let waiting = s.lib.has_edit(id);
    let focus = edit_in_review(s, id).is_some_and(|(_, current)| current);

    // Gutter: one click target that unfolds this paragraph's source. A dot marks a warning.
    let mut gutter = col()
        .key(format!("g{}", id.0))
        .w(GUTTER_W)
        .pt(7.0)
        .items_end()
        .pr(18.0);
    if side == Side::Translation {
        let imported = b
            .pair
            .and_then(|pair| page.pair(pair))
            .is_some_and(|pv| pv.flag == Some(PairFlag::Imported));
        gutter = gutter
            .cursor(Cursor::Pointer)
            .help(match (&warn, imported) {
                (Some(w), _) => w.clone(),
                (None, true) => "Show the source (paired by position)".to_string(),
                (None, false) => "Show the source".to_string(),
            })
            .child(if waiting {
                dot(p.red, 6.0).mr(2.0)
            } else if warn.is_some() {
                dot(p.amber, 6.0).mr(2.0)
            } else {
                icon(&I_LANG, 14.0)
                    .color(p.ink3)
                    .group_opacity(if unfolded { 1.0 } else { 0.0 }, 1.0)
            });
        if let Some(pair) = b.pair {
            gutter = gutter.on_press(move |s: &mut State| s.toggle_unfolded(pair));
        }
    }

    let mut text_col = col().w(s.text_width()).gap(6.0);
    if focus && let Some(pv) = b.pair.and_then(|pair| page.pair(pair)) {
        text_col = text_col.child(
            text(pv.source_text.clone())
                .font(INTERLINEAR)
                .color(p.ink3)
                .wrap()
                .pb(4.0),
        );
    }
    // A Chapter translating now: each source paragraph with its translation as it arrives.
    let streamed = (side == Side::Source)
        .then(|| streamed_text(s, page, index))
        .flatten();
    if let Some(done) = streamed {
        if let Some(t) = done {
            text_col = text_col.child(
                text(t.to_string())
                    .font(body(&s.prefs.reading))
                    .color(p.ink)
                    .wrap(),
            );
        }
        text_col = text_col.child(text(b.text.clone()).font(INTERLINEAR).color(p.ink3).wrap());
    } else {
        match b.kind {
            BlockKind::SceneBreak => {
                text_col = text_col.child(
                    text("·  ·  ·")
                        .font(body(&s.prefs.reading))
                        .color(p.ink3)
                        .pl(s.text_width() / 2.0 - 24.0),
                );
            }
            _ => text_col = text_col.child(body_text(s, b, side, p)),
        }
    }
    if side == Side::Translation
        && page.first_of_pair(index)
        && (s.interlinear || unfolded)
        && !focus
        && let Some(pv) = b.pair.and_then(|pair| page.pair(pair))
    {
        text_col = text_col.child(
            text(pv.source_text.clone())
                .font(INTERLINEAR)
                .color(p.ink3)
                .wrap()
                .selectable(true)
                .key(format!("s{}", id.0)),
        );
    }

    if side == Side::Translation
        && page.first_of_pair(index)
        && let Some(PairFlag::Problem(reason)) = b
            .pair
            .and_then(|pair| page.pair(pair))
            .map(|pv| &pv.flag)
            .and_then(Option::as_ref)
    {
        text_col = text_col.child(
            row()
                .key(format!("flag{}", id.0))
                .gap(6.0)
                .center_y()
                .child(dot(p.amber, 5.0))
                .child(
                    text(format!("Flagged: {reason}"))
                        .font(SMALL)
                        .color(p.ink3)
                        .wrap(),
                ),
        );
    }

    let padding = if side == Side::Source { 20.0 } else { 18.0 };
    let mut r = row()
        .key(format!("row{}", id.0))
        .w_full()
        .pb(padding)
        .hover_group()
        .child(gutter)
        .child(text_col);
    if s.sidebar_reflow_row() == Some(id) {
        r = r.on_layout_tx(move |s: &mut State, [_, _, _, height], tx| {
            s.finish_sidebar_reflow(id, height, padding, tx);
        });
    }
    // With the context sidebar closed, notes and a Term's card live in the page margin beside
    // their paragraph.
    if !s.context_shown() && s.margin_room() {
        let chapter = s.lib.chapters[page.index].id;
        let mut margin = col()
            .w(260.0)
            .pl(36.0)
            .pt(4.0)
            .gap(18.0)
            .absolute(GUTTER_W + s.text_width(), if focus { 18.0 } else { 4.0 });
        let mut any = false;
        for sug in s
            .lib
            .suggestions
            .iter()
            .filter(|x| x.chapter == chapter && x.block == id)
        {
            margin = margin.child(notes::note(s, sug, p, false));
            any = true;
        }
        for f in s
            .fix
            .finished
            .iter()
            .filter(|f| f.chapter == chapter && f.block == id)
        {
            margin = margin.child(notes::finished_note(s, f, p, false));
            any = true;
        }
        if term_here && let Some(tc) = s.term_card {
            margin = margin.child(term_card(s, tc, p));
            any = true;
        }
        if any {
            r = r.child(margin);
        }
    }
    if focus {
        r = r.fill(p.focus_bg).pt(14.0).radius(6.0);
    }
    r
}

/// The card on a Chapter that has no Translation, with what can be done about it.
fn untranslated_card(s: &State, at: usize, p: &Pal) -> El {
    let ch = &s.lib.chapters[at];
    let no_endpoint = !matches!(s.bg.status, Status::Running(_));
    let wait = if no_endpoint {
        "It translates on its own once an Endpoint is set up. The source is below."
    } else {
        "It translates on its own, in queue order. The source is below."
    };
    let (title, sub, color) = match ch.state {
        ChapterState::Queued => (
            format!(
                "Waiting to translate · {}",
                match ch.queue_place {
                    Some(1) => "first in queue".to_string(),
                    Some(2) => "second in queue".to_string(),
                    Some(n) => format!("#{n} in queue"),
                    None => "queued".to_string(),
                }
            ),
            wait.to_string(),
            p.amber,
        ),
        ChapterState::Translating => (
            match s.progress {
                Some(pr) if pr.chapter == ch.id && pr.total > 0 => {
                    format!("Translating now · {} of {} paragraphs", pr.done, pr.total)
                }
                _ => "Translating now".to_string(),
            },
            "Each paragraph appears below as it arrives, above its source.".to_string(),
            p.amber,
        ),
        ChapterState::Failed => (
            "Translation failed".to_string(),
            ch.failure
                .clone()
                .unwrap_or_else(|| "No reason was recorded.".into()),
            p.red,
        ),
        ChapterState::Empty => (
            "This chapter is empty".to_string(),
            "It had no text when it was added, so there is nothing to translate. It is probably missing from the source you imported.".to_string(),
            p.ink4,
        ),
        _ => (
            "Not translated yet".to_string(),
            "The source is below.".to_string(),
            p.amber,
        ),
    };
    let n = ch.number;
    let later = s.lib.chapters[at + 1..]
        .iter()
        .filter(|c| c.state == ChapterState::Waiting)
        .count();
    let action = match ch.state {
        ChapterState::Waiting => {
            let mut a = row().gap(8.0).child(
                btn("Translate", true, p)
                    .key(format!("card-translate-{n}"))
                    .on_press(move |s: &mut State| s.translate_chapter(at)),
            );
            if later > 0 {
                a = a.child(
                    btn(&format!("Translate all {} from here", later + 1), false, p)
                        .key(format!("card-translate-from-{n}"))
                        .help("Queues this chapter and every untranslated one after it, in order.")
                        .on_press(move |s: &mut State| s.translate_from(at)),
                );
            }
            Some(a)
        }
        ChapterState::Failed => Some(
            btn("Retry", true, p)
                .key(format!("card-retry-{n}"))
                .on_press(move |s: &mut State| s.retry_chapter(at)),
        ),
        ChapterState::Queued if no_endpoint => Some(
            btn("Set up an Endpoint", true, p)
                .key(format!("card-endpoint-{n}"))
                .on_press(|s: &mut State| s.open_settings()),
        ),
        _ => None,
    };
    let mut card = row()
        .key(format!("untranslated-{n}"))
        .w(s.text_width())
        .padding(16.0)
        .gap(14.0)
        .center_y()
        .radius(8.0)
        .border(1.0, p.rule)
        .fill(p.card)
        .child(dot(color, 8.0))
        .child(
            col()
                .grow(1.0)
                .gap(2.0)
                .child(text(title).font(UI_MED).color(p.ink))
                .child(text(sub).font(SMALL).color(p.ink3).wrap()),
        );
    if let Some(action) = action {
        card = card.child(action);
    }
    row().pl(GUTTER_W).pb(34.0).child(card)
}

/// How far down the window, in points, the line that decides the current Chapter sits.
const READING_LINE: f32 = 300.0;

/// What a Chapter's section shows: its Translation, or its source when it has none or the
/// reader asked for the source.
fn section_side(s: &State, page: &Page) -> Side {
    if s.source || page.translation.is_empty() {
        Side::Source
    } else {
        Side::Translation
    }
}

fn opener(s: &State, page: &Page, side: Side, p: &Pal) -> El {
    let label = if side == Side::Source && !page.translation.is_empty() {
        format!("SOURCE · {}", language(&s.lib.meta.source_lang))
    } else {
        "CHAPTER".to_string()
    };
    col()
        .key(format!("opener{}", page.index))
        .pl(GUTTER_W)
        .pb(36.0)
        .gap(2.0)
        .child(text(label).font(LABEL).color(p.red))
        .child(
            text(page.number.to_string())
                .font(TextStyle::new(60.0, 300.0).lh(72.0).family(SERIF))
                .color(p.ink),
        )
        .child(el().w(36.0).h(1.5).fill(p.ink).mt(14.0))
}

/// The quiet line after the last Chapter.
fn end_of_book(lib: &Library, width: f32, p: &Pal) -> El {
    col()
        .key("end")
        .ml(GUTTER_W)
        .w(width)
        .mt(40.0)
        .pt(22.0)
        .gap(4.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], p.rule)
        .child(
            text("The end, for now")
                .font(TextStyle::new(16.0, 400.0).italic().family(SERIF))
                .color(p.ink2),
        )
        .child(
            text(plural(lib.chapters.len(), "chapter"))
                .font(SMALL)
                .color(p.ink3),
        )
        .child(
            row()
                .key("end-next")
                .mt(18.0)
                .gap(8.0)
                .center_y()
                .cursor(Cursor::Pointer)
                .hover_group()
                .child(
                    text(format!(
                        "Add chapter {}",
                        lib.chapters.last().map_or(1, |c| c.number + 1)
                    ))
                    .font(TextStyle::new(16.0, 400.0).family(SERIF))
                    .color(p.red),
                )
                .child(text("→").font(UI_MED).color(p.red))
                .child(
                    text("paste it, type it, or open a text file")
                        .font(SMALL)
                        .color(p.ink3)
                        .group_opacity(0.0, 1.0),
                )
                .on_press_tx(|s: &mut State, tx| s.new_chapter(tx)),
        )
}

/// One Chapter on the page: opener, the card if it has no Translation, and its paragraphs.
/// It becomes current when it fills most of the window.
fn section(s: &State, page: &Page, p: &Pal) -> El {
    let at = page.index;
    let side = section_side(s, page);
    let blocks = page.blocks(side);
    let mut column = col()
        .key(format!("sec{at}"))
        .w_full()
        .pt(if at == s.strip.first().copied().unwrap_or(at) {
            56.0
        } else {
            120.0
        })
        // The Chapter crossing the reading line, a third of the way down, is the current one.
        .on_drawn_layout_tx(move |s: &mut State, [_, y, _, h], tx| {
            s.moved(tx);
            if y <= READING_LINE && y + h > READING_LINE && s.lib.current() != at {
                s.follow(tx, at);
            } else {
                tx.live_only();
            }
        })
        .child(opener(s, page, side, p));
    if s.lib.chapters[at].untranslated() || s.lib.chapters[at].state == ChapterState::Empty {
        column = column.child(untranslated_card(s, at, p));
    }
    let skip = side == Side::Translation && page.hides_first_translated();
    let mut body = col().w_full().selection_group(true).on_selection_change(
        move |s: &mut State, change, _| {
            let picked = change.selection().and_then(|sel| {
                let key = sel
                    .texts()
                    .iter()
                    .find(|t| t.is_selected())
                    .and_then(|t| t.key())
                    .map(str::to_string)?;
                let id: i64 = key.strip_prefix('b')?.parse().ok()?;
                let txt = sel.text().trim().to_string();
                (!txt.is_empty() && txt.len() < 200).then_some((BlockId(id), txt))
            });
            // Fixing happens in the Chapter the selection is in.
            if picked.is_some() && s.lib.current() != at && s.fix.review.is_none() {
                let _ = s.lib.set_current(at);
            }
            match picked {
                Some((id, txt)) if s.side() == Side::Translation && s.fix.review.is_none() => {
                    s.select(id, txt)
                }
                None if !s.fix.composing => s.fix.picked = None,
                _ => {}
            }
        },
    );
    for i in (skip as usize)..blocks.len() {
        body = body.child(paragraph(s, page, side, i, p));
    }
    column.child(body)
}

/// The page: the current Chapter between its neighbours, so reading runs on from one Chapter
/// into the next in either direction. The scroll keeps its place as Chapters load above.
pub fn reading(s: &State, cx: &mut Cx<State>, p: &Pal) -> El {
    // Remember where each row of the current Chapter sits, for Ctrl+T to keep the position.
    let current = s.lib.page();
    let origin = cx.layout("page").map_or(0.0, |r| r[1]);
    *s.tops.borrow_mut() = current
        .blocks(section_side(s, current))
        .iter()
        .filter_map(|b| {
            cx.layout(&format!("row{}", b.id.0)).map(|r| ReadingRow {
                id: b.id,
                top: r[1] - origin,
                height: r[3],
            })
        })
        .collect();

    let mut column = col().key("page").w(page_width(s)).pb(220.0);
    for &at in &s.strip {
        if let Some(page) = s.lib.page_at(at) {
            column = column.child(section(s, page, p));
        }
    }
    if s.strip.last() == Some(&(s.lib.chapters.len() - 1)) {
        column = column.child(end_of_book(&s.lib, s.text_width(), p));
    }

    let y = s.scroll_y.clone();
    col()
        .key("doc")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .items_center()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .scroll_anchor()
        .on_scroll_offset(move |at| y.set(at.offset[1]))
        .child(column)
}
