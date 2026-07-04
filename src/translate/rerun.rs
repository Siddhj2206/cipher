use crate::book::paths::{chapter_output_path, chapter_state_key};
use crate::glossary::{
    GlossaryTerm, InjectionMode, glossary_term_key, glossary_term_prompt_fingerprint,
    select_terms_for_text,
};
use crate::state::{
    ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterState, GlossaryState, GlossaryStateTerm,
    normalized_source_text_hash, save_chapter_state, save_glossary_state,
};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) const EMPTY_CHAPTER_SKIP_REASON: &str = "Chapter is empty";
pub(crate) const OUTPUT_EXISTS_SKIP_REASON: &str = "Output exists and no rerun reason matched";
pub(crate) const OUTPUT_MISSING_REASON: &str = "No output exists yet";

#[derive(Debug, Clone)]
pub(crate) struct GlossaryRerunDecision {
    pub reason: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ChapterRerunDecision {
    pub reason: String,
}

#[derive(Debug, Default)]
pub(crate) struct GlossaryRerunPlan {
    pub forced_chapters: BTreeMap<String, GlossaryRerunDecision>,
    pub warnings: Vec<String>,
    pub changed_term_count: usize,
    pub approximate_smart_checks: usize,
}

#[derive(Debug, Default)]
pub(crate) struct SourceRerunPlan {
    pub forced_chapters: BTreeMap<String, String>,
    pub untracked_chapters: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlossaryBaselineAdvance {
    KeepExisting,
    InitializeFromRunStart,
    CommitRunEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GlossaryBaselineOutcome {
    pub advance: GlossaryBaselineAdvance,
    pub remaining_forced_chapters: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegacyTrackingMigration {
    pub migrated_chapters: usize,
    pub migrated_glossary_baseline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewAction {
    Translate,
    Retranslate,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChapterPreview {
    pub chapter_path: String,
    pub action: PreviewAction,
    pub reason: String,
    pub approximate: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewSummary {
    pub translate: usize,
    pub retranslate: usize,
    pub skip: usize,
    pub approximate_reruns: usize,
    pub exact_reruns: usize,
    pub empty_skips: usize,
    pub output_exists_skips: usize,
    pub output_missing: usize,
}

impl GlossaryRerunPlan {
    pub fn decision_for(&self, filename: &str) -> Option<&GlossaryRerunDecision> {
        self.forced_chapters.get(filename)
    }
}

impl SourceRerunPlan {
    pub fn decision_for(&self, filename: &str) -> Option<&String> {
        self.forced_chapters.get(filename)
    }
}

pub(crate) fn build_glossary_state(
    glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> GlossaryState {
    GlossaryState::new(
        injection_mode,
        glossary
            .iter()
            .map(|term| {
                (
                    glossary_term_key(term),
                    GlossaryStateTerm {
                        term: term.term.clone(),
                        og_term: term.og_term.clone(),
                        definition: term.definition.clone(),
                        fingerprint: glossary_term_prompt_fingerprint(term),
                    },
                )
            })
            .collect(),
    )
}

pub(crate) fn build_chapter_glossary_usage(
    selection: &crate::glossary::SelectionResult,
    injection_mode: InjectionMode,
) -> ChapterGlossaryUsage {
    ChapterGlossaryUsage {
        injection_mode,
        used_fallback_to_full: selection.used_fallback_to_full,
        terms: selection
            .terms
            .iter()
            .map(|term| ChapterGlossaryTerm {
                key: glossary_term_key(term),
                fingerprint: glossary_term_prompt_fingerprint(term),
            })
            .collect(),
    }
}

pub(crate) fn chapter_translation_injection_mode(
    injection_mode: InjectionMode,
    _rerun_decision: Option<&ChapterRerunDecision>,
) -> InjectionMode {
    injection_mode
}

pub(crate) fn selection_fingerprints(terms: &[GlossaryTerm]) -> BTreeMap<String, String> {
    terms
        .iter()
        .map(|term| {
            (
                glossary_term_key(term),
                glossary_term_prompt_fingerprint(term),
            )
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

pub(crate) fn tracked_usage_state_label(usage: &ChapterGlossaryUsage) -> &'static str {
    if usage.injection_mode == InjectionMode::Full {
        "legacy full tracking"
    } else if usage.used_fallback_to_full {
        "fallback to full"
    } else {
        "smart selection only"
    }
}

pub(crate) fn changed_prompt_relevant_keys(
    previous_terms: &BTreeMap<String, GlossaryStateTerm>,
    current_terms: &BTreeMap<String, GlossaryStateTerm>,
) -> BTreeSet<String> {
    let all_keys: BTreeSet<String> = previous_terms
        .keys()
        .chain(current_terms.keys())
        .cloned()
        .collect();

    all_keys
        .into_iter()
        .filter(|key| {
            let previous = previous_terms
                .get(key)
                .map(|term| term.fingerprint.as_str());
            let current = current_terms.get(key).map(|term| term.fingerprint.as_str());
            previous != current
        })
        .collect()
}

pub(crate) fn changed_selected_term_keys(
    previous_terms: &BTreeMap<String, String>,
    current_terms: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    let all_keys: BTreeSet<String> = previous_terms
        .keys()
        .chain(current_terms.keys())
        .cloned()
        .collect();

    all_keys
        .into_iter()
        .filter(|key| previous_terms.get(key) != current_terms.get(key))
        .collect()
}

pub(crate) fn full_glossary_rerun_reason(changed_term_keys: &BTreeSet<String>) -> Option<String> {
    if changed_term_keys.is_empty() {
        None
    } else {
        Some(format!(
            "Full glossary changed: {}",
            changed_term_keys
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

pub(crate) fn current_expected_glossary_usage(
    raw_path: &Path,
    current_glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<Option<ChapterGlossaryUsage>> {
    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;
    if chapter_text.trim().is_empty() {
        return Ok(None);
    }

    let selection = select_terms_for_text(current_glossary, &chapter_text, injection_mode);
    Ok(Some(build_chapter_glossary_usage(
        &selection,
        injection_mode,
    )))
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

    let current_fingerprints: BTreeMap<String, String> = current_glossary
        .iter()
        .map(|term| {
            (
                glossary_term_key(term),
                glossary_term_prompt_fingerprint(term),
            )
        })
        .collect();

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

pub(crate) fn exact_rerun_decision(
    raw_path: &Path,
    chapter_state: &ChapterState,
    current_glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
) -> Result<Option<GlossaryRerunDecision>> {
    let Some(usage) = &chapter_state.glossary_usage else {
        return Ok(None);
    };

    let current_fingerprints: BTreeMap<String, String> = current_glossary
        .iter()
        .map(|term| {
            (
                glossary_term_key(term),
                glossary_term_prompt_fingerprint(term),
            )
        })
        .collect();

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
        return Ok(Some(GlossaryRerunDecision {
            reason: format!(
                "Imported or exported glossary term changed: {}",
                fingerprint_changed_keys.join(", ")
            ),
        }));
    }

    let Some(expected_usage) =
        current_expected_glossary_usage(raw_path, current_glossary, injection_mode)?
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

        return Ok(Some(GlossaryRerunDecision {
            reason: format!(
                "Smart glossary selection changed fallback behavior: {} -> {}",
                tracked_usage_state_label(usage),
                tracked_usage_state_label(&expected_usage)
            ),
        }));
    }

    if usage.injection_mode == InjectionMode::Full
        || usage.used_fallback_to_full != expected_usage.used_fallback_to_full
    {
        return Ok(Some(GlossaryRerunDecision {
            reason: format!(
                "Smart glossary selection changed fallback behavior: {} -> {}",
                tracked_usage_state_label(usage),
                tracked_usage_state_label(&expected_usage)
            ),
        }));
    }

    Ok(Some(GlossaryRerunDecision {
        reason: format!(
            "Smart glossary selection changed: {}",
            selection_changed_keys
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }))
}

pub(crate) fn approximate_smart_rerun_decision(
    raw_path: &Path,
    previous_glossary_state: &GlossaryState,
    current_glossary: &[GlossaryTerm],
    changed_term_keys: &BTreeSet<String>,
) -> Result<Option<GlossaryRerunDecision>> {
    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;

    if chapter_text.trim().is_empty() {
        return Ok(None);
    }

    let previous_glossary = glossary_terms_from_state(previous_glossary_state);
    let previous_selection =
        select_terms_for_text(&previous_glossary, &chapter_text, InjectionMode::Smart);
    let current_selection =
        select_terms_for_text(current_glossary, &chapter_text, InjectionMode::Smart);

    if previous_selection.used_fallback_to_full || current_selection.used_fallback_to_full {
        return Ok(full_glossary_rerun_reason(changed_term_keys).map(|reason| {
            GlossaryRerunDecision {
                reason: format!("Approximate rerun after smart fallback matched: {}", reason),
            }
        }));
    }

    let previous_terms = selection_fingerprints(&previous_selection.terms);
    let current_terms = selection_fingerprints(&current_selection.terms);
    let changed_keys = changed_selected_term_keys(&previous_terms, &current_terms);

    if changed_keys.is_empty() {
        Ok(None)
    } else {
        Ok(Some(GlossaryRerunDecision {
            reason: format!(
                "Approximate smart glossary selection changed: {}",
                changed_keys.into_iter().collect::<Vec<_>>().join(", ")
            ),
        }))
    }
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
    let current_glossary_state = build_glossary_state(current_glossary, injection_mode);

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

    for chapter_file in chapters {
        let chapter_path = chapter_state_key(raw_dir, chapter_file)?;
        let output_exists = chapter_output_path(out_dir, chapter_file)?.exists();

        if !output_exists {
            continue;
        }

        if let Some(previous_chapter_state) = previous_chapter_states.get(&chapter_path) {
            if previous_chapter_state.glossary_usage.is_some() {
                if let Some(decision) = exact_rerun_decision(
                    chapter_file,
                    previous_chapter_state,
                    current_glossary,
                    injection_mode,
                )? {
                    plan.forced_chapters.insert(chapter_path, decision);
                }
                continue;
            }
        }

        let Some(previous_glossary_state) = previous_glossary_state else {
            continue;
        };

        match previous_glossary_state.injection_mode {
            InjectionMode::Full => {
                if let Some(reason) = full_glossary_rerun_reason(&changed_term_keys) {
                    plan.forced_chapters
                        .insert(chapter_path, GlossaryRerunDecision { reason });
                }
            }
            InjectionMode::Smart => {
                if let Some(decision) = approximate_smart_rerun_decision(
                    chapter_file,
                    previous_glossary_state,
                    current_glossary,
                    &changed_term_keys,
                )? {
                    approximate_smart_checks += 1;
                    plan.forced_chapters.insert(chapter_path, decision);
                } else if previous_chapter_states
                    .get(&chapter_path)
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

    for chapter_file in chapters {
        let chapter_path = chapter_state_key(raw_dir, chapter_file)?;
        let output_exists = chapter_output_path(out_dir, chapter_file)?.exists();

        if !output_exists {
            continue;
        }

        let Some(previous_chapter_state) = previous_chapter_states.get(&chapter_path) else {
            continue;
        };

        let Some(previous_hash) = previous_chapter_state.source_text_hash.as_ref() else {
            plan.untracked_chapters += 1;
            continue;
        };

        let chapter_text = std::fs::read_to_string(chapter_file)
            .with_context(|| format!("Failed to read {}", chapter_file.display()))?;
        let current_hash = normalized_source_text_hash(&chapter_text);

        if current_hash != *previous_hash {
            plan.forced_chapters
                .insert(chapter_path, "Chapter source changed".to_string());
        }
    }

    Ok(plan)
}

pub(crate) fn combine_rerun_decisions(
    glossary_decision: Option<&GlossaryRerunDecision>,
    source_reason: Option<&String>,
) -> Option<ChapterRerunDecision> {
    match (glossary_decision, source_reason) {
        (None, None) => None,
        (Some(glossary_decision), None) => Some(ChapterRerunDecision {
            reason: glossary_decision.reason.clone(),
        }),
        (None, Some(source_reason)) => Some(ChapterRerunDecision {
            reason: source_reason.clone(),
        }),
        (Some(glossary_decision), Some(source_reason)) => Some(ChapterRerunDecision {
            reason: format!("{}; {}", source_reason, glossary_decision.reason),
        }),
    }
}

pub(crate) fn finalize_glossary_baseline(
    book_dir: &Path,
    rerun_glossary_enabled: bool,
    previous_glossary_state: Option<&GlossaryState>,
    run_start_glossary_state: &GlossaryState,
    chapters: &[PathBuf],
    raw_dir: &Path,
    out_dir: &Path,
    chapter_states: &BTreeMap<String, ChapterState>,
    glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
    failed: usize,
) -> Result<GlossaryBaselineOutcome> {
    if failed > 0 {
        return Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::KeepExisting,
            remaining_forced_chapters: 0,
        });
    }

    if previous_glossary_state.is_none() {
        save_glossary_state(book_dir, run_start_glossary_state)?;
        return Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::InitializeFromRunStart,
            remaining_forced_chapters: 0,
        });
    }

    if !rerun_glossary_enabled {
        return Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::KeepExisting,
            remaining_forced_chapters: 0,
        });
    }

    let current_glossary_state = build_glossary_state(glossary, injection_mode);
    let previous_glossary_state = previous_glossary_state.expect("checked above");

    if previous_glossary_state.injection_mode == current_glossary_state.injection_mode
        && changed_prompt_relevant_keys(
            &previous_glossary_state.terms,
            &current_glossary_state.terms,
        )
        .is_empty()
    {
        return Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::KeepExisting,
            remaining_forced_chapters: 0,
        });
    }

    let remaining_forced_chapters = count_chapters_still_stale_for_current_glossary(
        chapters,
        raw_dir,
        out_dir,
        chapter_states,
        glossary,
        injection_mode,
    )?;

    if remaining_forced_chapters == 0 {
        save_glossary_state(book_dir, &current_glossary_state)?;
        Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::CommitRunEnd,
            remaining_forced_chapters: 0,
        })
    } else {
        Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::KeepExisting,
            remaining_forced_chapters,
        })
    }
}

pub(crate) fn migrate_legacy_full_tracking(
    book_dir: &Path,
    previous_glossary_state: Option<&GlossaryState>,
    baseline_outcome: GlossaryBaselineOutcome,
    chapters: &[PathBuf],
    raw_dir: &Path,
    out_dir: &Path,
    chapter_states: &mut BTreeMap<String, ChapterState>,
    glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
    failed: usize,
) -> Result<LegacyTrackingMigration> {
    if failed > 0 {
        return Ok(LegacyTrackingMigration::default());
    }

    let mut migration = LegacyTrackingMigration::default();
    let mut all_output_chapters_tracked = true;

    for chapter_file in chapters {
        let chapter_path = chapter_state_key(raw_dir, chapter_file)?;
        if !chapter_output_path(out_dir, chapter_file)?.exists() {
            continue;
        }

        let Some(chapter_state) = chapter_states.get(&chapter_path).cloned() else {
            all_output_chapters_tracked = false;
            continue;
        };

        let Some(usage) = &chapter_state.glossary_usage else {
            all_output_chapters_tracked = false;
            continue;
        };

        if usage.injection_mode != InjectionMode::Full {
            continue;
        }

        let Some(migrated_usage) =
            migrated_legacy_full_usage(chapter_file, &chapter_state, glossary)?
        else {
            continue;
        };

        let mut migrated_state = chapter_state.clone();
        migrated_state.glossary_usage = Some(migrated_usage);
        save_chapter_state(book_dir, &migrated_state)?;
        chapter_states.insert(chapter_path, migrated_state);
        migration.migrated_chapters += 1;
    }

    let Some(previous_glossary_state) = previous_glossary_state else {
        return Ok(migration);
    };

    if previous_glossary_state.injection_mode != InjectionMode::Full || !all_output_chapters_tracked
    {
        return Ok(migration);
    }

    let current_glossary_state = build_glossary_state(glossary, injection_mode);
    let migrated_glossary_state = match baseline_outcome.advance {
        GlossaryBaselineAdvance::CommitRunEnd => Some(current_glossary_state),
        GlossaryBaselineAdvance::KeepExisting
            if changed_prompt_relevant_keys(
                &previous_glossary_state.terms,
                &current_glossary_state.terms,
            )
            .is_empty() =>
        {
            Some(GlossaryState::new(
                InjectionMode::Smart,
                previous_glossary_state.terms.clone(),
            ))
        }
        _ => None,
    };

    if let Some(glossary_state) = migrated_glossary_state {
        save_glossary_state(book_dir, &glossary_state)?;
        migration.migrated_glossary_baseline = true;
    }

    Ok(migration)
}

pub(crate) fn migrated_legacy_full_usage(
    raw_path: &Path,
    chapter_state: &ChapterState,
    current_glossary: &[GlossaryTerm],
) -> Result<Option<ChapterGlossaryUsage>> {
    let Some(usage) = &chapter_state.glossary_usage else {
        return Ok(None);
    };

    if usage.injection_mode != InjectionMode::Full {
        return Ok(None);
    }

    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;
    if chapter_text.trim().is_empty() {
        return Ok(None);
    }

    let selection = select_terms_for_text(current_glossary, &chapter_text, InjectionMode::Smart);
    if !selection.used_fallback_to_full {
        return Ok(None);
    }

    let migrated_usage = build_chapter_glossary_usage(&selection, InjectionMode::Smart);
    let tracked_usage: BTreeMap<String, String> = usage
        .terms
        .iter()
        .map(|term| (term.key.clone(), term.fingerprint.clone()))
        .collect();
    let migrated_tracked_usage: BTreeMap<String, String> = migrated_usage
        .terms
        .iter()
        .map(|term| (term.key.clone(), term.fingerprint.clone()))
        .collect();

    if tracked_usage != migrated_tracked_usage {
        return Ok(None);
    }

    Ok(Some(migrated_usage))
}

#[cfg(test)]
pub(crate) fn snapshot_fingerprints(
    terms: &BTreeMap<String, GlossaryStateTerm>,
) -> BTreeMap<String, String> {
    terms
        .iter()
        .map(|(key, term)| (key.clone(), term.fingerprint.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ChapterStatus, RunMetadata, load_chapter_state, load_glossary_state};
    use crate::translate::cmd::checkpoint_chapter_progress;
    use crate::translate::test_helpers::*;
    use std::collections::BTreeMap;

    fn assert_glossary_state_matches(
        actual: &GlossaryState,
        glossary: &[crate::glossary::GlossaryTerm],
        injection_mode: InjectionMode,
    ) {
        let expected = build_glossary_state(glossary, injection_mode);
        assert_eq!(actual.injection_mode, expected.injection_mode);
        assert_eq!(
            snapshot_fingerprints(&actual.terms),
            snapshot_fingerprints(&expected.terms)
        );
    }

    #[test]
    fn test_checkpoint_chapter_progress_does_not_advance_glossary_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let previous_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let previous_state = build_glossary_state(&previous_glossary, InjectionMode::Smart);
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

    #[test]
    fn test_finalize_glossary_baseline_initializes_from_run_start_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let run_start_glossary = vec![glossary_term("Hero", Some("hero"), "Definition")];
        let current_glossary = vec![
            glossary_term("Hero", Some("hero"), "Definition"),
            glossary_term("Mage", Some("mage"), "Added later"),
        ];
        let run_start_state = build_glossary_state(&run_start_glossary, InjectionMode::Smart);

        let outcome = finalize_glossary_baseline(
            dir.path(),
            false,
            None,
            &run_start_state,
            &[],
            &raw_dir,
            &out_dir,
            &BTreeMap::new(),
            &current_glossary,
            InjectionMode::Smart,
            0,
        )
        .unwrap();

        assert_eq!(
            outcome,
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::InitializeFromRunStart,
                remaining_forced_chapters: 0,
            }
        );

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &run_start_glossary, InjectionMode::Smart);
    }

    #[test]
    fn test_finalize_glossary_baseline_keeps_existing_when_reruns_remain() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "hero appears here").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let previous_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let current_glossary = vec![glossary_term("Hero", Some("hero"), "New definition")];
        let previous_state = build_glossary_state(&previous_glossary, InjectionMode::Full);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Full),
            &[chapter],
            &raw_dir,
            &out_dir,
            &BTreeMap::new(),
            &current_glossary,
            InjectionMode::Full,
            0,
        )
        .unwrap();

