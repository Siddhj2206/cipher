//! The bar above the page.

use rune::{El, col, el, icon, row, text};

use super::widgets::{dot, icon_btn, kbd, toggle};
use crate::fix::work::Status;
use crate::state::{Screen, State};
use crate::theme::{
    HEADER_H, I_GEAR, I_LEFT_PANEL, I_PLUG, I_RIGHT_PANEL, I_TYPE, Pal, SMALL, UI, UI_MED,
};
use cipher_book::ChapterState;
use rune::Cursor;

fn state_text(s: &State) -> String {
    let page = s.lib.page();
    let Some(row) = s.lib.chapters.get(page.index) else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    match row.state {
        ChapterState::Waiting => parts.push("waiting to translate".into()),
        ChapterState::Queued => parts.push("queued".into()),
        ChapterState::Translating => match s.progress {
            Some(pr) if pr.chapter == row.id && pr.total > 0 => {
                parts.push(format!("translating · {} of {}", pr.done, pr.total))
            }
            _ => parts.push("translating".into()),
        },
        ChapterState::Failed => parts.push("translation failed".into()),
        ChapterState::Empty => parts.push("empty".into()),
        ChapterState::Translated if s.source => parts.push("source".into()),
        ChapterState::Translated => {
            let shown = page.translation.len() - page.hides_first_translated() as usize;
            parts.push(format!("{shown} paragraphs"));
            let again = match s.progress {
                Some(pr) if pr.chapter == row.id => {
                    Some(format!("translating again · {} of {}", pr.done, pr.total))
                }
                _ => row
                    .queue_place
                    .map(|_| "queued to translate again".to_string()),
            };
            match &row.made_by {
                Some(m) => {
                    if let Some(model) = &m.model {
                        // The model, not the Endpoint's name: that is what differs.
                        parts.push(model.rsplit(" · ").next().unwrap_or(model).to_string());
                    }
                }
                None if page.imported() => parts.push("imported".into()),
                None => {}
            }
            parts.extend(again);
        }
    }
    if page.outdated() > 0 {
        parts.push(format!("{} outdated", page.outdated()));
    }
    if page.problems() > 0 {
        parts.push(format!("{} flagged", page.problems()));
    }
    parts.join(" · ")
}

/// The longer story behind the header's state line, for its tooltip: who translated the
/// Chapter, when, and the tokens it took.
fn made_by_help(s: &State) -> Option<String> {
    let page = s.lib.page();
    let row = s.lib.chapters.get(page.index)?;
    if row.state != ChapterState::Translated {
        return None;
    }
    let Some(m) = &row.made_by else {
        return page.imported().then(|| {
            "Imported with the Book. Its paragraphs were paired with the source by Cipher, not by a model.".to_string()
        });
    };
    let mut line = format!(
        "Translated by {} {}",
        m.model.as_deref().unwrap_or("an earlier model"),
        crate::view::ago(m.at)
    );
    if let (Some(i), Some(o)) = (m.tokens_in, m.tokens_out) {
        line.push_str(&format!(
            " · {} tokens in, {} out",
            crate::view::thousands(i),
            crate::view::thousands(o)
        ));
    }
    Some(line)
}

/// What the queue is doing, beside the Endpoint button: the running Chapter's progress, a
/// pause, or a problem with the Endpoint. Empty when there is nothing to say.
fn queue(s: &State, p: &Pal) -> El {
    let mut r = row().key("h-queue").gap(6.0).center_y().px(6.0);
    let current = s.lib.chapters.get(s.lib.current()).map(|c| c.id);
    if let Some(pr) = s.progress
        && Some(pr.chapter) != current
        && let Some(at) = s.lib.chapter_index(pr.chapter)
    {
        let n = s.lib.chapters[at].number;
        let line = if pr.total > 0 {
            format!("Chapter {n} · {} of {}", pr.done, pr.total)
        } else {
            format!("Chapter {n} · starting")
        };
        return r
            .radius(6.0)
            .hover(p.hover)
            .cursor(Cursor::Pointer)
            .help("Translating now. Click to open it.")
            .child(dot(p.amber, 6.0))
            .child(text(line).font(SMALL).color(p.ink3))
            .on_press_tx(move |s: &mut State, tx| s.go(tx, at));
    }
    if let Some(paused) = &s.paused {
        let mins = ((paused.until - now()) as f32 / 60.0).ceil().max(1.0) as i64;
        r = r
            .help(paused.reason.clone())
            .child(dot(p.amber, 6.0))
            .child(
                text(format!("Paused · resumes in {}", duration(mins)))
                    .font(SMALL)
                    .color(p.ink3),
            );
    } else if let Status::Unavailable(why) = &s.bg.status {
        r = r
            .help(why.clone())
            .child(dot(p.red, 6.0))
            .child(text("Endpoint unavailable").font(SMALL).color(p.red));
    }
    r
}

