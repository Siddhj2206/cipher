//! The marker protocol: numbered Blocks out, numbered Pairs back.
//!
//! The source goes to the model as ASCII-numbered lines and the translation
//! comes back the same way (see `docs/design/translation-pipeline.md`):
//!
//! ```text
//! [12] 세 명의 강자를 보유하고 있던 ...
//!
//! [12] The Ragnaype Republic of Magic, which once ...
//! [13-14] (two source Blocks rendered as one)
//! ```
//!
//! `[n-m]` marks a **merge**: one translated entry for source Blocks `n..=m`.
//! There is no split marker: a **split** is a line break inside one entry.
//! The numbering is what makes Pairs reliable; paragraph counts are not.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

/// Write numbered Blocks as `[n] text` entries separated by blank lines.
///
/// Newlines inside a Block are flattened to a single space (a marker entry is
/// one line, so a model never sees a line break and guesses at a split).
/// A Block with no text is written as just `[n]`.
///
/// ```
/// use cipher_text::markers::write;
/// assert_eq!(write(&[(1, "a\nb"), (2, "c")]), "[1] a b\n\n[2] c");
/// ```
pub fn write(blocks: &[(u32, &str)]) -> String {
    blocks
        .iter()
        .map(|(n, text)| {
            let flat = text
                .split('\n')
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if flat.is_empty() {
                format!("[{n}]")
            } else {
                format!("[{n}] {flat}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// One entry of a response: the translation of source Blocks `first..=last`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPair {
    /// First source Block number (inclusive).
    pub first: u32,
    /// Last source Block number (inclusive). Equal to `first` unless this is a merge.
    pub last: u32,
    /// The translation. Several lines in the entry (a split) are joined with
    /// `"\n\n"`, one translated Block each. May be empty if the model wrote a
    /// marker with no text; the caller should treat that as a problem.
    pub text: String,
}

/// A Term the model proposed in the `NEW_TERMS:` trailer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTerm {
    /// Source form.
    pub source: String,
    /// Proposed rendering.
    pub rendering: String,
    /// Kind (`name`, `place`, ...) if given.
    pub kind: Option<String>,
    /// Gender if given.
    pub gender: Option<String>,
}

/// The result of [`parse`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    /// Accepted entries, sorted by `first`. An entry is accepted only if all
    /// its numbers are inside `expected` and none was seen before.
    pub pairs: Vec<ParsedPair>,
    /// Rows of the `NEW_TERMS:` trailer.
    pub new_terms: Vec<NewTerm>,
    /// The `NOTES:` trailer, trimmed, with lines joined by `\n`; `None` if absent or empty.
    pub notes: Option<String>,
    /// Expected numbers that no accepted entry covers, ascending. Includes the
    /// numbers of the dropped last entry of an incomplete response.
    pub missing: Vec<u32>,
    /// Numbers covered by more than one entry, ascending. The first entry
    /// wins; later entries that repeat a number are dropped whole.
    pub duplicates: Vec<u32>,
    /// Numbers outside `expected` that the response used, ascending. Entries
    /// that use them are dropped whole.
    pub out_of_range: Vec<u32>,
}

/// Longest `[n-m]` range accepted as an entry; anything wider is garbage.
const MAX_SPAN: u32 = 10_000;

/// Parse a model response leniently.
///
/// `expected` is the range of source Block numbers that were requested.
/// `complete` is `false` when the stream was cut off: then the last entry may
/// be unfinished, so it is **dropped** and its numbers are reported as
/// missing, unless a trailer header has already been seen (which proves the
/// body ended). In an unfinished trailer the last `NEW_TERMS` row is dropped
/// for the same reason, unless `NOTES:` has started.
///
/// Accepted marker forms at the start of a line (leading whitespace allowed):
/// `[12]`, `[ 12 ]`, `[12-14]`, `[12–14]` (en dash, em dash, `~`), `【12】`,
/// `［12］`, `**[12]**`, `[12]:`, and, when the number is inside `expected`,
/// `12.`, `12)` and `**12.**`. Fullwidth digits work. Text before the first
/// marker is ignored, as are code-fence lines.
///
/// Every non-empty line after a marker belongs to that entry; lines are
/// joined with `"\n\n"`.
///
/// The trailer starts at a line `NEW_TERMS:` or `NOTES:` (case-insensitive;
/// `New terms`, `**NOTES:**` and `## Notes:` work; `NOTES:` may carry text on
/// the same line), in either order. `NEW_TERMS` rows are
/// `source | rendering | kind | gender`; missing columns, extra columns,
/// bullets, outer pipes, header and separator rows, and `source -> rendering`
/// are tolerated. A row needs at least a source and a rendering.
///
/// ```
/// use cipher_text::markers::parse;
/// let p = parse("[1] Hello.\n\n[2-3] Merged.\n\nNOTES:\nA fight.", 1..=4, true);
/// assert_eq!(p.pairs.len(), 2);
/// assert_eq!(p.missing, [4]);
/// assert_eq!(p.notes.as_deref(), Some("A fight."));
/// ```
pub fn parse(response: &str, expected: RangeInclusive<u32>, complete: bool) -> Parsed {
    struct Entry {
        first: u32,
        last: u32,
        paras: Vec<String>,
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Section {
        Body,
        Terms,
        Notes,
    }

    let mut entries: Vec<Entry> = Vec::new();
    let mut section = Section::Body;
    let mut term_lines: Vec<&str> = Vec::new();
    let mut note_lines: Vec<String> = Vec::new();
    let mut saw_trailer = false;
    let mut saw_notes = false;

    for line in response.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            continue;
        }
        if let Some((is_notes, rest)) = trailer_header(t) {
            saw_trailer = true;
            if is_notes {
                saw_notes = true;
                section = Section::Notes;
                if !rest.is_empty() {
                    note_lines.push(rest.to_string());
                }
            } else {
                section = Section::Terms;
                if !rest.is_empty() {
                    term_lines.push(rest);
                }
            }
            continue;
        }
        match section {
            Section::Body => {
                if let Some((first, last, rest)) = parse_marker(t, &expected) {
                    let paras = if rest.is_empty() {
                        Vec::new()
                    } else {
                        vec![rest.to_string()]
                    };
                    entries.push(Entry { first, last, paras });
                } else if !t.is_empty()
                    && let Some(e) = entries.last_mut()
                {
                    e.paras.push(t.to_string());
                }
            }
            Section::Terms => term_lines.push(t),
            Section::Notes => {
                if !t.is_empty() {
                    note_lines.push(t.to_string());
                }
            }
        }
    }

    if !complete && !saw_trailer {
        entries.pop();
    }

    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut duplicates: BTreeSet<u32> = BTreeSet::new();
    let mut out_of_range: BTreeSet<u32> = BTreeSet::new();
    let mut pairs = Vec::new();
    for e in entries {
        let (a, b) = if e.first <= e.last {
            (e.first, e.last)
        } else {
            (e.last, e.first)
        };
        if b - a > MAX_SPAN {
            out_of_range.insert(a);
            continue;
        }
        let outside: Vec<u32> = (a..=b).filter(|n| !expected.contains(n)).collect();
        if !outside.is_empty() {
            out_of_range.extend(outside);
            continue;
        }
        let repeated: Vec<u32> = (a..=b).filter(|n| seen.contains(n)).collect();
        if !repeated.is_empty() {
            duplicates.extend(repeated);
            continue;
        }
        seen.extend(a..=b);
        pairs.push(ParsedPair {
            first: a,
            last: b,
            text: e.paras.join("\n\n"),
        });
    }
    pairs.sort_by_key(|p| p.first);

    let missing = expected.clone().filter(|n| !seen.contains(n)).collect();

    let mut new_terms: Vec<NewTerm> = term_lines
        .iter()
        .filter_map(|l| parse_term_row(l))
        .collect();
    if !complete && saw_trailer && !saw_notes {
        new_terms.pop();
    }
    let notes = note_lines.join("\n");
    let notes = (!notes.trim().is_empty()).then(|| notes.trim().to_string());

    Parsed {
        pairs,
        new_terms,
        notes,
        missing,
        duplicates: duplicates.into_iter().collect(),
        out_of_range: out_of_range.into_iter().collect(),
    }
}

fn is_wrapper(c: char) -> bool {
    matches!(c, '*' | '_' | '`')
}

/// A trailer header line. Returns (is_notes, text after the colon).
fn trailer_header(line: &str) -> Option<(bool, &str)> {
    let core = line.trim_start_matches(|c: char| is_wrapper(c) || c == '#' || c == ' ');
    let upper = core.to_ascii_uppercase();
    const TERMS: &[&str] = &["NEW_TERMS", "NEW TERMS", "NEW-TERMS", "NEWTERMS"];
    const NOTES: &[&str] = &["STORY NOTES", "NOTES", "NOTE"];
    let (is_notes, prefix_len) = TERMS
        .iter()
        .find(|p| upper.starts_with(**p))
        .map(|p| (false, p.len()))
        .or_else(|| {
            NOTES
                .iter()
                .find(|p| upper.starts_with(**p))
                .map(|p| (true, p.len()))
        })?;
    let rest = core[prefix_len..].trim_start_matches(is_wrapper);
    let rest = if let Some(r) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：')) {
        r.trim_start_matches(is_wrapper).trim()
    } else if rest.trim().is_empty() {
        ""
    } else {
        return None;
    };
    Some((is_notes, rest))
}

