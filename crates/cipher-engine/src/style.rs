//! The Book's style: its Style Rules, written and read as one text (`docs/design/desktop-ux.md`
//! › Style). Every Chapter's prompt carries them (`prompt.rs`).

use cipher_book::{Book, Change, RevisionKind};

use crate::Result;

/// The rules in a style text: one per paragraph or list item. Headings (`# Style Guide`) and
/// list markers (`-`, `*`, `+`, `1.`) are dropped; lines of one paragraph are joined.
///
/// ```
/// let rules = cipher_engine::style_rules("# Style\n\nKeep it tense.\nShort lines.\n\n  * **Dialogue:** natural\n- Avoid footnotes");
/// assert_eq!(rules, ["Keep it tense. Short lines.", "**Dialogue:** natural", "Avoid footnotes"]);
/// ```
pub fn style_rules(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut open = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            open = false;
            continue;
        }
        let item = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| t.strip_prefix(m))
            .or_else(|| {
                let digits = t.chars().take_while(char::is_ascii_digit).count();
                (digits > 0)
                    .then(|| t[digits..].strip_prefix(". "))
                    .flatten()
            });
        match (item, open) {
            (Some(rest), _) => {
                out.push(rest.trim().to_string());
                open = true;
            }
            (None, true) => {
                let last = out.last_mut().expect("an open rule");
                last.push(' ');
                last.push_str(t);
            }
            (None, false) => {
                out.push(t.to_string());
                open = true;
            }
        }
    }
    out.retain(|r| !r.is_empty());
    out
}

/// The Book's Style Rules as one text, a paragraph each.
pub fn style_text(book: &Book) -> Result<String> {
    Ok(book
        .glossary()?
        .style_rules
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// Makes the Book's Style Rules `text`'s ([`style_rules`]), as one Revision. Returns whether
/// anything changed.
pub fn set_style(book: &mut Book, text: &str) -> Result<bool> {
    let old = book.glossary()?.style_rules;
    let new = style_rules(text);
    let mut changes = Vec::new();
    for (i, rule) in new.iter().enumerate() {
        match old.get(i) {
            Some(o) if o.text == *rule => {}
            Some(o) => changes.push(Change::EditStyleRule {
                id: o.id,
                text: Some(rule.clone()),
            }),
            None => changes.push(Change::AddStyleRule { text: rule.clone() }),
        }
    }
    for o in old.iter().skip(new.len()) {
        changes.push(Change::EditStyleRule {
            id: o.id,
            text: None,
        });
    }
    if changes.is_empty() {
        return Ok(false);
    }
    book.apply(Change::Group {
        kind: RevisionKind::Glossary,
        label: "Edited the style".into(),
        changes,
    })?;
    Ok(true)
}
