//! What the command palette (Ctrl+K) finds and what choosing a result does.

use rune::Tx;

use crate::state::{Screen, State};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Cast,
    Style,
    Source,
    Interlinear,
    Theme,
    Contents,
    Context,
    AddChapters,
    Endpoint,
    Shelf,
}

pub enum Hit {
    Chapter(usize),
    Term(usize),
    Action(Action),
}

/// Most results shown at once.
pub const MAX_HITS: usize = 9;

pub fn action_name(a: Action, s: &State) -> String {
    match a {
        Action::Cast => "Cast & terms".into(),
        Action::Style => "Style".into(),
        Action::Source if s.source => "Show translation".into(),
        Action::Source => "Show source".into(),
        Action::Interlinear => "Interlinear".into(),
        Action::Theme if s.dark() => "Light mode".into(),
        Action::Theme => "Dark mode".into(),
        Action::Contents => "Toggle contents".into(),
        Action::Context => "Toggle context".into(),
        Action::AddChapters => "Add chapters".into(),
        Action::Endpoint => "Endpoint settings".into(),
        Action::Shelf => "All books".into(),
    }
}

pub fn action_detail(a: Action, s: &State) -> String {
    match a {
        Action::Cast => format!(
            "{} terms, {} drifting (Ctrl+G)",
            s.lib.terms.len(),
            s.lib.drifting_terms()
        ),
        Action::Style => "How this book should read".into(),
        Action::Source => "Read the chapter in its own language (Ctrl+T)".into(),
        Action::Interlinear => "Source under every paragraph (Ctrl+I)".into(),
        Action::Theme => "Ctrl+D".into(),
        Action::Contents => "Chapter list (Ctrl+\\)".into(),
        Action::Context => "Terms in this chapter (Ctrl+J)".into(),
        Action::AddChapters => "From the clipboard (Ctrl+V)".into(),
        Action::Endpoint => match &s.bg.status {
            crate::fix::work::Status::Running(name) => format!("Translating with {name}"),
            _ => "API key or Sign in with ChatGPT".into(),
        },
        Action::Shelf => "Back to the shelf (Ctrl+O)".into(),
    }
}

pub fn hits(s: &State, query: &str) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    for a in [
        Action::Cast,
        Action::Style,
        Action::Source,
        Action::Interlinear,
        Action::Theme,
        Action::Contents,
        Action::Context,
        Action::AddChapters,
        Action::Endpoint,
        Action::Shelf,
    ] {
        if q.is_empty() || action_name(a, s).to_lowercase().contains(&q) {
            out.push(Hit::Action(a));
        }
    }
    if !q.is_empty() {
        out.extend(s.lib.find_chapters(&q).into_iter().map(Hit::Chapter));
        out.extend(s.lib.find_terms(&q).into_iter().map(Hit::Term));
    }
    out.truncate(MAX_HITS);
    out
}

pub fn run(s: &mut State, tx: &mut Tx, hit: &Hit) {
    s.palette = None;
    match *hit {
        Hit::Chapter(i) => s.go(tx, i),
        Hit::Term(i) => {
            s.screen = Screen::Cast;
            tx.scroll_key_to(format!("term{i}"), 120.0);
        }
        Hit::Action(a) => match a {
            Action::Cast => s.screen = Screen::Cast,
            Action::Style => s.open_style(),
            Action::Source => s.toggle_source(tx),
            Action::Interlinear => s.interlinear = !s.interlinear,
            Action::Theme => s.flip_dark(),
            Action::Contents => s.toggle_contents(),
            Action::Context => s.toggle_context(),
            Action::AddChapters => s.new_chapter(tx),
            Action::Endpoint => s.open_settings(),
            Action::Shelf => s.go_shelf(),
        },
    }
}

/// Runs result number `i` of the current query.
pub fn run_nth(s: &mut State, tx: &mut Tx, i: usize) {
    let query = s
        .palette
        .as_ref()
        .map(|q| q.text.clone())
        .unwrap_or_default();
    if let Some(hit) = hits(s, &query).get(i) {
        run(s, tx, hit);
    }
}
