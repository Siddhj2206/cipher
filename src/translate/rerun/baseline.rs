use crate::book::paths::{chapter_output_path, chapter_state_key};
use crate::error::{Error, Result};
use crate::glossary::{GlossaryTerm, InjectionMode, select_terms_for_text};
use crate::state::{
    ChapterGlossaryUsage, ChapterState, GlossaryState, save_chapter_state, save_glossary_state,
};
use crate::translate::rerun::glossary::{
    build_chapter_glossary_usage, build_glossary_state, changed_prompt_relevant_keys,
    count_chapters_still_stale_for_current_glossary,
};
use crate::translate::rerun::types::{
    BaselineAction, GlossaryBaselineOutcome, LegacyTrackingMigration,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[allow(clippy::too_many_arguments)]
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
            action: BaselineAction::KeepExisting,
            remaining_forced_chapters: 0,
        });
    }

    if previous_glossary_state.is_none() {
        save_glossary_state(book_dir, run_start_glossary_state)?;
        return Ok(GlossaryBaselineOutcome {
            action: BaselineAction::InitializeFromRunStart,
            remaining_forced_chapters: 0,
        });
    }

    if !rerun_glossary_enabled {
        return Ok(GlossaryBaselineOutcome {
            action: BaselineAction::KeepExisting,
            remaining_forced_chapters: 0,
        });
    }

    let current_glossary_state = build_glossary_state(glossary, injection_mode)?;
    let previous_glossary_state = previous_glossary_state.expect("checked above");

    if previous_glossary_state.injection_mode == current_glossary_state.injection_mode
        && changed_prompt_relevant_keys(
            &previous_glossary_state.terms,
            &current_glossary_state.terms,
        )
        .is_empty()
    {
        return Ok(GlossaryBaselineOutcome {
            action: BaselineAction::KeepExisting,
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
            action: BaselineAction::CommitRunEnd,
            remaining_forced_chapters: 0,
        })
    } else {
        Ok(GlossaryBaselineOutcome {
            action: BaselineAction::KeepExisting,
            remaining_forced_chapters,
        })
    }
}

#[allow(clippy::too_many_arguments)]
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

    let current_glossary_state = build_glossary_state(glossary, injection_mode)?;
    let migrated_glossary_state = match baseline_outcome.action {
        BaselineAction::CommitRunEnd => Some(current_glossary_state),
        BaselineAction::KeepExisting
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
        .map_err(|e| Error::io(format!("Failed to read {}", raw_path.display()), e))?;
    if chapter_text.trim().is_empty() {
        return Ok(None);
    }

    let selection = select_terms_for_text(current_glossary, &chapter_text, InjectionMode::Smart);
    if !selection.used_fallback_to_full {
        return Ok(None);
    }

    let migrated_usage = build_chapter_glossary_usage(&selection, InjectionMode::Smart)?;
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
mod tests {
    use super::*;
    use crate::glossary::GlossaryTerm;
    use crate::state::{
        ChapterStatus, load_chapter_state, load_glossary_state, normalized_source_text_hash,
        save_glossary_state,
    };
    use crate::translate::rerun::glossary::build_glossary_state;
    use crate::translate::rerun::types::snapshot_fingerprints;
    use crate::translate::test_helpers::*;
    use std::collections::BTreeMap;

    fn assert_glossary_state_matches(
        actual: &GlossaryState,
        glossary: &[GlossaryTerm],
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
        let run_start_state =
            build_glossary_state(&run_start_glossary, InjectionMode::Smart).unwrap();

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
                action: BaselineAction::InitializeFromRunStart,
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
        let previous_state = build_glossary_state(&previous_glossary, InjectionMode::Full).unwrap();
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Full).unwrap(),
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
                action: BaselineAction::KeepExisting,
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

        let current_glossary = smart_glossary("Hero definition");
        let previous_state = build_glossary_state(&current_glossary, InjectionMode::Smart).unwrap();
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
                Some(build_chapter_glossary_usage(&selection, InjectionMode::Smart).unwrap()),
                vec![],
                None,
            ),
        )]);

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart).unwrap(),
            &[chapter],
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
                action: BaselineAction::KeepExisting,
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
        let previous_state = build_glossary_state(&[], InjectionMode::Smart).unwrap();
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
                    Some(
                        build_chapter_glossary_usage(&smart_selection, InjectionMode::Smart)
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
                        build_chapter_glossary_usage(&fallback_selection, InjectionMode::Smart)
                            .unwrap(),
                    ),
                    vec![],
                    None,
                ),
            ),
        ]);

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart).unwrap(),
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
                action: BaselineAction::CommitRunEnd,
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
        let previous_glossary_state = build_glossary_state(&glossary, InjectionMode::Full).unwrap();
        save_glossary_state(dir.path(), &previous_glossary_state).unwrap();

        let legacy_selection =
            select_terms_for_text(&glossary, "hero appears here", InjectionMode::Full);
        let legacy_chapter_state = ChapterState::new(
            "chapter1.md".to_string(),
            ChapterStatus::Success,
            None,
            Some(100),
            None,
            Some(build_chapter_glossary_usage(&legacy_selection, InjectionMode::Full).unwrap()),
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
                action: BaselineAction::KeepExisting,
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
        let previous_glossary_state = build_glossary_state(&glossary, InjectionMode::Full).unwrap();
        save_glossary_state(dir.path(), &previous_glossary_state).unwrap();

        let legacy_selection = select_terms_for_text(&glossary, smart_text(), InjectionMode::Full);
        let legacy_chapter_state = ChapterState::new(
            "chapter1.md".to_string(),
            ChapterStatus::Success,
            None,
            Some(100),
            None,
            Some(build_chapter_glossary_usage(&legacy_selection, InjectionMode::Full).unwrap()),
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
                action: BaselineAction::KeepExisting,
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
        let previous_state =
            build_glossary_state(&previous_glossary, InjectionMode::Smart).unwrap();
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            true,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart).unwrap(),
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
                action: BaselineAction::KeepExisting,
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
        let previous_state =
            build_glossary_state(&previous_glossary, InjectionMode::Smart).unwrap();
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            false,
            Some(&previous_state),
            &build_glossary_state(&current_glossary, InjectionMode::Smart).unwrap(),
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
                action: BaselineAction::KeepExisting,
                remaining_forced_chapters: 0,
            }
        );

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_glossary_state_matches(&loaded, &previous_glossary, InjectionMode::Smart);
    }

    #[test]
    fn migrated_legacy_full_usage_returns_none_for_non_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ch.md");
        std::fs::write(&path, "content").unwrap();
        let state = ChapterState::new(
            "ch.md".to_string(),
            ChapterStatus::Success,
            None,
            None,
            None,
            Some(ChapterGlossaryUsage {
                injection_mode: InjectionMode::Smart,
                used_fallback_to_full: false,
                terms: vec![],
            }),
            vec![],
            None,
        );
        let result = migrated_legacy_full_usage(&path, &state, &[]).unwrap();
        assert!(result.is_none());
    }
}
