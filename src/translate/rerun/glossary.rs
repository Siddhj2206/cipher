use crate::book::paths::{chapter_output_path, chapter_state_key};
use crate::error::{Error, Result};
use crate::glossary::{
    GlossaryTerm, InjectionMode, glossary_term_key, glossary_term_prompt_fingerprint,
    select_terms_for_text,
};
use crate::state::{
    ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterState, GlossaryState, GlossaryStateTerm,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) fn build_glossary_state(
    glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<GlossaryState> {
    let terms = glossary
        .iter()
        .map(|term| {
            Ok((
                glossary_term_key(term),
                GlossaryStateTerm {
                    term: term.term.clone(),
                    og_term: term.og_term.clone(),
                    definition: term.definition.clone(),
                    fingerprint: glossary_term_prompt_fingerprint(term)?,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(GlossaryState::new(injection_mode, terms))
}

pub(crate) fn build_chapter_glossary_usage(
    selection: &crate::glossary::SelectionResult,
    injection_mode: InjectionMode,
) -> Result<ChapterGlossaryUsage> {
    let terms = selection
        .terms
        .iter()
        .map(|term| {
            Ok(ChapterGlossaryTerm {
                key: glossary_term_key(term),
                fingerprint: glossary_term_prompt_fingerprint(term)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ChapterGlossaryUsage {
        injection_mode,
        used_fallback_to_full: selection.used_fallback_to_full,
        terms,
    })
}

pub(crate) fn selection_fingerprints(terms: &[GlossaryTerm]) -> Result<BTreeMap<String, String>> {
    terms
        .iter()
        .map(|term| {
            Ok((
                glossary_term_key(term),
                glossary_term_prompt_fingerprint(term)?,
            ))
        })
        .collect()
}

pub(crate) fn usage_fingerprint_map(usage: &ChapterGlossaryUsage) -> BTreeMap<String, String> {
    usage
        .terms
        .iter()
        .map(|term| (term.key.clone(), term.fingerprint.clone()))
        .collect()
}

pub(crate) fn glossary_terms_from_state(glossary_state: &GlossaryState) -> Vec<GlossaryTerm> {
    glossary_state
        .terms
        .values()
        .map(|term| GlossaryTerm {
            term: term.term.clone(),
            og_term: term.og_term.clone(),
            definition: term.definition.clone(),
            notes: None,
        })
        .collect()
}

fn changed_keys<V>(
    previous_terms: &BTreeMap<String, V>,
    current_terms: &BTreeMap<String, V>,
    is_equal: impl Fn(&V, &V) -> bool,
) -> BTreeSet<String> {
    let all_keys: BTreeSet<String> = previous_terms
        .keys()
        .chain(current_terms.keys())
        .cloned()
        .collect();

    all_keys
        .into_iter()
        .filter(|key| {
            let previous = previous_terms.get(key);
            let current = current_terms.get(key);
            match (previous, current) {
                (Some(a), Some(b)) => !is_equal(a, b),
                _ => true,
            }
        })
        .collect()
}

pub(crate) fn changed_prompt_relevant_keys(
    previous_terms: &BTreeMap<String, GlossaryStateTerm>,
    current_terms: &BTreeMap<String, GlossaryStateTerm>,
) -> BTreeSet<String> {
    changed_keys(previous_terms, current_terms, |a, b| {
        a.fingerprint == b.fingerprint
    })
}

pub(crate) fn changed_selected_term_keys(
    previous_terms: &BTreeMap<String, String>,
    current_terms: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    changed_keys(previous_terms, current_terms, |a, b| a == b)
}

pub(crate) fn current_expected_glossary_usage(
    raw_path: &Path,
    current_glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<Option<ChapterGlossaryUsage>> {
    let chapter_text = std::fs::read_to_string(raw_path)
        .map_err(|e| Error::io(format!("Failed to read {}", raw_path.display()), e))?;
    if chapter_text.trim().is_empty() {
        return Ok(None);
    }

    let selection = select_terms_for_text(current_glossary, &chapter_text, injection_mode);
    Ok(Some(build_chapter_glossary_usage(
        &selection,
        injection_mode,
    )?))
}

pub(crate) fn chapter_matches_current_glossary(
    raw_path: &Path,
    chapter_state: &ChapterState,
    current_glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<bool> {
    let Some(usage) = &chapter_state.glossary_usage else {
        return Ok(false);
    };

    let current_fingerprints = selection_fingerprints(current_glossary)?;

    let tracked_terms_match = usage
        .terms
        .iter()
        .chain(chapter_state.exported_terms.iter())
        .all(|term| {
            current_fingerprints
                .get(&term.key)
                .is_some_and(|fingerprint| fingerprint == &term.fingerprint)
        });
    if !tracked_terms_match {
        return Ok(false);
    }

    let Some(expected_usage) =
        current_expected_glossary_usage(raw_path, current_glossary, injection_mode)?
    else {
        return Ok(true);
    };
    let tracked_usage = usage_fingerprint_map(usage);
    let expected_usage_terms = usage_fingerprint_map(&expected_usage);

    if usage.injection_mode == InjectionMode::Full {
        return Ok(tracked_usage == expected_usage_terms);
    }

    Ok(tracked_usage == expected_usage_terms
        && usage.used_fallback_to_full == expected_usage.used_fallback_to_full)
}

pub(crate) fn count_chapters_still_stale_for_current_glossary(
    chapters: &[PathBuf],
    raw_dir: &Path,
    out_dir: &Path,
    chapter_states: &BTreeMap<String, ChapterState>,
    current_glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<usize> {
    let mut remaining = 0;

    for chapter_file in chapters {
        let chapter_path = chapter_state_key(raw_dir, chapter_file)?;
        let output_exists = chapter_output_path(out_dir, chapter_file)?.exists();
        if !output_exists {
            continue;
        }

        let Some(chapter_state) = chapter_states.get(&chapter_path) else {
            remaining += 1;
            continue;
        };

        if !chapter_matches_current_glossary(
            chapter_file,
            chapter_state,
            current_glossary,
            injection_mode,
        )? {
            remaining += 1;
        }
    }

    Ok(remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary::GlossaryTerm;
    use crate::state::{
        ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterState, ChapterStatus, GlossaryStateTerm,
    };

    fn test_term(key: &str, term: &str, definition: &str) -> GlossaryTerm {
        GlossaryTerm {
            term: term.to_string(),
            og_term: Some(key.to_string()),
            definition: definition.to_string(),
            notes: None,
        }
    }

    fn usage_term(key: &str, fingerprint: &str) -> ChapterGlossaryTerm {
        ChapterGlossaryTerm {
            key: key.to_string(),
            fingerprint: fingerprint.to_string(),
        }
    }

    #[test]
    fn selection_fingerprints_empty() {
        assert!(selection_fingerprints(&[]).unwrap().is_empty());
    }

    #[test]
    fn selection_fingerprints_maps_keys() {
        use crate::glossary::{glossary_term_key, glossary_term_prompt_fingerprint};
        let terms = vec![
            test_term("hero", "Hero", "A hero"),
            test_term("city", "City", "A city"),
        ];
        let result = selection_fingerprints(&terms).unwrap();
        let key0 = glossary_term_key(&terms[0]);
        let key1 = glossary_term_key(&terms[1]);
        assert_eq!(
            result.get(&key0).unwrap(),
            &glossary_term_prompt_fingerprint(&terms[0]).unwrap()
        );
        assert_eq!(
            result.get(&key1).unwrap(),
            &glossary_term_prompt_fingerprint(&terms[1]).unwrap()
        );
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn usage_fingerprint_map_empty() {
        let usage = ChapterGlossaryUsage {
            injection_mode: InjectionMode::Smart,
            used_fallback_to_full: false,
            terms: vec![],
        };
        assert!(usage_fingerprint_map(&usage).is_empty());
    }

    #[test]
    fn usage_fingerprint_map_maps_terms() {
        let usage = ChapterGlossaryUsage {
            injection_mode: InjectionMode::Smart,
            used_fallback_to_full: false,
            terms: vec![usage_term("hero", "fp1"), usage_term("city", "fp2")],
        };
        let result = usage_fingerprint_map(&usage);
        assert_eq!(result.get("hero").unwrap(), "fp1");
        assert_eq!(result.get("city").unwrap(), "fp2");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn glossary_terms_from_state_converts_state_terms() {
        let mut state_terms = BTreeMap::new();
        state_terms.insert(
            "hero".to_string(),
            GlossaryStateTerm {
                term: "Hero".to_string(),
                og_term: Some("hero".to_string()),
                definition: "def".to_string(),
                fingerprint: "fp".to_string(),
            },
        );
        let state = GlossaryState::new(InjectionMode::Smart, state_terms);
        let result = glossary_terms_from_state(&state);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].term, "Hero");
        assert_eq!(result[0].og_term, Some("hero".to_string()));
        assert_eq!(result[0].definition, "def");
        assert!(result[0].notes.is_none());
    }

    #[test]
    fn changed_prompt_relevant_keys_detects_changes() {
        let mut prev = BTreeMap::new();
        let mut curr = BTreeMap::new();
        prev.insert(
            "a".to_string(),
            GlossaryStateTerm {
                term: "A".into(),
                og_term: None,
                definition: "d1".into(),
                fingerprint: "fp1".into(),
            },
        );
        curr.insert(
            "a".to_string(),
            GlossaryStateTerm {
                term: "A".into(),
                og_term: None,
                definition: "d1".into(),
                fingerprint: "fp2".into(),
            },
        );
        curr.insert(
            "b".to_string(),
            GlossaryStateTerm {
                term: "B".into(),
                og_term: None,
                definition: "d2".into(),
                fingerprint: "fp3".into(),
            },
        );
        let changed = changed_prompt_relevant_keys(&prev, &curr);
        assert!(changed.contains("a"), "fingerprint changed");
        assert!(changed.contains("b"), "new key added");
        assert_eq!(changed.len(), 2);
    }

    #[test]
    fn changed_selected_term_keys_detects_changes() {
        let mut prev = BTreeMap::new();
        let mut curr = BTreeMap::new();
        prev.insert("hero".to_string(), "fp1".to_string());
        curr.insert("hero".to_string(), "fp2".to_string());
        curr.insert("city".to_string(), "fp3".to_string());
        let changed = changed_selected_term_keys(&prev, &curr);
        assert!(changed.contains("hero"), "fingerprint changed");
        assert!(changed.contains("city"), "new key added");
        assert_eq!(changed.len(), 2);
    }

    #[test]
    fn current_expected_glossary_usage_returns_none_for_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.md");
        std::fs::write(&path, "  \n  ").unwrap();
        let result = current_expected_glossary_usage(&path, &[], InjectionMode::Smart).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn current_expected_glossary_usage_computes_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ch.md");
        std::fs::write(&path, "勇者が登場").unwrap();
        let glossary = vec![test_term("hero", "Hero", "definition")];
        let result = current_expected_glossary_usage(&path, &glossary, InjectionMode::Smart)
            .unwrap()
            .unwrap();
        assert_eq!(result.injection_mode, InjectionMode::Smart);
        assert!(!result.terms.is_empty());
    }

    #[test]
    fn chapter_matches_glossary_when_usage_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ch.md");
        std::fs::write(&path, "勇者が登場").unwrap();
        let state = ChapterState::new(
            "ch.md".to_string(),
            ChapterStatus::Success,
            None,
            None,
            None,
            None,
            vec![],
            None,
        );
        let result =
            chapter_matches_current_glossary(&path, &state, &[], InjectionMode::Smart).unwrap();
        assert!(!result, "no usage should not match");
    }
}
