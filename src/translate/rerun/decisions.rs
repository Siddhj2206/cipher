use crate::book::paths::{chapter_output_path, chapter_state_key};
use crate::error::{Error, Result};
use crate::glossary::{GlossaryTerm, InjectionMode, select_terms_for_text};
use crate::state::{
    ChapterGlossaryUsage, ChapterState, GlossaryState, normalized_source_text_hash,
};
use crate::translate::rerun::glossary::{
    build_glossary_state, changed_prompt_relevant_keys, changed_selected_term_keys,
    glossary_terms_from_state, selection_fingerprints, usage_fingerprint_map,
};
use crate::translate::rerun::types::{GlossaryRerunPlan, RerunDecision, SourceRerunPlan};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn tracked_usage_state_label(usage: &ChapterGlossaryUsage) -> &'static str {
    match usage.injection_mode {
        InjectionMode::Full => "full",
        _ if usage.used_fallback_to_full => "fallback",
        _ => "smart",
    }
}

pub(crate) fn exact_rerun_decision(
    raw_path: &Path,
    chapter_state: &ChapterState,
    current_glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<Option<RerunDecision>> {
    let Some(usage) = &chapter_state.glossary_usage else {
        return Ok(None);
    };

    let current_fingerprints = selection_fingerprints(current_glossary)?;

    let fingerprint_changed_keys: Vec<String> = usage
        .terms
        .iter()
        .chain(chapter_state.exported_terms.iter())
        .filter_map(|term| match current_fingerprints.get(&term.key) {
            Some(fingerprint) if fingerprint == &term.fingerprint => None,
            _ => Some(term.key.clone()),
        })
        .collect();

    if !fingerprint_changed_keys.is_empty() {
        return Ok(Some(RerunDecision {
            reason: format!(
                "Glossary term changed: {}",
                fingerprint_changed_keys.join(", ")
            ),
            is_approximate: false,
        }));
    }

    let Some(expected_usage) = crate::translate::rerun::glossary::current_expected_glossary_usage(
        raw_path,
        current_glossary,
        injection_mode,
    )?
    else {
        return Ok(None);
    };

    let tracked_usage = usage_fingerprint_map(usage);
    let expected_usage_terms = usage_fingerprint_map(&expected_usage);
    let selection_changed_keys = changed_selected_term_keys(&tracked_usage, &expected_usage_terms);

    if selection_changed_keys.is_empty() {
        if usage.injection_mode == InjectionMode::Full
            || usage.used_fallback_to_full == expected_usage.used_fallback_to_full
        {
            return Ok(None);
        }

        return Ok(Some(RerunDecision {
            reason: format!(
                "Fallback behavior: {} -> {}",
                tracked_usage_state_label(usage),
                tracked_usage_state_label(&expected_usage)
            ),
            is_approximate: false,
        }));
    }

    if usage.injection_mode == InjectionMode::Full
        || usage.used_fallback_to_full != expected_usage.used_fallback_to_full
    {
        return Ok(Some(RerunDecision {
            reason: format!(
                "Fallback behavior: {} -> {}",
                tracked_usage_state_label(usage),
                tracked_usage_state_label(&expected_usage)
            ),
            is_approximate: false,
        }));
    }

    Ok(Some(RerunDecision {
        reason: format!(
            "Glossary selection changed: {}",
            selection_changed_keys
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        ),
        is_approximate: false,
    }))
}

pub(crate) fn approximate_smart_rerun_decision(
    raw_path: &Path,
    previous_glossary_state: &GlossaryState,
    current_glossary: &[GlossaryTerm],
    changed_term_keys: &BTreeSet<String>,
) -> Result<Option<RerunDecision>> {
    let chapter_text = std::fs::read_to_string(raw_path)
        .map_err(|e| Error::io(format!("Failed to read {}", raw_path.display()), e))?;

    if chapter_text.trim().is_empty() {
        return Ok(None);
    }

    let previous_glossary = glossary_terms_from_state(previous_glossary_state);
    let previous_selection =
        select_terms_for_text(&previous_glossary, &chapter_text, InjectionMode::Smart);
    let current_selection =
        select_terms_for_text(current_glossary, &chapter_text, InjectionMode::Smart);

    if previous_selection.used_fallback_to_full || current_selection.used_fallback_to_full {
        if changed_term_keys.is_empty() {
            return Ok(None);
        }
        return Ok(Some(RerunDecision {
            reason: format!(
                "Full glossary changed (approx): {}",
                changed_term_keys
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            is_approximate: true,
        }));
    }

    let previous_terms = selection_fingerprints(&previous_selection.terms)?;
    let current_terms = selection_fingerprints(&current_selection.terms)?;
    let changed = changed_selected_term_keys(&previous_terms, &current_terms);

    if changed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(RerunDecision {
            reason: format!(
                "Glossary selection changed (approx): {}",
                changed.into_iter().collect::<Vec<_>>().join(", ")
            ),
            is_approximate: true,
        }))
    }
}

