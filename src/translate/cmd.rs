use crate::book::paths::{chapter_output_path, chapter_state_key, discover_chapters};
use crate::book::{
    BookLayout, OutputConfig, StructuredChapter, init::BookConfig, load_book_config,
    render_chapter_markdown, render_starts_with_markdown_heading, validate_structured_chapter,
};
use crate::config::{GlobalConfig, validate_profile};
use crate::glossary::{
    GlossaryTerm, InjectionMode, SelectionResult, book_config_injection_mode, glossary_term_key,
    glossary_term_prompt_fingerprint, load_glossary, merge_terms, save_glossary,
    select_terms_for_text,
};
use crate::output;
use crate::output::{
    stderr_detail, stderr_detail_kv, stderr_status as output_status, stderr_warn, verbose_detail,
    verbose_detail_kv,
};
use crate::state::{
    ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterState, ChapterStatus, RunMetadata,
    RunOptions, load_all_chapter_states, load_glossary_state, normalized_source_text_hash,
    save_chapter_state, save_run_metadata,
};
use crate::translate::preview::preview_translation_run;
use crate::translate::rerun::{
    ChapterRerunDecision, EMPTY_CHAPTER_SKIP_REASON, GlossaryRerunPlan, SourceRerunPlan,
    build_chapter_glossary_usage, build_glossary_rerun_plan, build_glossary_state,
    build_source_rerun_plan, chapter_translation_injection_mode, combine_rerun_decisions,
    finalize_glossary_baseline, migrate_legacy_full_tracking,
};
use crate::translate::{
    AcceptedTranslation, ProviderTranslationResult, TranslationUsage, Translator,
};
use crate::validate::{ValidationOptions, validate_translation};
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::io;

pub struct TranslateOptions {
    pub profile: Option<String>,
    pub repair_profile: Option<String>,
    pub glossary_profile: Option<String>,
    pub overwrite: bool,
    pub fail_fast: bool,
    pub rerun: Option<crate::RerunMode>,
    pub dry_run: bool,
}

struct TranslateProfiles<'a> {
    translation_name: &'a str,
    repair_name: &'a str,
    glossary_name: &'a str,
}

struct Translators {
    translation: Translator,
    repair: Translator,
    glossary: Translator,
}

fn resolve_translate_profiles<'a>(
    global_config: &'a GlobalConfig,
    book_config: &'a BookConfig,
    options: &'a TranslateOptions,
) -> Option<TranslateProfiles<'a>> {
    let translation_name = options
        .profile
        .as_deref()
        .or_else(|| global_config.effective_profile_name(book_config.profile.as_deref()))?;
    let repair_name = options
        .repair_profile
        .as_deref()
        .or(book_config.repair_profile.as_deref())
        .unwrap_or(translation_name);
    let glossary_name = options
        .glossary_profile
        .as_deref()
        .or(book_config.glossary_profile.as_deref())
        .unwrap_or(translation_name);

    Some(TranslateProfiles {
        translation_name,
        repair_name,
        glossary_name,
    })
}

fn validate_translate_profiles(
    config: &GlobalConfig,
    profiles: &TranslateProfiles<'_>,
) -> Result<()> {
    for (label, name) in [
        ("translation", profiles.translation_name),
        ("repair", profiles.repair_name),
        ("glossary", profiles.glossary_name),
    ] {
        let validation = validate_profile(config, name);
        if !validation.is_valid() {
            output_status(format!("{} profile validation failed", label));
            for error in &validation.errors {
                stderr_detail(error);
            }
            anyhow::bail!("Cannot translate with invalid {} profile", label);
        }
    }

    Ok(())
}

fn print_profile_details(config: &GlobalConfig, profiles: &TranslateProfiles<'_>) {
    print_profile_detail(config, "Translation profile", profiles.translation_name);
    if profiles.repair_name != profiles.translation_name {
        print_profile_detail(config, "Repair profile", profiles.repair_name);
    }
    if profiles.glossary_name != profiles.translation_name {
        print_profile_detail(config, "Glossary profile", profiles.glossary_name);
    }
}

