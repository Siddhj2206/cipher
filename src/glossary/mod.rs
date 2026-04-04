pub mod cli;
mod closest_match;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use closest_match::ClosestMatch;

use crate::output::stderr_warn;

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

impl std::str::FromStr for InjectionMode {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let normalized = value.trim().to_lowercase();
        Ok(match normalized.as_str() {
            "full" => InjectionMode::Full,
            "smart" => InjectionMode::Smart,
            _ => {
                if !normalized.is_empty() {
                    stderr_warn(format!(
                        "Unknown glossary_injection '{}', using 'smart'.",
                        value
                    ));
                }
                InjectionMode::Smart
            }
        })
    }
}

pub fn book_config_injection_mode(value: &str) -> InjectionMode {
    let normalized = value.trim().to_lowercase();

    match normalized.as_str() {
        "smart" | "" => InjectionMode::Smart,
        "full" => {
            stderr_warn(
                "Book config glossary_injection 'full' is deprecated; using 'smart' with per-chapter fallback to full.",
            );
            InjectionMode::Smart
        }
        _ => {
            stderr_warn(format!(
                "Unknown glossary_injection '{}', using 'smart'.",
                value
            ));
            InjectionMode::Smart
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

pub fn save_glossary<P: AsRef<Path>>(path: P, terms: &[GlossaryTerm]) -> Result<()> {
    let mut deduped = terms.to_vec();
    dedupe_terms(&mut deduped);

    let path = path.as_ref();
    let json = serde_json::to_string_pretty(&deduped)?;
    fs::write(path, json + "\n")?;
    Ok(())
}

fn dedupe_terms(terms: &mut Vec<GlossaryTerm>) {
    let mut seen = std::collections::HashSet::new();
    terms.retain(|t| seen.insert(glossary_term_key(t)));
}

pub fn glossary_term_key(term: &GlossaryTerm) -> String {
    normalize_key(term.og_term.as_deref().unwrap_or(&term.term))
}

pub fn glossary_term_prompt_fingerprint(term: &GlossaryTerm) -> String {
    #[derive(Serialize)]
    struct PromptFingerprint<'a> {
        term: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        og_term: Option<&'a str>,
        definition: &'a str,
    }

    serde_json::to_string(&PromptFingerprint {
        term: &term.term,
        og_term: term.og_term.as_deref(),
        definition: &term.definition,
    })
    .expect("prompt fingerprint should serialize")
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
) -> (Vec<GlossaryTerm>, usize, usize, Vec<GlossaryTerm>) {
    let mut result = existing;
    let existing_keys: std::collections::HashSet<_> =
        result.iter().map(glossary_term_key).collect();

    let mut added = 0;
    let mut skipped = 0;
    let mut added_terms = Vec::new();
    let mut seen_keys = existing_keys;

    for term in incoming {
        let key = glossary_term_key(&term);
        if seen_keys.contains(&key) {
            skipped += 1;
        } else {
            seen_keys.insert(key);
            result.push(term.clone());
            added_terms.push(term);
            added += 1;
        }
    }

    dedupe_terms(&mut result);
    (result, added, skipped, added_terms)
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
    fn test_dedupe_preserves_order() {
        let mut terms = vec![
            term("Apple", None, "A fruit"),
            term("apple", None, "Another fruit"), // dup
            term("Banana", Some("香蕉"), "Yellow fruit"),
            term("Apple", Some("苹果"), "Different key"), // different key
        ];

        dedupe_terms(&mut terms);
        assert_eq!(terms.len(), 3);
        // Order preserved: Apple (no og), Banana, Apple (苹果)
        assert_eq!(terms[0].term, "Apple");
        assert_eq!(terms[0].og_term, None);
        assert_eq!(terms[1].term, "Banana");
        assert_eq!(terms[2].og_term.as_deref(), Some("苹果"));
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

    #[test]
    fn test_merge_terms_adds_new() {
        let existing = vec![term("Apple", Some("苹果"), "A fruit")];
        let incoming = vec![term("Banana", Some("香蕉"), "Yellow fruit")];

        let (merged, added, skipped, _) = merge_terms(existing, incoming);
        assert_eq!(added, 1);
        assert_eq!(skipped, 0);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_merge_terms_skips_duplicates() {
        let existing = vec![term("Apple", Some("苹果"), "A fruit")];
        let incoming = vec![term("Apple", Some("苹果"), "Same fruit")];

        let (merged, added, skipped, _) = merge_terms(existing, incoming);
        assert_eq!(added, 0);
        assert_eq!(skipped, 1);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn test_merge_terms_mixed() {
        let existing = vec![
            term("Apple", Some("苹果"), "A fruit"),
            term("Cherry", Some("樱桃"), "Red fruit"),
        ];
        let incoming = vec![
            term("Apple", Some("苹果"), "Dup"),
            term("Banana", Some("香蕉"), "New"),
            term("Date", Some("枣"), "Also new"),
        ];

        let (merged, added, skipped, _) = merge_terms(existing, incoming);
        assert_eq!(added, 2);
        assert_eq!(skipped, 1);
        assert_eq!(merged.len(), 4);
    }

    #[test]
    fn test_merge_terms_empty_incoming() {
        let existing = vec![term("Apple", Some("苹果"), "A fruit")];
        let (merged, added, skipped, _) = merge_terms(existing, vec![]);
        assert_eq!(added, 0);
        assert_eq!(skipped, 0);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn test_merge_terms_returns_added_terms() {
        let existing = vec![term("Apple", Some("苹果"), "A fruit")];
        let incoming = vec![
            term("Apple", Some("苹果"), "Dup"),
            term("Banana", Some("香蕉"), "Yellow fruit"),
        ];

        let (_, added, skipped, added_terms) = merge_terms(existing, incoming);
        assert_eq!(added, 1);
        assert_eq!(skipped, 1);
        assert_eq!(added_terms.len(), 1);
        assert_eq!(added_terms[0].term, "Banana");
    }

    #[test]
    fn test_merge_terms_skips_duplicate_entries_within_incoming_batch() {
        let existing = vec![term("Apple", Some("苹果"), "A fruit")];
        let incoming = vec![
            term("Banana", Some("香蕉"), "Yellow fruit"),
            term("Banana", Some("香蕉"), "Yellow fruit duplicate"),
            term("Cherry", Some("樱桃"), "Red fruit"),
        ];

        let (merged, added, skipped, added_terms) = merge_terms(existing, incoming);
        assert_eq!(added, 2);
        assert_eq!(skipped, 1);
        assert_eq!(merged.len(), 3);
        assert_eq!(added_terms.len(), 2);
        assert_eq!(added_terms[0].term, "Banana");
        assert_eq!(added_terms[1].term, "Cherry");
    }

    #[test]
    fn test_save_and_load_glossary_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glossary.json");

        let terms = vec![
            term("Banana", Some("香蕉"), "Yellow fruit"),
            term("Apple", Some("苹果"), "A fruit"),
        ];

        save_glossary(&path, &terms).unwrap();
        let loaded = load_glossary(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        // Order is preserved (no sorting)
        assert_eq!(loaded[0].term, "Banana");
        assert_eq!(loaded[1].term, "Apple");
    }

    #[test]
    fn test_load_glossary_nonexistent_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no_such_file.json");
        let terms = load_glossary(&path).unwrap();
        assert!(terms.is_empty());
    }

    #[test]
    fn test_save_glossary_deduplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glossary.json");

        let terms = vec![
            term("Apple", Some("苹果"), "First"),
            term("apple", Some("苹果"), "Duplicate"),
        ];

        save_glossary(&path, &terms).unwrap();
        let loaded = load_glossary(&path).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn test_injection_mode_from_str() {
        assert!(matches!(
            "full".parse::<InjectionMode>().unwrap(),
            InjectionMode::Full
        ));
        assert!(matches!(
            "smart".parse::<InjectionMode>().unwrap(),
            InjectionMode::Smart
        ));
        assert!(matches!(
            "SMART".parse::<InjectionMode>().unwrap(),
            InjectionMode::Smart
        ));
        assert!(matches!(
            "Full".parse::<InjectionMode>().unwrap(),
            InjectionMode::Full
        ));
        assert!(matches!(
            "unknown".parse::<InjectionMode>().unwrap(),
            InjectionMode::Smart
        ));
        assert!(matches!(
            "".parse::<InjectionMode>().unwrap(),
            InjectionMode::Smart
        ));
    }

    #[test]
    fn test_injection_mode_parse_trait() {
        let mode: InjectionMode = "full".parse().unwrap();
        assert!(matches!(mode, InjectionMode::Full));
        let mode: InjectionMode = "smart".parse().unwrap();
        assert!(matches!(mode, InjectionMode::Smart));
    }

    #[test]
    fn test_book_config_injection_mode_demotes_full_to_smart() {
        assert!(matches!(
            book_config_injection_mode("full"),
            InjectionMode::Smart
        ));
        assert!(matches!(
            book_config_injection_mode("Full"),
            InjectionMode::Smart
        ));
        assert!(matches!(
            book_config_injection_mode("smart"),
            InjectionMode::Smart
        ));
        assert!(matches!(
            book_config_injection_mode("unknown"),
            InjectionMode::Smart
        ));
    }

    #[test]
    fn test_prompt_fingerprint_ignores_notes() {
        let mut first = term("Hero", Some("勇者"), "Main hero");
        first.notes = Some("first note".into());

        let mut second = term("Hero", Some("勇者"), "Main hero");
        second.notes = Some("second note".into());

        assert_eq!(
            glossary_term_prompt_fingerprint(&first),
            glossary_term_prompt_fingerprint(&second)
        );
    }
}
