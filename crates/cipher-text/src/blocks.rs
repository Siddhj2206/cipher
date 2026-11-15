//! Chapter markdown → Blocks, and a Block's inline markdown → plain text plus
//! styled spans.
//!
//! A **Block** is the smallest unit of text in a Chapter: a paragraph,
//! heading, quote or list item. Blocks are stored and numbered exactly as
//! [`split`] returns them, so changing the splitting rules changes Pair
//! numbering; treat the rules below as part of the storage format.

use std::ops::Range;

/// Split Chapter markdown into Blocks.
///
/// Rules:
/// - Blocks are separated by blank lines. Each Block is trimmed; empty ones are dropped.
/// - A line inside a paragraph that is a heading (`# Title`), a scene break
///   (`***`, `---`, `___`, `* * *`, `◇◇◇`, ...) or a list item (`- x`, `* x`,
///   `1. x`) always starts its own Block, even without blank lines around it.
///   A heading or a scene break also ends its Block on the same line.
/// - Other lines of a paragraph stay together in one Block, joined with `\n`.
///   Real source files glue several lines (for example system messages) into
///   one paragraph, and the Block keeps them.
/// - `\r\n` is treated as `\n`.
/// - Dialogue dashes such as `-……` (no space after the dash) are plain text,
///   not list items.
///
/// ```
/// use cipher_text::blocks::split;
/// let blocks = split("# Chapter 1\n\nHe ran.\n\n***\n\nShe stayed.\n");
/// assert_eq!(blocks, ["# Chapter 1", "He ran.", "***", "She stayed."]);
/// ```
pub fn split(chapter: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in chapter.lines() {
        let t = line.trim();
        if t.is_empty() {
            flush(&mut current, &mut out);
        } else if is_scene_break(t) || heading_level(t).is_some() {
            flush(&mut current, &mut out);
            out.push(t.to_string());
        } else if is_list_item(line) {
            flush(&mut current, &mut out);
            current.push(t);
        } else {
            current.push(t);
        }
    }
    flush(&mut current, &mut out);
    out
}

fn flush(current: &mut Vec<&str>, out: &mut Vec<String>) {
    if !current.is_empty() {
        out.push(current.join("\n"));
        current.clear();
    }
}

/// What kind of Block a string is, judged from its first line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Ordinary text.
    Paragraph,
    /// `#`..`######` heading; the payload is the level 1–6.
    Heading(u8),
    /// A scene break line such as `***`.
    SceneBreak,
    /// A `>` quote.
    Quote,
    /// A list item (`- x`, `* x`, `+ x`, `1. x`).
    ListItem,
}

/// Classify a Block. A multi-line Block is classified by its first line.
pub fn kind(block: &str) -> BlockKind {
    let first = block.lines().next().unwrap_or("").trim();
    if is_scene_break(first) {
        BlockKind::SceneBreak
    } else if let Some(level) = heading_level(first) {
        BlockKind::Heading(level)
    } else if first.starts_with('>') {
        BlockKind::Quote
    } else if is_list_item(first) {
        BlockKind::ListItem
    } else {
        BlockKind::Paragraph
    }
}

fn is_scene_break(t: &str) -> bool {
    const BREAK_CHARS: &[char] = &['*', '-', '_', '=', '~', '◇', '◆', '※', '＊', '·', '•'];
    let mut chars = t.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else {
        return false;
    };
    if !BREAK_CHARS.contains(&first) {
        return false;
    }
    let mut n = 1;
    for c in chars {
        if c != first {
            return false;
        }
        n += 1;
    }
    n >= 3
}

fn heading_level(t: &str) -> Option<u8> {
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &t[hashes..];
    (rest.is_empty() || rest.starts_with(' ')).then_some(hashes as u8)
}