fn print_profile_detail(config: &GlobalConfig, label: &str, name: &str) {
    verbose_detail_kv(label, name);
    match config.resolve_profile(name) {
        Some(profile) => {
            verbose_detail_kv("Provider", &profile.provider);
            verbose_detail_kv("Model", &profile.model);
        }
        None => verbose_detail_kv("Profile status", "not found"),
    }
}
impl TranslateOptions {
    fn rerun_glossary_enabled(&self) -> bool {
        matches!(
            self.rerun,
            Some(crate::RerunMode::All) | Some(crate::RerunMode::Glossary)
        )
    }

    fn rerun_chapters_enabled(&self) -> bool {
        matches!(
            self.rerun,
            Some(crate::RerunMode::All) | Some(crate::RerunMode::Source)
        )
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

#[derive(Debug, Clone, Default)]
struct PreviousChapterArtifacts {
    translation_usage: Option<TranslationUsage>,
    glossary_usage: Option<ChapterGlossaryUsage>,
    exported_terms: Vec<ChapterGlossaryTerm>,
}

pub async fn translate_book(book_dir: &Path, options: TranslateOptions) -> Result<i32> {
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

    let book_config = load_book_config(&layout.paths.config_toml).unwrap_or_default();
    let injection_mode = book_config_injection_mode(&book_config.glossary_injection);

    // Discover chapters
    let chapters: VecDeque<PathBuf> = discover_chapters(&layout.paths.raw_dir)?
        .into_iter()
        .collect();
    if chapters.is_empty() {
        output_status("No chapters found");
        stderr_detail_kv("Directory", layout.paths.raw_dir.display());
        return Ok(0);
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

    // Load previous checkpointed state for glossary diffing
    let previous_glossary_state = load_glossary_state(book_dir)?;
    let mut previous_chapter_states = load_all_chapter_states(book_dir)?;

    let rerun_plan = if options.rerun_glossary_enabled() {
        verbose_detail_kv("Planning", "glossary-affected chapter reruns");
        let plan = build_glossary_rerun_plan(
            &Vec::from(chapters.clone()),
            &layout.paths.raw_dir,
            out_dir,
            previous_glossary_state.as_ref(),
            &previous_chapter_states,
            &glossary,
            injection_mode,
        )?;
        verbose_detail_kv("Changed glossary entries", plan.changed_term_count);
        verbose_detail_kv("Affected chapters", plan.forced_chapters.len());
        if plan.approximate_smart_checks > 0 {
            verbose_detail_kv(
                "Approximate smart rerun checks",
                plan.approximate_smart_checks,
            );
        }
        for warning in &plan.warnings {
            stderr_warn(warning);
        }
        plan
    } else {
        GlossaryRerunPlan::default()
    };

    let source_rerun_plan = if options.rerun_chapters_enabled() {
        verbose_detail_kv("Planning", "source-affected chapter reruns");
        let plan = build_source_rerun_plan(
            &Vec::from(chapters.clone()),
            &layout.paths.raw_dir,
            out_dir,
            &previous_chapter_states,
        )?;
        verbose_detail_kv("Affected chapters", plan.forced_chapters.len());
        if plan.untracked_chapters > 0 {
            verbose_detail_kv("Untracked chapters", plan.untracked_chapters);
        }
        plan
    } else {
        SourceRerunPlan::default()
    };

    if options.dry_run {
        output_status("Translation preview");
        stderr_detail_kv("Book", book_dir.display());
        if let Some(profile_names) =
            resolve_translate_profiles(&global_config, &book_config, &options)
        {
            print_profile_details(&global_config, &profile_names);
        }
        return preview_translation_run(
            &chapters,
            &layout.paths.raw_dir,
            out_dir,
            &options,
            &rerun_plan,
            &source_rerun_plan,
        );
    }

    let profile_names = resolve_translate_profiles(&global_config, &book_config, &options)
        .ok_or_else(|| {
            anyhow::anyhow!("No profile configured. Run 'cipher profile new' to create one.")
        })?;

    validate_translate_profiles(&global_config, &profile_names)?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("Failed to create output directory {}", out_dir.display()))?;

    let profile = global_config
        .resolve_profile(profile_names.translation_name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", profile_names.translation_name))?;

    verbose_detail_kv("Using profiles", "");
    print_profile_details(&global_config, &profile_names);
    if style_guide.is_some() {
        verbose_detail_kv("Style guide", layout.paths.style_md.display());
    }

    let translators = Translators {
        translation: Translator::from_config(&global_config, profile_names.translation_name)
            .context("Failed to create translation translator")?,
        repair: Translator::from_config(&global_config, profile_names.repair_name)
            .context("Failed to create repair translator")?,
        glossary: Translator::from_config(&global_config, profile_names.glossary_name)
            .context("Failed to create glossary translator")?,
    };

    output_status("Translating chapters");
    verbose_detail_kv("Chapters found", chapters.len());

    let pb = if output::is_quiet() {
        None
    } else {
        let bar = ProgressBar::new(chapters.len() as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({msg})",
            )
            .unwrap()
            .progress_chars("#>-"),
        );
        bar.set_message("translating");
        Some(bar)
    };

    // Create run state with options
    let run_options = RunOptions {
        overwrite: options.overwrite,
        fail_fast: options.fail_fast,
        rerun: options.rerun.is_some(),
        rerun_affected_glossary: options.rerun_glossary_enabled(),
        rerun_affected_chapters: options.rerun_chapters_enabled(),
    };

    let mut run_metadata = RunMetadata::new(
        profile_names.translation_name.to_string(),
        profile.provider.clone(),
        profile.model.clone(),
        Some(run_options),
    )
    .with_task_profiles(
        (profile_names.repair_name != profile_names.translation_name)
            .then(|| profile_names.repair_name.to_string()),
        (profile_names.glossary_name != profile_names.translation_name)
            .then(|| profile_names.glossary_name.to_string()),
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

        if let Some(ref pb) = pb {
            pb.set_message(chapter_path.clone());
        }

        let result = translate_single_chapter(
            &translators,
            &chapter_file,
            &out_path,
            &chapter_path,
            &options,
            previous_chapter_state,
            rerun_decision.as_ref(),
            &mut glossary,
            &style_guide,
            &book_config.output,
            injection_mode,
            &layout.paths.glossary_json,
            book_dir,
        )
        .await?;

        checkpoint_chapter_progress(book_dir, &mut run_metadata, &result.chapter_state)?;
        previous_chapter_states.insert(chapter_path.clone(), result.chapter_state.clone());
        if let Some(ref pb) = pb {
            pb.inc(1);
        }

        if result.translated {
            translated += 1;
        }
        if result.skipped {
            skipped += 1;
        }
        if result.failed {
            failed += 1;
            if options.fail_fast {
                verbose_detail("Stopping due to --fail-fast");
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
        options.rerun_glossary_enabled(),
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
        stderr_warn(format!(
            "Glossary baseline was not updated because {} affected chapter(s) still need reruns.",
            baseline_outcome.remaining_forced_chapters
        ));
    }

    run_metadata.mark_finished();
    save_run_metadata(book_dir, &run_metadata)?;

    if let Some(pb) = pb {
        pb.finish_with_message("done");
    }

    // Print summary
    output_status("Translation complete");
    stderr_detail_kv("Translated", translated);
    stderr_detail_kv("Skipped", skipped);
    stderr_detail_kv("Failed", failed);
    stderr_detail_kv("Glossary terms added", new_glossary_terms);
    if legacy_tracking_migration.migrated_chapters > 0 {
        stderr_detail_kv(
            "Legacy chapters migrated",
            legacy_tracking_migration.migrated_chapters,
        );
    }
    if legacy_tracking_migration.migrated_glossary_baseline {
        stderr_detail("Migrated legacy full-glossary baseline to canonical smart tracking");
    }
    if total_usage.total_tokens > 0 {
        print_usage_info_with_label("Token usage", &total_usage);
    }

    if failed > 0 {
        return Ok(2);
    }

    Ok(0)
}

#[allow(clippy::too_many_arguments)]
async fn translate_single_chapter(
    translators: &Translators,
    raw_path: &Path,
    out_path: &Path,
    chapter_path: &str,
    options: &TranslateOptions,
    previous_chapter_state: Option<&ChapterState>,
    rerun_decision: Option<&ChapterRerunDecision>,
    glossary: &mut Vec<GlossaryTerm>,
    style_guide: &Option<String>,
    output_config: &OutputConfig,
    injection_mode: InjectionMode,
    glossary_path: &Path,
    book_dir: &Path,
) -> Result<ChapterResult> {
    let translation_injection_mode =
        chapter_translation_injection_mode(injection_mode, rerun_decision);
    let previous_artifacts = previous_chapter_artifacts(previous_chapter_state);

    // Check if output exists
    let output_exists = out_path.exists();
    if !options.overwrite && output_exists && rerun_decision.is_none() {
        let source_text_hash =
            skipped_chapter_source_hash(raw_path, previous_chapter_state, options)?;

        return Ok(build_skipped_chapter_result(
            chapter_path,
            None,
            None,
            previous_artifacts,
            source_text_hash,
        ));
    }

    // Read chapter
    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;
    let source_text_hash = normalized_source_text_hash(&chapter_text);

    if chapter_text.trim().is_empty() {
        output_status(format!("Skip {}: chapter is empty", chapter_path));
        return Ok(build_skipped_chapter_result(
            chapter_path,
            Some(EMPTY_CHAPTER_SKIP_REASON.to_string()),
            None,
            previous_artifacts,
            Some(source_text_hash),
        ));
    }

    if let Some(decision) = rerun_decision {
        verbose_detail_kv("Rerun reason", &decision.reason);
    }

    // Select glossary terms and display info
    let start = Instant::now();
    let selection = select_terms_for_text(glossary, &chapter_text, translation_injection_mode);
    print_glossary_info(&selection, translation_injection_mode);

    // Attempt translation with retries
    let (response, last_error, failed_usage) = attempt_translation(
        translators,
        &chapter_text,
        &selection,
        glossary,
        style_guide,
        output_config,
    )
    .await;

    let duration = start.elapsed();

    if let Some(resp) = response {
        print_usage_info(&resp.usage);

        // Backup if overwriting existing file
        if output_exists {
            let backup_path = create_backup(book_dir, out_path)?;
            verbose_detail_kv("Backup", backup_path.display());
        }

        let rendered_translation = render_chapter_markdown(&resp.response.chapter, output_config);

        // Write output atomically
        io::atomic_write(out_path, &rendered_translation)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;

        // Merge glossary terms
        let (new_terms_added, exported_terms) =
            merge_new_glossary_terms(glossary, resp.response.new_glossary_terms, glossary_path)?;

        verbose_detail_kv("Result", "success");
        return Ok(build_success_chapter_result(
            chapter_path,
            duration.as_millis() as u64,
            resp.usage,
            build_chapter_glossary_usage(&selection, translation_injection_mode),
            exported_terms,
            source_text_hash,
            new_terms_added,
        ));
    }

    let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
    verbose_detail_kv("Result", "failed");
    verbose_detail_kv("Error", &error_msg);
    let failed_source_text_hash =
        failed_chapter_source_hash(previous_chapter_state, &source_text_hash);
    Ok(build_failed_chapter_result(
        chapter_path,
        error_msg,
        duration.as_millis() as u64,
        failed_usage,
        previous_artifacts,
        failed_source_text_hash,
    ))
}

fn previous_chapter_artifacts(
    previous_chapter_state: Option<&ChapterState>,
) -> PreviousChapterArtifacts {
    PreviousChapterArtifacts {
        translation_usage: previous_chapter_state.and_then(|state| state.translation_usage.clone()),
        glossary_usage: previous_chapter_state.and_then(|state| state.glossary_usage.clone()),
        exported_terms: previous_chapter_state
            .map(|state| state.exported_terms.clone())
            .unwrap_or_default(),
    }
}

fn build_skipped_chapter_result(
    chapter_path: &str,
    message: Option<String>,
    duration_ms: Option<u64>,
    previous_artifacts: PreviousChapterArtifacts,
    source_text_hash: Option<String>,
) -> ChapterResult {
    ChapterResult {
        translated: false,
        failed: false,
        skipped: true,
        new_terms_added: 0,
        usage: None,
        chapter_state: ChapterState::new(
            chapter_path.to_string(),
            ChapterStatus::Skipped,
            message,
            duration_ms,
            previous_artifacts.translation_usage,
            previous_artifacts.glossary_usage,
            previous_artifacts.exported_terms,
            source_text_hash,
        ),
    }
}

fn build_success_chapter_result(
    chapter_path: &str,
    duration_ms: u64,
    usage: TranslationUsage,
    glossary_usage: ChapterGlossaryUsage,
    exported_terms: Vec<ChapterGlossaryTerm>,
    source_text_hash: String,
    new_terms_added: usize,
) -> ChapterResult {
    ChapterResult {
        translated: true,
        failed: false,
        skipped: false,
        new_terms_added,
        usage: Some(usage.clone()),
        chapter_state: ChapterState::new(
            chapter_path.to_string(),
            ChapterStatus::Success,
            None,
            Some(duration_ms),
            Some(usage),
            Some(glossary_usage),
            exported_terms,
            Some(source_text_hash),
        ),
    }
}

fn build_failed_chapter_result(
    chapter_path: &str,
    error_msg: String,
    duration_ms: u64,
    usage: Option<TranslationUsage>,
    previous_artifacts: PreviousChapterArtifacts,
    source_text_hash: Option<String>,
) -> ChapterResult {
    ChapterResult {
        translated: false,
        failed: true,
        skipped: false,
        new_terms_added: 0,
        usage: usage.clone(),
        chapter_state: ChapterState::new(
            chapter_path.to_string(),
            ChapterStatus::Failed,
            Some(error_msg),
            Some(duration_ms),
            usage,
            previous_artifacts.glossary_usage,
            previous_artifacts.exported_terms,
            source_text_hash,
        ),
    }
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

pub(crate) fn checkpoint_chapter_progress(
    book_dir: &Path,
    run_metadata: &mut RunMetadata,
    chapter_state: &ChapterState,
) -> Result<()> {
    save_chapter_state(book_dir, chapter_state)?;
    run_metadata.touch();
    save_run_metadata(book_dir, run_metadata)
}

fn print_glossary_info(selection: &SelectionResult, injection_mode: InjectionMode) {
    match injection_mode {
        InjectionMode::Smart => {
            if selection.used_fallback_to_full {
                verbose_detail_kv(
                    "Glossary",
                    format!(
                        "smart fallback to full, {}/{} terms",
                        selection.selected_count, selection.total_count
                    ),
                );
            } else {
                verbose_detail_kv(
                    "Glossary",
                    format!(
                        "smart selection, {}/{} terms",
                        selection.selected_count, selection.total_count
                    ),
                );
            }
        }
        InjectionMode::Full => {
            verbose_detail_kv("Glossary", format!("full, {} terms", selection.total_count));
        }
    }
}

const MAX_API_RETRIES: usize = 3;

async fn attempt_translation(
    translators: &Translators,
    chapter_text: &str,
    selection: &SelectionResult,
    glossary: &[GlossaryTerm],
    style_guide: &Option<String>,
    output_config: &OutputConfig,
) -> (
    Option<ProviderTranslationResult>,
    Option<String>,
    Option<TranslationUsage>,
) {
    let mut last_error: Option<String> = None;
    let mut failed_usage: Option<TranslationUsage> = None;
    let validation_options = ValidationOptions {
        require_markdown_heading: render_starts_with_markdown_heading(output_config),
    };

    for api_attempt in 1..=MAX_API_RETRIES {
        match translators
            .translation
            .translate_chapter(
                chapter_text,
                &selection.terms,
                style_guide.clone(),
                output_config.clone(),
            )
            .await
        {
            Ok(resp) => {
                let rendered = render_chapter_markdown(&resp.chapter, output_config);
                let mut validation_errors =
                    validate_structured_chapter(&resp.chapter, output_config);
                let rendered_validation = validate_translation(&rendered, validation_options);
                validation_errors.extend(rendered_validation.errors().iter().cloned());
                let original_translation_usage = resp.usage.clone();
                failed_usage = Some(original_translation_usage.clone());

                if validation_errors.is_empty() {
                    let result = finish_accepted_translation(
                        &translators.glossary,
                        chapter_text,
                        resp.chapter,
                        rendered,
                        glossary,
                        resp.usage,
                    )
                    .await;
                    return (Some(result), None, None);
                }

                last_error = Some(format!(
                    "Validation failed: {}",
                    validation_errors.join(", ")
                ));

                if api_attempt == 1 {
                    verbose_detail_kv(
                        "Validation",
                        format!("{} Attempting repair.", validation_errors.join(", ")),
                    );

                    match translators
                        .repair
                        .repair_chapter(
                            chapter_text,
                            rendered,
                            &selection.terms,
                            style_guide.clone(),
                            validation_errors,
                            output_config.clone(),
                        )
                        .await
                    {
                        Ok(repair_resp) => {
                            let repaired_rendered =
                                render_chapter_markdown(&repair_resp.chapter, output_config);
                            let mut combined_usage = original_translation_usage.clone();
                            combined_usage += repair_resp.usage.clone();
                            failed_usage = Some(combined_usage.clone());
                            let mut repair_errors =
                                validate_structured_chapter(&repair_resp.chapter, output_config);
                            let repair_validation =
                                validate_translation(&repaired_rendered, validation_options);
                            repair_errors.extend(repair_validation.errors().iter().cloned());

                            if repair_errors.is_empty() {
                                verbose_detail_kv("Repair", "success");
                                let result = finish_accepted_translation(
                                    &translators.glossary,
                                    chapter_text,
                                    repair_resp.chapter,
                                    repaired_rendered,
                                    glossary,
                                    combined_usage,
                                )
                                .await;
                                return (Some(result), None, None);
                            } else {
                                last_error = Some(format!(
                                    "Repair failed validation: {}",
                                    repair_errors.join(", ")
                                ));
                                verbose_detail_kv("Repair", last_error.as_ref().unwrap());
                            }
                        }
                        Err(e) => {
                            last_error = Some(format!("Repair request failed: {}", e));
                            verbose_detail_kv("Repair", last_error.as_ref().unwrap());
                        }
                    }
                }

                break;
            }
            Err(e) => {
                last_error = Some(format!("API error: {}", e));
                if api_attempt < MAX_API_RETRIES {
                    let delay_secs = 2u64.pow(api_attempt as u32);
                    verbose_detail_kv(
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

    (None, last_error, failed_usage)
}

async fn finish_accepted_translation(
    translator: &Translator,
    chapter_text: &str,
    chapter: StructuredChapter,
    rendered_markdown: String,
    glossary: &[GlossaryTerm],
    mut usage: TranslationUsage,
) -> ProviderTranslationResult {
    let new_glossary_terms = match translator
        .extract_glossary(chapter_text, rendered_markdown, glossary)
        .await
    {
        Ok(glossary_resp) => {
            usage += glossary_resp.usage;
            verbose_detail_kv("Glossary extraction", "success");
            glossary_resp.new_glossary_terms
        }
        Err(e) => {
            verbose_detail_kv(
                "Glossary extraction",
                format!("failed: {}. Chapter kept without new terms.", e),
            );
            Vec::new()
        }
    };

    ProviderTranslationResult {
        response: AcceptedTranslation {
            chapter,
            new_glossary_terms,
        },
        usage,
    }
}

fn print_usage_info(usage: &TranslationUsage) {
    print_usage_info_with_label("Usage", usage);
}

fn print_usage_info_with_label(label: &str, usage: &TranslationUsage) {
    verbose_detail_kv(
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
            verbose_detail(format!(
                "Added {} glossary {}; skipped {} duplicate{}.",
                added,
                pluralize(added, "term", "terms"),
                dupes,
                pluralize(dupes, "", "s")
            ));
        } else {
            verbose_detail(format!(
                "Added {} glossary {}.",
                added,
                pluralize(added, "term", "terms")
            ));
        }
        save_glossary(glossary_path, glossary)?;
    } else if dupes > 0 {
        verbose_detail(format!(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::providers::Provider;
    use crate::translate::test_helpers::translate_options;
    use crate::translate::{GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult};

    struct FailingGlossaryProvider;

    #[async_trait::async_trait]
    impl Provider for FailingGlossaryProvider {
        async fn translate(
            &self,
            _req: crate::translate::TranslationRequest,
        ) -> Result<ProviderTextResult> {
            unreachable!()
        }
        async fn repair(
            &self,
            _req: crate::translate::RepairRequest,
        ) -> Result<ProviderTextResult> {
            unreachable!()
        }
        async fn extract_glossary(
            &self,
            _req: GlossaryExtractionRequest,
        ) -> Result<ProviderGlossaryResult> {
            Err(anyhow::anyhow!("extractor unavailable"))
        }
    }

    fn profile_options(
        profile: Option<&str>,
        repair_profile: Option<&str>,
        glossary_profile: Option<&str>,
    ) -> TranslateOptions {
        TranslateOptions {
            profile: profile.map(str::to_string),
            repair_profile: repair_profile.map(str::to_string),
            glossary_profile: glossary_profile.map(str::to_string),
            overwrite: false,
            fail_fast: false,
            rerun: None,
            dry_run: false,
        }
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

    #[test]
    fn test_resolve_translate_profiles_defaults_to_translation_profile() {
        let config = GlobalConfig {
            default_profile: Some("default".to_string()),
            ..GlobalConfig::default()
        };
        let book_config = BookConfig::default();
        let options = profile_options(None, None, None);

        let profiles = resolve_translate_profiles(&config, &book_config, &options).unwrap();

        assert_eq!(profiles.translation_name, "default");
        assert_eq!(profiles.repair_name, "default");
        assert_eq!(profiles.glossary_name, "default");
    }

    #[test]
    fn test_resolve_translate_profiles_uses_task_overrides() {
        let config = GlobalConfig::default();
        let mut book_config = BookConfig::with_profile("book");
        book_config.repair_profile = Some("book-repair".to_string());
        book_config.glossary_profile = Some("book-glossary".to_string());
        let options = profile_options(Some("cli"), Some("cli-repair"), Some("cli-glossary"));

        let profiles = resolve_translate_profiles(&config, &book_config, &options).unwrap();

        assert_eq!(profiles.translation_name, "cli");
        assert_eq!(profiles.repair_name, "cli-repair");
        assert_eq!(profiles.glossary_name, "cli-glossary");
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
    fn test_skipped_chapter_source_hash_backfills_legacy_hash_during_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        let chapter_text = "# Chapter 1\n\nSource text\n";
        std::fs::write(&raw_path, chapter_text).unwrap();

        let previous_chapter_state = previous_chapter_state_with_hash(None);

        let source_text_hash = skipped_chapter_source_hash(
            &raw_path,
            Some(&previous_chapter_state),
            &translate_options(Some(crate::RerunMode::Source)),
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
            &translate_options(None),
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
    fn test_failed_chapter_result_preserves_usage() {
        let usage = TranslationUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            cached_input_tokens: 4,
            cache_creation_input_tokens: 5,
        };

        let result = build_failed_chapter_result(
            "chapter1.md",
            "Validation failed".to_string(),
            100,
            Some(usage.clone()),
            PreviousChapterArtifacts::default(),
            Some("source-hash".to_string()),
        );

        assert_eq!(result.usage, Some(usage.clone()));
        assert_eq!(result.chapter_state.translation_usage, Some(usage));
    }

    #[tokio::test]
    async fn test_glossary_extraction_failure_keeps_accepted_chapter() {
        let translator = Translator {
            provider: Box::new(FailingGlossaryProvider),
        };
        let chapter = StructuredChapter {
            chapter_number: Some("1".to_string()),
            chapter_title: Some("Opening".to_string()),
            content: "Translated body".to_string(),
        };
        let usage = TranslationUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            cached_input_tokens: 4,
            cache_creation_input_tokens: 5,
        };

        let result = finish_accepted_translation(
            &translator,
            "# Chapter 1\n\nSource",
            chapter.clone(),
            "# Chapter 1: Opening\n\nTranslated body".to_string(),
            &[],
            usage.clone(),
        )
        .await;

        assert_eq!(result.response.chapter.content, chapter.content);
        assert!(result.response.new_glossary_terms.is_empty());
        assert_eq!(result.usage, usage);
    }

    #[test]
    fn test_translate_options_rerun_enables_both_rerun_modes() {
        let options = translate_options(Some(crate::RerunMode::All));

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
            &translate_options(Some(crate::RerunMode::All)),
        )
        .unwrap();

        assert_eq!(
            source_text_hash,
            Some(normalized_source_text_hash(chapter_text))
        );
    }
}
