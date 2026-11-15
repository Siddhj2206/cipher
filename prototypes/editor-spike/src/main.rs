//! PROTOTYPE: throwaway spike, redesign #2 ("Folio"), refinement 1.
//!
//! Idea: Cipher is a book you read, not an editor you operate. One chapter at a time on paper.
//! - Contents on the left: every Chapter with its first line and state.
//! - Context on the right: Suggestions and the Terms in this Chapter.
//! - Fixes are proofreader's marks you review inline.
//!
//! Close either sidebar and the page takes the room:
//! - With the right sidebar closed, notes move into the page margin.
//! - With the left sidebar closed, the contents shrink to a scrubber on the window edge.
//!
//! Run: `cargo run --release` (book from `CIPHER_BOOK`, default ../../Test-Books/S-Class-Transmigrator)
//!
//! Keys:
//! - Ctrl+K: palette.
//! - Ctrl+\: contents.
//! - Ctrl+J: context.
//! - Ctrl+G: Cast & terms.
//! - Ctrl+I: interlinear.
//! - Ctrl+T: show the source.
//! - Ctrl+D: dark / light.
//! - Ctrl+V anywhere: add a chapter.
//! - With a selection, start typing to suggest a fix.
//! - Review: J/K next/prev, A accept, X skip, Shift+A accept rest, Esc back to where you were.
//!
//! Real: chapters, the glossary, per-chapter counts and drift. Simulated, with no LLM: a fix is
//! a word-bounded replace, the last two chapters pretend to be queued, and Publish only logs.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rune::tok::{Family, TextStyle};
use rune::{Align, AppSpec, Color, Cursor, Cx, El, Icon, Key, KeyEvent, Link, LinkPaint, ScrollSpec, Side, Span, TextEdit, Tx, col, el, icon, rich, row, text, text_input};

// ---------- tokens ----------

const SERIF: Family = Family::Named("Noto Serif, Noto Serif CJK KR");
const CJK: Family = Family::Named("Noto Serif CJK KR, Noto Serif");
const BODY: TextStyle = TextStyle::new(18.0, 400.0).lh(31.0).family(SERIF);
const SRC_BODY: TextStyle = TextStyle::new(17.0, 400.0).lh(31.0).family(CJK);
const INTERLINEAR: TextStyle = TextStyle::new(14.0, 400.0).lh(23.0).family(CJK);
const UI: TextStyle = TextStyle::new(13.0, 400.0);
const UI_MED: TextStyle = TextStyle::new(13.0, 550.0);
const SMALL: TextStyle = TextStyle::new(12.0, 400.0);
const LABEL: TextStyle = TextStyle::new(10.5, 650.0).tracking(1.4);
const SPARK: TextStyle = TextStyle::new(7.5, 400.0).lh(14.0).family(Family::Named("DejaVu Sans Mono"));

const LEFT_W: f32 = 272.0;
const RIGHT_W: f32 = 320.0;
const HEADER_H: f32 = 52.0;

fn c(hex: &str) -> Color { rune::color::hex(hex).unwrap() }

/// Two themes from one set of roles: warm paper by day, warm ink-dark by night.
struct Pal {
    paper: Color, side: Color, card: Color, ink: Color, ink2: Color, ink3: Color, ink4: Color, rule: Color, hover: Color, selected: Color,
    red: Color, red_soft: Color, red_tint: Color, on_red: Color, green: Color, green_tint: Color, amber: Color,
    highlight: Color, term_bg: Color, focus_bg: Color, primary_hover: Color,
    bar: Color, bar_fg: Color, bar_fg2: Color, bar_hover: Color, scrim: Color, shadow: Color,
}
fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            paper: c("#1A1916"), side: c("#151412"), card: c("#22201C"), ink: c("#E9E4D9"), ink2: c("#B5AE9F"), ink3: c("#7F796D"),
            ink4: c("#4B4740"), rule: c("#2D2A25"), hover: c("#26241F"), selected: c("#2E2B26"),
            red: c("#E5694C"), red_soft: c("#A65141"), red_tint: c("#E5694C2B"), on_red: c("#1A1916"), green: c("#86C496"),
            green_tint: c("#86C4962B"), amber: c("#D9A13A"), highlight: c("#D9A93E5C"), term_bg: c("#312D27"),
            focus_bg: c("#23211D"), primary_hover: c("#D6D0C4"),
            bar: c("#2C2A25"), bar_fg: c("#E9E4D9"), bar_fg2: c("#8F887B"), bar_hover: c("#3A3731"), scrim: c("#00000080"), shadow: c("#00000099"),
        }
    } else {
        Pal {
            paper: c("#F3EFE6"), side: c("#EDE8DD"), card: c("#FBF9F4"), ink: c("#1E1C18"), ink2: c("#5B564C"), ink3: c("#948D7F"),
            ink4: c("#C9C2B3"), rule: c("#E0DACD"), hover: c("#E6E0D3"), selected: c("#E1DACB"),
            red: c("#B4361E"), red_soft: c("#D9806D"), red_tint: c("#B4361E24"), on_red: c("#FFF6F2"), green: c("#2E6A3A"),
            green_tint: c("#2E6A3A2E"), amber: c("#C98A1B"), highlight: c("#F4C84A8C"), term_bg: c("#E6DECC"),
            focus_bg: c("#ECE5D5"), primary_hover: c("#3A3730"),
            bar: c("#1E1C18"), bar_fg: c("#F3EFE6"), bar_fg2: c("#A39C8E"), bar_hover: c("#3A3730"), scrim: c("#1E1C1838"), shadow: c("#1E1C1830"),
        }
    }
}

