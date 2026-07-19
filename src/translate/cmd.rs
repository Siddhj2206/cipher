use crate::book::paths::{chapter_output_path, chapter_state_key, discover_chapters};
use crate::book::{BookLayout, OutputConfig, load_book_config};
use crate::config::GlobalConfig;
use crate::glossary::{
    GlossaryTerm, InjectionMode, book_config_injection_mode, load_glossary,
};
use crate::output;
use crate::output::{
    stderr_detail, stderr_detail_kv, stderr_status, stderr_warn, verbose_detail, verbose_detail_kv,
};
use crate::state::{
    ChapterState, GlossaryState, RunMetadata, RunOptions,
    load_all_chapter_states, load_glossary_state, save_run_metadata,
};
use crate::translate::orchestrate::{
    Translators, checkpoint_chapter_progress,
    print_profile_details, resolve_translate_profiles, translate_single_chapter,
    validate_translate_profiles,
};
use crate::translate::preview::preview_translation_run;
use crate::translate::rerun::{
    GlossaryRerunPlan, SourceRerunPlan, build_glossary_rerun_plan, build_glossary_state,
    build_source_rerun_plan, combine_rerun_decisions, finalize_glossary_baseline,
    migrate_legacy_full_tracking,
};
use crate::translate::{TranslationUsage, Translator};
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
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
    let layout = BookLayout::discover(book_dir);

    if !layout.is_valid_book() {
        anyhow::bail!(
            "Invalid book layout. Run 'cipher doctor {}' for details.",
            book_dir.display()
        );
    }

    let global_config = GlobalConfig::load().context("Failed to load global config")?;

    let book_config = load_book_config(&layout.paths.config_toml).unwrap_or_default();
    let injection_mode = book_config_injection_mode(&book_config.glossary_injection);

    let chapters: VecDeque<PathBuf> = discover_chapters(&layout.paths.raw_dir)?
        .into_iter()
        .collect();
    if chapters.is_empty() {
        stderr_status("No chapters found");
        stderr_detail_kv("Directory", layout.paths.raw_dir.display());
        return Ok(0);
    }

    let mut glossary = load_glossary(&layout.paths.glossary_json)?;
    let run_start_glossary_state = build_glossary_state(&glossary, injection_mode);

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
        stderr_status("Translation preview");
        stderr_detail_kv("Book", book_dir.display());
        if let Some(profile_names) =
            resolve_translate_profiles(&global_config, &book_config, options.profile.as_deref(), options.repair_profile.as_deref(), options.glossary_profile.as_deref())
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

    let profile_names = resolve_translate_profiles(&global_config, &book_config, options.profile.as_deref(), options.repair_profile.as_deref(), options.glossary_profile.as_deref())
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

    stderr_status("Translating chapters");
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

    let (translated, skipped, failed, new_glossary_terms, total_usage) = iterate_translation(
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
        pb.as_ref(),
    )
    .await?;

    let exit_code = finalize_run(
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
        total_usage,
        run_metadata,
        pb,
    )?;

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
    pb: Option<&ProgressBar>,
) -> Result<(usize, usize, usize, usize, TranslationUsage)> {
    let mut translated = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut new_glossary_terms = 0;
    let mut total_usage = TranslationUsage::default();

    let mut remaining_chapters = chapters;

    while let Some(chapter_file) = remaining_chapters.pop_front() {
        let chapter_path = chapter_state_key(raw_dir, &chapter_file)?;
        let out_path = chapter_output_path(out_dir, &chapter_file)?;
        let previous_chapter_state = previous_chapter_states.get(&chapter_path);
        let rerun_decision = combine_rerun_decisions(
            rerun_plan.decision_for(&chapter_path),
            source_rerun_plan.decision_for(&chapter_path),
        );

        if let Some(pb) = pb {
            pb.set_message(chapter_path.clone());
        }

        let result = translate_single_chapter(
            translators,
            &chapter_file,
            &out_path,
            &chapter_path,
            options.overwrite,
            options.rerun_chapters_enabled(),
            previous_chapter_state,
            rerun_decision.as_ref(),
            glossary,
            style_guide,
            output_config,
            injection_mode,
            glossary_json_path,
            book_dir,
        )
        .await?;

        checkpoint_chapter_progress(book_dir, &mut *run_metadata, &result.chapter_state)?;
        previous_chapter_states.insert(chapter_path.clone(), result.chapter_state.clone());
        if let Some(pb) = pb {
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
                raw_dir,
                out_dir,
                previous_glossary_state,
                previous_chapter_states,
                glossary,
                injection_mode,
            )?;
        }
    }

    Ok((translated, skipped, failed, new_glossary_terms, total_usage))
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
    total_usage: TranslationUsage,
    mut run_metadata: RunMetadata,
    pb: Option<ProgressBar>,
) -> Result<i32> {
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

    let legacy_tracking_migration = migrate_legacy_full_tracking(
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

    stderr_status("Translation complete");
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
        crate::translate::orchestrate::print_usage_info_with_label("Token usage", &total_usage);
    }

    if failed > 0 {
        return Ok(2);
    }

    Ok(0)
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
}