        assert_eq!(
            outcome,
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::KeepExisting,
                remaining_forced_chapters: 1,
            }
        );

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &previous_glossary, InjectionMode::Full);
    }

    #[test]
    fn test_finalize_glossary_baseline_commits_run_end_when_reruns_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        // Use the same glossary for previous and current to simulate a completed run
        // where glossary hasn't changed since last translation
        let current_glossary = smart_glossary("Hero definition");
        let previous_state = build_glossary_state(&current_glossary, InjectionMode::Smart);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let selection =
            select_terms_for_text(&current_glossary, smart_text(), InjectionMode::Smart);
        assert!(!selection.used_fallback_to_full);

        let chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
                vec![],
                None,
            ),
        )]);

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart),
            &[chapter],
            &raw_dir,
            &out_dir,
            &chapter_states,
            &current_glossary,
            InjectionMode::Smart,
            0,
        )
        .unwrap();

        // When glossary hasn't changed, KeepExisting is returned (glossary already up to date)
        assert_eq!(
            outcome,
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::KeepExisting,
                remaining_forced_chapters: 0,
            }
        );

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &current_glossary, InjectionMode::Smart);
    }

    #[test]
    fn test_finalize_glossary_baseline_commits_after_rerun_from_stale_empty_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter1 = raw_dir.join("chapter1.md");
        let chapter2 = raw_dir.join("chapter2.md");
        std::fs::write(&chapter1, smart_text()).unwrap();
        std::fs::write(&chapter2, "勇者").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();
        std::fs::write(out_dir.join("chapter2.md"), "translated").unwrap();

        let current_glossary = smart_glossary("Hero definition");
        let previous_state = build_glossary_state(&[], InjectionMode::Smart);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let smart_selection =
            select_terms_for_text(&current_glossary, smart_text(), InjectionMode::Smart);
        let fallback_selection =
            select_terms_for_text(&current_glossary, "勇者", InjectionMode::Smart);
        assert!(fallback_selection.used_fallback_to_full);

        let chapter_states = BTreeMap::from([
            (
                "chapter1.md".to_string(),
                ChapterState::new(
                    "chapter1.md".to_string(),
                    ChapterStatus::Success,
                    None,
                    Some(100),
                    None,
                    Some(build_chapter_glossary_usage(
                        &smart_selection,
                        InjectionMode::Smart,
                    )),
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
                    Some(build_chapter_glossary_usage(
                        &fallback_selection,
                        InjectionMode::Smart,
                    )),
                    vec![],
                    None,
                ),
            ),
        ]);

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart),
            &[chapter1, chapter2],
            &raw_dir,
            &out_dir,
            &chapter_states,
            &current_glossary,
            InjectionMode::Smart,
            0,
        )
        .unwrap();

        assert_eq!(
            outcome,
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::CommitRunEnd,
                remaining_forced_chapters: 0,
            }
        );

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &current_glossary, InjectionMode::Smart);
    }

    #[test]
    fn test_migrate_legacy_full_tracking_rewrites_equivalent_fallback_state() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, "hero appears here").unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let glossary = vec![glossary_term("Hero", Some("hero"), "Definition")];
        let previous_glossary_state = build_glossary_state(&glossary, InjectionMode::Full);
        save_glossary_state(dir.path(), &previous_glossary_state).unwrap();

        let legacy_selection =
            select_terms_for_text(&glossary, "hero appears here", InjectionMode::Full);
        let legacy_chapter_state = ChapterState::new(
            "chapter1.md".to_string(),
            ChapterStatus::Success,
            None,
            Some(100),
            None,
            Some(build_chapter_glossary_usage(
                &legacy_selection,
                InjectionMode::Full,
            )),
            vec![],
            Some(normalized_source_text_hash("hero appears here")),
        );
        save_chapter_state(dir.path(), &legacy_chapter_state).unwrap();

        let mut chapter_states =
            BTreeMap::from([("chapter1.md".to_string(), legacy_chapter_state.clone())]);

        let migration = migrate_legacy_full_tracking(
            dir.path(),
            Some(&previous_glossary_state),
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::KeepExisting,
                remaining_forced_chapters: 0,
            },
            std::slice::from_ref(&chapter),
            &raw_dir,
            &out_dir,
            &mut chapter_states,
            &glossary,
            InjectionMode::Smart,
            0,
        )
        .unwrap();

        assert_eq!(
            migration,
            LegacyTrackingMigration {
                migrated_chapters: 1,
                migrated_glossary_baseline: true,
            }
        );

        let migrated_usage = chapter_states["chapter1.md"]
            .glossary_usage
            .as_ref()
            .unwrap();
        assert_eq!(migrated_usage.injection_mode, InjectionMode::Smart);
        assert!(migrated_usage.used_fallback_to_full);

        let loaded_chapter = load_chapter_state(dir.path(), "chapter1.md")
            .unwrap()
            .unwrap();
        let loaded_usage = loaded_chapter.glossary_usage.unwrap();
        assert_eq!(loaded_usage.injection_mode, InjectionMode::Smart);
        assert!(loaded_usage.used_fallback_to_full);

        let loaded_glossary_state = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_eq!(loaded_glossary_state.injection_mode, InjectionMode::Smart);
        assert_eq!(
            snapshot_fingerprints(&loaded_glossary_state.terms),
            snapshot_fingerprints(&previous_glossary_state.terms)
        );
    }

    #[test]
    fn test_migrate_legacy_full_tracking_skips_non_fallback_legacy_chapter() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let chapter = raw_dir.join("chapter1.md");
        std::fs::write(&chapter, smart_text()).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        let glossary = smart_glossary("Hero definition");
        let previous_glossary_state = build_glossary_state(&glossary, InjectionMode::Full);
        save_glossary_state(dir.path(), &previous_glossary_state).unwrap();

        let legacy_selection = select_terms_for_text(&glossary, smart_text(), InjectionMode::Full);
        let legacy_chapter_state = ChapterState::new(
            "chapter1.md".to_string(),
            ChapterStatus::Success,
            None,
            Some(100),
            None,
            Some(build_chapter_glossary_usage(
                &legacy_selection,
                InjectionMode::Full,
            )),
            vec![],
            Some(normalized_source_text_hash(smart_text())),
        );
        save_chapter_state(dir.path(), &legacy_chapter_state).unwrap();

        let mut chapter_states =
            BTreeMap::from([("chapter1.md".to_string(), legacy_chapter_state.clone())]);

        let migration = migrate_legacy_full_tracking(
            dir.path(),
            Some(&previous_glossary_state),
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::KeepExisting,
                remaining_forced_chapters: 0,
            },
            std::slice::from_ref(&chapter),
            &raw_dir,
            &out_dir,
            &mut chapter_states,
            &glossary,
            InjectionMode::Smart,
            0,
        )
        .unwrap();

        assert_eq!(
            migration,
            LegacyTrackingMigration {
                migrated_chapters: 0,
                migrated_glossary_baseline: true,
            }
        );

        let migrated_usage = chapter_states["chapter1.md"]
            .glossary_usage
            .as_ref()
            .unwrap();
        assert_eq!(migrated_usage.injection_mode, InjectionMode::Full);
        assert!(!migrated_usage.used_fallback_to_full);
    }

    #[test]
    fn test_finalize_glossary_baseline_keeps_existing_after_failures() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let previous_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let current_glossary = vec![glossary_term("Hero", Some("hero"), "New definition")];
        let previous_state = build_glossary_state(&previous_glossary, InjectionMode::Smart);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart),
            &[],
            &raw_dir,
            &out_dir,
            &BTreeMap::new(),
            &current_glossary,
            InjectionMode::Smart,
            1,
        )
        .unwrap();

        assert_eq!(
            outcome,
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::KeepExisting,
                remaining_forced_chapters: 0,
            }
        );

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &previous_glossary, InjectionMode::Smart);
    }

    #[test]
    fn test_finalize_glossary_baseline_keeps_existing_for_normal_translate_runs() {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let previous_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let current_glossary = vec![glossary_term("Hero", Some("hero"), "New definition")];
        let previous_state = build_glossary_state(&previous_glossary, InjectionMode::Smart);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            false,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart),
            &[],
            &raw_dir,
            &out_dir,
            &BTreeMap::new(),
            &current_glossary,
            InjectionMode::Smart,
            0,
        )
        .unwrap();

        assert_eq!(
            outcome,
            GlossaryBaselineOutcome {
                advance: GlossaryBaselineAdvance::KeepExisting,
                remaining_forced_chapters: 0,
            }
        );

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &previous_glossary, InjectionMode::Smart);
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
            plan.forced_chapters.get("chapter1.md").map(String::as_str),
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
    fn test_combine_rerun_decisions_merges_source_and_glossary_reasons() {
        let glossary_decision = GlossaryRerunDecision {
            reason: "Full glossary changed: hero".to_string(),
        };
        let source_reason = "Chapter source changed".to_string();

        let decision =
            combine_rerun_decisions(Some(&glossary_decision), Some(&source_reason)).unwrap();
        assert_eq!(
            decision.reason,
            "Chapter source changed; Full glossary changed: hero"
        );
    }

    #[test]
    fn test_chapter_translation_injection_mode_keeps_smart_on_full_rerun_reason() {
        let rerun_decision = ChapterRerunDecision {
            reason: "Full glossary changed: hero".to_string(),
        };

        assert_eq!(
            chapter_translation_injection_mode(InjectionMode::Smart, Some(&rerun_decision)),
            InjectionMode::Smart
        );
    }

    #[test]
    fn test_build_chapter_glossary_usage_records_smart_fallback_canonically() {
        let glossary = smart_glossary("Hero definition");
        let selection = select_terms_for_text(&glossary, "勇者", InjectionMode::Smart);

        assert!(selection.used_fallback_to_full);

        let usage = build_chapter_glossary_usage(&selection, InjectionMode::Smart);

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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Full);
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Full,
                )),
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
        assert!(
            decision
                .reason
                .contains("Imported or exported glossary term changed")
        );
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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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
        assert!(
            decision
                .reason
                .contains("Imported or exported glossary term changed")
        );
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
        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);

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
                .contains("Approximate smart glossary selection changed")
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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Full);

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
        let previous_glossary_state = build_glossary_state(&glossary, InjectionMode::Smart);

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
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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
        assert!(
            decision
                .reason
                .contains("Imported or exported glossary term changed")
        );
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
        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);

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
                    Some(build_chapter_glossary_usage(
                        &current_selection,
                        InjectionMode::Smart,
                    )),
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
                    Some(build_chapter_glossary_usage(
                        &old_selection,
                        InjectionMode::Smart,
                    )),
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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);
        let exported_terms = vec![ChapterGlossaryTerm {
            key: "hero".to_string(),
            fingerprint: glossary_term_prompt_fingerprint(&glossary_term(
                "Hero",
                Some("hero"),
                "Old definition",
            )),
        }];
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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
        assert!(
            decision
                .reason
                .contains("Imported or exported glossary term changed")
        );
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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);
        let previous_chapter_states = BTreeMap::from([(
            "chapter1.md".to_string(),
            ChapterState::new(
                "chapter1.md".to_string(),
                ChapterStatus::Success,
                None,
                Some(100),
                None,
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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

        let previous_glossary_state = build_glossary_state(&old_glossary, InjectionMode::Smart);

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
                    Some(build_chapter_glossary_usage(
                        &selection,
                        InjectionMode::Smart,
                    )),
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
                    Some(build_chapter_glossary_usage(
                        &selection,
                        InjectionMode::Smart,
                    )),
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
                    Some(build_chapter_glossary_usage(
                        &selection,
                        InjectionMode::Smart,
                    )),
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
}
