use crate::book::paths::{chapter_output_path, chapter_state_key, discover_chapters};
use crate::book::{BookLayout, OutputConfig, load_book_config};
use crate::config::GlobalConfig;
use crate::error::{Error, Result};
use crate::glossary::{GlossaryTerm, InjectionMode, book_config_injection_mode, load_glossary};
use crate::output;
use crate::output::{
    stderr_detail_kv, stderr_status, stderr_warn, verbose_detail, verbose_detail_kv,
};
use crate::state::{
    ChapterState, GlossaryState, RunMetadata, RunOptions, load_all_chapter_states,
    load_glossary_state, save_run_metadata,
};
use crate::translate::orchestrate::{
    ChapterContext, ChapterPaths, ChapterResult, Translators, checkpoint_chapter_progress,
    print_profile_details, resolve_translate_profiles, translate_single_chapter,
    validate_translate_profiles,
};
use crate::translate::preview::{build_preview_data, preview_translation_run};
use crate::translate::rerun::{
    GlossaryRerunPlan, SourceRerunPlan, build_glossary_rerun_plan, build_glossary_state,
    build_source_rerun_plan, combine_rerun_decisions, finalize_glossary_baseline,
    migrate_legacy_full_tracking,
};
use crate::translate::{TranslationUsage, Translator, report};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