fn chapters_with_output<'a>(
    chapters: &'a [PathBuf],
    raw_dir: &Path,
    out_dir: &Path,
) -> Result<Vec<(String, &'a PathBuf)>> {
    let mut result = Vec::new();
    for chapter_file in chapters {
        let chapter_path = chapter_state_key(raw_dir, chapter_file)?;
        if !chapter_output_path(out_dir, chapter_file)?.exists() {
            continue;
        }
        result.push((chapter_path, chapter_file));
    }
    Ok(result)
}

pub(crate) fn build_glossary_rerun_plan(
    chapters: &[PathBuf],
    raw_dir: &Path,
    out_dir: &Path,
    previous_glossary_state: Option<&GlossaryState>,
    previous_chapter_states: &BTreeMap<String, ChapterState>,
    current_glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<GlossaryRerunPlan> {
    let mut plan = GlossaryRerunPlan::default();
    let current_glossary_state = build_glossary_state(current_glossary, injection_mode)?;

    let changed_term_keys = previous_glossary_state
        .map(|glossary| {
            changed_prompt_relevant_keys(&glossary.terms, &current_glossary_state.terms)
        })
        .unwrap_or_default();
    plan.changed_term_count = changed_term_keys.len();

    if previous_glossary_state.is_none() {
        plan.warnings.push(
            "No glossary tracking state found; changed-term counts start after this run."
                .to_string(),
        );
    }

    let mut approximate_smart_checks = 0;
    let output_chapters = chapters_with_output(chapters, raw_dir, out_dir)?;

    for (chapter_path, chapter_file) in &output_chapters {
        if let Some(previous_chapter_state) = previous_chapter_states.get(chapter_path)
            && previous_chapter_state.glossary_usage.is_some()
        {
            if let Some(decision) = exact_rerun_decision(
                chapter_file,
                previous_chapter_state,
                current_glossary,
                injection_mode,
            )? {
                plan.forced_chapters.insert(chapter_path.clone(), decision);
            }
            continue;
        }

        let Some(prev_glossary_state) = previous_glossary_state else {
            continue;
        };

        match prev_glossary_state.injection_mode {
            InjectionMode::Full => {
                if !changed_term_keys.is_empty() {
                    let reason = format!(
                        "Full glossary changed: {}",
                        changed_term_keys
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    plan.forced_chapters.insert(
                        chapter_path.clone(),
                        RerunDecision {
                            reason,
                            is_approximate: false,
                        },
                    );
                }
            }
            InjectionMode::Smart => {
                if let Some(decision) = approximate_smart_rerun_decision(
                    chapter_file,
                    prev_glossary_state,
                    current_glossary,
                    &changed_term_keys,
                )? {
                    approximate_smart_checks += 1;
                    plan.forced_chapters.insert(chapter_path.clone(), decision);
                } else if previous_chapter_states
                    .get(chapter_path)
                    .and_then(|state| state.glossary_usage.as_ref())
                    .is_none()
                {
                    approximate_smart_checks += 1;
                }
            }
        }
    }

    plan.approximate_smart_checks = approximate_smart_checks;

    Ok(plan)
}

pub(crate) fn build_source_rerun_plan(
    chapters: &[PathBuf],
    raw_dir: &Path,
    out_dir: &Path,
    previous_chapter_states: &BTreeMap<String, ChapterState>,
) -> Result<SourceRerunPlan> {
    let mut plan = SourceRerunPlan::default();
    let output_chapters = chapters_with_output(chapters, raw_dir, out_dir)?;

    for (chapter_path, chapter_file) in &output_chapters {
        let Some(previous_chapter_state) = previous_chapter_states.get(chapter_path) else {
            continue;
        };

        let Some(previous_hash) = previous_chapter_state.source_text_hash.as_ref() else {
            plan.untracked_chapters += 1;
            continue;
        };

        let chapter_text = std::fs::read_to_string(chapter_file)
            .map_err(|e| Error::io(format!("Failed to read {}", chapter_file.display()), e))?;
        let current_hash = normalized_source_text_hash(&chapter_text);

        if current_hash != *previous_hash {
            plan.forced_chapters.insert(
                chapter_path.clone(),
                RerunDecision {
                    reason: "Chapter source changed".to_string(),
                    is_approximate: false,
                },
            );
        }
    }

    Ok(plan)
}

pub(crate) fn combine_rerun_decisions(
    glossary_decision: Option<&RerunDecision>,
    source_decision: Option<&RerunDecision>,
) -> Option<RerunDecision> {
    match (glossary_decision, source_decision) {
        (None, None) => None,
        (Some(g), None) => Some(g.clone()),
        (None, Some(s)) => Some(s.clone()),
        (Some(g), Some(s)) => Some(RerunDecision {
            reason: format!("{}; {}", g.reason, s.reason),
            is_approximate: g.is_approximate || s.is_approximate,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary::{glossary_term_prompt_fingerprint, select_terms_for_text};
    use crate::state::{
        ChapterGlossaryTerm, ChapterStatus, RunMetadata, load_glossary_state, save_glossary_state,
    };
    use crate::translate::orchestrate::checkpoint_chapter_progress;
    use crate::translate::rerun::glossary::{build_chapter_glossary_usage, build_glossary_state};
    use crate::translate::rerun::types::snapshot_fingerprints;
    use crate::translate::test_helpers::*;
    use std::collections::BTreeMap;

    fn assert_glossary_state_matches(
        actual: &GlossaryState,
        glossary: &[crate::glossary::GlossaryTerm],
        injection_mode: InjectionMode,
    ) {
        let expected = build_glossary_state(glossary, injection_mode).unwrap();
        assert_eq!(actual.injection_mode, expected.injection_mode);
        assert_eq!(
            snapshot_fingerprints(&actual.terms),
            snapshot_fingerprints(&expected.terms)
        );
    }

    #[test]
    fn test_combine_rerun_decisions_merges_source_and_glossary_reasons() {
        let glossary_decision = RerunDecision {
            reason: "Full glossary changed: hero".to_string(),
            is_approximate: false,
        };
        let source_decision = RerunDecision {
            reason: "Chapter source changed".to_string(),
            is_approximate: false,
        };

        let decision =
            combine_rerun_decisions(Some(&glossary_decision), Some(&source_decision)).unwrap();
        assert_eq!(
            decision.reason,
            "Full glossary changed: hero; Chapter source changed"
        );
    }

    #[test]
    fn test_build_source_rerun_plan_detects_changed_source_hash() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "new source text").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                None,
                vec![],
                Some(normalized_source_text_hash("old source text")),
            ),
        )]);

        let plan =
            build_source_rerun_plan(&[chapter], &raw_dir, &out_dir, &previous_chapter_states)
                .unwrap();

        assert_eq!(plan.forced_chapters.len(), 1);
        assert_eq!(
            plan.forced_chapters
                .get("chapter1.md")
                .map(|d| d.reason.as_str()),
            Some("Chapter source changed")
        );
        assert_eq!(plan.untracked_chapters, 0);
    }

    #[test]
    fn test_build_source_rerun_plan_skips_unchanged_source_hash() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let chapter = raw_dir.join("chapter1.md");
        let chapter_text = "# Chapter 1\n\nSame content\n";
        std::fs::write(&chapter, chapter_text).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                None,
                vec![],
                Some(normalized_source_text_hash(chapter_text)),
            ),
        )]);

        let plan =
            build_source_rerun_plan(&[chapter], &raw_dir, &out_dir, &previous_chapter_states)
                .unwrap();

        assert!(plan.forced_chapters.is_empty());
        assert_eq!(plan.untracked_chapters, 0);
    }

    #[test]
    fn test_build_source_rerun_plan_counts_untracked_chapters() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "source text").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                None,
                vec![],
                None,
            ),
        )]);

        let plan =
            build_source_rerun_plan(&[chapter], &raw_dir, &out_dir, &previous_chapter_states)
                .unwrap();

        assert!(plan.forced_chapters.is_empty());
        assert_eq!(plan.untracked_chapters, 1);
    }

    #[test]
    fn test_build_chapter_glossary_usage_records_smart_fallback_canonically() {
        let glossary = smart_glossary("Hero definition");
        let selection = select_terms_for_text(&glossary, "勇者", InjectionMode::Smart);

        assert!(selection.used_fallback_to_full);

        let usage = build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap();

        assert_eq!(usage.injection_mode, InjectionMode::Smart);
        assert!(usage.used_fallback_to_full);
        assert_eq!(usage.terms.len(), glossary.len());
    }

    #[test]
    fn test_build_glossary_rerun_plan_detects_exact_changed_full_usage() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "hero appears here").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let new_glossary = vec![glossary_term("Hero", Some("hero"), "New definition")];
        let selection =
            select_terms_for_text(&old_glossary, "hero appears here", InjectionMode::Full);

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Full).unwrap();
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Full).unwrap()),
                vec![],
                None,
            ),
        )]);

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &new_glossary,
            InjectionMode::Full,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        assert_eq!(plan.forced_chapters.len(), 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(decision.reason.contains("Glossary term changed"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_detects_exact_changed_smart_usage() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = smart_glossary("Old hero definition");
        let new_glossary = smart_glossary("New hero definition");
        let selection = select_terms_for_text(&old_glossary, smart_text(), InjectionMode::Smart);
        assert!(!selection.used_fallback_to_full);

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                vec![],
                None,
            ),
        )]);

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &new_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(decision.reason.contains("Glossary term changed"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_approximates_untracked_smart_output() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = smart_glossary("Old hero definition");
        let new_glossary = smart_glossary("New hero definition");
        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &BTreeMap::new(),
            &new_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(
            decision
                .reason
                .contains("Glossary selection changed (approx)")
        );
        assert!(plan.warnings.is_empty());
        assert_eq!(plan.approximate_smart_checks, 1);
    }

    #[test]
    fn test_build_glossary_rerun_plan_reruns_untracked_full_output_on_added_term() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "hero appears here").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = vec![glossary_term("Hero", Some("hero"), "Definition")];
        let new_glossary = vec![
            glossary_term("Hero", Some("hero"), "Definition"),
            glossary_term("Mage", Some("mage"), "New term"),
        ];

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Full).unwrap();

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &BTreeMap::new(),
            &new_glossary,
            InjectionMode::Full,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(decision.reason.contains("mage"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_short_circuits_when_glossary_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let glossary = smart_glossary("Hero definition");
        let previous_glossary_state =
            build_glossary_state(&glossary, InjectionMode::Smart).unwrap();

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &BTreeMap::new(),
            &glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 0);
        assert!(plan.forced_chapters.is_empty());
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn test_build_glossary_rerun_plan_ignores_stale_empty_baseline_for_tracked_chapters() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let current_glossary = smart_glossary("Hero definition");
        let selection =
            select_terms_for_text(&current_glossary, smart_text(), InjectionMode::Smart);
        assert!(!selection.used_fallback_to_full);

        let stale_empty_baseline = GlossaryState::new(InjectionMode::Smart, BTreeMap::new());
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                vec![],
                None,
            ),
        )]);

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&stale_empty_baseline),
            &previous_chapter_states,
            &current_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, current_glossary.len());
        assert!(plan.forced_chapters.is_empty());
    }

    #[test]
    fn test_build_glossary_rerun_plan_treats_tracked_smart_fallback_as_full() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "hero appears here").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let new_glossary = vec![glossary_term("Hero", Some("hero"), "New definition")];
        let selection =
            select_terms_for_text(&old_glossary, "hero appears here", InjectionMode::Smart);
        assert!(selection.used_fallback_to_full);

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                vec![],
                None,
            ),
        )]);

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &new_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(decision.reason.contains("Glossary term changed"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_skips_chapter_already_updated_during_partial_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let chapter1 = raw_dir.join("chapter1.md");
        let chapter2 = raw_dir.join("chapter2.md");
        std::fs::write(&chapter1, smart_text()).unwrap();
        std::fs::write(&chapter2, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();
        std::fs::write(out_dir.join("chapter2.md"), "translated").unwrap();

        let old_glossary = smart_glossary("Old hero definition");
        let current_glossary = smart_glossary("New hero definition");
        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();

        let old_selection =
            select_terms_for_text(&old_glossary, smart_text(), InjectionMode::Smart);
        let current_selection =
            select_terms_for_text(&current_glossary, smart_text(), InjectionMode::Smart);
        assert!(!old_selection.used_fallback_to_full);
        assert!(!current_selection.used_fallback_to_full);

        let previous_chapter_states = BTreeMap::from([
            (
                "chapter1.md".to_string(),
                ChapterState::new(
                    "chapter1.md".to_string(),
                    ChapterStatus::Success,
                    None,
                    Some(100),
                    None,
                    Some(
                        build_chapter_glossary_usage(&current_selection, InjectionMode::Smart)
                            .unwrap(),
                    ),
                    vec![],
                    None,
                ),
            ),
            (
                "chapter2.md".to_string(),
                ChapterState::new(
                    "chapter2.md".to_string(),
                    ChapterStatus::Success,
                    None,
                    Some(100),
                    None,
                    Some(
                        build_chapter_glossary_usage(&old_selection, InjectionMode::Smart).unwrap(),
                    ),
                    vec![],
                    None,
                ),
            ),
        ]);

        let plan = build_glossary_rerun_plan(
            &[chapter1, chapter2],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &current_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        assert_eq!(plan.forced_chapters.len(), 1);
        assert!(!plan.forced_chapters.contains_key("chapter1.md"));
        assert!(plan.forced_chapters.contains_key("chapter2.md"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_detects_changed_exported_term() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "hero appears here").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let new_glossary = vec![glossary_term("Hero", Some("hero"), "New definition")];
        let selection =
            select_terms_for_text(&old_glossary, "hero appears here", InjectionMode::Smart);

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();
        let exported_terms = vec![ChapterGlossaryTerm {
            key: "hero".to_string(),
            fingerprint: glossary_term_prompt_fingerprint(&glossary_term(
                "Hero",
                Some("hero"),
                "Old definition",
            ))
            .unwrap(),
        }];
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                exported_terms,
                None,
            ),
        )]);

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &new_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(decision.reason.contains("Glossary term changed"));
        assert!(decision.reason.contains("hero"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_detects_newly_matchable_term() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        let chapter_text = "勇者は魔導士と聖剣を手に王城で戦い竜王と戦った。";
        std::fs::write(&chapter, chapter_text).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Battle", Some("戦い"), "Battle definition"),
        ];

        let new_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Battle", Some("戦い"), "Battle definition"),
            glossary_term("Dragon King", Some("竜王"), "Dragon King definition"),
        ];

        let selection = select_terms_for_text(&old_glossary, chapter_text, InjectionMode::Smart);
        assert!(!selection.used_fallback_to_full, "Selection used fallback");

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                vec![],
                None,
            ),
        )]);

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &new_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        assert_eq!(plan.forced_chapters.len(), 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(decision.reason.contains("竜王") || decision.reason.contains("dragon king"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_detects_removed_previously_matched_term() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        let chapter_text = "勇者は魔導士と聖剣を手に王城で戦い竜王と戦った。";
        std::fs::write(&chapter, chapter_text).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let old_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Battle", Some("戦い"), "Battle definition"),
            glossary_term("Dragon King", Some("竜王"), "Dragon King definition"),
        ];

        let new_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Shield", Some("盾"), "Shield definition"),
        ];

        let selection = select_terms_for_text(&old_glossary, chapter_text, InjectionMode::Smart);
        assert!(!selection.used_fallback_to_full, "Selection used fallback");

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                vec![],
                None,
            ),
        )]);

        let plan = build_glossary_rerun_plan(
            &[chapter],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &new_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 3);
        assert_eq!(plan.forced_chapters.len(), 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(
            decision.reason.contains("竜王") || decision.reason.contains("dragon king"),
            "Expected reason to mention removed term, got: {}",
            decision.reason
        );
    }

    #[test]
    fn test_build_glossary_rerun_plan_with_remaining_chapters_subset() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let chapter1 = raw_dir.join("chapter1.md");
        let chapter2 = raw_dir.join("chapter2.md");
        let chapter3 = raw_dir.join("chapter3.md");
        std::fs::write(&chapter1, smart_text()).unwrap();
        std::fs::write(&chapter2, smart_text()).unwrap();
        std::fs::write(&chapter3, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();
        std::fs::write(out_dir.join("chapter2.md"), "translated").unwrap();
        std::fs::write(out_dir.join("chapter3.md"), "translated").unwrap();

        let old_glossary = smart_glossary("Old hero definition");
        let new_glossary = smart_glossary("New hero definition");

        let previous_glossary_state =
            build_glossary_state(&old_glossary, InjectionMode::Smart).unwrap();

        let selection = select_terms_for_text(&old_glossary, smart_text(), InjectionMode::Smart);
        assert!(!selection.used_fallback_to_full);

        let previous_chapter_states = BTreeMap::from([
            (
                "chapter1.md".to_string(),
                ChapterState::new(
                    "chapter1.md".to_string(),
                    ChapterStatus::Success,
                    None,
                    Some(100),
                    None,
                    Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                    vec![],
                    None,
                ),
            ),
            (
                "chapter2.md".to_string(),
                ChapterState::new(
                    "chapter2.md".to_string(),
                    ChapterStatus::Success,
                    None,
                    Some(100),
                    None,
                    Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                    vec![],
                    None,
                ),
            ),
            (
                "chapter3.md".to_string(),
                ChapterState::new(
                    "chapter3.md".to_string(),
                    ChapterStatus::Success,
                    None,
                    Some(100),
                    None,
                    Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                    vec![],
                    None,
                ),
            ),
        ]);

        let plan = build_glossary_rerun_plan(
            &[chapter2, chapter3],
            &raw_dir,
            &out_dir,
            Some(&previous_glossary_state),
            &previous_chapter_states,
            &new_glossary,
            InjectionMode::Smart,
        )
        .unwrap();

        assert_eq!(plan.changed_term_count, 1);
        assert_eq!(plan.forced_chapters.len(), 2);
        assert!(plan.forced_chapters.contains_key("chapter2.md"));
        assert!(plan.forced_chapters.contains_key("chapter3.md"));
    }

    #[test]
    fn test_checkpoint_chapter_progress_does_not_advance_glossary_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let previous_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let previous_state =
            build_glossary_state(&previous_glossary, InjectionMode::Smart).unwrap();
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let chapter_state = ChapterState::new(
            "chapter1.md".to_string(),
            ChapterStatus::Skipped,
            None,
            None,
            None,
            None,
            vec![],
            None,
        );
        let mut run_metadata = RunMetadata::new(
            "default".to_string(),
            "openai".to_string(),
            "gpt-test".to_string(),
            None,
        );

        checkpoint_chapter_progress(dir.path(), &mut run_metadata, &chapter_state).unwrap();

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &previous_glossary, InjectionMode::Smart);
    }
}
