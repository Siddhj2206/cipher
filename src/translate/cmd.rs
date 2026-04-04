use crate::book::{BookLayout, load_book_config};
use crate::config::{GlobalConfig, validate_profile};
use crate::glossary::{
    GlossaryTerm, InjectionMode, SelectionResult, book_config_injection_mode,
    glossary_term_key, glossary_term_prompt_fingerprint, load_glossary, merge_terms,
    save_glossary, select_terms_for_text,
};
use crate::output::{detail, detail_kv, section, stderr_detail, warn};
use crate::state::{
    ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterState, ChapterStatus, GlossaryInjectionMode,
    GlossaryState, GlossaryStateTerm, RunMetadata, RunOptions, load_all_chapter_states,
    load_glossary_state, normalize_chapter_path, normalized_source_text_hash, save_chapter_state,
    save_glossary_state, save_run_metadata,
};
use crate::translate::{ProviderTranslationResult, TranslationUsage, Translator};
use crate::validate::validate_translation;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct TranslateOptions {
    pub profile: Option<String>,
    pub overwrite: bool,
    pub fail_fast: bool,
    pub rerun: bool,
    pub rerun_affected_glossary: bool,
    pub rerun_affected_chapters: bool,
}

impl TranslateOptions {
    fn rerun_glossary_enabled(&self) -> bool {
        self.rerun || self.rerun_affected_glossary
    }

    fn rerun_chapters_enabled(&self) -> bool {
        self.rerun || self.rerun_affected_chapters
    }
}

struct ChapterResult {
    translated: bool,
    failed: bool,
    skipped: bool,
    new_terms_added: usize,
    usage: Option<TranslationUsage>,
    chapter_state: ChapterState,
}

#[derive(Debug, Clone)]
struct GlossaryRerunDecision {
    reason: String,
}

#[derive(Debug, Clone)]
struct ChapterRerunDecision {
    reason: String,
}

#[derive(Debug, Default)]
struct GlossaryRerunPlan {
    forced_chapters: BTreeMap<String, GlossaryRerunDecision>,
    warnings: Vec<String>,
    changed_term_count: usize,
    approximate_smart_checks: usize,
}

