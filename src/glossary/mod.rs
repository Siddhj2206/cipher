mod closest_match;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use closest_match::ClosestMatch;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
pub struct GlossaryTerm {
    pub term: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub og_term: Option<String>,
    pub definition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMode {
    Full,
    Smart,
}

impl InjectionMode {
    pub fn from_str(value: &str) -> Self {
        let normalized = value.trim().to_lowercase();
        match normalized.as_str() {
            "full" => InjectionMode::Full,
            "smart" => InjectionMode::Smart,
            _ => {
                if !normalized.is_empty() {
                    eprintln!(
                        "Warning: Unknown glossary_injection '{}', using 'smart'",
                        value
                    );
                }
                InjectionMode::Smart
            }
        }
    }
}

pub fn load_glossary<P: AsRef<Path>>(path: P) -> Result<Vec<GlossaryTerm>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read glossary file {}", path.display()))?;
    let terms: Vec<GlossaryTerm> = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse glossary JSON in {}", path.display()))?;
    Ok(terms)
}

pub fn save_glossary<P: AsRef<Path>>(path: P, terms: &mut Vec<GlossaryTerm>) -> Result<()> {
    let sorted = {
        let mut sorted = terms.clone();
        dedupe_and_sort_terms(&mut sorted);
        sorted
    };

    let path = path.as_ref();
    let json = serde_json::to_string_pretty(&sorted)?;
    fs::write(path, json + "\n")?;
    *terms = sorted;
    Ok(())
}

fn dedupe_and_sort_terms(terms: &mut Vec<GlossaryTerm>) {
    let mut seen = std::collections::HashSet::new();
    terms.retain(|t| seen.insert(term_dedupe_key(t)));

    terms.sort_by(|a, b| {
        let key_a = term_dedupe_key(a);
        let key_b = term_dedupe_key(b);
        key_a.cmp(&key_b).then_with(|| a.term.cmp(&b.term))
    });
}

fn term_dedupe_key(term: &GlossaryTerm) -> String {
    normalize_key(term.og_term.as_deref().unwrap_or(&term.term))
}

