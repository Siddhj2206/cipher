//! Word-level diff of two strings, for Review display.

/// One step of a [`diff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOp {
    /// Text present in both.
    Equal(String),
    /// Text only in `before`.
    Delete(String),
    /// Text only in `after`.
    Insert(String),
}

/// Word-level diff.
///
/// - Changes always cover **whole words**: `Argent` → `Argent's` shows the
///   word deleted and inserted, never a lone `'s`. Punctuation marks are
///   tokens of their own, so adding a comma shows as just the comma.
/// - **Whitespace is kept**: the concatenation of the `Equal` and `Delete`
///   ops is exactly `before`, and of `Equal` and `Insert` exactly `after`.
///   Whitespace belongs to the word before it, so removing a word removes the
///   space after it; whitespace-only differences between equal words show as
///   a separate delete/insert of the whitespace.
/// - Adjacent ops of the same kind are merged, and inside a changed region
///   the `Delete` always comes before the `Insert`. Whitespace that both
///   sides of a replacement share stays outside it (`old ` → `new ` is
///   `Delete("old")`, `Insert("new")`, `Equal(" ")`).
/// - Identical inputs give a single `Equal` (or nothing for two empty strings).
///
/// Very large inputs (more than about 4 million token pairs) fall back to
/// trimming the common prefix and suffix and showing the middle as one
/// replacement.
///
/// ```
/// use cipher_text::diff::{diff, DiffOp::*};
/// assert_eq!(
///     diff("the old king", "the new king"),
///     [Equal("the ".into()), Delete("old".into()), Insert("new".into()), Equal(" king".into())],
/// );
/// ```
pub fn diff(before: &str, after: &str) -> Vec<DiffOp> {
    let a = tokenize(before);
    let b = tokenize(after);

    let prefix = a
        .iter()
        .zip(&b)
        .take_while(|(x, y)| x.word == y.word)
        .count();
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x.word == y.word)
        .count();
    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];

    let mut raw: Vec<DiffOp> = Vec::new();
    for t in &a[..prefix].iter().zip(&b[..prefix]).collect::<Vec<_>>() {
        push_equal(&mut raw, t.0, t.1);
    }
    if a_mid.len().saturating_mul(b_mid.len()) > 4_000_000 {
        raw.extend(a_mid.iter().map(|t| DiffOp::Delete(t.full())));
        raw.extend(b_mid.iter().map(|t| DiffOp::Insert(t.full())));
    } else {
        lcs_ops(a_mid, b_mid, &mut raw);
    }
    for (x, y) in a[a.len() - suffix..].iter().zip(&b[b.len() - suffix..]) {
        push_equal(&mut raw, x, y);
    }
    normalize(raw)
}

#[derive(Debug)]
struct Token<'a> {
    /// The word or punctuation mark, without whitespace.
    word: &'a str,
    /// Whitespace that follows it (leading whitespace is its own token with an empty word).
    space: &'a str,
}

impl Token<'_> {
    fn full(&self) -> String {
        format!("{}{}", self.word, self.space)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn tokenize(s: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    let bytes_end = s.len();
    // Leading whitespace.
    let lead = s.len() - s.trim_start().len();
    if lead > 0 {
        out.push(Token {
            word: "",
            space: &s[..lead],
        });
        i = lead;
    }
    while i < bytes_end {
        let rest = &s[i..];
        let first = rest.chars().next().unwrap_or(' ');
        let word_len = if is_word_char(first) {
            let mut end = 0;
            let mut chars = rest.char_indices().peekable();
            while let Some((j, c)) = chars.next() {
                if is_word_char(c) {
                    end = j + c.len_utf8();
                } else if (c == '\'' || c == '’' || c == '-')
                    && end == j
                    && chars.peek().is_some_and(|(_, n)| is_word_char(*n))
                {
                    // Apostrophe or hyphen inside a word: keep going.
                    end = j + c.len_utf8();
                } else {
                    break;
                }
            }
            end
        } else {
            first.len_utf8()
        };
        let after = &rest[word_len..];
        let space_len = after.len() - after.trim_start().len();
        out.push(Token {
            word: &rest[..word_len],
            space: &after[..space_len],
        });
        i += word_len + space_len;
    }
    out
}

fn push_equal(raw: &mut Vec<DiffOp>, x: &Token, y: &Token) {
    if x.space == y.space {
        raw.push(DiffOp::Equal(x.full()));
    } else {
        raw.push(DiffOp::Equal(x.word.to_string()));
        if !x.space.is_empty() {
            raw.push(DiffOp::Delete(x.space.to_string()));
        }
        if !y.space.is_empty() {
            raw.push(DiffOp::Insert(y.space.to_string()));
        }
    }
}

fn lcs_ops(a: &[Token], b: &[Token], out: &mut Vec<DiffOp>) {
    let (n, m) = (a.len(), b.len());
    // table[i][j] = LCS length of a[i..] and b[j..]
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if a[i].word == b[j].word {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i].word == b[j].word {
            push_equal(out, &a[i], &b[j]);
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            out.push(DiffOp::Delete(a[i].full()));
            i += 1;
        } else {
            out.push(DiffOp::Insert(b[j].full()));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|t| DiffOp::Delete(t.full())));
    out.extend(b[j..].iter().map(|t| DiffOp::Insert(t.full())));
}