macro_rules! lucide {
    ($name:ident, $id:literal, $body:literal) => {
        static $name: Icon = Icon { name: $id, view: 24.0, body: concat!(r#"<g fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">"#, $body, "</g>") };
    };
}
lucide!(I_LANG, "l-languages", r#"<path d="m5 8 6 6"/><path d="m4 14 6-6 2-3"/><path d="M2 5h12"/><path d="M7 2h1"/><path d="m22 22-5-10-5 10"/><path d="M14 18h6"/>"#);
lucide!(I_X, "l-x", r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#);
lucide!(I_ARROW, "l-arrow-right", r#"<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>"#);
lucide!(I_UP, "l-chevron-up", r#"<path d="m18 15-6-6-6 6"/>"#);
lucide!(I_DOWN, "l-chevron-down", r#"<path d="m6 9 6 6 6-6"/>"#);
lucide!(I_SUN, "l-sun", r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#);
lucide!(I_MOON, "l-moon", r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#);
lucide!(I_LEFT_PANEL, "l-panel-left", r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#);
lucide!(I_RIGHT_PANEL, "l-panel-right", r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/>"#);
lucide!(I_USERS, "l-users", r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>"#);
lucide!(I_PLUS, "l-plus", r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#);
lucide!(I_UPLOAD, "l-upload", r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m17 8-5-5-5 5"/><path d="M12 3v12"/>"#);
lucide!(I_BOOK, "l-book-open", r#"<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>"#);

// ---------- model ----------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Lang { Source, Translation }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Page { Read, Cast }
#[derive(Clone, Copy, PartialEq, Debug)]
enum RightTab { Suggestions, Terms }
#[derive(Clone, Copy, PartialEq, Debug)]
enum St { Pending, Accepted, Skipped, Stale }

struct Block { id: u32, pair: u32, edit: TextEdit }
struct Chapter { num: u32, sides: [Vec<Block>; 2], held: Option<String>, queued: bool }
struct Term { term: String, og: String, def: String, forbidden: Vec<String>, counts: Vec<u32>, drift: Vec<usize> }
struct Proposed { chapter: usize, block: u32, before: String, after: String, st: St }
struct Mark { chapter: usize, block: u32, quote: String, said: String, scope: usize, rename: bool, edits: Vec<Proposed> }
struct Intake { text: String, num: Option<u32>, dup: bool }

struct State {
    book: String,
    chapters: Vec<Chapter>,
    terms: Vec<Term>,
    by_len: Vec<usize>,
    current: usize,
    page: Page,
    lang: Lang,
    dark: bool,
    left_open: bool,
    right_open: bool,
    right_tab: RightTab,
    interlinear: bool,
    unfolded: Vec<(usize, u32)>,
    selection: Option<(u32, String)>,
    reach: (usize, usize),
    composing: bool,
    composer: TextEdit,
    scope: usize,
    rename: bool,
    marks: Vec<Mark>,
    review: Option<(usize, usize)>,
    review_return: (usize, f32),
    term_card: Option<(u32, usize)>,
    palette: Option<TextEdit>,
    intake: Option<Intake>,
    toast: Option<String>,
    scroll_y: Rc<Cell<f32>>,
    tops: Rc<RefCell<Vec<(u32, f32)>>>,
    next_id: u32,
}

// ---------- text helpers ----------

fn is_word(ch: char) -> bool { ch.is_alphanumeric() && ch.is_ascii() }

/// Byte offsets of `needle` in `hay`; Latin needles must sit on word boundaries.
fn find_all(hay: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() { return vec![]; }
    let bounded = needle.chars().next().is_some_and(|ch| ch.is_ascii());
    hay.match_indices(needle).map(|(i, _)| i).filter(|&i| {
        !bounded || (!hay[..i].chars().next_back().is_some_and(is_word) && !hay[i + needle.len()..].chars().next().is_some_and(is_word))
    }).collect()
}

fn replace_all(hay: &str, from: &str, to: &str) -> (String, usize) {
    let hits = find_all(hay, from);
    let mut out = String::new();
    let mut last = 0;
    for &i in &hits { out.push_str(&hay[last..i]); out.push_str(to); last = i + from.len(); }
    out.push_str(&hay[last..]);
    (out, hits.len())
}

fn split_blocks(md: &str) -> Vec<String> {
    md.split("\n\n").map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).collect()
}

/// A chapter number from the first lines of pasted raw text ("227화", "第227話", "Chapter 227").
fn detect_num(raw: &str) -> Option<u32> {
    for line in raw.lines().filter(|l| !l.trim().is_empty()).take(5) {
        if !["화", "話", "章", "Chapter", "chapter"].iter().any(|m| line.contains(m)) { continue; }
        let digits: String = line.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse() { return Some(n); }
    }
    None
}

fn short(t: &str) -> String { if t.chars().count() > 32 { format!("{}…", t.chars().take(30).collect::<String>().trim_end()) } else { t.to_string() } }
fn plural(n: usize, w: &str) -> String { format!("{n} {w}{}", if n == 1 { "" } else { "s" }) }

// ---------- loading ----------

fn blocks(next: &mut u32, src: Vec<String>, dst: Vec<String>) -> [Vec<Block>; 2] {
    // Naive pairing by index; the real app gets Pairs from numbered paragraphs in translation.
    let pairs = src.len().min(dst.len()).max(1);
    let mut mk = |v: Vec<String>| v.into_iter().enumerate().map(|(i, t)| { *next += 1; Block { id: *next, pair: i.min(pairs - 1) as u32, edit: TextEdit::new(t) } }).collect::<Vec<_>>();
    let s = mk(src);
    [s, mk(dst)]
}

fn load() -> (String, Vec<Chapter>, Vec<Term>, u32) {
    let dir = std::path::PathBuf::from(std::env::var("CIPHER_BOOK").unwrap_or_else(|_| "../../Test-Books/S-Class-Transmigrator".into()));
    let mut nums: Vec<u32> = std::fs::read_dir(dir.join("raw")).map(|r| r.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_string_lossy().trim_end_matches(".md").parse().ok()).collect()).unwrap_or_default();
    nums.sort();
    let mut next = 0;
    let n = nums.len();
    let mut chapters = vec![];
    for (i, num) in nums.into_iter().enumerate() {
        let raw = std::fs::read_to_string(dir.join(format!("raw/{num}.md"))).unwrap_or_default();
        let tl = std::fs::read_to_string(dir.join(format!("tl/{num}.md"))).unwrap_or_default();
        // Simulated: the last two chapters are still waiting in the translation queue.
        let queued = i + 2 >= n;
        let sides = blocks(&mut next, split_blocks(&raw), if queued { vec![] } else { split_blocks(&tl) });
        chapters.push(Chapter { num, sides, held: queued.then_some(tl), queued });
    }
    let glossary: Vec<serde_json::Value> = std::fs::read_to_string(dir.join("glossary.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut terms: Vec<Term> = glossary.iter().map(|g| Term {
        term: g["term"].as_str().unwrap_or("").into(), og: g["og_term"].as_str().unwrap_or("").into(),
        def: g["definition"].as_str().unwrap_or("").into(), forbidden: vec![], counts: vec![], drift: vec![],
    }).filter(|t| t.term.len() >= 2).collect();
    for t in terms.iter_mut() { measure(t, &chapters); }
    let name = dir.file_name().map(|f| f.to_string_lossy().replace('-', " ")).unwrap_or_default();
    (name, chapters, terms, next)
}

fn side_text(ch: &Chapter, side: usize) -> String {
    ch.sides[side].iter().map(|b| b.edit.text.as_str()).collect::<Vec<_>>().join("\n\n")
}

/// Per-chapter use of a Term, and Drift (docs/design/translation-pipeline.md): a Forbidden
/// Rendering is still present, or the source names the Term 3+ times and the translation never
/// renders it. Local substring counts, so it costs no tokens.
fn measure(t: &mut Term, chapters: &[Chapter]) {
    t.counts.clear();
    t.drift.clear();
    for (i, ch) in chapters.iter().enumerate() {
        let tl = if ch.queued { 0 } else { find_all(&side_text(ch, 1), &t.term).len() };
        t.counts.push(tl as u32);
        if ch.queued { continue; }
        let text = side_text(ch, 1);
        let src = if t.og.chars().count() < 3 { 0 } else { find_all(&side_text(ch, 0), &t.og).len() };
        let forbidden = t.forbidden.iter().any(|f| !find_all(&text, f).is_empty());
        if forbidden || (src >= 3 && tl == 0) { t.drift.push(i); }
    }
}

fn init() -> State {
    let (book, chapters, terms, next_id) = load();
    let mut by_len: Vec<usize> = (0..terms.len()).collect();
    by_len.sort_by_key(|&i| std::cmp::Reverse(terms[i].term.len()));
    State {
        book, chapters, terms, by_len, current: 0, page: Page::Read, lang: Lang::Translation, dark: false,
        left_open: true, right_open: true, right_tab: RightTab::Terms, interlinear: false,
        unfolded: vec![], selection: None, reach: (0, 0), composing: false, composer: TextEdit::new(""), scope: 0, rename: false, marks: vec![],
        review: None, review_return: (0, 0.0), term_card: None, palette: None, intake: None, toast: None,
        scroll_y: Rc::default(), tops: Rc::default(), next_id,
    }
}

// ---------- rendering text ----------

/// Markdown to display text: headings plus **bold** / *italic* spans.
fn render(md: &str, body: TextStyle) -> (String, Vec<Span>, TextStyle) {
    let (src, style) = match md.find(|c| c != '#') {
        Some(n) if n > 0 && md[n..].starts_with(' ') => (&md[n + 1..], TextStyle::new(24.0, 600.0).lh(34.0).family(SERIF)),
        _ => (md, body),
    };
    let (mut out, mut spans, mut open) = (String::new(), vec![], Vec::<(&str, usize)>::new());
    let mut i = 0;
    while i < src.len() {
        let m = if src[i..].starts_with("**") { Some("**") } else if src[i..].starts_with('*') || src[i..].starts_with('_') { Some(&src[i..i + 1]) } else { None };
        if let Some(m) = m {
            if let Some(p) = open.iter().rposition(|(o, _)| *o == m) {
                let (_, st) = open.remove(p);
                let sp = Span::new(st, out.len());
                spans.push(if m == "**" { sp.bold() } else { sp.italic() });
            } else { open.push((m, out.len())); }
            i += m.len();
            continue;
        }
        let ch = src[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, spans, style)
}

/// Glossary Terms in the shown text, longest first, never overlapping.
fn term_hits(s: &State, shown: &str, side: usize) -> Vec<(usize, usize, usize)> {
    let mut hits: Vec<(usize, usize, usize)> = vec![];
    for &ti in &s.by_len {
        let t = &s.terms[ti];
        let needle = if side == 1 { &t.term } else { &t.og };
        if needle.chars().count() < 2 { continue; }
        for st in find_all(shown, needle) {
            let en = st + needle.len();
            if hits.iter().all(|&(a, b, _)| en <= a || st >= b) { hits.push((st, en, ti)); }
        }
    }
    hits
}

fn find_block(s: &State, ci: usize, id: u32) -> Option<(usize, usize)> {
    let ch = &s.chapters[ci];
    (0..2).find_map(|side| ch.sides[side].iter().position(|b| b.id == id).map(|i| (side, i)))
}

fn block_text(s: &State, ci: usize, id: u32) -> Option<&str> {
    find_block(s, ci, id).map(|(sd, i)| s.chapters[ci].sides[sd][i].edit.text.as_str())
}

fn source_of(s: &State, ci: usize, id: u32) -> String {
    let Some((sd, i)) = find_block(s, ci, id) else { return String::new() };
    let ch = &s.chapters[ci];
    let pair = ch.sides[sd][i].pair;
    ch.sides[0].iter().filter(|b| b.pair == pair).map(|b| b.edit.text.as_str()).collect::<Vec<_>>().join("\n")
}

/// The first line a reader would recognise a Chapter by.
fn opening_line(ch: &Chapter) -> String {
    let side = if ch.queued { 0 } else { 1 };
    ch.sides[side].iter().map(|b| b.edit.text.trim_start_matches('#').trim())
        .find(|t| t.chars().count() > 12 && !t.starts_with("Chapter"))
        .map(|t| t.chars().take(80).collect()).unwrap_or_default()
}

// ---------- actions ----------

fn go(s: &mut State, tx: &mut Tx, i: usize) {
    s.current = i.min(s.chapters.len() - 1);
    s.page = Page::Read;
    s.selection = None;
    s.composing = false;
    s.term_card = None;
    tx.scroll_to_y("doc", 0.0);
}

fn go_block(s: &mut State, tx: &mut Tx, ci: usize, id: u32) {
    s.current = ci;
    s.page = Page::Read;
    tx.scroll_key_to(format!("row{id}"), 160.0);
}

fn select(s: &mut State, id: u32, txt: String) {
    let mut blocks_n = 0;
    let mut chapters_n = 0;
    for ch in s.chapters.iter().filter(|c| !c.queued) {
        let n = ch.sides[1].iter().filter(|b| !find_all(&b.edit.text, &txt).is_empty()).count();
        blocks_n += n;
        chapters_n += (n > 0) as usize;
    }
    s.reach = (blocks_n, chapters_n);
    // A known Term defaults to "Everywhere + glossary": a renamed character needs one request.
    s.scope = if s.terms.iter().any(|t| t.term == txt) { 2 } else { 0 };
    // Short selections are usually names: default to a deterministic rename.
    s.rename = txt.split_whitespace().count() <= 4;
    s.selection = Some((id, txt));
    s.composing = false;
}

fn open_composer(s: &mut State, seed: &str) {
    s.composing = true;
    s.composer.replace_text(seed.to_string());
    s.composer.focused = true;
    s.composer.set_caret(seed.len());
}

/// Turns the composer into a Mark. Simulated model: the request text is the new rendering
/// ("A -> B" also works), applied as a word-bounded replace to every Block in Scope.
fn submit(s: &mut State) {
    let Some((id, quote)) = s.selection.clone() else { return };
    let said = s.composer.text.trim().to_string();
    if said.is_empty() { return; }
    let (from, to) = match said.split_once("->").or_else(|| said.split_once('→')) {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => (quote.clone(), said.clone()),
    };
    let mut edits = vec![];
    for (ci, ch) in s.chapters.iter().enumerate() {
        if ch.queued { continue; }
        for b in &ch.sides[1] {
            if s.scope == 0 && !(ci == s.current && b.id == id) { continue; }
            let (after, n) = replace_all(&b.edit.text, &from, &to);
            if n > 0 { edits.push(Proposed { chapter: ci, block: b.id, before: b.edit.text.clone(), after, st: St::Pending }); }
        }
    }
    if s.scope == 2 {
        if let Some(t) = s.terms.iter_mut().find(|t| t.term == from) {
            t.forbidden.push(std::mem::replace(&mut t.term, to.clone()));
            measure(t, &s.chapters);
        }
        else { s.terms.push(Term { term: to.clone(), og: String::new(), def: format!("Added from a fix: “{from}” → “{to}”."), forbidden: vec![], counts: vec![0; s.chapters.len()], drift: vec![] }); }
        s.by_len = (0..s.terms.len()).collect();
        let terms = &s.terms;
        s.by_len.sort_by_key(|&i| std::cmp::Reverse(terms[i].term.len()));
    }
    s.marks.push(Mark { chapter: s.current, block: id, quote, said, scope: s.scope, rename: s.rename, edits });
    s.selection = None;
    s.composing = false;
    s.composer.replace_text("");
    s.right_tab = RightTab::Suggestions;
}

/// After edits land, Terms with Forbidden Renderings may stop drifting.
fn remeasure(s: &mut State) {
    let chapters = std::mem::take(&mut s.chapters);
    for t in s.terms.iter_mut().filter(|t| !t.forbidden.is_empty()) { measure(t, &chapters); }
    s.chapters = chapters;
}

fn next_pending(m: &Mark, from: usize, dir: i32) -> Option<usize> {
    let n = m.edits.len() as i32;
    (1..=n).map(|k| (from as i32 + dir * k).rem_euclid(n) as usize).find(|&i| m.edits[i].st == St::Pending)
}

fn show_edit(s: &mut State, tx: &mut Tx, mi: usize, ei: usize) {
    let e = &s.marks[mi].edits[ei];
    let (ci, id) = (e.chapter, e.block);
    s.lang = Lang::Translation;
    s.review = Some((mi, ei));
    go_block(s, tx, ci, id);
    tx.scroll_key_to(format!("ch{ci}"), 200.0);
}

fn start_review(s: &mut State, tx: &mut Tx, mi: usize) {
    s.review_return = (s.current, s.scroll_y.get());
    if let Some(ei) = s.marks[mi].edits.iter().position(|e| e.st == St::Pending) { show_edit(s, tx, mi, ei); }
}

fn end_review(s: &mut State, tx: &mut Tx) {
    s.review = None;
    let (ci, y) = s.review_return;
    s.current = ci;
    tx.scroll_to_y("doc", y);
}

/// Accepts (or skips) one Proposed Edit. An edit whose Block changed since it was proposed is
/// Stale and is never applied over the newer text.
fn decide(s: &mut State, mi: usize, ei: usize, accept: bool) {
    let (ci, id, before, after) = { let e = &s.marks[mi].edits[ei]; (e.chapter, e.block, e.before.clone(), e.after.clone()) };
    let st = if !accept { St::Skipped } else if block_text(s, ci, id) != Some(&before) { St::Stale } else {
        if let Some((sd, i)) = find_block(s, ci, id) { s.chapters[ci].sides[sd][i].edit.replace_text(after); }
        St::Accepted
    };
    s.marks[mi].edits[ei].st = st;
}

fn accept_all(s: &mut State, mi: usize) {
    for i in 0..s.marks[mi].edits.len() { if s.marks[mi].edits[i].st == St::Pending { decide(s, mi, i, true); } }
    remeasure(s);
}

fn step(s: &mut State, tx: &mut Tx, accept: Option<bool>, all: bool) {
    let Some((mi, ei)) = s.review else { return };
    if all { accept_all(s, mi); } else if let Some(a) = accept { decide(s, mi, ei, a); }
    match next_pending(&s.marks[mi], ei, 1) {
        Some(n) => show_edit(s, tx, mi, n),
        None => { s.toast = Some(summary(&s.marks[mi])); remeasure(s); end_review(s, tx); }
    }
}

fn undo_mark(s: &mut State, mi: usize) {
    for ei in 0..s.marks[mi].edits.len() {
        let (ci, id, before, after, st) = { let e = &s.marks[mi].edits[ei]; (e.chapter, e.block, e.before.clone(), e.after.clone(), e.st) };
        if st == St::Accepted && block_text(s, ci, id) == Some(&after) {
            if let Some((sd, i)) = find_block(s, ci, id) { s.chapters[ci].sides[sd][i].edit.replace_text(before); }
            s.marks[mi].edits[ei].st = St::Pending;
        }
    }
    remeasure(s);
}

fn count(m: &Mark, st: St) -> usize { m.edits.iter().filter(|e| e.st == st).count() }
fn summary(m: &Mark) -> String {
    let (a, x, z) = (count(m, St::Accepted), count(m, St::Skipped), count(m, St::Stale));
    let mut v = vec![format!("{a} applied")];
    if x > 0 { v.push(format!("{x} skipped")); }
    if z > 0 { v.push(format!("{z} changed since")); }
    format!("“{}” → “{}”: {}", short(&m.quote), short(&m.said), v.join(", "))
}
fn chapters_of(m: &Mark) -> usize {
    let mut v: Vec<usize> = m.edits.iter().map(|e| e.chapter).collect();
    v.dedup();
    v.len()
}

fn toggle_lang(s: &mut State, tx: &mut Tx) {
    let y = s.scroll_y.get();
    let tops = s.tops.borrow().clone();
    let top = tops.iter().filter(|(_, t)| *t <= y + 4.0).last().or(tops.first()).copied();
    let ci = s.current;
    let pair = top.and_then(|(id, _)| find_block(s, ci, id)).map(|(sd, i)| s.chapters[ci].sides[sd][i].pair);
    s.lang = if s.lang == Lang::Source { Lang::Translation } else { Lang::Source };
    s.selection = None;
    let side = if s.lang == Lang::Source { 0 } else { 1 };
    if let (Some(p), Some((_, t))) = (pair, top) {
        if let Some(b) = s.chapters[ci].sides[side].iter().find(|b| b.pair == p) { tx.scroll_key_to(format!("row{}", b.id), t - y); }
    }
}

fn translate_now(s: &mut State, ci: usize) {
    let ch = &mut s.chapters[ci];
    match ch.held.take() {
        Some(tl) => {
            let src: Vec<String> = ch.sides[0].iter().map(|b| b.edit.text.clone()).collect();
            ch.sides = blocks(&mut s.next_id, src, split_blocks(&tl));
            ch.queued = false;
            let chapters = std::mem::take(&mut s.chapters);
            for t in s.terms.iter_mut() { measure(t, &chapters); }
            s.chapters = chapters;
        }
        None => s.toast = Some("No Endpoint in this prototype, so pasted chapters stay queued.".into()),
    }
}

fn add_chapter(s: &mut State, tx: &mut Tx) {
    let Some(it) = s.intake.take() else { return };
    let num = it.num.unwrap_or_else(|| s.chapters.last().map_or(1, |c| c.num + 1));
    let src = split_blocks(&it.text);
    let sides = blocks(&mut s.next_id, src, vec![]);
    let at = s.chapters.iter().position(|c| c.num > num).unwrap_or(s.chapters.len());
    s.chapters.insert(at, Chapter { num, sides, held: None, queued: true });
    for t in s.terms.iter_mut() {
        t.counts.insert(at, 0);
        for d in t.drift.iter_mut() { if *d >= at { *d += 1; } }
    }
    for m in s.marks.iter_mut() {
        if m.chapter >= at { m.chapter += 1; }
        for e in m.edits.iter_mut() { if e.chapter >= at { e.chapter += 1; } }
    }
    go(s, tx, at);
    tx.scroll_key_to(format!("ch{at}"), 200.0);
}

fn open_palette(s: &mut State) {
    let mut e = TextEdit::new("");
    e.focused = true;
    s.palette = Some(e);
}

/// Shows a Term in the context sidebar (or the margin), anchored at its first use here.
fn show_term(s: &mut State, tx: Option<&mut Tx>, ti: usize) {
    let ci = s.current;
    let needle = s.terms[ti].term.clone();
    let id = s.chapters[ci].sides[1].iter().find(|b| !find_all(&b.edit.text, &needle).is_empty()).map(|b| b.id);
    if let Some(id) = id {
        s.term_card = Some((id, ti));
        s.right_tab = RightTab::Terms;
        if let Some(tx) = tx { tx.scroll_key_to(format!("row{id}"), 160.0); }
    }
}

enum Hit { Chapter(usize), Term(usize), Act(&'static str, String) }

fn palette_hits(s: &State, q: &str) -> Vec<Hit> {
    let q = q.trim().to_lowercase();
    let mut v = vec![];
    for (name, sub) in [
        ("Cast & terms", format!("{} terms, {} drifting", s.terms.len(), s.terms.iter().filter(|t| !t.drift.is_empty()).count())),
        ("Suggestions", format!("{} open", s.marks.iter().filter(|m| count(m, St::Pending) > 0).count())),
        ("Show source", "Read the chapter in its own language (Ctrl+T)".into()),
        ("Interlinear", "Source under every paragraph (Ctrl+I)".into()),
        (if s.dark { "Light mode" } else { "Dark mode" }, "Ctrl+D".into()),
        ("Publish to Reader", "Upload changed chapters to your phone".into()),
        ("Add chapters", "Paste raw text anywhere (Ctrl+V)".into()),
    ] {
        if q.is_empty() || name.to_lowercase().contains(&q) { v.push(Hit::Act(name, sub)); }
    }
    if !q.is_empty() {
        for (i, ch) in s.chapters.iter().enumerate() { if ch.num.to_string().starts_with(&q) { v.push(Hit::Chapter(i)); } }
        for (i, t) in s.terms.iter().enumerate() { if t.term.to_lowercase().contains(&q) || t.og.contains(&q) { v.push(Hit::Term(i)); } }
    }
    v.truncate(9);
    v
}

fn run_hit(s: &mut State, tx: &mut Tx, h: &Hit) {
    s.palette = None;
    match h {
        Hit::Chapter(i) => { go(s, tx, *i); tx.scroll_key_to(format!("ch{i}"), 200.0); }
        Hit::Term(i) => { s.page = Page::Cast; tx.scroll_key_to(format!("term{i}"), 120.0); }
        Hit::Act(name, _) => match *name {
            "Cast & terms" => s.page = Page::Cast,
            "Suggestions" => { s.right_open = true; s.right_tab = RightTab::Suggestions; }
            "Show source" => toggle_lang(s, tx),
            "Interlinear" => s.interlinear = !s.interlinear,
            "Dark mode" | "Light mode" => s.dark = !s.dark,
            "Publish to Reader" => s.toast = Some("Publish is not wired in this prototype.".into()),
            _ => s.toast = Some("Copy a raw chapter from the site, then press Ctrl+V anywhere.".into()),
        },
    }
}

fn on_key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> bool {
    match &k.key {
        Key::Command('k') => { open_palette(s); return true; }
        Key::Command('\\') => { s.left_open = !s.left_open; return true; }
        Key::Command('j') => { s.right_open = !s.right_open; return true; }
        Key::Command('g') => { s.page = if s.page == Page::Cast { Page::Read } else { Page::Cast }; return true; }
        Key::Command('i') => { s.interlinear = !s.interlinear; return true; }
        Key::Command('t') => { toggle_lang(s, tx); return true; }
        Key::Command('d') => { s.dark = !s.dark; return true; }
        _ => {}
    }
    if let Some((mi, ei)) = s.review {
        match &k.key {
            Key::Text(t) if t == "j" => { if let Some(n) = next_pending(&s.marks[mi], ei, 1) { show_edit(s, tx, mi, n); } }
            Key::Text(t) if t == "k" => { if let Some(n) = next_pending(&s.marks[mi], ei, -1) { show_edit(s, tx, mi, n); } }
            Key::Text(t) if t == "a" => step(s, tx, Some(true), false),
            Key::Text(t) if t == "A" => step(s, tx, None, true),
            Key::Text(t) if t == "x" => step(s, tx, Some(false), false),
            Key::Escape => end_review(s, tx),
            _ => return false,
        }
        return true;
    }
    match &k.key {
        // With a Selection, typing starts a fix; Ctrl+C still copies.
        Key::Text(t) if s.selection.is_some() && !s.composing && !t.trim().is_empty() => { open_composer(s, t); true }
        Key::Escape => {
            if s.composing { s.composing = false; } else if s.selection.is_some() { s.selection = None; }
            else if s.term_card.is_some() { s.term_card = None; }
            else if s.page == Page::Cast { s.page = Page::Read; } else { s.toast = None; }
            true
        }
        _ => false,
    }
}

// ---------- view: small parts ----------

fn btn(label: &str, primary: bool, p: &Pal) -> El {
    let (bg, fg) = if primary { (p.ink, p.paper) } else { (Color::TRANSPARENT, p.ink2) };
    row().h(26.0).px(10.0).center_y().radius(5.0).fill(bg).hover(if primary { p.primary_hover } else { p.hover }).cursor(Cursor::Pointer)
        .border(1.0, if primary { p.ink } else { p.rule }).child(text(label).font(UI_MED).color(fg))
}

fn chip(label: &str, on: bool, p: &Pal) -> El {
    text(label).font(SMALL).color(if on { p.paper } else { p.ink2 }).px(9.0).py(3.0).radius(12.0)
        .fill(if on { p.ink } else { Color::TRANSPARENT }).border(1.0, if on { p.ink } else { p.rule })
        .hover(if on { p.ink } else { p.hover }).cursor(Cursor::Pointer)
}

fn kbd(k: &str, p: &Pal) -> El {
    text(k).font(TextStyle::new(11.0, 500.0)).color(p.ink3).px(5.0).py(1.0).radius(4.0).border(1.0, p.rule)
}

fn icon_btn(i: &'static Icon, key: &str, on: bool, p: &Pal) -> El {
    el().key(key.to_string()).size(28.0, 28.0).center().radius(6.0).hover(p.hover).cursor(Cursor::Pointer)
        .fill(if on { p.selected } else { Color::TRANSPARENT }).child(icon(i, 16.0).color(if on { p.ink } else { p.ink3 }))
}

fn dot(color: Color, d: f32) -> El { el().size(d, d).radius(d / 2.0).fill(color) }

fn card(p: &Pal) -> El { col().fill(p.card).radius(10.0).border(1.0, p.rule).shadow(10.0, 30.0, p.shadow) }

fn section(label: &str, extra: String, p: &Pal) -> El {
    row().w_full().px(18.0).pt(18.0).pb(8.0).center_y().child(text(label.to_string()).font(LABEL).color(p.ink3).grow(1.0))
        .child(text(extra).font(SMALL).color(p.ink4))
}

// ---------- view: contents (left) ----------

fn contents(s: &State, p: &Pal) -> El {
    let total = s.chapters.len();
    let waiting = s.chapters.iter().filter(|c| c.queued).count();
    let done = total - waiting;
    let pending: Vec<usize> = s.marks.iter().flat_map(|m| m.edits.iter().filter(|e| e.st == St::Pending).map(|e| e.chapter)).collect();
    let drift_n = s.terms.iter().filter(|t| !t.drift.is_empty()).count();

    let header = row().h(HEADER_H).w_full().pl(18.0).pr(10.0).center_y().gap(8.0)
        .child(text(s.book.clone()).font(TextStyle::new(16.0, 500.0).italic().family(SERIF)).color(p.ink).grow(1.0).truncate())
        .child(icon_btn(&I_LEFT_PANEL, "hide-left", false, p).help("Hide contents (Ctrl+\\)").on_press(|s: &mut State| s.left_open = false));

    // Progress: translated vs waiting, as one hairline.
    let frac = done as f32 / total.max(1) as f32;
    let progress = col().w_full().px(18.0).pb(14.0).gap(7.0)
        .child(row().w_full().h(3.0).radius(2.0).fill(p.rule)
            .child(el().h_full().w(frac * (LEFT_W - 36.0)).radius(2.0).fill(p.ink3))
            .child(el().h_full().grow(1.0).radius(2.0).fill(if waiting > 0 { p.amber } else { p.rule })))
        .child(text(format!("{done} translated · {waiting} waiting · {} chapters", total)).font(SMALL).color(p.ink3));

    let nav = |i: &'static Icon, label: String, extra: String, key: &str, on: bool| row().key(key.to_string()).h(32.0).mx(8.0).px(10.0).gap(10.0).center_y().radius(6.0)
        .fill(if on { p.selected } else { Color::TRANSPARENT }).hover(p.hover).cursor(Cursor::Pointer)
        .child(icon(i, 15.0).color(if on { p.ink } else { p.ink3 })).child(text(label).font(UI).color(if on { p.ink } else { p.ink2 }).grow(1.0))
        .child(text(extra).font(SMALL).color(p.ink3));
    let navs = col().w_full().gap(1.0).pb(6.0)
        .child(nav(&I_BOOK, "Reading".into(), String::new(), "nav-read", s.page == Page::Read).on_press(|s: &mut State| s.page = Page::Read))
        .child(nav(&I_USERS, "Cast & terms".into(), if drift_n > 0 { format!("{drift_n} drifting") } else { String::new() }, "nav-cast", s.page == Page::Cast)
            .on_press(|s: &mut State| s.page = Page::Cast))
        .child(nav(&I_PLUS, "Add chapters".into(), "Ctrl V".into(), "nav-add", false)
            .on_press(|s: &mut State| s.toast = Some("Copy a raw chapter from the site, then press Ctrl+V anywhere.".into())));

    let mut list = col().w_full().px(8.0).pb(16.0).gap(1.0);
    for (i, ch) in s.chapters.iter().enumerate() {
        let on = i == s.current && s.page == Page::Read;
        let marks = pending.iter().filter(|&&c| c == i).count();
        let mut right = row().gap(6.0).center_y();
        if marks > 0 { right = right.child(text(marks.to_string()).font(TextStyle::new(10.5, 600.0)).color(p.on_red).px(5.0).radius(7.0).fill(p.red)); }
        if ch.queued { right = right.child(dot(p.amber, 6.0)); }
        list = list.child(row().key(format!("ch{i}")).w_full().py(7.0).pl(10.0).pr(10.0).gap(12.0).radius(6.0).cursor(Cursor::Pointer)
            .fill(if on { p.selected } else { Color::TRANSPARENT }).hover(p.hover)
            .child(text(ch.num.to_string()).font(TextStyle::new(13.0, if on { 600.0 } else { 450.0 }).features("tnum")).color(if on { p.red } else { p.ink3 }).w(28.0).pt(1.0))
            .child(col().grow(1.0).min_w(0.0).gap(1.0)
                .child(text(opening_line(ch)).font(TextStyle::new(12.5, 400.0).lh(17.0).family(if ch.queued { CJK } else { SERIF }))
                    .color(if on { p.ink } else if ch.queued { p.ink3 } else { p.ink2 }).wrap().max_lines(2)))
            .child(right)
            .on_press_tx(move |s: &mut State, tx| go(s, tx, i)));
    }

    col().key("contents").w(LEFT_W).h_full().fill(p.side).border_sides([0.0, 1.0, 0.0, 0.0], p.rule)
        .child(header).child(progress).child(navs)
        .child(section("CHAPTERS", String::new(), p).pt(10.0))
        .child(list.scroll(ScrollSpec { stick_bottom: false }).key("contents-list").grow(1.0).min_h(0.0))
}

/// With the contents closed, the window edge keeps a scrubber: one tick per Chapter, a red one
/// where edits wait for Review, a longer tick and label every ten.
fn scrubber(s: &State, p: &Pal) -> El {
    let pending: Vec<usize> = s.marks.iter().flat_map(|m| m.edits.iter().filter(|e| e.st == St::Pending).map(|e| e.chapter)).collect();
    let mut bar = col().w(44.0).h_full().py(14.0);
    for (i, ch) in s.chapters.iter().enumerate() {
        let cur = i == s.current;
        let tens = ch.num % 10 == 0;
        let color = if cur { p.red } else if pending.contains(&i) { p.red_soft } else if ch.queued { p.amber } else if i < s.current { p.ink3 } else { p.ink4 };
        let w = if cur { 20.0 } else if tens { 12.0 } else { 7.0 };
        let label = if cur || tens { text(ch.num.to_string()).font(TextStyle::new(9.5, if cur { 650.0 } else { 400.0 })).color(if cur { p.red } else { p.ink4 }) } else { el() };
        bar = bar.child(row().key(format!("tick{i}")).w_full().grow(1.0).min_h(0.0).center_y().justify_start().gap(4.0).pl(10.0).cursor(Cursor::Pointer)
            .hover(p.hover).help(format!("Chapter {}{}", ch.num, if ch.queued { " · waiting to translate" } else { "" }))
            .child(el().w(w).h(if cur { 2.0 } else { 1.5 }).fill(color)).child(label)
            .on_press_tx(move |s: &mut State, tx| go(s, tx, i)));
    }
    bar
}

// ---------- view: context (right) ----------

/// A one-line sample of an edit: the diff with ~40 characters of context either side.
fn sample(e: &Proposed, p: &Pal) -> El {
    let (shown, spans) = diff_text(&e.before, &e.after, p);
    let (a, z) = (spans[0].start as usize, spans[1].end as usize);
    let lo = shown[..a].char_indices().rev().nth(40).map_or(0, |(i, _)| i);
    let hi = shown[z..].char_indices().nth(40).map_or(shown.len(), |(i, _)| z + i);
    let lead = if lo > 0 { "…" } else { "" };
    let off = lo as i64 - lead.len() as i64;
    let text = format!("{lead}{}{}", &shown[lo..hi], if hi < shown.len() { "…" } else { "" });
    let spans = spans.into_iter().map(|mut sp| { sp.start = (sp.start as i64 - off) as u32; sp.end = (sp.end as i64 - off) as u32; sp }).collect();
    rich(text, spans).font(TextStyle::new(13.0, 400.0).lh(19.0).family(SERIF)).color(p.ink2).wrap().max_lines(2)
}

fn mark_card(s: &State, mi: usize, p: &Pal, show_where: bool) -> El {
    let m = &s.marks[mi];
    let pending = count(m, St::Pending);
    let scope = ["JUST HERE", "EVERYWHERE", "EVERYWHERE + GLOSSARY"][m.scope];
    let what = if m.rename { "exact replacement" } else { "edit" };
    let status = if pending > 0 { format!("{} in {}", plural(pending, what), plural(chapters_of(m), "chapter")) } else { summary(m).split(": ").nth(1).unwrap_or("").to_string() };
    let mut actions = row().gap(6.0).pt(4.0);
    // Renames are deterministic, so they review as one group: a few samples and Accept all.
    let mut samples = col().gap(6.0);
    if m.rename && pending > 0 {
        for e in m.edits.iter().filter(|e| e.st == St::Pending).take(2) { samples = samples.child(sample(e, p)); }
        actions = actions.child(btn(&format!("Accept all {pending}"), true, p).key(format!("all{mi}")).on_press(move |s: &mut State| accept_all(s, mi)))
            .child(btn("One by one", false, p).key(format!("rev{mi}")).on_press_tx(move |s: &mut State, tx| start_review(s, tx, mi)));
    } else if pending > 0 {
        actions = actions.child(btn("Review", true, p).key(format!("rev{mi}")).on_press_tx(move |s: &mut State, tx| start_review(s, tx, mi)))
            .child(btn("Accept all", false, p).key(format!("all{mi}")).on_press(move |s: &mut State| accept_all(s, mi)));
    } else if count(m, St::Accepted) > 0 {
        actions = actions.child(btn("Undo", false, p).key(format!("undo{mi}")).on_press(move |s: &mut State| undo_mark(s, mi)));
    }
    let (ci, id) = (m.chapter, m.block);
    let mut head = row().w_full().gap(6.0).center_y().child(text(scope).font(LABEL).color(if pending > 0 { p.red } else { p.ink3 }).grow(1.0));
    if show_where {
        head = head.child(text(format!("ch {}", s.chapters[ci].num)).font(SMALL).color(p.ink3).px(6.0).radius(4.0).hover(p.hover).cursor(Cursor::Pointer)
            .key(format!("where{mi}")).on_press_tx(move |s: &mut State, tx| go_block(s, tx, ci, id)));
    }
    col().w_full().gap(4.0).pl(12.0).border_sides([0.0, 0.0, 0.0, 2.0], if pending > 0 { p.red } else { p.ink4 })
        .child(head)
        .child(text(format!("“{}”", m.quote)).font(TextStyle::new(14.0, 400.0).italic().family(SERIF)).color(p.ink3).max_lines(2))
        .child(text(m.said.clone()).font(TextStyle::new(14.0, 550.0)).color(p.ink).wrap())
        .child(text(status).font(SMALL).color(p.ink3).wrap())
        .child(samples)
        .child(actions)
}

fn term_card(s: &State, id: u32, ti: usize, p: &Pal) -> El {
    let t = &s.terms[ti];
    let total: u32 = t.counts.iter().sum();
    let chs = t.counts.iter().filter(|&&n| n > 0).count();
    let mut c = col().w_full().gap(5.0).pl(12.0).border_sides([0.0, 0.0, 0.0, 2.0], p.ink)
        .child(row().w_full().child(text("TERM").font(LABEL).color(p.ink3).grow(1.0))
            .child(el().key("term-x").size(18.0, 18.0).center().radius(4.0).hover(p.hover).cursor(Cursor::Pointer).child(icon(&I_X, 12.0).color(p.ink3))
                .on_press(|s: &mut State| s.term_card = None)))
        .child(text(t.term.clone()).font(TextStyle::new(17.0, 600.0).family(SERIF)).color(p.ink))
        .child(text(t.og.clone()).font(TextStyle::new(14.0, 400.0).family(CJK)).color(p.ink2))
        .child(text(t.def.clone()).font(SMALL.lh(17.0)).color(p.ink2).wrap().max_lines(5))
        .child(text(format!("{} in {}", plural(total as usize, "use"), plural(chs, "chapter"))).font(SMALL).color(p.ink3));
    if !t.forbidden.is_empty() {
        c = c.child(text(format!("Never: {}", t.forbidden.join(", "))).font(SMALL).color(p.ink2).wrap());
    }
    if !t.drift.is_empty() {
        c = c.child(text(format!("The source names it more often than the translation does in {}", plural(t.drift.len(), "chapter"))).font(SMALL).color(p.red).wrap());
    }
    let name = t.term.clone();
    c.child(row().pt(4.0).child(btn("Rename everywhere", false, p).key("term-rename").on_press(move |s: &mut State| {
        select(s, id, name.clone());
        s.scope = 2;
        open_composer(s, "");
    })))
}

fn context(s: &State, p: &Pal) -> El {
    let ci = s.current;
    let ch = &s.chapters[ci];
    let open = s.marks.iter().filter(|m| count(m, St::Pending) > 0).count();

    // Terms in this Chapter, in order of first appearance.
    let tl = if ch.queued { String::new() } else { side_text(ch, 1) };
    let mut here: Vec<(usize, usize, usize)> = s.terms.iter().enumerate().filter_map(|(i, t)| {
        let hits = find_all(&tl, &t.term);
        hits.first().map(|&f| (f, i, hits.len()))
    }).collect();
    here.sort();

    let tab = |label: String, on: bool, key: &str| row().key(key.to_string()).h(28.0).px(10.0).center_y().radius(6.0).cursor(Cursor::Pointer)
        .fill(if on { p.selected } else { Color::TRANSPARENT }).hover(p.hover)
        .child(text(label).font(if on { UI_MED } else { UI }).color(if on { p.ink } else { p.ink3 }));
    let header = row().h(HEADER_H).w_full().pl(10.0).pr(10.0).gap(2.0).center_y()
        .child(tab(format!("Terms · {}", here.len()), s.right_tab == RightTab::Terms, "rt-terms").on_press(|s: &mut State| s.right_tab = RightTab::Terms))
        .child(tab(if open > 0 { format!("Suggestions · {open}") } else { "Suggestions".into() }, s.right_tab == RightTab::Suggestions, "rt-req")
            .on_press(|s: &mut State| s.right_tab = RightTab::Suggestions))
        .child(el().grow(1.0))
        .child(icon_btn(&I_RIGHT_PANEL, "hide-right", false, p).help("Hide context (Ctrl+J)").on_press(|s: &mut State| s.right_open = false));

    let mut body = col().w_full().pb(24.0);
    match s.right_tab {
        RightTab::Terms => {
            if let Some((id, ti)) = s.term_card { body = body.child(col().w_full().px(18.0).pt(14.0).pb(6.0).child(term_card(s, id, ti, p))); }
            body = body.child(section("IN THIS CHAPTER", if ch.queued { "after translation".into() } else { String::new() }, p));
            for (_, ti, n) in here {
                let t = &s.terms[ti];
                let on = s.term_card.is_some_and(|(_, x)| x == ti);
                let drift = t.drift.contains(&ci);
                body = body.child(row().key(format!("tl{ti}")).mx(8.0).px(10.0).py(6.0).gap(10.0).center_y().radius(6.0).cursor(Cursor::Pointer)
                    .fill(if on { p.selected } else { Color::TRANSPARENT }).hover(p.hover)
                    .child(col().grow(1.0).min_w(0.0).gap(1.0)
                        .child(text(t.term.clone()).font(TextStyle::new(14.0, 500.0).family(SERIF)).color(p.ink).truncate())
                        .child(text(t.og.clone()).font(TextStyle::new(12.0, 400.0).family(CJK)).color(p.ink3).truncate()))
                    .child(if drift { dot(p.red, 6.0).help("Drifting in this chapter") } else { el() })
                    .child(text(n.to_string()).font(SMALL.features("tnum")).color(p.ink3))
                    .on_press_tx(move |s: &mut State, tx| show_term(s, Some(tx), ti)));
            }
        }
        RightTab::Suggestions => {
            let (mut here_m, mut elsewhere): (Vec<usize>, Vec<usize>) = ((0..s.marks.len()).filter(|&mi| s.marks[mi].chapter == ci).collect(), vec![]);
            here_m.reverse();
            for mi in (0..s.marks.len()).rev() { if s.marks[mi].chapter != ci { elsewhere.push(mi); } }
            if s.marks.is_empty() {
                body = body.child(text("Nothing yet. Select text while reading and type what it should say. Suggestions wait here while you keep reading.")
                    .font(UI.lh(19.0)).color(p.ink3).wrap().px(18.0).pt(18.0));
            }
            for (label, list) in [("THIS CHAPTER", here_m), ("ELSEWHERE", elsewhere)] {
                if list.is_empty() { continue; }
                body = body.child(section(label, String::new(), p));
                let mut cards = col().w_full().px(18.0).gap(22.0).pt(4.0);
                for mi in list { cards = cards.child(mark_card(s, mi, p, label == "ELSEWHERE")); }
                body = body.child(cards);
            }
        }
    }
    col().key("context").w(RIGHT_W).h_full().fill(p.side).border_sides([0.0, 0.0, 0.0, 1.0], p.rule)
        .child(header)
        .child(body.scroll(ScrollSpec { stick_bottom: false }).key("context-body").grow(1.0).min_h(0.0))
}

// ---------- view: the page ----------

fn diff_text(before: &str, after: &str, p: &Pal) -> (String, Vec<Span>) {
    let pre: usize = before.chars().zip(after.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
    let mut pre = pre;
    let mut suf = before[pre..].chars().rev().zip(after[pre..].chars().rev()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
    // Whole words read better than "Magi[c|e]": widen the change to word edges.
    while let Some(ch) = before[..pre].chars().next_back().filter(|c| c.is_alphanumeric()) { pre -= ch.len_utf8(); }
    while let Some(ch) = before[before.len() - suf..].chars().next().filter(|c| c.is_alphanumeric()) { suf -= ch.len_utf8(); }
    let (del, ins) = (&before[pre..before.len() - suf], &after[pre..after.len() - suf]);
    // A thin space keeps the struck-out and inserted words from reading as one.
    let gap = if del.is_empty() || ins.is_empty() { "" } else { "\u{2009}" };
    let shown = format!("{}{}{}{}{}", &before[..pre], del, gap, ins, &before[before.len() - suf..]);
    let at = pre + del.len() + gap.len();
    let spans = vec![
        Span::new(pre, pre + del.len()).color(p.red).bg(p.red_tint, 1.5).underline(p.red),
        Span::new(at, at + ins.len()).color(p.green).bg(p.green_tint, 1.5),
    ];
    (shown, spans)
}

fn para(s: &State, b: &Block, side: usize, p: &Pal) -> El {
    let id = b.id;
    let ci = s.current;
    let ch = &s.chapters[ci];
    let first_of_pair = ch.sides[side].iter().find(|x| x.pair == b.pair).map(|x| x.id) == Some(id);
    let unfolded = s.unfolded.contains(&(ci, b.pair));
    let review = s.review.map(|(mi, ei)| (&s.marks[mi], ei));
    let diff = review.and_then(|(m, ei)| m.edits.iter().enumerate().find(|(_, e)| e.chapter == ci && e.block == id && e.st == St::Pending).map(|(i, e)| (i == ei, e)));
    let waiting = s.marks.iter().any(|m| m.edits.iter().any(|e| e.chapter == ci && e.block == id && e.st == St::Pending));
    let marked_here = s.marks.iter().any(|m| m.chapter == ci && m.block == id);
    let term_here = s.term_card.is_some_and(|(tid, _)| tid == id);
    let focus = diff.is_some_and(|(cur, _)| cur);

    // Gutter: one click target that unfolds this paragraph's source. A red dot marks edits.
    let gutter = col().key(format!("g{id}")).w(56.0).pt(7.0).items_end().pr(18.0).cursor(Cursor::Pointer).help("Show the source")
        .child(if waiting { dot(p.red, 6.0).mr(2.0) } else { icon(&I_LANG, 14.0).color(p.ink3).group_opacity(if unfolded { 1.0 } else { 0.0 }, 1.0) })
        .on_press(move |s: &mut State| {
            let Some((sd, i)) = find_block(s, ci, id) else { return };
            let k = (ci, s.chapters[ci].sides[sd][i].pair);
            if let Some(at) = s.unfolded.iter().position(|x| *x == k) { s.unfolded.remove(at); } else { s.unfolded.push(k); }
        });

    let style_body = if side == 0 { SRC_BODY } else { BODY };
    let body = if let Some((_, e)) = diff {
        let (shown, spans) = diff_text(&e.before, &e.after, p);
        rich(shown, spans).font(style_body).color(p.ink).wrap().key(format!("b{id}")).w_full()
    } else {
        let (shown, mut spans, style) = render(&b.edit.text, style_body);
        let hits = term_hits(s, &shown, side);
        let sel = s.selection.as_ref().filter(|(sid, _)| *sid == id).map(|(_, t)| t.clone());
        if let Some(t) = &sel { for st in find_all(&shown, t).into_iter().take(1) { spans.push(Span::new(st, st + t.len()).bg(p.highlight, 1.0)); } }
        let picked = s.term_card.map(|(_, ti)| ti);
        for &(a, z, ti) in &hits {
            let sp = Span::new(a, z).underline(if Some(ti) == picked { p.ink2 } else { p.ink4 });
            spans.push(if Some(ti) == picked { sp.bg(p.term_bg, 1.0) } else { sp });
        }
        let mut t = rich(shown, spans).font(style).color(p.ink).wrap().selectable(true).selection_color(p.highlight).key(format!("b{id}")).w_full();
        for (a, z, ti) in hits {
            let term = &s.terms[ti];
            t = t.link(Link::new(a..z, format!("{ti}")).hover(LinkPaint::new().bg(p.term_bg)).help(format!("{} · {}", if side == 1 { &term.og } else { &term.term }, term.def)));
        }
        t.on_link(move |s: &mut State, tid, _| {
            if let Ok(ti) = tid.parse() { s.term_card = Some((id, ti)); s.right_tab = RightTab::Terms; }
            s.selection = None;
        })
    };

    let mut text_col = col().w(640.0).gap(6.0);
    if focus { text_col = text_col.child(text(source_of(s, ci, id)).font(INTERLINEAR).color(p.ink3).wrap().pb(4.0)); }
    text_col = text_col.child(body);
    if side == 1 && first_of_pair && (s.interlinear || unfolded) && !focus {
        let src: Vec<&str> = ch.sides[0].iter().filter(|x| x.pair == b.pair).map(|x| x.edit.text.as_str()).collect();
        text_col = text_col.child(text(src.join("\n")).font(INTERLINEAR).color(p.ink3).wrap().selectable(true).key(format!("s{id}")));
    }

    let mut r = row().key(format!("row{id}")).w_full().pb(if side == 0 { 20.0 } else { 18.0 }).hover_group().child(gutter).child(text_col);
    // With the context sidebar closed, notes live in the page margin beside their paragraph.
    if !s.right_open && (marked_here || term_here) {
        let mut margin = col().w(260.0).pl(36.0).pt(4.0).gap(18.0).absolute(696.0, if focus { 18.0 } else { 4.0 });
        for (mi, m) in s.marks.iter().enumerate() { if m.chapter == ci && m.block == id { margin = margin.child(mark_card(s, mi, p, false)); } }
        if let Some((tid, ti)) = s.term_card { if tid == id { margin = margin.child(term_card(s, id, ti, p)); } }
        r = r.child(margin);
    }
    if focus { r = r.fill(p.focus_bg).pt(14.0).radius(6.0); }
    r
}

fn reading(s: &State, cx: &mut Cx<State>, p: &Pal) -> El {
    let ch = &s.chapters[s.current];
    let side = if s.lang == Lang::Source || ch.queued { 0 } else { 1 };
    let tops: Vec<(u32, f32)> = ch.sides[side].iter().filter_map(|b| cx.layout(&format!("row{}", b.id)).map(|r| (b.id, r[1]))).collect();
    let origin = cx.layout("page").map(|r| r[1]).unwrap_or(0.0);
    *s.tops.borrow_mut() = tops.into_iter().map(|(id, y)| (id, y - origin)).collect();

    let page_w = if s.right_open { 736.0 } else { 956.0 };
    let mut page = col().key("page").w(page_w).pt(56.0).pb(220.0)
        .child(col().pl(56.0).pb(36.0).gap(2.0)
            .child(text(if side == 0 && !ch.queued { "SOURCE · 한국어" } else { "CHAPTER" }).font(LABEL).color(p.red))
            .child(text(ch.num.to_string()).font(TextStyle::new(60.0, 300.0).lh(72.0).family(SERIF)).color(p.ink))
            .child(el().w(36.0).h(1.5).fill(p.ink).mt(14.0)));
    if ch.queued {
        let ci = s.current;
        let place = s.chapters[..=ci].iter().filter(|c| c.queued).count();
        page = page.child(row().pl(56.0).pb(34.0).child(
            row().w(640.0).padding(16.0).gap(14.0).center_y().radius(8.0).border(1.0, p.rule).fill(p.card)
                .child(dot(p.amber, 8.0))
                .child(col().grow(1.0).gap(2.0)
                    .child(text(format!("Waiting to translate · {} in queue", match place { 1 => "first".into(), 2 => "second".into(), n => format!("#{n}") })).font(UI_MED).color(p.ink))
                    .child(text("It translates on its own once Cipher is open with an Endpoint. The source is below.").font(SMALL).color(p.ink3)))
                .child(btn("Translate now", true, p).key("translate-now").on_press(move |s: &mut State| translate_now(s, ci)))));
    }
    let skip_heading = side == 1 && ch.sides[1].first().is_some_and(|b| b.edit.text.starts_with("# "));
    let mut body = col().w_full().selection_group(true).on_selection_change(|s: &mut State, change, _| {
        let picked = change.selection().and_then(|sel| {
            let key = sel.texts().iter().find(|t| t.is_selected()).and_then(|t| t.key()).map(str::to_string)?;
            let id: u32 = key.strip_prefix('b')?.parse().ok()?;
            let txt = sel.text().trim().to_string();
            (!txt.is_empty() && txt.len() < 200).then_some((id, txt))
        });
        match picked {
            Some((id, txt)) => select(s, id, txt),
            None if !s.composing => s.selection = None,
            None => {}
        }
    });
    for (i, b) in ch.sides[side].iter().enumerate() {
        if i == 0 && skip_heading { continue; }
        body = body.child(para(s, b, side, p));
    }
    page = page.child(body);

    let ci = s.current;
    if let Some(next) = s.chapters.get(ci + 1) {
        page = page.child(row().key("next").ml(56.0).w(640.0).mt(28.0).pt(18.0).gap(8.0).center_y().border_sides([1.0, 0.0, 0.0, 0.0], p.rule).cursor(Cursor::Pointer)
            .child(text(format!("Chapter {}", next.num)).font(TextStyle::new(16.0, 400.0).family(SERIF)).color(p.ink2))
            .child(icon(&I_ARROW, 14.0).color(p.ink3)).child(el().grow(1.0))
            .child(text(if next.queued { "waiting to translate" } else { "" }).font(SMALL).color(p.ink3))
            .on_press_tx(move |s: &mut State, tx| { go(s, tx, ci + 1); tx.scroll_key_to(format!("ch{}", ci + 1), 200.0); }));
    } else {
        page = page.child(text("End of what you have. Copy the next raw chapter and press Ctrl+V to add it.").font(SMALL).color(p.ink3).ml(56.0).mt(28.0));
    }
    let y = s.scroll_y.clone();
    col().key("doc").w_full().grow(1.0).min_h(0.0).items_center().scroll(ScrollSpec { stick_bottom: false })
        .on_scroll_offset(move |at| y.set(at.offset[1])).child(page)
}

/// Cast & terms: the Glossary as a page the book fills in itself.
fn cast(s: &State, p: &Pal) -> El {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut order: Vec<usize> = (0..s.terms.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(s.terms[i].drift.len().min(1)), std::cmp::Reverse(s.terms[i].counts.iter().sum::<u32>())));
    let drifting = s.terms.iter().filter(|t| !t.drift.is_empty()).count();
    let mut list = col().w(if s.right_open { 736.0 } else { 956.0 }).pt(48.0).pb(160.0).px(56.0)
        .child(text("CAST & TERMS").font(LABEL).color(p.red))
        .child(text(format!("{} terms", s.terms.len())).font(TextStyle::new(34.0, 300.0).family(SERIF)).color(p.ink).pt(4.0))
        .child(text(format!("{drifting} drifting: the source names them more often than the translation renders them. Counted locally, no tokens.")).font(SMALL).color(p.ink3).pt(6.0).pb(26.0).wrap());
    for i in order {
        let t = &s.terms[i];
        let max = (*t.counts.iter().max().unwrap_or(&0)).max(1);
        let spark: String = t.counts.iter().map(|&n| if n == 0 { ' ' } else { BARS[((n * 7) / max) as usize] }).collect();
        let total: u32 = t.counts.iter().sum();
        let mut right = row().w(110.0).justify_end();
        if let Some(&first) = t.drift.first() {
            right = right.child(text(format!("drift · {} ch", t.drift.len())).font(SMALL).color(p.red).px(8.0).py(2.0).radius(10.0).fill(p.red_tint)
                .cursor(Cursor::Pointer).key(format!("drift{i}")).help(format!("First at chapter {}", s.chapters[first].num))
                .on_press_tx(move |s: &mut State, tx| { go(s, tx, first); tx.scroll_key_to(format!("ch{first}"), 200.0); }));
        }
        list = list.child(row().key(format!("term{i}")).w_full().py(9.0).gap(16.0).center_y().border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
            .child(col().w(200.0).gap(1.0)
                .child(text(t.term.clone()).font(TextStyle::new(16.0, 500.0).family(SERIF)).color(p.ink).truncate())
                .child(text(if t.forbidden.is_empty() { t.og.clone() } else { format!("{} · never {}", t.og, t.forbidden.join(", ")) })
                    .font(TextStyle::new(12.5, 400.0).family(CJK)).color(p.ink3).truncate()))
            .child(text(total.to_string()).font(SMALL.features("tnum")).color(p.ink2).w(36.0))
            .child(text(spark).font(SPARK).color(if t.drift.is_empty() { p.ink3 } else { p.red_soft }).grow(1.0).min_w(0.0).truncate())
            .child(right));
    }
    col().key("cast-doc").w_full().grow(1.0).min_h(0.0).items_center().scroll(ScrollSpec { stick_bottom: false }).child(list)
}

fn header(s: &State, p: &Pal) -> El {
    let ch = &s.chapters[s.current];
    let crumb = if s.page == Page::Cast { "Cast & terms".to_string() } else { format!("Chapter {}", ch.num) };
    let state = if s.page == Page::Cast { String::new() } else if ch.queued { "waiting to translate".into() }
        else if s.lang == Lang::Source { "source".into() } else { format!("{} paragraphs", ch.sides[1].len()) };
    let mut left = row().gap(10.0).center_y();
    if !s.left_open { left = left.child(icon_btn(&I_LEFT_PANEL, "show-left", false, p).help("Show contents (Ctrl+\\)").on_press(|s: &mut State| s.left_open = true)); }
    left = left.child(text(crumb).font(UI_MED).color(p.ink)).child(text(state).font(SMALL).color(p.ink3));
    let toggle = |label: &str, on: bool, key: &str| text(label.to_string()).font(UI).color(if on { p.ink } else { p.ink3 }).px(8.0).py(4.0).radius(6.0)
        .fill(if on { p.selected } else { Color::TRANSPARENT }).hover(p.hover).cursor(Cursor::Pointer).key(key.to_string());
    let mut right = row().gap(4.0).center_y()
        .child(toggle("Interlinear", s.interlinear, "h-inter").help("Source under every paragraph (Ctrl+I)").on_press(|s: &mut State| s.interlinear = !s.interlinear))
        .child(toggle("Source", s.lang == Lang::Source, "h-source").help("Read the source (Ctrl+T)").on_press_tx(toggle_lang))
        .child(el().w(1.0).h(16.0).fill(p.rule).mx(4.0))
        .child(icon_btn(&I_UPLOAD, "h-publish", false, p).help("Publish to Reader").on_press(|s: &mut State| s.toast = Some("Publish is not wired in this prototype.".into())))
        .child(icon_btn(if s.dark { &I_SUN } else { &I_MOON }, "h-theme", false, p).help("Dark / light (Ctrl+D)").on_press(|s: &mut State| s.dark = !s.dark))
        .child(row().key("h-k").pl(4.0).cursor(Cursor::Pointer).child(kbd("Ctrl K", p)).on_press(open_palette));
    if !s.right_open { right = right.child(icon_btn(&I_RIGHT_PANEL, "show-right", false, p).help("Show context (Ctrl+J)").on_press(|s: &mut State| s.right_open = true)); }
    row().h(HEADER_H).w_full().px(16.0).center_y().fill(p.paper).border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
        .child(left).child(el().grow(1.0)).child(right)
}

// ---------- view: root and overlays ----------

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let p = pal(s.dark);
    let content = if s.page == Page::Cast { cast(s, &p) } else { reading(s, cx, &p) };
    let mut middle = row().w_full().grow(1.0).min_h(0.0);
    if !s.left_open { middle = middle.child(scrubber(s, &p)); }
    middle = middle.child(col().grow(1.0).h_full().min_w(0.0).child(content));
    let main = col().grow(1.0).h_full().min_w(0.0).fill(p.paper).child(header(s, &p)).child(middle);

    let mut shell = row().w_full().h_full();
    if s.left_open { shell = shell.child(contents(s, &p)); }
    shell = shell.child(main);
    if s.right_open { shell = shell.child(context(s, &p)); }

    let mut root = col().w_full().h_full().fill(p.paper)
        .on_paste(|s: &mut State, txt: &str, _| {
            if txt.trim().len() < 20 { return; }
            let num = detect_num(txt);
            let dup = num.is_some_and(|n| s.chapters.iter().any(|c| c.num == n));
            s.intake = Some(Intake { text: txt.to_string(), num, dup });
        })
        .child(shell);

    // Selection: a small chip first, so selecting to copy is never blocked. Typing opens it.
    if let Some((id, quote)) = &s.selection {
        let key = format!("b{id}");
        if !s.composing {
            root = root.child(row().key("fix-chip").gap(8.0).px(10.0).h(30.0).center_y().radius(15.0).fill(p.bar).shadow(6.0, 16.0, p.shadow).cursor(Cursor::Pointer)
                .anchored(key, Side::Bottom, Align::Start, 8.0)
                .child(text("Suggest a fix").font(UI_MED).color(p.bar_fg))
                .child(text("or just start typing").font(SMALL).color(p.bar_fg2))
                .on_press(|s: &mut State| open_composer(s, "")));
        } else {
            let term = s.terms.iter().find(|t| &t.term == quote);
            let src = match term { Some(t) => format!("{} · glossary term", t.og), None => source_of(s, s.current, *id) };
            let (blocks_n, chapters_n) = s.reach;
            let labels = [
                "Just here".to_string(),
                format!("Everywhere · {blocks_n} in {chapters_n} ch"),
                if term.is_some() { "Everywhere + glossary".to_string() } else { "From now on".to_string() },
            ];
            let mut scopes = row().gap(6.0).center_y()
                .child(row().key("rename").gap(5.0).center_y().px(8.0).py(3.0).radius(12.0).cursor(Cursor::Pointer).hover(p.hover)
                    .border(1.0, if s.rename { p.ink2 } else { p.rule })
                    .child(el().size(10.0, 10.0).radius(2.0).border(1.0, p.ink2).fill(if s.rename { p.ink } else { Color::TRANSPARENT }))
                    .child(text("Rename").font(SMALL).color(if s.rename { p.ink } else { p.ink3 }))
                    .help("Exact replace, no model call")
                    .on_press(|s: &mut State| s.rename = !s.rename))
                .child(el().w(1.0).h(16.0).fill(p.rule).mx(2.0));
            for (i, l) in labels.iter().enumerate() {
                scopes = scopes.child(chip(l, s.scope == i, &p).key(format!("scope{i}")).on_press(move |s: &mut State| s.scope = i));
            }
            root = root.child(card(&p).key("composer").w(640.0).padding(14.0).gap(10.0)
                .anchored(key, Side::Bottom, Align::Start, 10.0)
                .child(row().w_full().gap(10.0).center_y()
                    .child(text(short(quote)).font(TextStyle::new(14.0, 400.0).family(SERIF)).color(p.ink).px(6.0).py(2.0).radius(3.0).fill(p.highlight))
                    .child(icon(&I_ARROW, 14.0).color(p.ink3))
                    .child(text_input(&s.composer, TextStyle::new(14.0, 400.0)).key("composer-input").grow(1.0).min_w(0.0).single_line()
                        .color(p.ink).caret(p.red, p.highlight).placeholder("What should it say? Or say what's wrong.", p.ink3)
                        .on_edit(|s: &mut State| &mut s.composer)
                        .on_input_key(|s: &mut State, k, _| match &k.key {
                            Key::Enter => { submit(s); true }
                            Key::Tab => { s.scope = (s.scope + 1) % 3; true }
                            Key::Escape => { s.composing = false; true }
                            _ => false,
                        })))
                .child(text(src).font(INTERLINEAR).color(p.ink3).wrap().max_lines(3).pl(10.0).border_sides([0.0, 0.0, 0.0, 2.0], p.rule))
                .child(row().w_full().center_y().child(scopes).child(el().grow(1.0))
                    .child(row().gap(4.0).center_y().child(kbd("Tab", &p)).child(kbd("↵", &p)))));
        }
    }

    // Review: one bar, keyboard first. The diff sits in the text itself.
    let (inset_l, inset_r) = (if s.left_open { LEFT_W } else { 44.0 }, if s.right_open { RIGHT_W } else { 0.0 });
    if let Some((mi, ei)) = s.review {
        let m = &s.marks[mi];
        let e = &m.edits[ei];
        let done = m.edits.len() - count(m, St::Pending);
        let nav = |i: &'static Icon, k: &str| el().key(k.to_string()).size(26.0, 26.0).center().radius(5.0).hover(p.bar_hover).cursor(Cursor::Pointer).child(icon(i, 15.0).color(p.bar_fg));
        let dbtn = |l: &str, k: &str, key: &str, primary: bool| row().key(key.to_string()).h(28.0).px(10.0).gap(6.0).center_y().radius(6.0).cursor(Cursor::Pointer)
            .fill(if primary { p.red } else { Color::TRANSPARENT }).hover(if primary { p.red_soft } else { p.bar_hover })
            .child(text(l.to_string()).font(UI_MED).color(if primary { p.on_red } else { p.bar_fg }))
            .child(text(k.to_string()).font(SMALL).color(if primary { p.on_red } else { p.bar_fg2 }).opacity(if primary { 0.7 } else { 1.0 }));
        let bar = row().gap(10.0).h(48.0).px(14.0).center_y().radius(12.0).fill(p.bar).border(1.0, p.rule).shadow(10.0, 30.0, p.shadow)
            .child(col().gap(1.0).pr(6.0)
                .child(text(format!("“{}” → “{}”", short(&m.quote), short(&m.said))).font(UI_MED).color(p.bar_fg).max_lines(1))
                .child(text(format!("{} of {} · Chapter {}", done + 1, m.edits.len(), s.chapters[e.chapter].num)).font(SMALL).color(p.bar_fg2)))
            .child(nav(&I_UP, "r-prev").on_press_tx(move |s: &mut State, tx| if let Some(n) = next_pending(&s.marks[mi], ei, -1) { show_edit(s, tx, mi, n) }))
            .child(nav(&I_DOWN, "r-next").on_press_tx(move |s: &mut State, tx| if let Some(n) = next_pending(&s.marks[mi], ei, 1) { show_edit(s, tx, mi, n) }))
            .child(el().w(1.0).h(22.0).fill(p.bar_hover))
            .child(dbtn("Skip", "X", "r-skip", false).on_press_tx(|s: &mut State, tx| step(s, tx, Some(false), false)))
            .child(dbtn("Accept rest", "⇧A", "r-all", false).on_press_tx(|s: &mut State, tx| step(s, tx, None, true)))
            .child(dbtn("Accept", "A", "r-accept", true).on_press_tx(|s: &mut State, tx| step(s, tx, Some(true), false)))
            .child(el().key("r-close").size(26.0, 26.0).center().radius(5.0).hover(p.bar_hover).cursor(Cursor::Pointer).child(icon(&I_X, 14.0).color(p.bar_fg2))
                .on_press_tx(|s: &mut State, tx| end_review(s, tx)));
        root = root.child(row().pin(Some(inset_l), None, Some(inset_r), Some(26.0)).justify_center().child(bar));
    }

    if let Some(t) = &s.toast {
        root = root.child(row().pin(Some(inset_l), None, Some(inset_r), Some(if s.review.is_some() { 86.0 } else { 26.0 })).justify_center().child(
            row().key("toast").gap(10.0).px(14.0).h(36.0).center_y().radius(18.0).fill(p.card).border(1.0, p.rule).shadow(6.0, 18.0, p.shadow)
                .child(text(t.clone()).font(UI).color(p.ink2))
                .child(el().size(18.0, 18.0).center().cursor(Cursor::Pointer).child(icon(&I_X, 12.0).color(p.ink3)).on_press(|s: &mut State| s.toast = None))));
    }

    // Intake: what was pasted, where it goes, and whether it's already here.
    if let Some(it) = &s.intake {
        let after = it.num.and_then(|n| s.chapters.iter().filter(|c| c.num < n).last()).or(s.chapters.last()).map(|c| c.num);
        let preview: String = it.text.lines().filter(|l| !l.trim().is_empty()).take(3).collect::<Vec<_>>().join("\n");
        let paras = split_blocks(&it.text).len();
        let title = match it.num { Some(n) => format!("Chapter {n}"), None => "A new chapter".into() };
        let mut sheet = card(&p).key("intake").w(520.0).padding(22.0).gap(12.0).modal(true, |s: &mut State| s.intake = None)
            .child(text("ADD FROM CLIPBOARD").font(LABEL).color(p.red))
            .child(text(title).font(TextStyle::new(26.0, 400.0).family(SERIF)).color(p.ink))
            .child(text(format!("{paras} paragraphs · goes after Chapter {}{}", after.unwrap_or(0), if it.num.is_none() { " · no chapter number found" } else { "" })).font(SMALL).color(p.ink3))
            .child(text(preview).font(INTERLINEAR).color(p.ink2).wrap().max_lines(3).padding(12.0).radius(6.0).fill(p.paper));
        if it.dup {
            sheet = sheet.child(text("You already have this chapter. Adding it again keeps both.").font(SMALL).color(p.red));
        }
        sheet = sheet.child(row().w_full().gap(8.0).pt(6.0).justify_end()
            .child(btn("Cancel", false, &p).key("intake-cancel").on_press(|s: &mut State| s.intake = None))
            .child(btn("Add and translate", true, &p).key("intake-add").on_press_tx(add_chapter)));
        root = root.child(col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(p.scrim).items_center().pt(120.0).child(sheet));
    }

    if let Some(q) = &s.palette {
        let hits = palette_hits(s, &q.text);
        let mut list = col().w_full().padding(6.0).gap(1.0);
        for (i, h) in hits.iter().enumerate() {
            let (a, b) = match h {
                Hit::Chapter(ci) => (format!("Chapter {}", s.chapters[*ci].num), short(&opening_line(&s.chapters[*ci]))),
                Hit::Term(ti) => (s.terms[*ti].term.clone(), format!("{} · term", s.terms[*ti].og)),
                Hit::Act(n, sub) => (n.to_string(), sub.clone()),
            };
            list = list.child(row().key(format!("hit{i}")).w_full().h(38.0).px(12.0).gap(12.0).center_y().radius(6.0).hover(p.hover).cursor(Cursor::Pointer)
                .fill(if i == 0 { p.hover } else { Color::TRANSPARENT })
                .child(text(a).font(UI_MED).color(p.ink)).child(text(b).font(SMALL).color(p.ink3).grow(1.0).truncate())
                .on_press_tx(move |s: &mut State, tx| { let q = s.palette.as_ref().map(|q| q.text.clone()).unwrap_or_default(); if let Some(h) = palette_hits(s, &q).into_iter().nth(i) { run_hit(s, tx, &h) } }));
        }
        let pal_card = card(&p).key("palette").w(600.0).modal(true, |s: &mut State| s.palette = None)
            .child(text_input(q, TextStyle::new(16.0, 400.0)).key("palette-input").w_full().px(18.0).py(14.0).single_line().color(p.ink)
                .caret(p.red, p.highlight).placeholder("Chapter number, a term, or an action", p.ink3)
                .on_edit(|s: &mut State| s.palette.as_mut().unwrap())
                .on_input_key(|s: &mut State, k, tx| match &k.key {
                    Key::Enter => { let q = s.palette.as_ref().map(|q| q.text.clone()).unwrap_or_default(); if let Some(h) = palette_hits(s, &q).into_iter().next() { run_hit(s, tx, &h) } true }
                    Key::Escape => { s.palette = None; true }
                    _ => false,
                }))
            .child(el().w_full().h(1.0).fill(p.rule))
            .child(list);
        root = root.child(col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(p.scrim).items_center().pt(110.0).child(pal_card));
    }
    root
}

fn first_with(s: &State, needle: &str) -> u32 {
    s.chapters[0].sides[1].iter().find(|b| b.edit.text.contains(needle)).map(|b| b.id).unwrap()
}

fn seed_marks(s: &mut State) {
    let id = first_with(s, "Holy Scripture");
    select(s, id, "Holy Scripture".into()); s.scope = 1; open_composer(s, "Divine Scripture"); submit(s);
    let id = first_with(s, "Magic Corps");
    select(s, id, "Magic Corps".into()); s.scope = 0; open_composer(s, "Mage Corps"); submit(s);
}

fn main() {
    rune::run(AppSpec {
        on_key: Some(on_key),
        scenes: &[
            ("chip", |s| { let id = first_with(s, "Magic Tower Lord"); select(s, id, "Magic Tower Lord".into()); }),
            ("compose", |s| { let id = first_with(s, "Magic Tower Lord"); select(s, id, "Magic Tower Lord".into()); open_composer(s, "Tower Master"); }),
            ("review", |s| { seed_marks(s); s.review = Some((0, 0)); }),
            ("requests", seed_marks),
            ("term", |s| { let ti = s.terms.iter().position(|t| t.term == "Magic Tower Lord").unwrap(); show_term(s, None, ti); }),
            ("focus", |s| { s.left_open = false; s.right_open = false; seed_marks(s); }),
            ("interlinear", |s| s.interlinear = true),
            ("source", |s| s.lang = Lang::Source),
            ("cast", |s| s.page = Page::Cast),
            ("queued", |s| s.current = s.chapters.len() - 1),
            ("palette", |s| s.palette = Some(TextEdit::new("23"))),
            ("intake", |s| { let t = "\n\n321화\n\n그날 밤, 탑 위에서 바람이 불었다.\n\n테실리드는 조용히 고개를 들었다.\n\n\"이제 시작이군.\"".to_string(); s.intake = Some(Intake { num: detect_num(&t), dup: false, text: t }); }),
            ("dark", |s| { s.dark = true; let ti = s.terms.iter().position(|t| t.term == "Magic Tower Lord").unwrap(); show_term(s, None, ti); }),
            ("dark-review", |s| { s.dark = true; seed_marks(s); s.review = Some((0, 0)); }),
            ("dark-compose", |s| { s.dark = true; let id = first_with(s, "Magic Tower Lord"); select(s, id, "Magic Tower Lord".into()); open_composer(s, "Tower Master"); }),
            ("dark-cast", |s| { s.dark = true; s.page = Page::Cast; }),
        ],
        ..AppSpec::new("Cipher", (1480.0, 920.0), init, view)
    });
}