fn parse_digits(s: &str) -> Option<(u32, &str)> {
    let mut n: u32 = 0;
    let mut len = 0;
    for c in s.chars() {
        let d = match c {
            '0'..='9' => c as u32 - '0' as u32,
            '０'..='９' => c as u32 - '０' as u32,
            _ => break,
        };
        n = n.checked_mul(10)?.checked_add(d)?;
        len += c.len_utf8();
    }
    (len > 0).then(|| (n, &s[len..]))
}

/// `N` or `N-M` with optional spaces; returns (first, last, rest).
fn parse_number_or_range(s: &str) -> Option<(u32, u32, &str)> {
    let s = s.trim_start();
    let (a, rest) = parse_digits(s)?;
    let rest = rest.trim_start();
    if let Some(sep) = rest.chars().next().filter(|c| "-–—~〜－".contains(*c))
        && let Some((b, rest2)) = parse_digits(rest[sep.len_utf8()..].trim_start())
    {
        return Some((a, b, rest2.trim_start()));
    }
    Some((a, a, rest))
}

fn parse_bracketed(body: &str) -> Option<(u32, u32, &str)> {
    let open = body.chars().next().filter(|c| "[【［〔".contains(*c))?;
    let (a, b, rest) = parse_number_or_range(&body[open.len_utf8()..])?;
    let close = rest.chars().next().filter(|c| "]】］〕".contains(*c))?;
    Some((a, b, &rest[close.len_utf8()..]))
}