/// Merge neighbours, put Deletes before Inserts inside each changed region.
fn normalize(raw: Vec<DiffOp>) -> Vec<DiffOp> {
    let mut out: Vec<DiffOp> = Vec::new();
    let mut deletes = String::new();
    let mut inserts = String::new();
    let mut equal = String::new();
    let flush_changes = |out: &mut Vec<DiffOp>, d: &mut String, i: &mut String| {
        if !d.is_empty() {
            out.push(DiffOp::Delete(std::mem::take(d)));
        }
        if !i.is_empty() {
            out.push(DiffOp::Insert(std::mem::take(i)));
        }
    };
    for op in raw {
        match op {
            DiffOp::Equal(s) => {
                flush_changes(&mut out, &mut deletes, &mut inserts);
                equal.push_str(&s);
            }
            DiffOp::Delete(s) => {
                if !equal.is_empty() {
                    out.push(DiffOp::Equal(std::mem::take(&mut equal)));
                }
                deletes.push_str(&s);
            }
            DiffOp::Insert(s) => {
                if !equal.is_empty() {
                    out.push(DiffOp::Equal(std::mem::take(&mut equal)));
                }
                inserts.push_str(&s);
            }
        }
    }
    flush_changes(&mut out, &mut deletes, &mut inserts);
    if !equal.is_empty() {
        out.push(DiffOp::Equal(equal));
    }
    share_whitespace(out)
}

/// In a replacement (`Delete` then `Insert`), whitespace that both sides start
/// or end with is not a change: move it into the neighbouring `Equal`s.
fn share_whitespace(ops: Vec<DiffOp>) -> Vec<DiffOp> {
    let mut out: Vec<DiffOp> = Vec::new();
    let mut iter = ops.into_iter().peekable();
    while let Some(op) = iter.next() {
        let (DiffOp::Delete(del), Some(DiffOp::Insert(_))) = (&op, iter.peek()) else {
            push_merged(&mut out, op);
            continue;
        };
        let Some(DiffOp::Insert(ins)) = iter.next() else {
            unreachable!("peeked an Insert");
        };
        let mut del = del.as_str();
        let mut ins = ins.as_str();
        let lead: String = del
            .chars()
            .zip(ins.chars())
            .take_while(|(a, b)| a == b && a.is_whitespace())
            .map(|(a, _)| a)
            .collect();
        del = &del[lead.len()..];
        ins = &ins[lead.len()..];
        let trail: String = del
            .chars()
            .rev()
            .zip(ins.chars().rev())
            .take_while(|(a, b)| a == b && a.is_whitespace())
            .map(|(a, _)| a)
            .collect();
        let (del, ins) = (
            &del[..del.len() - trail.len()],
            &ins[..ins.len() - trail.len()],
        );
        push_merged(&mut out, DiffOp::Equal(lead));
        if !del.is_empty() {
            out.push(DiffOp::Delete(del.to_string()));
        }
        if !ins.is_empty() {
            out.push(DiffOp::Insert(ins.to_string()));
        }
        // Whitespace is symmetric here, so reversing it keeps the text intact.
        push_merged(&mut out, DiffOp::Equal(trail.chars().rev().collect()));
    }
    out
}

fn push_merged(out: &mut Vec<DiffOp>, op: DiffOp) {
    match (out.last_mut(), op) {
        (_, DiffOp::Equal(s)) if s.is_empty() => {}
        (Some(DiffOp::Equal(prev)), DiffOp::Equal(s)) => prev.push_str(&s),
        (_, op) => out.push(op),
    }
}
