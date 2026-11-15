//! A Proposed Edit's word diff as text with spans: deleted words red and underlined, a thin
//! space, inserted words green.

use cipher_text::blocks;
use cipher_text::diff::{DiffOp, diff};
use rune::Span;
use rune::tok::TextStyle;
use rune::{El, rich};

use crate::theme::{Pal, SERIF};

pub struct Shown {
    pub text: String,
    pub spans: Vec<Span>,
    /// Byte range from the first change to the last.
    pub changes: Option<(usize, usize)>,
}

pub fn shown(before: &str, after: &str, p: &Pal) -> Shown {
    let (before, after) = (blocks::render(before).text, blocks::render(after).text);
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut first = None;
    let mut last = 0;
    let mut deleted = false;
    for op in diff(&before, &after) {
        match op {
            DiffOp::Equal(t) => {
                text.push_str(&t);
                deleted = false;
            }
            DiffOp::Delete(t) => {
                let at = text.len();
                text.push_str(&t);
                spans.push(
                    Span::new(at, text.len())
                        .color(p.red)
                        .bg(p.red_tint, 1.5)
                        .underline(p.red),
                );
                first.get_or_insert(at);
                last = text.len();
                deleted = true;
            }
            DiffOp::Insert(t) => {
                // A thin space keeps the struck-out and inserted words from reading as one.
                if deleted {
                    text.push('\u{2009}');
                }
                let at = text.len();
                text.push_str(&t);
                spans.push(
                    Span::new(at, text.len())
                        .color(p.green)
                        .bg(p.green_tint, 1.5),
                );
                first.get_or_insert(at);
                last = text.len();
                deleted = false;
            }
        }
    }
    Shown {
        changes: first.map(|f| (f, last)),
        text,
        spans,
    }
}

/// The change with about 40 characters either side, for a note.
pub fn sample(before: &str, after: &str, p: &Pal) -> El {
    let Shown {
        text,
        spans,
        changes,
    } = shown(before, after, p);
    let (a, z) = changes.unwrap_or((0, text.len()));
    let lo = text[..a].char_indices().rev().nth(40).map_or(0, |(i, _)| i);
    let hi = text[z..]
        .char_indices()
        .nth(40)
        .map_or(text.len(), |(i, _)| z + i);
    let lead = if lo > 0 { "…" } else { "" };
    let trail = if hi < text.len() { "…" } else { "" };
    let off = lo as i64 - lead.len() as i64;
    let cut = format!("{lead}{}{trail}", &text[lo..hi]);
    let spans: Vec<Span> = spans
        .into_iter()
        .filter(|sp| sp.start as usize >= lo && sp.end as usize <= hi)
        .map(|mut sp| {
            sp.start = (sp.start as i64 - off) as u32;
            sp.end = (sp.end as i64 - off) as u32;
            sp
        })
        .collect();
    rich(cut, spans)
        .font(TextStyle::new(13.0, 400.0).lh(19.0).family(SERIF))
        .color(p.ink2)
        .wrap()
        .max_lines(2)
}