fn is_list_item(line: &str) -> bool {
    let indent = line.len() - line.trim_start().len();
    if indent > 3 {
        return false;
    }
    let t = line.trim_start();
    if is_scene_break(t.trim_end()) {
        return false;
    }
    let mut chars = t.chars();
    match chars.next() {
        Some('-' | '*' | '+') => chars.next() == Some(' ') && !chars.as_str().trim().is_empty(),
        Some(d) if d.is_ascii_digit() => {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            let rest = &t[digits..];
            digits <= 9
                && (rest.starts_with(". ") || rest.starts_with(") "))
                && !rest[2..].trim().is_empty()
        }
        _ => false,
    }
}

/// Emphasis level of a [`StyledSpan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// `*x*` or `_x_`.
    Emphasis,
    /// `**x**`.
    Strong,
}

/// A styled part of [`Rendered::text`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    /// Byte range in [`Rendered::text`] (never in the markdown source).
    pub range: Range<usize>,
    /// The style.
    pub style: Style,
}

/// A Block rendered to display text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    /// The text with inline markers removed.
    pub text: String,
    /// Styled spans over `text`, sorted by start, outer spans before inner
    /// ones that start at the same place. `***x***` yields two spans on the
    /// same range (Emphasis outside, Strong inside).
    pub spans: Vec<StyledSpan>,
}

/// Render a Block's inline markdown to text plus spans.
///
/// Supported: `*em*`, `_em_`, `**strong**`, `***both***`, and `\*` escapes.
/// Emphasis follows CommonMark's flanking rules, so `a * b * c`, `snake_case`,
/// the scene break `***` and a lone `*` stay literal, as do markers with no
/// partner (`*open`, `x**`). Nothing else is interpreted inline.
///
/// Block-level syntax: a heading's leading `#`s and a quote's leading `> ` are
/// stripped (use [`kind`] to learn which it was). List markers are kept,
/// because `43. Heretic Inquisitor` is text as much as syntax. Newlines inside
/// the Block are kept.
///
/// ```
/// use cipher_text::blocks::{render, Style};
/// let r = render("She said *no* to **him**.");
/// assert_eq!(r.text, "She said no to him.");
/// assert_eq!(&r.text[r.spans[0].range.clone()], "no");
/// assert_eq!(r.spans[1].style, Style::Strong);
/// ```
pub fn render(block: &str) -> Rendered {
    let stripped = strip_block_prefix(block);
    inline(&stripped)
}