pub struct TranslateOptions {
    pub profile: Option<String>,
    pub repair_profile: Option<String>,
    pub glossary_profile: Option<String>,
    pub overwrite: bool,
    pub fail_fast: bool,
    pub rerun: Option<crate::RerunMode>,
    pub dry_run: bool,
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

pub async fn translate_book(book_dir: &Path, options: TranslateOptions) -> Result<i32> {
    match translate_book_inner(book_dir, options).await {
        Ok(code) => Ok(code),
        Err(e) if output::is_json() => {
            report::print_error_report(&e)?;
            Ok(e.exit_code())
        }
        Err(e) => Err(e),
    }
}

async fn translate_book_inner(book_dir: &Path, options: TranslateOptions) -> Result<i32> {
    let layout = BookLayout::discover(book_dir);

    if !layout.is_valid_book() {
        return Err(Error::Validation {
            message: format!(
                "Invalid book layout. Run 'cipher doctor {}' for details.",
                book_dir.display()
            ),
        });
    }

    let global_config = GlobalConfig::load()
        .map_err(|e| Error::Config(format!("Failed to load global config: {e}")))?;

    let book_config = load_book_config(&layout.paths.config_toml)?;
    let injection_mode = book_config_injection_mode(&book_config.glossary_injection);

    let chapters: VecDeque<PathBuf> = discover_chapters(&layout.paths.raw_dir)?
        .into_iter()
        .collect();
    if chapters.is_empty() {
        if output::is_json() {
            report::print_run_report(&report::build_run_report(&report::ReportData::empty(
                book_dir.display().to_string(),
            )))?;
            return Ok(0);
        }
        stderr_status("No chapters found");
        stderr_detail_kv("Directory", layout.paths.raw_dir.display());
        return Ok(0);
    }

    let mut glossary = load_glossary(&layout.paths.glossary_json)?;
    let run_start_glossary_state = build_glossary_state(&glossary, injection_mode)?;

    let out_dir = layout.effective_out_dir();

    let style_guide = if layout.exists.style_md {
        match std::fs::read_to_string(&layout.paths.style_md) {
            Ok(content) if !content.trim().is_empty() => Some(content),
            _ => None,
        }
    } else {
        None
    };

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
        if output::is_json() {
            let (previews, summary) = build_preview_data(
                &chapters,
                &layout.paths.raw_dir,
                out_dir,
                &options,
                &rerun_plan,
                &source_rerun_plan,
            )?;
            report::print_run_report(&report::build_preview_report(
                book_dir.display().to_string(),
                &previews,
                &summary,
            ))?;
            return Ok(0);
        }
        stderr_status("Translation preview");
        stderr_detail_kv("Book", book_dir.display());
        if let Some(profile_names) = resolve_translate_profiles(
            &global_config,
            &book_config,
            options.profile.as_deref(),
            options.repair_profile.as_deref(),
            options.glossary_profile.as_deref(),
        ) {
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

    let profile_names = resolve_translate_profiles(
        &global_config,
        &book_config,
        options.profile.as_deref(),
        options.repair_profile.as_deref(),
        options.glossary_profile.as_deref(),
    )
    .ok_or_else(|| Error::Validation {
        message: "No profile configured. Run 'cipher profile new' to create one.".to_string(),
    })?;

    validate_translate_profiles(&global_config, &profile_names)?;

    std::fs::create_dir_all(out_dir).map_err(|e| {
        Error::io(
            format!("Failed to create output directory {}", out_dir.display()),
            e,
        )
    })?;

    let profile = global_config
        .resolve_profile(profile_names.translation_name)
        .ok_or_else(|| Error::ProfileNotFound {
            name: profile_names.translation_name.to_string(),
        })?;

    verbose_detail_kv("Using profiles", "");
    print_profile_details(&global_config, &profile_names);
    if style_guide.is_some() {
        verbose_detail_kv("Style guide", layout.paths.style_md.display());
    }

    let translators = Translators {
        translation: Translator::from_config(&global_config, profile_names.translation_name)?,
        repair: Translator::from_config(&global_config, profile_names.repair_name)?,
        glossary: Translator::from_config(&global_config, profile_names.glossary_name)?,
    };

    if !output::is_json() {
        stderr_status("Translating chapters");
    }
    verbose_detail_kv("Chapters found", chapters.len());

    let to_process = if options.overwrite {
        chapters.len()
    } else {
        chapters
            .iter()
            .filter(|ch| {
                chapter_output_path(out_dir, ch)
                    .map(|p| !p.exists())
                    .unwrap_or(true)
            })
            .count()
    };

    if !output::is_quiet() && !output::is_json() {
        output::stderr_section(format!(
            "Translating {} {}",
            to_process,
            if to_process == 1 {
                "chapter"
            } else {
                "chapters"
            }
        ));
    }

    let run_options = RunOptions {
        overwrite: options.overwrite,
        fail_fast: options.fail_fast,
        rerun_mode: options.rerun.clone(),
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

    let (translated, skipped, failed, new_glossary_terms, total_usage, cancelled, chapter_results) =
        iterate_translation(
            &translators,
            &mut glossary,
            chapters.clone(),
            &options,
            &layout.paths.raw_dir,
            out_dir,
            &style_guide,
            &book_config.output,
            injection_mode,
            &layout.paths.glossary_json,
            book_dir,
            &mut run_metadata,
            rerun_plan,
            &source_rerun_plan,
            &mut previous_chapter_states,
            previous_glossary_state.as_ref(),
        )
        .await?;

    let (exit_code, run_metadata) = finalize_run(
        book_dir,
        &chapters,
        &layout.paths.raw_dir,
        out_dir,
        &previous_chapter_states,
        &glossary,
        injection_mode,
        options.rerun_glossary_enabled(),
        previous_glossary_state.as_ref(),
        &run_start_glossary_state,
        failed,
        translated,
        skipped,
        new_glossary_terms,
        &total_usage,
        run_metadata,
        cancelled,
    )?;

    if output::is_json() {
        report::print_run_report(&report::build_run_report(&report::ReportData {
            book: book_dir.display().to_string(),
            run: Some(run_metadata),
            chapters: chapter_results,
            total: chapters.len(),
            translated,
            skipped,
            failed,
            new_glossary_terms,
            usage: total_usage,
            cancelled,
            exit_code,
        }))?;
    }

    Ok(exit_code)
}

#[allow(clippy::too_many_arguments)]
async fn iterate_translation(
    translators: &Translators,
    glossary: &mut Vec<GlossaryTerm>,
    chapters: VecDeque<PathBuf>,
    options: &TranslateOptions,
    raw_dir: &Path,
    out_dir: &Path,
    style_guide: &Option<String>,
    output_config: &OutputConfig,
    injection_mode: InjectionMode,
    glossary_json_path: &Path,
    book_dir: &Path,
    run_metadata: &mut RunMetadata,
    mut rerun_plan: GlossaryRerunPlan,
    source_rerun_plan: &SourceRerunPlan,
    previous_chapter_states: &mut BTreeMap<String, ChapterState>,
    previous_glossary_state: Option<&GlossaryState>,
) -> Result<(
    usize,
    usize,
    usize,
    usize,
    TranslationUsage,
    bool,
    Vec<ChapterResult>,
)> {
    let mut translated = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut new_glossary_terms = 0;
    let mut total_usage = TranslationUsage::default();
    let mut cancelled = false;
    let mut chapter_results: Vec<ChapterResult> = Vec::new();

    let mut remaining_chapters = chapters;
    let ctrl_c = tokio::signal::ctrl_c();

    tokio::pin!(ctrl_c);

    while let Some(chapter_file) = remaining_chapters.pop_front() {
        let chapter_path = chapter_state_key(raw_dir, &chapter_file)?;
        let out_path = chapter_output_path(out_dir, &chapter_file)?;
        let previous_chapter_state = previous_chapter_states.get(&chapter_path);
        let rerun_decision = combine_rerun_decisions(
            rerun_plan.decision_for(&chapter_path),
            source_rerun_plan.decision_for(&chapter_path),
        );

        let ctx = ChapterContext::new(
            translators,
            style_guide,
            output_config,
            injection_mode,
            glossary_json_path,
            book_dir,
        );
        let paths = ChapterPaths::new(&chapter_file, &out_path, &chapter_path);

        if !output::is_quiet() && !output::is_json() {
            let short = Path::new(&chapter_path)
                .file_name()
                .map(|f| f.to_string_lossy())
                .unwrap_or_default();
            eprint!("\r\x1b[K  Translating {} ...", short);
        }

        let result = tokio::select! {
            result = translate_single_chapter(
                &ctx,
                &paths,
                options.overwrite,
                options.rerun_chapters_enabled(),
                previous_chapter_state,
                rerun_decision.as_ref(),
                glossary,
            ) => result?,
            _ = &mut ctrl_c => {
                cancelled = true;
                break;
            }
        };

        checkpoint_chapter_progress(book_dir, &mut *run_metadata, &result.chapter_state)?;
        previous_chapter_states.insert(chapter_path.clone(), result.chapter_state.clone());

        if !output::is_quiet() && !output::is_json() {
            if result.translated || result.failed {
                print_chapter_result(&result, &chapter_path);
            } else {
                eprint!("\r\x1b[K");
            }
        }

        let chapter_failed = result.failed;
        if result.translated {
            translated += 1;
        }
        if result.skipped {
            skipped += 1;
        }
        if chapter_failed {
            failed += 1;
        }
        if let Some(usage) = result.usage.clone() {
            total_usage += usage;
        }
        new_glossary_terms += result.new_terms_added;

        if result.new_terms_added > 0
            && options.rerun_glossary_enabled()
            && !remaining_chapters.is_empty()
        {
            rerun_plan = build_glossary_rerun_plan(
                &Vec::from(remaining_chapters.clone()),
                raw_dir,
                out_dir,
                previous_glossary_state,
                previous_chapter_states,
                glossary,
                injection_mode,
            )?;
        }

        chapter_results.push(result);

        if chapter_failed && options.fail_fast {
            verbose_detail("Stopping due to --fail-fast");
            break;
        }
    }

    Ok((
        translated,
        skipped,
        failed,
        new_glossary_terms,
        total_usage,
        cancelled,
        chapter_results,
    ))
}

fn print_chapter_result(result: &super::orchestrate::ChapterResult, chapter_path: &str) {
    let time = result
        .chapter_state
        .translation_time_ms
        .map(fmt_time)
        .unwrap_or_else(|| "\u{2014}".to_string());
    let tokens = result
        .usage
        .as_ref()
        .map(|u| fmt_tokens(u.total_tokens))
        .unwrap_or_else(|| "\u{2014}".to_string());

    if result.translated {
        let mut tags: Vec<String> = Vec::new();
        if result.new_terms_added > 0 {
            let label = if result.new_terms_added == 1 {
                "term"
            } else {
                "terms"
            };
            tags.push(output::styled_green(format!(
                "+{} {}",
                result.new_terms_added, label
            )));
        }
        if let Some(ref err) = result.glossary_extraction_error {
            tags.push(output::styled_yellow(format!("\u{26A0} glos: {}", err)));
        }
        output::chapter_line_ok(chapter_path, &time, &tokens, &tags);
    } else if result.failed {
        let error = result
            .chapter_state
            .error
            .as_deref()
            .unwrap_or("unknown error");
        output::chapter_line_fail(chapter_path, &time, &tokens, error);
    }
}

fn fmt_time(ms: u64) -> String {
    let total_s = ms / 1000;
    if total_s >= 60 {
        format!("{}m{:02}s", total_s / 60, total_s % 60)
    } else {
        format!("{total_s}s")
    }
}

fn fmt_tokens(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{:.1}K", tokens as f64 / 1000.0)
    } else {
        tokens.to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn finalize_run(
    book_dir: &Path,
    chapters: &VecDeque<PathBuf>,
    raw_dir: &Path,
    out_dir: &Path,
    previous_chapter_states: &BTreeMap<String, ChapterState>,
    glossary: &[GlossaryTerm],
    injection_mode: InjectionMode,
    rerun_glossary_enabled: bool,
    previous_glossary_state: Option<&GlossaryState>,
    run_start_glossary_state: &GlossaryState,
    failed: usize,
    translated: usize,
    skipped: usize,
    new_glossary_terms: usize,
    total_usage: &TranslationUsage,
    mut run_metadata: RunMetadata,
    cancelled: bool,
) -> Result<(i32, RunMetadata)> {
    let baseline_outcome = finalize_glossary_baseline(
        book_dir,
        rerun_glossary_enabled,
        previous_glossary_state,
        run_start_glossary_state,
        &Vec::from(chapters.clone()),
        raw_dir,
        out_dir,
        previous_chapter_states,
        glossary,
        injection_mode,
        failed,
    )?;

    let _legacy = migrate_legacy_full_tracking(
        book_dir,
        previous_glossary_state,
        baseline_outcome,
        &Vec::from(chapters.clone()),
        raw_dir,
        out_dir,
        &mut previous_chapter_states.clone(),
        glossary,
        injection_mode,
        failed,
    )?;

    run_metadata.mark_finished();
    save_run_metadata(book_dir, &run_metadata)?;

    if output::is_json() {
        return Ok((if failed > 0 { 2 } else { 0 }, run_metadata));
    }

    if baseline_outcome.remaining_forced_chapters > 0 {
        output::stderr_warn(format!(
            "Glossary baseline was not updated because {} affected chapter(s) still need reruns.",
            baseline_outcome.remaining_forced_chapters
        ));
    }

    if cancelled {
        output::cancel_banner(translated + skipped + failed, chapters.len());
    }
    output::summary_header();
    let total_done = translated + skipped + failed;
    output::summary_item("Processed", format!("{total_done}/{}", chapters.len()));
    if translated > 0 {
        output::summary_item("Translated", output::styled_green(translated));
    }
    if skipped > 0 {
        output::summary_item("Skipped", skipped);
    }
    if failed > 0 {
        output::summary_item("Failed", output::styled_red(failed));
    }
    if new_glossary_terms > 0 {
        output::summary_item("New glossary terms", new_glossary_terms);
    }
    if total_usage.total_tokens > 0 {
        output::summary_item("Token usage", total_usage.total_tokens);
    }
    if _legacy.migrated_chapters > 0 {
        output::summary_item("Legacy chapters migrated", _legacy.migrated_chapters);
    }
    if _legacy.migrated_glossary_baseline {
        eprintln!(
            " {} Migrated legacy full-glossary baseline to canonical smart tracking",
            output::styled_green("\u{2713}")
        );
    }
    eprintln!();
    if cancelled {
        eprintln!(
            " {} {}",
            output::styled_yellow("\u{26A0}"),
            output::styled_yellow("Translation cancelled. Partial results saved.")
        );
    } else if failed > 0 {
        eprintln!(
            " {} Translation finished ({} failed)",
            output::styled_green("\u{2713}"),
            failed
        );
    } else {
        eprintln!(" {} Translation complete", output::styled_green("\u{2713}"));
    }

    Ok((if failed > 0 { 2 } else { 0 }, run_metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::translate::test_helpers::translate_options;

    fn rerun_opts(rerun: Option<crate::RerunMode>) -> TranslateOptions {
        translate_options(rerun)
    }

    #[test]
    fn rerun_glossary_enabled_for_all() {
        assert!(rerun_opts(Some(crate::RerunMode::All)).rerun_glossary_enabled());
    }

    #[test]
    fn rerun_glossary_enabled_for_glossary() {
        assert!(rerun_opts(Some(crate::RerunMode::Glossary)).rerun_glossary_enabled());
    }

    #[test]
    fn rerun_glossary_disabled_for_source() {
        assert!(!rerun_opts(Some(crate::RerunMode::Source)).rerun_glossary_enabled());
    }

    #[test]
    fn rerun_glossary_disabled_for_none() {
        assert!(!rerun_opts(None).rerun_glossary_enabled());
    }

    #[test]
    fn rerun_chapters_enabled_for_all() {
        assert!(rerun_opts(Some(crate::RerunMode::All)).rerun_chapters_enabled());
    }

    #[test]
    fn rerun_chapters_enabled_for_source() {
        assert!(rerun_opts(Some(crate::RerunMode::Source)).rerun_chapters_enabled());
    }

    #[test]
    fn rerun_chapters_disabled_for_glossary() {
        assert!(!rerun_opts(Some(crate::RerunMode::Glossary)).rerun_chapters_enabled());
    }

    #[test]
    fn rerun_chapters_disabled_for_none() {
        assert!(!rerun_opts(None).rerun_chapters_enabled());
    }

    #[test]
    fn test_translate_options_rerun_enables_both_rerun_modes() {
        let options = translate_options(Some(crate::RerunMode::All));

        assert!(options.rerun_glossary_enabled());
        assert!(options.rerun_chapters_enabled());
    }

    fn run_metadata_sample() -> RunMetadata {
        RunMetadata::new(
            "default".to_string(),
            "gemini".to_string(),
            "gemini-2.5-flash".to_string(),
            None,
        )
    }

    fn empty_glossary_state() -> GlossaryState {
        GlossaryState::new(InjectionMode::Smart, BTreeMap::new())
    }

    fn finalize_for_failed_count(failed: usize) -> i32 {
        let dir = tempfile::tempdir().unwrap();
        let raw_dir = dir.path().join("raw");
        let out_dir = dir.path().join("tl");

        let (exit_code, metadata) = finalize_run(
            dir.path(),
            &VecDeque::new(),
            &raw_dir,
            &out_dir,
            &BTreeMap::new(),
            &[],
            InjectionMode::Smart,
            false,
            None,
            &empty_glossary_state(),
            failed,
            0,
            0,
            0,
            &TranslationUsage::default(),
            run_metadata_sample(),
            false,
        )
        .unwrap();

        assert!(
            metadata.finished_at.is_some(),
            "run must be marked finished"
        );
        exit_code
    }

    #[test]
    fn finalize_run_exits_2_when_chapters_failed() {
        assert_eq!(finalize_for_failed_count(1), 2);
    }

    #[test]
    fn finalize_run_exits_0_when_nothing_failed() {
        assert_eq!(finalize_for_failed_count(0), 0);
    }
}
