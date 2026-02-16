use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GlossaryTerm {
    pub term: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub og_term: Option<String>,
    pub definition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

pub fn load_glossary<P: AsRef<Path>>(path: P) -> Result<Vec<GlossaryTerm>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path)?;
    let terms: Vec<GlossaryTerm> = serde_json::from_str(&content)?;
    Ok(terms)
}

pub fn save_glossary<P: AsRef<Path>>(path: P, terms: &mut Vec<GlossaryTerm>) -> Result<()> {
    dedupe_and_sort_terms(terms);

    let path = path.as_ref();
    let json = serde_json::to_string_pretty(terms)?;
    fs::write(path, json + "\n")?;
    Ok(())
}

fn dedupe_and_sort_terms(terms: &mut Vec<GlossaryTerm>) {
    fn dedupe_key(term: &GlossaryTerm) -> String {
        normalize_key(term.og_term.as_deref().unwrap_or(&term.term))
    }

    let mut seen = std::collections::HashSet::new();
    terms.retain(|t| seen.insert(dedupe_key(t)));

    terms.sort_by(|a, b| {
        let key_a = dedupe_key(a);
        let key_b = dedupe_key(b);
        key_a.cmp(&key_b).then_with(|| a.term.cmp(&b.term))
    });
}

fn normalize_key(s: &str) -> String {
    s.trim().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn render_for_prompt(terms: &[GlossaryTerm]) -> String {
    if terms.is_empty() {
        return "No glossary terms.".to_string();
    }

    let mut lines = Vec::new();
    for term in terms {
        let line = if let Some(ref og) = term.og_term {
            format!("{} [{}]: {}", term.term, og, term.definition)
        } else {
            format!("{}: {}", term.term, term.definition)
        };
        lines.push(line);
    }
    lines.join("\n")
}

pub fn merge_terms(
    existing: Vec<GlossaryTerm>,
    incoming: Vec<GlossaryTerm>,
) -> (Vec<GlossaryTerm>, usize, usize) {
    fn dedupe_key(term: &GlossaryTerm) -> String {
        normalize_key(term.og_term.as_deref().unwrap_or(&term.term))
    }

    let mut result = existing;
    let existing_keys: std::collections::HashSet<_> = result.iter().map(dedupe_key).collect();

    let mut added = 0;
    let mut skipped = 0;

    for term in incoming {
        let key = dedupe_key(&term);
        if existing_keys.contains(&key) {
            skipped += 1;
        } else {
            result.push(term);
            added += 1;
        }
    }

    dedupe_and_sort_terms(&mut result);
    (result, added, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(t: &str, og: Option<&str>, d: &str) -> GlossaryTerm {
        GlossaryTerm {
            term: t.to_string(),
            og_term: og.map(|s| s.to_string()),
            definition: d.to_string(),
            notes: None,
        }
    }

    #[test]
    fn test_normalize_key() {
        assert_eq!(normalize_key("  Hello   World  "), "Hello World");
        assert_eq!(normalize_key("test"), "test");
    }

    #[test]
    fn test_dedupe_and_sort() {
        let mut terms = vec![
            term("Apple", None, "A fruit"),
            term("apple", None, "Another fruit"), // dup
            term("Banana", Some("香蕉"), "Yellow fruit"),
            term("Apple", Some("苹果"), "Different key"), // different key
        ];

        dedupe_and_sort_terms(&mut terms);
        assert_eq!(terms.len(), 3);
        assert_eq!(terms[0].term, "Apple"); // by og_term (empty comes first)
        assert_eq!(terms[2].term, "Banana");
    }

    #[test]
    fn test_render_prompt() {
        let terms = vec![
            term("Hello", Some("你好"), "Greeting"),
            term("World", None, "The Earth"),
        ];
        let prompt = render_for_prompt(&terms);
        assert!(prompt.contains("Hello [你好]: Greeting"));
        assert!(prompt.contains("World: The Earth"));
    }
}