fn strip_block_prefix(block: &str) -> String {
    match kind(block) {
        BlockKind::Heading(level) => {
            let mut lines = block.lines();
            let first = lines.next().unwrap_or("").trim();
            let mut out = first[level as usize..].trim().to_string();
            for l in lines {
                out.push('\n');
                out.push_str(l);
            }
            out
        }
        BlockKind::Quote => block
            .lines()
            .map(|l| {
                let t = l.trim_start();
                t.strip_prefix('>')
                    .map_or(t, |r| r.strip_prefix(' ').unwrap_or(r))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => block.to_string(),
    }
}

#[derive(Debug)]
enum Tok {
    Text(String),
    Delim(Delim),
}

#[derive(Debug)]
struct Delim {
    ch: char,
    count: usize,
    original: usize,
    can_open: bool,
    can_close: bool,
    active: bool,
}

fn is_punct(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace()
}

fn tokenize(s: &str) -> Vec<Tok> {
    let chars: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() && matches!(chars[i + 1], '*' | '_' | '\\') {
            text.push(chars[i + 1]);
            i += 2;
        } else if c == '*' || c == '_' {
            let mut j = i;
            while j < chars.len() && chars[j] == c {
                j += 1;
            }
            let prev = if i == 0 { None } else { Some(chars[i - 1]) };
            let next = chars.get(j).copied();
            let prev_ws = prev.is_none_or(char::is_whitespace);
            let next_ws = next.is_none_or(char::is_whitespace);
            let prev_p = prev.is_some_and(is_punct);
            let next_p = next.is_some_and(is_punct);
            let left = !next_ws && (!next_p || prev_ws || prev_p);
            let right = !prev_ws && (!prev_p || next_ws || next_p);
            let (can_open, can_close) = if c == '*' {
                (left, right)
            } else {
                (left && (!right || prev_p), right && (!left || next_p))
            };
            if !text.is_empty() {
                toks.push(Tok::Text(std::mem::take(&mut text)));
            }
            toks.push(Tok::Delim(Delim {
                ch: c,
                count: j - i,
                original: j - i,
                can_open,
                can_close,
                active: true,
            }));
            i = j;
        } else {
            text.push(c);
            i += 1;
        }
    }
    if !text.is_empty() {
        toks.push(Tok::Text(text));
    }
    toks
}

fn delim(toks: &[Tok], i: usize) -> &Delim {
    match &toks[i] {
        Tok::Delim(d) => d,
        Tok::Text(_) => unreachable!("index refers to a delimiter"),
    }
}

fn delim_mut(toks: &mut [Tok], i: usize) -> &mut Delim {
    match &mut toks[i] {
        Tok::Delim(d) => d,
        Tok::Text(_) => unreachable!("index refers to a delimiter"),
    }
}

fn inline(s: &str) -> Rendered {
    let mut toks = tokenize(s);
    // (opener token, closer token, style)
    let mut matches: Vec<(usize, usize, Style)> = Vec::new();

    for ci in 0..toks.len() {
        loop {
            let (closer_ch, closer_orig, closer_can_open) = match &toks[ci] {
                Tok::Delim(d) if d.can_close && d.count > 0 && d.active => {
                    (d.ch, d.original, d.can_open)
                }
                _ => break,
            };
            let mut found = None;
            for oi in (0..ci).rev() {
                let Tok::Delim(o) = &toks[oi] else { continue };
                if o.ch != closer_ch || !o.can_open || o.count == 0 || !o.active {
                    continue;
                }
                // CommonMark "rule of 3".
                if (o.can_close || closer_can_open)
                    && (o.original + closer_orig) % 3 == 0
                    && !(o.original % 3 == 0 && closer_orig % 3 == 0)
                {
                    continue;
                }
                found = Some(oi);
                break;
            }
            let Some(oi) = found else { break };
            let use_n = if delim(&toks, oi).count >= 2 && delim(&toks, ci).count >= 2 {
                2
            } else {
                1
            };
            let style = if use_n == 2 {
                Style::Strong
            } else {
                Style::Emphasis
            };
            {
                let o = delim_mut(&mut toks, oi);
                o.count -= use_n;
            }
            {
                let c = delim_mut(&mut toks, ci);
                c.count -= use_n;
            }
            // Delimiters strictly between the pair can no longer match.
            for t in &mut toks[oi + 1..ci] {
                if let Tok::Delim(d) = t {
                    d.active = false;
                }
            }
            matches.push((oi, ci, style));
        }
    }

    // Emit text, remembering where each delimiter run starts and where its
    // literal characters end.
    let mut text = String::new();
    let mut run_start = vec![0usize; toks.len()];
    let mut literal_end = vec![0usize; toks.len()];
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Text(s) => text.push_str(s),
            Tok::Delim(d) => {
                run_start[i] = text.len();
                for _ in 0..d.count {
                    text.push(d.ch);
                }
                literal_end[i] = text.len();
            }
        }
    }
    let mut spans: Vec<StyledSpan> = matches
        .into_iter()
        .map(|(o, c, style)| StyledSpan {
            range: literal_end[o]..run_start[c],
            style,
        })
        .filter(|s| s.range.start < s.range.end)
        .collect();
    // Outer first: by start, then longer first, then emphasis outside strong.
    spans.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(b.range.end.cmp(&a.range.end))
            .then((a.style == Style::Strong).cmp(&(b.style == Style::Strong)))
    });
    Rendered { text, spans }
}