fn normalize_key(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub struct SelectionResult {
    pub terms: Vec<GlossaryTerm>,
    pub total_count: usize,
    pub selected_count: usize,
    #[allow(dead_code)]
    pub is_subset: bool,
    pub used_fallback_to_full: bool,
}

pub fn select_terms_for_text(
    all_terms: &[GlossaryTerm],
    text: &str,
    mode: InjectionMode,
) -> SelectionResult {
    match mode {
        InjectionMode::Full => SelectionResult {
            terms: all_terms.to_vec(),
            total_count: all_terms.len(),
            selected_count: all_terms.len(),
            is_subset: false,
            used_fallback_to_full: false,
        },
        InjectionMode::Smart => select_terms_smart(all_terms, text),
    }
}

const MIN_GLOSSARY_MATCHES: usize = 5;
const SLIDING_WINDOW_MIN: usize = 3;
const SLIDING_WINDOW_MAX: usize = 6;

fn select_terms_smart(all_terms: &[GlossaryTerm], text: &str) -> SelectionResult {
    let mut matched_indices: HashSet<usize> = HashSet::new();
    let mut og_term_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
    let mut unique_og_terms: Vec<String> = Vec::new();
    let mut seen_og_terms: HashSet<String> = HashSet::new();

    for (idx, term) in all_terms.iter().enumerate() {
        let og_term = term.og_term.as_deref().map(str::trim).unwrap_or("");
        if og_term.is_empty() {
            matched_indices.insert(idx);
            continue;
        }

        og_term_to_indices
            .entry(og_term.to_string())
            .or_default()
            .push(idx);

        if seen_og_terms.insert(og_term.to_string()) {
            unique_og_terms.push(og_term.to_string());
        }
    }

    if unique_og_terms.is_empty() {
        return SelectionResult {
            terms: all_terms.to_vec(),
            total_count: all_terms.len(),
            selected_count: all_terms.len(),
            is_subset: false,
            used_fallback_to_full: true,
        };
    }

    let og_terms_refs: Vec<&str> = unique_og_terms.iter().map(|s| s.as_str()).collect();
    let matcher = ClosestMatch::new(&og_terms_refs, &[2, 3, 4]);

    let candidates = extract_candidates(text);

    for candidate in candidates {
        if let Some(match_term) = matcher.closest(&candidate)
            && text.contains(match_term)
            && let Some(indices) = og_term_to_indices.get(match_term)
        {
            for &idx in indices {
                matched_indices.insert(idx);
            }
        }
    }

    if matched_indices.len() < MIN_GLOSSARY_MATCHES {
        return SelectionResult {
            terms: all_terms.to_vec(),
            total_count: all_terms.len(),
            selected_count: all_terms.len(),
            is_subset: false,
            used_fallback_to_full: true,
        };
    }

    let mut indices: Vec<usize> = matched_indices.into_iter().collect();
    indices.sort_unstable();
    let selected_count = indices.len();
    let terms: Vec<GlossaryTerm> = indices
        .into_iter()
        .map(|idx| all_terms[idx].clone())
        .collect();

    SelectionResult {
        terms,
        total_count: all_terms.len(),
        selected_count,
        is_subset: selected_count < all_terms.len(),
        used_fallback_to_full: false,
    }
}

fn extract_candidates(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut candidates: BTreeSet<String> = BTreeSet::new();

    for start in 0..chars.len() {
        for len in SLIDING_WINDOW_MIN..=SLIDING_WINDOW_MAX {
            if start + len > chars.len() {
                continue;
            }

            let slice = &chars[start..start + len];
            if slice.iter().all(|c| c.is_ascii()) {
                continue;
            }

            let candidate: String = slice.iter().collect();
            candidates.insert(candidate);
        }
    }

    candidates.into_iter().collect()
}

pub fn merge_terms(
    existing: Vec<GlossaryTerm>,
    incoming: Vec<GlossaryTerm>,
) -> (Vec<GlossaryTerm>, usize, usize) {
    let mut result = existing;
    let existing_keys: std::collections::HashSet<_> = result.iter().map(term_dedupe_key).collect();

    let mut added = 0;
    let mut skipped = 0;

    for term in incoming {
        let key = term_dedupe_key(&term);
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
        assert_eq!(normalize_key("  Hello   World  "), "hello world");
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
    fn test_select_terms_full_when_no_og_term() {
        let terms = vec![
            term("Apple", None, "A fruit"),
            term("Banana", None, "A fruit"),
        ];
        let result = select_terms_for_text(&terms, "Any text", InjectionMode::Smart);
        assert_eq!(result.terms.len(), terms.len());
        assert!(result.used_fallback_to_full);
    }

    #[test]
    fn test_select_terms_fallback_to_full_when_too_few_matches() {
        let terms = vec![
            term("Starship", Some("星空舰"), "Ship"),
            term("River", Some("山河图"), "River"),
            term("Lotus", Some("红莲花"), "Lotus"),
            term("Blade", Some("青锋剑"), "Blade"),
            term("Gate", Some("玉门关"), "Gate"),
            term("Shadow", Some("孤城影"), "Shadow"),
        ];
        let text = "星空舰 与 山河图";
        let result = select_terms_for_text(&terms, text, InjectionMode::Smart);
        assert_eq!(result.terms.len(), terms.len());
        assert!(result.used_fallback_to_full);
    }

    #[test]
    fn test_select_terms_smart_subset_and_include_empty_og() {
        let terms = vec![
            term("Starship", Some("星空舰"), "Ship"),
            term("River", Some("山河图"), "River"),
            term("Lotus", Some("红莲花"), "Lotus"),
            term("Blade", Some("青锋剑"), "Blade"),
            term("Gate", Some("玉门关"), "Gate"),
            term("Shadow", Some("孤城影"), "Shadow"),
            term("Always", None, "Always include"),
        ];
        let text = "星空舰 山河图 红莲花 青锋剑 玉门关";
        let result = select_terms_for_text(&terms, text, InjectionMode::Smart);
        assert_eq!(result.terms.len(), 6);
        assert!(result.terms.iter().any(|t| t.term == "Always"));
        assert!(!result.terms.iter().any(|t| t.term == "Shadow"));
        assert!(!result.used_fallback_to_full);
    }
}