fn duration(mins: i64) -> String {
    if mins < 60 {
        format!("{mins} min")
    } else {
        format!("{} h {} min", mins / 60, mins % 60)
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// What this Book translates with; pressed, the menu to choose another.
fn model_chip(s: &State, p: &Pal) -> El {
    let (label, color, tip) = match &s.bg.status {
        Status::Running(name) => (
            name.clone(),
            p.ink2,
            "The model this book translates with".to_string(),
        ),
        Status::NoEndpoint if s.lib.is_saved() => (
            "Choose a model".to_string(),
            p.amber,
            "No Endpoint is set up yet".to_string(),
        ),
        Status::NoEndpoint => (
            "Model".to_string(),
            p.ink3,
            "The model this book translates with".to_string(),
        ),
        Status::Unavailable(why) => ("Model unavailable".to_string(), p.red, why.clone()),
    };
    row()
        .key("h-model")
        .gap(6.0)
        .center_y()
        .px(8.0)
        .py(4.0)
        .radius(6.0)
        .fill(if s.model_menu.is_some() {
            p.selected
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(p.hover)
        .cursor(Cursor::Pointer)
        .help(tip)
        .child(icon(&I_PLUG, 14.0).color(color))
        .child(text(super::widgets::short(&label)).font(UI).color(color))
        .on_press(|s: &mut State| s.toggle_model_menu())
}

/// The models a Book can pick: the default, then every Endpoint's models.
pub fn model_menu(s: &State, p: &Pal) -> Option<El> {
    let (endpoints, default) = s.model_menu.as_ref()?;
    let chosen = s.lib.model_choice();
    let item = |label: String, on: bool, key: String| {
        row()
            .key(key)
            .w_full()
            .gap(8.0)
            .px(10.0)
            .py(6.0)
            .radius(5.0)
            .hover(p.hover)
            .cursor(Cursor::Pointer)
            .child(
                text(if on { "✓" } else { " " })
                    .font(UI)
                    .color(p.ink)
                    .w(12.0),
            )
            .child(
                text(label)
                    .font(UI)
                    .color(p.ink)
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate(),
            )
    };
    let default_name = default
        .as_deref()
        .and_then(|id| endpoints.iter().find(|e| e.id == id))
        .map(|e| format!("Default · {} · {}", e.name, e.translate_model))
        .unwrap_or_else(|| "Default".to_string());
    let mut menu = col().key("model-menu").w(300.0).padding(4.0).child(
        item(default_name, chosen.is_none(), "mm-default".into())
            .on_press(|s: &mut State| s.choose_model(None)),
    );
    for (i, e) in endpoints.iter().enumerate() {
        menu = menu.child(
            text(e.name.to_uppercase())
                .font(crate::theme::LABEL)
                .color(p.ink3)
                .px(10.0)
                .pt(10.0)
                .pb(4.0),
        );
        for (j, m) in e.all_models().into_iter().enumerate() {
            let on = chosen
                .as_ref()
                .is_some_and(|c| c.endpoint == e.id && c.model == m);
            let choice = cipher_book::ModelChoice {
                endpoint: e.id.clone(),
                model: m.clone(),
            };
            menu = menu.child(
                item(m, on, format!("mm-{i}-{j}"))
                    .on_press(move |s: &mut State| s.choose_model(Some(choice.clone()))),
            );
        }
    }
    if endpoints.is_empty() {
        menu = menu.child(
            text("No Endpoints yet.")
                .font(SMALL)
                .color(p.ink3)
                .px(10.0)
                .py(6.0),
        );
    }
    menu = menu.child(el().w_full().h(1.0).fill(p.rule).my(4.0)).child(
        row()
            .key("mm-manage")
            .w_full()
            .px(10.0)
            .py(6.0)
            .radius(5.0)
            .hover(p.hover)
            .cursor(Cursor::Pointer)
            .child(text("Endpoints and models…").font(UI).color(p.ink2))
            .on_press(|s: &mut State| {
                s.model_menu = None;
                s.open_settings();
            }),
    );
    Some(
        super::widgets::card(p)
            .child(menu)
            .anchored("h-model", rune::Side::Bottom, rune::Align::End, 6.0)
            .modal(true, |s: &mut State| s.model_menu = None),
    )
}

pub fn header(s: &State, p: &Pal) -> El {
    let (crumb, state) = if s.screen == Screen::Cast {
        ("Cast & terms".to_string(), String::new())
    } else if s.screen == Screen::Draft {
        ("New chapter".to_string(), String::new())
    } else if s.screen == Screen::Style {
        ("Style".to_string(), String::new())
    } else if s.screen == Screen::History {
        ("History".to_string(), String::new())
    } else {
        (format!("Chapter {}", s.lib.page().number), state_text(s))
    };
    let mut left = row().gap(10.0).center_y().grow(1.0).min_w(0.0);
    if !s.contents_shown() {
        left = left.child(
            icon_btn(&I_LEFT_PANEL, "show-left", false, p)
                .help("Show contents (Ctrl+\\)")
                .on_press(|s: &mut State| s.toggle_contents()),
        );
    }
    left = left.child(text(crumb).font(UI_MED).color(p.ink)).child(
        match (s.screen == Screen::Reading)
            .then(|| made_by_help(s))
            .flatten()
        {
            Some(help) => text(state)
                .font(SMALL)
                .color(p.ink3)
                .truncate()
                .min_w(0.0)
                .help(help),
            None => text(state).font(SMALL).color(p.ink3).truncate().min_w(0.0),
        },
    );
    let mut right = row().gap(4.0).center_y();
    // Reading toggles mean nothing on the page of a Chapter not added yet.
    if !matches!(s.screen, Screen::Draft | Screen::Style | Screen::History) {
        right = right
            .child(
                toggle("Interlinear", s.interlinear, "h-inter", p)
                    .help("Source under every paragraph (Ctrl+I)")
                    .on_press(|s: &mut State| s.interlinear = !s.interlinear),
            )
            .child(
                toggle("Source", s.source, "h-source", p)
                    .help("Read the source (Ctrl+T)")
                    .on_press_tx(|s: &mut State, tx| s.toggle_source(tx)),
            )
            .child(el().w(1.0).h(16.0).fill(p.rule).mx(4.0));
    }
    right = right
        .child(queue(s, p))
        .child(model_chip(s, p))
        .child(
            icon_btn(&I_TYPE, "h-type", s.type_open, p)
                .help("Text size, typeface and theme")
                .on_press(|s: &mut State| s.type_open = !s.type_open),
        )
        .child(
            icon_btn(&I_GEAR, "h-settings", false, p)
                .help("Settings (Ctrl+,)")
                .on_press(|s: &mut State| s.open_settings_at(crate::state::Section::General)),
        )
        .child(
            row()
                .key("h-k")
                .pl(4.0)
                .cursor(Cursor::Pointer)
                .child(kbd("Ctrl K", p))
                .on_press(|s: &mut State| s.open_palette()),
        );
    if !s.context_shown() && !matches!(s.screen, Screen::Draft | Screen::Style | Screen::History) {
        right = right.child(
            icon_btn(&I_RIGHT_PANEL, "show-right", false, p)
                .help("Show context (Ctrl+J)")
                .on_press(|s: &mut State| s.toggle_context()),
        );
    }
    row()
        .h(HEADER_H)
        .w_full()
        .px(16.0)
        .center_y()
        .fill(p.paper)
        .border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
        .window_drag()
        .gap(12.0)
        .child(left)
        .child(right)
}