fn parse_dotted<'a>(body: &'a str, expected: &RangeInclusive<u32>) -> Option<(u32, u32, &'a str)> {
    let (a, b, rest) = parse_number_or_range(body)?;
    if !expected.contains(&a) || !expected.contains(&b) {
        return None;
    }
    let punct = rest
        .chars()
        .next()
        .filter(|c| matches!(c, '.' | ')' | '．'))?;
    let after = &rest[punct.len_utf8()..];
    match after.chars().next() {
        None => Some((a, b, after)),
        Some(c) if c.is_whitespace() || is_wrapper(c) => Some((a, b, after)),
        _ => None,
    }
}

fn parse_marker<'a>(line: &'a str, expected: &RangeInclusive<u32>) -> Option<(u32, u32, &'a str)> {
    let s = line.trim_start();
    let body = s.trim_start_matches(is_wrapper);
    let wrapped = body.len() != s.len();
    let (first, last, after) = parse_bracketed(body).or_else(|| parse_dotted(body, expected))?;
    let mut rest = after;
    if wrapped {
        rest = rest.trim_start_matches(is_wrapper);
    }
    let rest = rest.strip_prefix([':', '：']).unwrap_or(rest);
    Some((first, last, rest.trim()))
}

fn clean_cell(cell: &str) -> String {
    cell.trim()
        .trim_matches(|c: char| is_wrapper(c) || c.is_whitespace())
        .to_string()
}

fn optional(cell: Option<&String>) -> Option<String> {
    cell.filter(|c| {
        !c.is_empty()
            && !matches!(
                c.to_lowercase().as_str(),
                "-" | "–" | "—" | "n/a" | "na" | "none" | "null"
            )
    })
    .cloned()
}

fn parse_term_row(line: &str) -> Option<NewTerm> {
    let l = line.trim().trim_start_matches(['-', '*', '•', '·']).trim();
    if l.is_empty() {
        return None;
    }
    let mut cells: Vec<String> = if l.contains('|') {
        let mut parts: Vec<&str> = l.split('|').collect();
        if parts.first().is_some_and(|p| p.trim().is_empty()) {
            parts.remove(0);
        }
        if parts.last().is_some_and(|p| p.trim().is_empty()) {
            parts.pop();
        }
        parts.into_iter().map(clean_cell).collect()
    } else {
        let arrow = ["→", "->", "=>", "="]
            .iter()
            .find_map(|a| l.split_once(a))?;
        vec![clean_cell(arrow.0), clean_cell(arrow.1)]
    };
    if cells
        .iter()
        .all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
    {
        return None; // table separator row
    }
    if cells.first().is_some_and(|c| {
        let c = c.to_lowercase();
        c == "source" || c == "source form"
    }) {
        return None; // header row
    }
    if cells.len() < 2 || cells[0].is_empty() || cells[1].is_empty() {
        return None;
    }
    let gender = optional(cells.get(3));
    let kind = optional(cells.get(2));
    Some(NewTerm {
        source: std::mem::take(&mut cells[0]),
        rendering: std::mem::take(&mut cells[1]),
        kind,
        gender,
    })
}