#[derive(Debug, Default)]
struct SourceRerunPlan {
    forced_chapters: BTreeMap<String, String>,
    untracked_chapters: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlossaryBaselineAdvance {
    KeepExisting,
    InitializeFromRunStart,
    CommitRunEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GlossaryBaselineOutcome {
    advance: GlossaryBaselineAdvance,
    remaining_forced_chapters: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct LegacyTrackingMigration {
    migrated_chapters: usize,
    migrated_glossary_baseline: bool,
}

impl GlossaryRerunPlan {
    fn decision_for(&self, filename: &str) -> Option<&GlossaryRerunDecision> {
        self.forced_chapters.get(filename)
    }
}

impl SourceRerunPlan {
    fn decision_for(&self, filename: &str) -> Option<&String> {
        self.forced_chapters.get(filename)
    }
}

pub async fn translate_book(book_dir: &Path, options: TranslateOptions) -> Result<()> {
    // Load book layout
    let layout = BookLayout::discover(book_dir);

    if !layout.is_valid_book() {
        anyhow::bail!(
            "Invalid book layout. Run 'cipher doctor {}' for details.",
            book_dir.display()
        );
    }

    // Load global config
    let global_config = GlobalConfig::load().context("Failed to load global config")?;

    // Resolve effective profile (CLI override takes precedence)
    let book_config = load_book_config(&layout.paths.config_json).unwrap_or_default();
    let injection_mode = book_config_injection_mode(&book_config.glossary_injection);
    let profile_name = options
        .profile
        .as_deref()
        .or_else(|| global_config.effective_profile_name(book_config.profile.as_deref()));

    let profile_name = profile_name.ok_or_else(|| {
        anyhow::anyhow!("No profile configured. Run 'cipher profile new' to create one.")
    })?;

    // Validate profile
    let validation = validate_profile(&global_config, profile_name);
    if !validation.is_valid() {
        eprintln!("Profile validation failed");
        for error in &validation.errors {
            stderr_detail(error);
        }
        anyhow::bail!("Cannot translate with invalid profile");
    }

    let profile = global_config
        .resolve_profile(profile_name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", profile_name))?;

    // Create translator
    let translator = Translator::from_config(&global_config, profile_name)
        .context("Failed to create translator")?;

    // Discover chapters
    let chapters: VecDeque<PathBuf> = discover_chapters(&layout.paths.raw_dir)?
        .into_iter()
        .collect();
    if chapters.is_empty() {
        section("No chapters found");
        detail_kv("Directory", layout.paths.raw_dir.display());
        return Ok(());
    }

    // Load existing glossary
    let mut glossary = load_glossary(&layout.paths.glossary_json)?;
    let run_start_glossary_state = build_glossary_state(&glossary, injection_mode);

    // Determine output directory
    let out_dir = layout.effective_out_dir();

    // Load style guide if it exists
    let style_guide = if layout.exists.style_md {
        match std::fs::read_to_string(&layout.paths.style_md) {
            Ok(content) if !content.trim().is_empty() => Some(content),
            _ => None,
        }
    } else {
        None
    };

    section(format!("Using profile {}", profile_name));
    detail_kv("Provider", &profile.provider);
    detail_kv("Model", &profile.model);
    if style_guide.is_some() {
        detail_kv("Style guide", layout.paths.style_md.display());
    }

    // Load previous checkpointed state for glossary diffing
    let previous_glossary_state = load_glossary_state(book_dir)?;
    let mut previous_chapter_states = load_all_chapter_states(book_dir)?;

    let rerun_plan = if options.rerun_glossary_enabled() {
        section("Planning glossary-affected chapter reruns");
        let plan = build_glossary_rerun_plan(
            &Vec::from(chapters.clone()),
            &layout.paths.raw_dir,
            out_dir,
            previous_glossary_state.as_ref(),
            &previous_chapter_states,
            &glossary,
            injection_mode,
        )?;
        detail_kv("Changed glossary entries", plan.changed_term_count);
        detail_kv("Affected chapters", plan.forced_chapters.len());
        if plan.approximate_smart_checks > 0 {
            detail_kv(
                "Approximate smart rerun checks",
                plan.approximate_smart_checks,
            );
        }
        for warning in &plan.warnings {
            warn(warning);
        }
        plan
    } else {
        GlossaryRerunPlan::default()
    };

    let source_rerun_plan = if options.rerun_chapters_enabled() {
        section("Planning source-affected chapter reruns");
        let plan = build_source_rerun_plan(
            &Vec::from(chapters.clone()),
            &layout.paths.raw_dir,
            out_dir,
            &previous_chapter_states,
        )?;
        detail_kv("Affected chapters", plan.forced_chapters.len());
        if plan.untracked_chapters > 0 {
            detail_kv("Untracked chapters", plan.untracked_chapters);
        }
        plan
    } else {
        SourceRerunPlan::default()
    };

    section("Translating chapters");
    detail_kv("Chapters found", chapters.len());

    // Create run state with options
    let run_options = RunOptions {
        overwrite: options.overwrite,
        fail_fast: options.fail_fast,
        rerun: options.rerun,
        rerun_affected_glossary: options.rerun_affected_glossary,
        rerun_affected_chapters: options.rerun_affected_chapters,
    };

    let mut run_metadata = RunMetadata::new(
        profile_name.to_string(),
        profile.provider.clone(),
        profile.model.clone(),
        Some(run_options),
    );
    save_run_metadata(book_dir, &run_metadata)?;

    // Track stats
    let mut translated = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut new_glossary_terms = 0;
    let mut total_usage = TranslationUsage::default();

    let mut remaining_chapters = chapters.clone();
    let mut rerun_plan = rerun_plan;
    let source_rerun_plan = source_rerun_plan;

    while let Some(chapter_file) = remaining_chapters.pop_front() {
        let chapter_path = chapter_state_key(&layout.paths.raw_dir, &chapter_file)?;
        let out_path = chapter_output_path(out_dir, &chapter_file)?;
        let previous_chapter_state = previous_chapter_states.get(&chapter_path);
        let rerun_decision = combine_rerun_decisions(
            rerun_plan.decision_for(&chapter_path),
            source_rerun_plan.decision_for(&chapter_path),
        );

        let result = translate_single_chapter(
            &translator,
            &chapter_file,
            &out_path,
            &chapter_path,
            &options,
            previous_chapter_state,
            rerun_decision.as_ref(),
            &mut glossary,
            &style_guide,
            injection_mode,
            &layout.paths.glossary_json,
            book_dir,
        )
        .await?;

        checkpoint_chapter_progress(book_dir, &mut run_metadata, &result.chapter_state)?;
        previous_chapter_states.insert(chapter_path.clone(), result.chapter_state.clone());

        if result.translated {
            translated += 1;
        }
        if result.skipped {
            skipped += 1;
        }
        if result.failed {
            failed += 1;
            if options.fail_fast {
                detail("Stopping due to --fail-fast");
                break;
            }
        }
        if let Some(usage) = result.usage {
            total_usage += usage;
        }
        new_glossary_terms += result.new_terms_added;

        if result.new_terms_added > 0
            && options.rerun_glossary_enabled()
            && !remaining_chapters.is_empty()
        {
            rerun_plan = build_glossary_rerun_plan(
                &Vec::from(remaining_chapters.clone()),
                &layout.paths.raw_dir,
                out_dir,
                previous_glossary_state.as_ref(),
                &previous_chapter_states,
                &glossary,
                injection_mode,
            )?;
        }
    }

    let baseline_outcome = finalize_glossary_baseline(
        book_dir,
        &options,
        previous_glossary_state.as_ref(),
        &run_start_glossary_state,
        &Vec::from(chapters.clone()),
        &layout.paths.raw_dir,
        out_dir,
        &previous_chapter_states,
        &glossary,
        injection_mode,
        failed,
    )?;

    let legacy_tracking_migration = migrate_legacy_full_tracking(
        book_dir,
        previous_glossary_state.as_ref(),
        baseline_outcome,
        &Vec::from(chapters.clone()),
        &layout.paths.raw_dir,
        out_dir,
        &mut previous_chapter_states,
        &glossary,
        injection_mode,
        failed,
    )?;

    if baseline_outcome.remaining_forced_chapters > 0 {
        warn(format!(
            "Glossary baseline was not updated because {} affected chapter(s) still need reruns.",
            baseline_outcome.remaining_forced_chapters
        ));
    }

    run_metadata.mark_finished();
    save_run_metadata(book_dir, &run_metadata)?;

    // Print summary
    section("Translation complete");
    detail_kv("Translated", translated);
    detail_kv("Skipped", skipped);
    detail_kv("Failed", failed);
    detail_kv("Glossary terms added", new_glossary_terms);
    if legacy_tracking_migration.migrated_chapters > 0 {
        detail_kv(
            "Legacy chapters migrated",
            legacy_tracking_migration.migrated_chapters,
        );
    }
    if legacy_tracking_migration.migrated_glossary_baseline {
        detail("Migrated legacy full-glossary baseline to canonical smart tracking");
    }
    if total_usage.total_tokens > 0 {
        print_usage_info_with_label("Token usage", &total_usage);
    }

    if failed > 0 {
        anyhow::bail!("{} chapter(s) failed to translate", failed);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn translate_single_chapter(
    translator: &Translator,
    raw_path: &Path,
    out_path: &Path,
    chapter_path: &str,
    options: &TranslateOptions,
    previous_chapter_state: Option<&ChapterState>,
    rerun_decision: Option<&ChapterRerunDecision>,
    glossary: &mut Vec<GlossaryTerm>,
    style_guide: &Option<String>,
    injection_mode: InjectionMode,
    glossary_path: &Path,
    book_dir: &Path,
) -> Result<ChapterResult> {
    let translation_injection_mode =
        chapter_translation_injection_mode(injection_mode, rerun_decision);

    // Check if output exists
    let output_exists = out_path.exists();
    if !options.overwrite && output_exists && rerun_decision.is_none() {
        let source_text_hash =
            skipped_chapter_source_hash(raw_path, previous_chapter_state, options)?;

        return Ok(ChapterResult {
            translated: false,
            failed: false,
            skipped: true,
            new_terms_added: 0,
            usage: None,
            chapter_state: ChapterState::new(
                chapter_path.to_string(),
                ChapterStatus::Skipped,
                None,
                None,
                previous_chapter_state.and_then(|state| state.translation_usage.clone()),
                previous_chapter_state.and_then(|state| state.glossary_usage.clone()),
                previous_chapter_state
                    .map(|s| s.exported_terms.clone())
                    .unwrap_or_default(),
                source_text_hash,
            ),
        });
    }

    if let Some(decision) = rerun_decision {
        println!("Retranslating {}", chapter_path);
        detail_kv("Rerun reason", &decision.reason);
    } else {
        println!("Translating {}", chapter_path);
    }

    // Read chapter
    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;
    let source_text_hash = normalized_source_text_hash(&chapter_text);

    if chapter_text.trim().is_empty() {
        return Ok(ChapterResult {
            translated: false,
            failed: false,
            skipped: true,
            new_terms_added: 0,
            usage: None,
            chapter_state: ChapterState::new(
                chapter_path.to_string(),
                ChapterStatus::Skipped,
                Some("Chapter is empty".to_string()),
                None,
                previous_chapter_state.and_then(|state| state.translation_usage.clone()),
                previous_chapter_state.and_then(|state| state.glossary_usage.clone()),
                previous_chapter_state
                    .map(|s| s.exported_terms.clone())
                    .unwrap_or_default(),
                Some(source_text_hash),
            ),
        });
    }

    // Select glossary terms and display info
    let start = Instant::now();
    let selection = select_terms_for_text(glossary, &chapter_text, translation_injection_mode);
    print_glossary_info(&selection, translation_injection_mode);

    // Attempt translation with retries
    let (response, last_error) =
        attempt_translation(translator, &chapter_text, &selection, style_guide).await;

    let duration = start.elapsed();

    if let Some(resp) = response {
        print_usage_info(&resp.usage);

        // Backup if overwriting existing file
        if output_exists {
            let backup_path = create_backup(book_dir, out_path)?;
            detail_kv("Backup", backup_path.display());
        }

        // Write output atomically
        atomic_write(out_path, &resp.response.translation)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;

        // Merge glossary terms
        let (new_terms_added, exported_terms) =
            merge_new_glossary_terms(glossary, resp.response.new_glossary_terms, glossary_path)?;

        detail_kv("Result", "success");
        return Ok(ChapterResult {
            translated: true,
            failed: false,
            skipped: false,
            new_terms_added,
            usage: Some(resp.usage.clone()),
            chapter_state: ChapterState::new(
                chapter_path.to_string(),
                ChapterStatus::Success,
                None,
                Some(duration.as_millis() as u64),
                Some(resp.usage),
                Some(build_chapter_glossary_usage(
                    &selection,
                    translation_injection_mode,
                )),
                exported_terms,
                Some(source_text_hash),
            ),
        });
    }

    let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
    detail_kv(
        "Result",
        format!("failed after {} attempts", MAX_API_RETRIES),
    );
    detail_kv("Error", &error_msg);
    let failed_source_text_hash =
        failed_chapter_source_hash(previous_chapter_state, &source_text_hash);
    Ok(ChapterResult {
        translated: false,
        failed: true,
        skipped: false,
        new_terms_added: 0,
        usage: None,
        chapter_state: ChapterState::new(
            chapter_path.to_string(),
            ChapterStatus::Failed,
            Some(error_msg),
            Some(duration.as_millis() as u64),
            None,
            previous_chapter_state.and_then(|state| state.glossary_usage.clone()),
            previous_chapter_state
                .map(|s| s.exported_terms.clone())
                .unwrap_or_default(),
            failed_source_text_hash,
        ),
    })
}

fn skipped_chapter_source_hash(
    raw_path: &Path,
    previous_chapter_state: Option<&ChapterState>,
    options: &TranslateOptions,
) -> Result<Option<String>> {
    if !options.rerun_chapters_enabled() {
        return Ok(previous_chapter_state.and_then(|state| state.source_text_hash.clone()));
    }

    match previous_chapter_state.and_then(|state| state.source_text_hash.clone()) {
        Some(existing_hash) => Ok(Some(existing_hash)),
        None => Ok(Some(source_text_hash_for_path(raw_path)?)),
    }
}

fn failed_chapter_source_hash(
    previous_chapter_state: Option<&ChapterState>,
    current_source_hash: &str,
) -> Option<String> {
    previous_chapter_state
        .and_then(|state| state.source_text_hash.clone())
        .or_else(|| Some(current_source_hash.to_string()))
}

fn source_text_hash_for_path(path: &Path) -> Result<String> {
    let chapter_text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(normalized_source_text_hash(&chapter_text))
}

fn checkpoint_chapter_progress(
    book_dir: &Path,
    run_metadata: &mut RunMetadata,
    chapter_state: &ChapterState,
) -> Result<()> {
    save_chapter_state(book_dir, chapter_state)?;
    run_metadata.touch();
    save_run_metadata(book_dir, run_metadata)
}

#[allow(clippy::too_many_arguments)]
fn finalize_glossary_baseline(
    book_dir: &Path,
    options: &TranslateOptions,
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

    if !options.rerun_glossary_enabled() {
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

#[allow(clippy::too_many_arguments)]
fn migrate_legacy_full_tracking(
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

        if usage.injection_mode != GlossaryInjectionMode::Full {
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

    if previous_glossary_state.injection_mode != GlossaryInjectionMode::Full
        || !all_output_chapters_tracked
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
                GlossaryInjectionMode::Smart,
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

fn migrated_legacy_full_usage(
    raw_path: &Path,
    chapter_state: &ChapterState,
    current_glossary: &[GlossaryTerm],
) -> Result<Option<ChapterGlossaryUsage>> {
    let Some(usage) = &chapter_state.glossary_usage else {
        return Ok(None);
    };

    if usage.injection_mode != GlossaryInjectionMode::Full {
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

fn count_chapters_still_stale_for_current_glossary(
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

fn chapter_matches_current_glossary(
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

    let tracked_usage: BTreeMap<String, String> = usage
        .terms
        .iter()
        .map(|term| (term.key.clone(), term.fingerprint.clone()))
        .collect();
    let full_glossary_usage = selection_fingerprints(current_glossary);

    if usage.injection_mode == GlossaryInjectionMode::Full {
        return Ok(tracked_usage == full_glossary_usage);
    }

    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;
    if chapter_text.trim().is_empty() {
        return Ok(true);
    }

    let current_selection = select_terms_for_text(current_glossary, &chapter_text, injection_mode);
    let expected_usage = if current_selection.used_fallback_to_full {
        full_glossary_usage
    } else {
        selection_fingerprints(&current_selection.terms)
    };

    Ok(tracked_usage == expected_usage
        && usage.used_fallback_to_full == current_selection.used_fallback_to_full)
}

fn build_glossary_state(glossary: &[GlossaryTerm], injection_mode: InjectionMode) -> GlossaryState {
    GlossaryState::new(
        glossary_injection_mode(injection_mode),
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

fn build_source_rerun_plan(
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

fn combine_rerun_decisions(
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

fn build_chapter_glossary_usage(
    selection: &SelectionResult,
    injection_mode: InjectionMode,
) -> ChapterGlossaryUsage {
    ChapterGlossaryUsage {
        injection_mode: glossary_injection_mode(injection_mode),
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

fn chapter_translation_injection_mode(
    injection_mode: InjectionMode,
    _rerun_decision: Option<&ChapterRerunDecision>,
) -> InjectionMode {
    injection_mode
}

fn glossary_injection_mode(mode: InjectionMode) -> GlossaryInjectionMode {
    match mode {
        InjectionMode::Full => GlossaryInjectionMode::Full,
        InjectionMode::Smart => GlossaryInjectionMode::Smart,
    }
}

fn build_glossary_rerun_plan(
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
            if let Some(usage) = &previous_chapter_state.glossary_usage {
                if chapter_matches_current_glossary(
                    chapter_file,
                    previous_chapter_state,
                    current_glossary,
                    injection_mode,
                )? {
                    continue;
                }

                // Need previous_glossary_state for smart mode comparison
                if let Some(prev_state) = previous_glossary_state {
                    if let Some(decision) = exact_rerun_decision(
                        chapter_file,
                        usage,
                        &previous_chapter_state.exported_terms,
                        prev_state,
                        current_glossary,
                        &changed_term_keys,
                        injection_mode,
                    )? {
                        plan.forced_chapters.insert(chapter_path, decision);
                    }
                } else {
                    // No glossary state but chapter has tracked usage - can't compare
                    plan.warnings.push(format!(
                        "Chapter {} has glossary usage but no glossary state recorded",
                        chapter_path
                    ));
                }
                continue;
            }
        }

        let Some(previous_glossary_state) = previous_glossary_state else {
            continue;
        };

        match glossary_state_injection_mode(previous_glossary_state.injection_mode) {
            InjectionMode::Full => {
                if let Some(reason) = full_glossary_rerun_reason(&changed_term_keys) {
                    plan.forced_chapters.insert(
                        chapter_path,
                        GlossaryRerunDecision {
                            reason,
                        },
                    );
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

#[cfg(test)]
fn snapshot_fingerprints(terms: &BTreeMap<String, GlossaryStateTerm>) -> BTreeMap<String, String> {
    terms
        .iter()
        .map(|(key, term)| (key.clone(), term.fingerprint.clone()))
        .collect()
}

fn changed_prompt_relevant_keys(
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

fn exact_rerun_decision(
    raw_path: &Path,
    usage: &ChapterGlossaryUsage,
    exported_terms: &[ChapterGlossaryTerm],
    previous_glossary_state: &GlossaryState,
    current_glossary: &[GlossaryTerm],
    changed_term_keys: &BTreeSet<String>,
    injection_mode: InjectionMode,
) -> Result<Option<GlossaryRerunDecision>> {
    match injection_mode {
        InjectionMode::Full => {
            // For full mode, check if any glossary terms changed
            Ok(
                full_glossary_rerun_reason(changed_term_keys)
                    .map(|reason| GlossaryRerunDecision { reason }),
            )
        }
        InjectionMode::Smart => {
            // For smart mode, first check if the glossary fingerprint changed for any
            // previously selected or exported term. If so, we need to rerun.
            let current_fingerprints: BTreeMap<String, String> = current_glossary
                .iter()
                .map(|term| {
                    (
                        glossary_term_key(term),
                        glossary_term_prompt_fingerprint(term),
                    )
                })
                .collect();

            let all_terms: Vec<&ChapterGlossaryTerm> =
                usage.terms.iter().chain(exported_terms.iter()).collect();

            let fingerprint_changed_keys: Vec<String> = all_terms
                .iter()
                .filter_map(|term| match current_fingerprints.get(&term.key) {
                    Some(fingerprint) if fingerprint == &term.fingerprint => None,
                    _ => Some(term.key.clone()),
                })
                .collect();

            if !fingerprint_changed_keys.is_empty() {
                // At least one previously selected/exported term's fingerprint changed
                return Ok(Some(GlossaryRerunDecision {
                    reason: format!(
                        "Imported or exported glossary term changed: {}",
                        fingerprint_changed_keys.join(", ")
                    ),
                }));
            }

            // No fingerprint changes for tracked terms. Now check if the smart selection
            // itself would produce a different set of terms (e.g., new terms that now match
            // the chapter text, or terms that were removed from the glossary).
            // This handles the case where a glossary term is added later that would now
            // be selected for this chapter.
            let chapter_text = match std::fs::read_to_string(raw_path) {
                Ok(text) => text,
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("Failed to read {}", raw_path.display()));
                }
            };

            if chapter_text.trim().is_empty() {
                return Ok(None);
            }

            // Reconstruct previous glossary from state
            let previous_glossary = glossary_terms_from_state(previous_glossary_state);

            // Run smart selection on both old and new glossaries
            let previous_selection =
                select_terms_for_text(&previous_glossary, &chapter_text, InjectionMode::Smart);
            let current_selection =
                select_terms_for_text(current_glossary, &chapter_text, InjectionMode::Smart);

            // Check if fallback state changed (e.g., was fallback before but not now, or vice versa)
            let fallback_changed =
                previous_selection.used_fallback_to_full != current_selection.used_fallback_to_full;

            // Compare the two selections to detect changes
            let previous_terms = selection_fingerprints(&previous_selection.terms);
            let current_terms = selection_fingerprints(&current_selection.terms);
            let selection_changed_keys =
                changed_selected_term_keys(&previous_terms, &current_terms);

            if selection_changed_keys.is_empty() && !fallback_changed {
                Ok(None)
            } else {
                let reason = if fallback_changed {
                    format!(
                        "Smart glossary selection changed fallback behavior: {} -> {}",
                        fallback_state_label(previous_selection.used_fallback_to_full),
                        fallback_state_label(current_selection.used_fallback_to_full)
                    )
                } else {
                    format!(
                        "Smart glossary selection changed: {}",
                        selection_changed_keys
                            .into_iter()
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                Ok(Some(GlossaryRerunDecision {
                    reason,
                }))
            }
        }
    }
}

fn approximate_smart_rerun_decision(
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

fn full_glossary_rerun_reason(changed_term_keys: &BTreeSet<String>) -> Option<String> {
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

fn fallback_state_label(used_fallback_to_full: bool) -> &'static str {
    if used_fallback_to_full {
        "fallback to full"
    } else {
        "smart selection only"
    }
}

fn glossary_terms_from_state(glossary_state: &GlossaryState) -> Vec<GlossaryTerm> {
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

fn selection_fingerprints(terms: &[GlossaryTerm]) -> BTreeMap<String, String> {
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

fn changed_selected_term_keys(
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

fn glossary_state_injection_mode(mode: GlossaryInjectionMode) -> InjectionMode {
    match mode {
        GlossaryInjectionMode::Full => InjectionMode::Full,
        GlossaryInjectionMode::Smart => InjectionMode::Smart,
    }
}

fn chapter_state_key(raw_dir: &Path, chapter_file: &Path) -> Result<String> {
    let relative_path = chapter_file
        .strip_prefix(raw_dir)
        .with_context(|| format!("Failed to relativize {}", chapter_file.display()))?;
    Ok(normalize_chapter_path(relative_path))
}

fn chapter_output_path(out_dir: &Path, chapter_file: &Path) -> Result<PathBuf> {
    let filename = chapter_file
        .file_name()
        .context("Invalid chapter filename")?;
    Ok(out_dir.join(filename))
}

fn print_glossary_info(selection: &SelectionResult, injection_mode: InjectionMode) {
    match injection_mode {
        InjectionMode::Smart => {
            if selection.used_fallback_to_full {
                detail_kv(
                    "Glossary",
                    format!(
                        "smart fallback to full, {}/{} terms",
                        selection.selected_count, selection.total_count
                    ),
                );
            } else {
                detail_kv(
                    "Glossary",
                    format!(
                        "smart selection, {}/{} terms",
                        selection.selected_count, selection.total_count
                    ),
                );
            }
        }
        InjectionMode::Full => {
            detail_kv("Glossary", format!("full, {} terms", selection.total_count));
        }
    }
}

const MAX_API_RETRIES: usize = 3;

async fn attempt_translation(
    translator: &Translator,
    chapter_text: &str,
    selection: &SelectionResult,
    style_guide: &Option<String>,
) -> (Option<ProviderTranslationResult>, Option<String>) {
    let mut last_error: Option<String> = None;

    for api_attempt in 1..=MAX_API_RETRIES {
        match translator
            .translate_chapter(chapter_text, &selection.terms, style_guide.clone())
            .await
        {
            Ok(resp) => {
                let validation = validate_translation(&resp.response.translation);
                if validation.is_valid() {
                    return (Some(resp), None);
                }

                let validation_errors = validation.errors();
                last_error = Some(format!(
                    "Validation failed: {}",
                    validation_errors.join(", ")
                ));

                if api_attempt == 1 {
                    detail_kv(
                        "Validation",
                        format!("{} Attempting repair.", validation_errors.join(", ")),
                    );

                    let repair_req =
                        crate::translate::TranslationRequest::new(chapter_text.to_string())
                            .with_glossary_terms(selection.terms.clone())
                            .with_style_guide(style_guide.clone())
                            .with_failed_translation(resp.response.translation)
                            .with_validation_errors(validation_errors.to_vec());

                    match translator.translate_with_request(&repair_req).await {
                        Ok(repair_resp) => {
                            let repair_validation =
                                validate_translation(&repair_resp.response.translation);
                            if repair_validation.is_valid() {
                                print_usage_info(&repair_resp.usage);
                                detail_kv("Repair", "success");
                                return (Some(repair_resp), None);
                            }
                            last_error = Some(format!(
                                "Repair failed validation: {}",
                                repair_validation.errors().join(", ")
                            ));
                            detail_kv("Repair", last_error.as_ref().unwrap());
                        }
                        Err(e) => {
                            last_error = Some(format!("Repair request failed: {}", e));
                            detail_kv("Repair", last_error.as_ref().unwrap());
                        }
                    }
                }

                break;
            }
            Err(e) => {
                last_error = Some(format!("API error: {}", e));
                if api_attempt < MAX_API_RETRIES {
                    let delay_secs = 2u64.pow(api_attempt as u32);
                    detail_kv(
                        "Attempt",
                        format!(
                            "Attempt {}/{} failed: {}. Retrying in {}s.",
                            api_attempt,
                            MAX_API_RETRIES,
                            last_error.as_ref().unwrap(),
                            delay_secs
                        ),
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    (None, last_error)
}

fn print_usage_info(usage: &TranslationUsage) {
    print_usage_info_with_label("Usage", usage);
}

fn print_usage_info_with_label(label: &str, usage: &TranslationUsage) {
    detail_kv(
        label,
        format!(
            "{} input, {} output, {} total",
            usage.input_tokens, usage.output_tokens, usage.total_tokens
        ),
    );
}

fn merge_new_glossary_terms(
    glossary: &mut Vec<GlossaryTerm>,
    new_terms: Vec<GlossaryTerm>,
    glossary_path: &Path,
) -> Result<(usize, Vec<ChapterGlossaryTerm>)> {
    if new_terms.is_empty() {
        return Ok((0, Vec::new()));
    }

    let (merged, added, dupes, added_terms) = merge_terms(std::mem::take(glossary), new_terms);
    *glossary = merged;

    let added_term_fingerprints: Vec<ChapterGlossaryTerm> = added_terms
        .iter()
        .map(|term| ChapterGlossaryTerm {
            key: glossary_term_key(term),
            fingerprint: glossary_term_prompt_fingerprint(term),
        })
        .collect();

    if added > 0 {
        if dupes > 0 {
            detail(format!(
                "Added {} glossary {}; skipped {} duplicate{}.",
                added,
                pluralize(added, "term", "terms"),
                dupes,
                pluralize(dupes, "", "s")
            ));
        } else {
            detail(format!(
                "Added {} glossary {}.",
                added,
                pluralize(added, "term", "terms")
            ));
        }
        save_glossary(glossary_path, glossary)?;
    } else if dupes > 0 {
        detail(format!(
            "No glossary terms added; skipped {} duplicate{}.",
            dupes,
            pluralize(dupes, "", "s")
        ));
    }

    Ok((added, added_term_fingerprints))
}

fn pluralize<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn discover_chapters(raw_dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut chapters = Vec::new();

    if !raw_dir.exists() {
        return Ok(chapters);
    }

    for entry in std::fs::read_dir(raw_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e == "md").unwrap_or(false) {
            chapters.push(path);
        }
    }

    // Sort by numeric-first, then alphabetically
    chapters.sort_by(|a, b| {
        let a_name = a
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let b_name = b
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();

        let a_num = extract_number(&a_name);
        let b_num = extract_number(&b_name);

        match (a_num, b_num) {
            (Some(n1), Some(n2)) => n1.cmp(&n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a_name.cmp(&b_name),
        }
    });

    Ok(chapters)
}

fn extract_number(filename: &str) -> Option<u32> {
    // Extract first sequence of digits
    let digits: String = filename
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();

    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn create_backup(book_dir: &Path, path: &Path) -> Result<PathBuf> {
    use chrono::Local;

    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let filename = path
        .file_stem()
        .context("Cannot determine file stem for backup")?
        .to_string_lossy();
    let backup_name = format!("{}_{}.md", filename, timestamp);

    let backup_dir = book_dir.join(".cipher").join("backups");
    std::fs::create_dir_all(&backup_dir)?;

    let backup_path = backup_dir.join(&backup_name);
    std::fs::copy(path, &backup_path)?;
    Ok(backup_path)
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, content)?;
    if let Err(e) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e).with_context(|| format!("Failed to write to {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::load_chapter_state;

    fn glossary_term(term: &str, og_term: Option<&str>, definition: &str) -> GlossaryTerm {
        GlossaryTerm {
            term: term.to_string(),
            og_term: og_term.map(str::to_string),
            definition: definition.to_string(),
            notes: None,
        }
    }

    fn previous_glossary_state(glossary: &[GlossaryTerm], mode: InjectionMode) -> GlossaryState {
        build_glossary_state(glossary, mode)
    }

    fn translate_options(
        rerun: bool,
        rerun_affected_glossary: bool,
        rerun_affected_chapters: bool,
    ) -> TranslateOptions {
        TranslateOptions {
            profile: None,
            overwrite: false,
            fail_fast: false,
            rerun,
            rerun_affected_glossary,
            rerun_affected_chapters,
        }
    }

    fn assert_glossary_state_matches(
        actual: &GlossaryState,
        glossary: &[GlossaryTerm],
        injection_mode: InjectionMode,
    ) {
        let expected = build_glossary_state(glossary, injection_mode);
        assert_eq!(actual.injection_mode, expected.injection_mode);
        assert_eq!(
            snapshot_fingerprints(&actual.terms),
            snapshot_fingerprints(&expected.terms)
        );
    }

    fn previous_chapter_state_with_hash(source_text_hash: Option<&str>) -> ChapterState {
        ChapterState::new(
            "chapter1.md".to_string(),
            ChapterStatus::Success,
            None,
            Some(100),
            None,
            None,
            vec![],
            source_text_hash.map(str::to_string),
        )
    }

    fn smart_glossary(hero_definition: &str) -> Vec<GlossaryTerm> {
        vec![
            glossary_term("Hero", Some("勇者"), hero_definition),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Dragon King", Some("竜王"), "Dragon definition"),
        ]
    }

    fn smart_text() -> &'static str {
        "勇者は魔導士と聖剣を手に王城で竜王と戦った。"
    }

    #[test]
    fn test_extract_number() {
        assert_eq!(extract_number("chapter01"), Some(1));
        assert_eq!(extract_number("chapter1"), Some(1));
        assert_eq!(extract_number("chapter10"), Some(10));
        assert_eq!(extract_number("01-chapter"), Some(1));
        assert_eq!(extract_number("no-number"), None);
        assert_eq!(extract_number(""), None);
    }

    #[test]
    fn test_extract_number_multiple_groups() {
        // Should extract first sequence of digits
        assert_eq!(extract_number("ch3_part2"), Some(3));
        assert_eq!(extract_number("v2_chapter10"), Some(2));
    }

    #[test]
    fn test_discover_chapters_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = discover_chapters(dir.path()).unwrap();
        assert!(chapters.is_empty());
    }

    #[test]
    fn test_discover_chapters_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let non_existent = dir.path().join("does_not_exist");
        let chapters = discover_chapters(&non_existent).unwrap();
        assert!(chapters.is_empty());
    }

    #[test]
    fn test_discover_chapters_filters_non_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("chapter01.md"), "# Ch 1").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "notes").unwrap();
        std::fs::write(dir.path().join("image.png"), "binary").unwrap();

        let chapters = discover_chapters(dir.path()).unwrap();
        assert_eq!(chapters.len(), 1);
        assert!(chapters[0].file_name().unwrap().to_str().unwrap() == "chapter01.md");
    }

    #[test]
    fn test_discover_chapters_sorted_by_number() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("chapter10.md"), "# Ch 10").unwrap();
        std::fs::write(dir.path().join("chapter2.md"), "# Ch 2").unwrap();
        std::fs::write(dir.path().join("chapter1.md"), "# Ch 1").unwrap();

        let chapters = discover_chapters(dir.path()).unwrap();
        let names: Vec<_> = chapters
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["chapter1.md", "chapter2.md", "chapter10.md"]);
    }

    #[test]
    fn test_discover_chapters_non_numeric_sorted_alpha() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("prologue.md"), "# Prologue").unwrap();
        std::fs::write(dir.path().join("epilogue.md"), "# Epilogue").unwrap();
        std::fs::write(dir.path().join("chapter1.md"), "# Ch 1").unwrap();

        let chapters = discover_chapters(dir.path()).unwrap();
        let names: Vec<_> = chapters
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        // Numeric first, then alphabetical
        assert_eq!(names[0], "chapter1.md");
        // epilogue and prologue are alphabetical after numeric
        assert_eq!(names[1], "epilogue.md");
        assert_eq!(names[2], "prologue.md");
    }

    #[test]
    fn test_create_backup() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("chapter01.md");
        std::fs::write(&source, "# Chapter 1\n\nContent here.").unwrap();

        let backup_path = create_backup(dir.path(), &source).unwrap();
        assert!(backup_path.exists());
        assert!(backup_path.to_str().unwrap().contains("chapter01_"));
        assert!(backup_path.to_str().unwrap().ends_with(".md"));

        let content = std::fs::read_to_string(&backup_path).unwrap();
        assert_eq!(content, "# Chapter 1\n\nContent here.");
    }

    #[test]
    fn test_create_backup_creates_backup_dir() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("test.md");
        std::fs::write(&source, "content").unwrap();

        let backup_dir = dir.path().join(".cipher").join("backups");
        assert!(!backup_dir.exists());

        create_backup(dir.path(), &source).unwrap();
        assert!(backup_dir.exists());
    }

    #[test]
    fn test_checkpoint_chapter_progress_does_not_advance_glossary_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let previous_glossary = vec![glossary_term("Hero", Some("hero"), "Old definition")];
        let previous_state = previous_glossary_state(&previous_glossary, InjectionMode::Smart);
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
            &translate_options(false, false, false),
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
        let previous_state = previous_glossary_state(&previous_glossary, InjectionMode::Full);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            &translate_options(false, true, false),
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
        let previous_state = previous_glossary_state(&current_glossary, InjectionMode::Smart);
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
            &translate_options(false, true, false),
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
        let previous_state = previous_glossary_state(&[], InjectionMode::Smart);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let smart_selection =
            select_terms_for_text(&current_glossary, smart_text(), InjectionMode::Smart);
        let fallback_selection = select_terms_for_text(&current_glossary, "勇者", InjectionMode::Smart);
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
            &translate_options(false, true, false),
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
        let previous_glossary_state = previous_glossary_state(&glossary, InjectionMode::Full);
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

        let migrated_usage = chapter_states["chapter1.md"].glossary_usage.as_ref().unwrap();
        assert_eq!(migrated_usage.injection_mode, GlossaryInjectionMode::Smart);
        assert!(migrated_usage.used_fallback_to_full);

        let loaded_chapter = load_chapter_state(dir.path(), "chapter1.md").unwrap().unwrap();
        let loaded_usage = loaded_chapter.glossary_usage.unwrap();
        assert_eq!(loaded_usage.injection_mode, GlossaryInjectionMode::Smart);
        assert!(loaded_usage.used_fallback_to_full);

        let loaded_glossary_state = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_eq!(loaded_glossary_state.injection_mode, GlossaryInjectionMode::Smart);
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
        let previous_glossary_state = previous_glossary_state(&glossary, InjectionMode::Full);
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

        let migrated_usage = chapter_states["chapter1.md"].glossary_usage.as_ref().unwrap();
        assert_eq!(migrated_usage.injection_mode, GlossaryInjectionMode::Full);
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
        let previous_state = previous_glossary_state(&previous_glossary, InjectionMode::Smart);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            &translate_options(false, true, false),
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
        let previous_state = previous_glossary_state(&previous_glossary, InjectionMode::Smart);
        save_glossary_state(dir.path(), &previous_state).unwrap();

        let outcome = finalize_glossary_baseline(
            dir.path(),
            &translate_options(false, false, false),
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
    fn test_skipped_chapter_source_hash_backfills_legacy_hash_during_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        let chapter_text = "# Chapter 1\n\nSource text\n";
        std::fs::write(&raw_path, chapter_text).unwrap();

        let previous_chapter_state = previous_chapter_state_with_hash(None);

        let source_text_hash = skipped_chapter_source_hash(
            &raw_path,
            Some(&previous_chapter_state),
            &translate_options(false, false, true),
        )
        .unwrap();

        assert_eq!(
            source_text_hash,
            Some(normalized_source_text_hash(chapter_text))
        );
    }

    #[test]
    fn test_skipped_chapter_source_hash_keeps_legacy_hash_untracked_without_rerun_flag() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        std::fs::write(&raw_path, "# Chapter 1\n\nSource text\n").unwrap();

        let previous_chapter_state = previous_chapter_state_with_hash(None);

        let source_text_hash = skipped_chapter_source_hash(
            &raw_path,
            Some(&previous_chapter_state),
            &translate_options(false, false, false),
        )
        .unwrap();

        assert_eq!(source_text_hash, None);
    }

    #[test]
    fn test_failed_chapter_source_hash_preserves_previous_hash() {
        let previous_hash = normalized_source_text_hash("old source text");
        let previous_chapter_state = previous_chapter_state_with_hash(Some(&previous_hash));

        let source_text_hash =
            failed_chapter_source_hash(Some(&previous_chapter_state), "new-source-hash");

        assert_eq!(source_text_hash, Some(previous_hash));
    }

    #[test]
    fn test_failed_chapter_source_hash_uses_current_hash_when_untracked() {
        let previous_chapter_state = previous_chapter_state_with_hash(None);

        let source_text_hash =
            failed_chapter_source_hash(Some(&previous_chapter_state), "new-source-hash");

        assert_eq!(source_text_hash, Some("new-source-hash".to_string()));
    }

    #[test]
    fn test_translate_options_rerun_enables_both_rerun_modes() {
        let options = translate_options(true, false, false);

        assert!(options.rerun_glossary_enabled());
        assert!(options.rerun_chapters_enabled());
    }

    #[test]
    fn test_skipped_chapter_source_hash_backfills_legacy_hash_during_combined_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        let chapter_text = "# Chapter 1\n\nSource text\n";
        std::fs::write(&raw_path, chapter_text).unwrap();

        let previous_chapter_state = previous_chapter_state_with_hash(None);

        let source_text_hash = skipped_chapter_source_hash(
            &raw_path,
            Some(&previous_chapter_state),
            &translate_options(true, false, false),
        )
        .unwrap();

        assert_eq!(
            source_text_hash,
            Some(normalized_source_text_hash(chapter_text))
        );
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

        assert_eq!(usage.injection_mode, GlossaryInjectionMode::Smart);
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

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Full);
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
        assert!(decision.reason.contains("Full glossary changed"));
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

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);
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
        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);

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

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Full);

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
        let previous_glossary_state = previous_glossary_state(&glossary, InjectionMode::Smart);

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

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);
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
        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);

        let old_selection = select_terms_for_text(&old_glossary, smart_text(), InjectionMode::Smart);
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

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);
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
        // Test that adding a new glossary term that matches chapter text
        // triggers a rerun for tracked smart-mode chapters
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        // Text contains all 6 terms including "竜王" (Dragon King)
        let chapter_text = "勇者は魔導士と聖剣を手に王城で戦い竜王と戦った。";
        std::fs::write(&chapter, chapter_text).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        // Old glossary: has 5 terms to avoid fallback, but NOT "Dragon King" (竜王)
        // Need at least 5 terms to avoid fallback (MIN_GLOSSARY_MATCHES = 5)
        let old_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Battle", Some("戦い"), "Battle definition"),
            // Note: Dragon King (竜王) is NOT in old glossary
        ];

        // New glossary: now includes "Dragon King" (竜王) which appears in the text
        let new_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Battle", Some("戦い"), "Battle definition"),
            glossary_term("Dragon King", Some("竜王"), "Dragon King definition"),
        ];

        let selection = select_terms_for_text(&old_glossary, chapter_text, InjectionMode::Smart);
        // All 5 terms from old glossary matched (戦い appears in text)
        assert!(!selection.used_fallback_to_full, "Selection used fallback");

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);
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

        // The new glossary has 1 added term
        assert_eq!(plan.changed_term_count, 1);
        // Chapter should be flagged for rerun because Dragon King now matches
        assert_eq!(plan.forced_chapters.len(), 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        assert!(decision.reason.contains("竜王") || decision.reason.contains("dragon king"));
    }

    #[test]
    fn test_build_glossary_rerun_plan_detects_removed_previously_matched_term() {
        // Test that removing a glossary term that was previously selected
        // triggers a rerun for tracked smart-mode chapters
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");
        std::fs::create_dir_all(&raw_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        let chapter = raw_dir.join("chapter1.md");
        // Text with 6 terms: 勇者, 魔導士, 聖剣, 王城, 戦い, 竜王
        let chapter_text = "勇者は魔導士と聖剣を手に王城で戦い竜王と戦った。";
        std::fs::write(&chapter, chapter_text).unwrap();
        std::fs::write(out_dir.join("chapter1.md"), "translated").unwrap();

        // Old glossary: has all 6 terms including "Dragon King" (竜王)
        let old_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Battle", Some("戦い"), "Battle definition"),
            glossary_term("Dragon King", Some("竜王"), "Dragon King definition"),
        ];

        // New glossary: "Dragon King" and "Battle" removed, replaced with "Shield" which doesn't appear
        let new_glossary = vec![
            glossary_term("Hero", Some("勇者"), "Hero definition"),
            glossary_term("Mage", Some("魔導士"), "Mage definition"),
            glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
            glossary_term("Royal Castle", Some("王城"), "Castle definition"),
            glossary_term("Shield", Some("盾"), "Shield definition"),
        ];

        let selection = select_terms_for_text(&old_glossary, chapter_text, InjectionMode::Smart);
        // Should have 6 terms matched without fallback
        assert!(!selection.used_fallback_to_full, "Selection used fallback");

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);
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

        // Chapter should be flagged for rerun because selection changed (Dragon King removed)
        // changed_term_count is 3: 2 removed (竜王, 戦い) + 1 added (盾)
        assert_eq!(plan.changed_term_count, 3);
        assert_eq!(plan.forced_chapters.len(), 1);
        let decision = plan.forced_chapters.get("chapter1.md").unwrap();
        // Reason should indicate the selection changed
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

        let previous_glossary_state = previous_glossary_state(&old_glossary, InjectionMode::Smart);

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
            &[chapter2.clone(), chapter3],
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
        assert!(!plan.forced_chapters.contains_key("chapter1.md"));
    }
}
