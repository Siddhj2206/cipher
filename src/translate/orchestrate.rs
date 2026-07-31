use crate::book::init::BookConfig;
use crate::book::{
    OutputConfig, StructuredChapter, render_chapter_markdown, render_starts_with_markdown_heading,
    validate_structured_chapter,
};
use crate::config::{GlobalConfig, validate_profile};
use crate::error::{Error, Result};
use crate::glossary::{
    GlossaryTerm, InjectionMode, SelectionResult, glossary_term_key,
    glossary_term_prompt_fingerprint, merge_terms, save_glossary, select_terms_for_text,
};
use crate::io;
use crate::output::{stderr_detail, stderr_status, verbose_detail, verbose_detail_kv};
use crate::state::{
    ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterState, ChapterStatus,
    normalized_source_text_hash, save_chapter_state, save_run_metadata,
};
use crate::translate::backup::create_backup;
use crate::translate::preview::EMPTY_CHAPTER_SKIP_REASON;
use crate::translate::rerun::{RerunDecision, build_chapter_glossary_usage};
use crate::translate::{
    AcceptedTranslation, ProviderTextResult, ProviderTranslationResult, TranslationUsage,
    Translator,
};
use crate::validate::{ValidationOptions, validate_translation};
use std::path::Path;
use std::time::Instant;

pub(crate) struct TranslateProfiles<'a> {
    pub translation_name: &'a str,
    pub repair_name: &'a str,
    pub glossary_name: &'a str,
}

pub(crate) struct Translators {
    pub translation: Translator,
    pub repair: Translator,
    pub glossary: Translator,
}

pub(crate) fn resolve_translate_profiles<'a>(
    global_config: &'a GlobalConfig,
    book_config: &'a BookConfig,
    profile: Option<&'a str>,
    repair_profile: Option<&'a str>,
    glossary_profile: Option<&'a str>,
) -> Option<TranslateProfiles<'a>> {
    let translation_name =
        profile.or_else(|| global_config.effective_profile_name(book_config.profile.as_deref()))?;
    let repair_name = repair_profile
        .or(book_config.repair_profile.as_deref())
        .unwrap_or(translation_name);
    let glossary_name = glossary_profile
        .or(book_config.glossary_profile.as_deref())
        .unwrap_or(translation_name);

    Some(TranslateProfiles {
        translation_name,
        repair_name,
        glossary_name,
    })
}

pub(crate) fn validate_translate_profiles(
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
            stderr_status(format!("{} profile validation failed", label));
            for error in &validation.errors {
                stderr_detail(error);
            }
            return Err(Error::Validation {
                message: format!("Cannot translate with invalid {} profile", label),
            });
        }
    }

    Ok(())
}

pub(crate) fn print_profile_details(config: &GlobalConfig, profiles: &TranslateProfiles<'_>) {
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

pub(crate) struct ChapterResult {
    pub translated: bool,
    pub failed: bool,
    pub skipped: bool,
    pub new_terms_added: usize,
    pub usage: Option<TranslationUsage>,
    pub chapter_state: ChapterState,
    pub glossary_extraction_error: Option<String>,
}

impl ChapterResult {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        chapter_path: &str,
        status: ChapterStatus,
        translated: bool,
        failed: bool,
        skipped: bool,
        new_terms_added: usize,
        usage: Option<TranslationUsage>,
        error: Option<String>,
        duration_ms: Option<u64>,
        translation_usage: Option<TranslationUsage>,
        glossary_usage: Option<ChapterGlossaryUsage>,
        exported_terms: Vec<ChapterGlossaryTerm>,
        source_text_hash: Option<String>,
        glossary_extraction_error: Option<String>,
    ) -> Self {
        ChapterResult {
            translated,
            failed,
            skipped,
            new_terms_added,
            usage,
            chapter_state: ChapterState::new(
                chapter_path.to_string(),
                status,
                error,
                duration_ms,
                translation_usage,
                glossary_usage,
                exported_terms,
                source_text_hash,
            ),
            glossary_extraction_error,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PreviousChapterArtifacts {
    pub translation_usage: Option<TranslationUsage>,
    pub glossary_usage: Option<ChapterGlossaryUsage>,
    pub exported_terms: Vec<ChapterGlossaryTerm>,
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
    ChapterResult::new(
        chapter_path,
        ChapterStatus::Skipped,
        false,
        false,
        true,
        0,
        None,
        message,
        duration_ms,
        previous_artifacts.translation_usage,
        previous_artifacts.glossary_usage,
        previous_artifacts.exported_terms,
        source_text_hash,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_success_chapter_result(
    chapter_path: &str,
    duration_ms: u64,
    usage: TranslationUsage,
    glossary_usage: ChapterGlossaryUsage,
    exported_terms: Vec<ChapterGlossaryTerm>,
    source_text_hash: String,
    new_terms_added: usize,
    glossary_extraction_error: Option<String>,
) -> ChapterResult {
    ChapterResult::new(
        chapter_path,
        ChapterStatus::Success,
        true,
        false,
        false,
        new_terms_added,
        Some(usage.clone()),
        None,
        Some(duration_ms),
        Some(usage),
        Some(glossary_usage),
        exported_terms,
        Some(source_text_hash),
        glossary_extraction_error,
    )
}

fn build_failed_chapter_result(
    chapter_path: &str,
    error_msg: String,
    duration_ms: u64,
    usage: Option<TranslationUsage>,
    previous_artifacts: PreviousChapterArtifacts,
    source_text_hash: Option<String>,
) -> ChapterResult {
    ChapterResult::new(
        chapter_path,
        ChapterStatus::Failed,
        false,
        true,
        false,
        0,
        usage.clone(),
        Some(error_msg),
        Some(duration_ms),
        usage,
        previous_artifacts.glossary_usage,
        previous_artifacts.exported_terms,
        source_text_hash,
        None,
    )
}

fn skipped_chapter_source_hash(
    raw_path: &Path,
    previous_chapter_state: Option<&ChapterState>,
    rerun_chapters_enabled: bool,
) -> Result<Option<String>> {
    if !rerun_chapters_enabled {
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
        .map_err(|e| Error::io(format!("Failed to read {}", path.display()), e))?;
    Ok(normalized_source_text_hash(&chapter_text))
}

pub(crate) fn checkpoint_chapter_progress(
    book_dir: &Path,
    run_metadata: &mut crate::state::RunMetadata,
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

fn retry_delay_secs(attempt: u32) -> u64 {
    2u64.pow(attempt)
}

async fn call_translation_with_retry(
    translator: &Translator,
    chapter_text: &str,
    terms: &[GlossaryTerm],
    style_guide: &Option<String>,
    output_config: &OutputConfig,
) -> (Option<ProviderTextResult>, Option<String>, u32) {
    for attempt in 1..=MAX_API_RETRIES as u32 {
        let attempt_start = Instant::now();
        match translator
            .translate_chapter(
                chapter_text,
                terms,
                style_guide.clone(),
                output_config.clone(),
            )
            .await
        {
            Ok(resp) => return (Some(resp), None, attempt),
            Err(e) => {
                let msg = format!("API error: {e}");
                let elapsed_ms = attempt_start.elapsed().as_millis();
                if attempt < MAX_API_RETRIES as u32 {
                    let delay = retry_delay_secs(attempt);
                    verbose_detail_kv(
                        "Attempt",
                        format!(
                            "{attempt}/{MAX_API_RETRIES} failed after {elapsed_ms} ms: {e}. Retrying in {delay}s."
                        ),
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                } else {
                    verbose_detail_kv(
                        "Attempt",
                        format!("{attempt}/{MAX_API_RETRIES} failed after {elapsed_ms} ms: {e}."),
                    );
                    return (None, Some(msg), attempt);
                }
            }
        }
    }
    (None, Some("API error".to_string()), MAX_API_RETRIES as u32)
}

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
    let validation_options = ValidationOptions {
        require_markdown_heading: render_starts_with_markdown_heading(output_config),
    };

    let (translation_result, api_error, api_attempt) = call_translation_with_retry(
        &translators.translation,
        chapter_text,
        &selection.terms,
        style_guide,
        output_config,
    )
    .await;

    let Some(resp) = translation_result else {
        return (None, api_error, None);
    };

    let rendered = render_chapter_markdown(&resp.chapter, output_config);
    let mut validation_errors = validate_structured_chapter(&resp.chapter, output_config);
    validation_errors.extend(
        validate_translation(&rendered, validation_options)
            .errors()
            .iter()
            .cloned(),
    );
    let original_usage = resp.usage.clone();

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

    let mut last_error = Some(format!(
        "Validation failed: {}",
        validation_errors.join(", ")
    ));
    let mut failed_usage = Some(original_usage.clone());

    verbose_detail_kv("Validation", validation_errors.join(", "));

    if api_attempt == 1 {
        verbose_detail("Attempting repair.");
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
                let mut combined_usage = original_usage;
                combined_usage += repair_resp.usage.clone();
                failed_usage = Some(combined_usage.clone());
                let mut repair_errors =
                    validate_structured_chapter(&repair_resp.chapter, output_config);
                repair_errors.extend(
                    validate_translation(&repaired_rendered, validation_options)
                        .errors()
                        .iter()
                        .cloned(),
                );

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
                }

                let msg = format!("Repair failed validation: {}", repair_errors.join(", "));
                verbose_detail_kv("Repair", &msg);
                last_error = Some(msg);
            }
            Err(e) => {
                let msg = format!("Repair request failed: {e}");
                verbose_detail_kv("Repair", &msg);
                last_error = Some(msg);
            }
        }
    } else {
        verbose_detail_kv(
            "Repair",
            format!("skipped (API call already retried {api_attempt} times)"),
        );
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
    let (new_glossary_terms, glossary_extraction_error) = match translator
        .extract_glossary(chapter_text, rendered_markdown, glossary)
        .await
    {
        Ok(glossary_resp) => {
            usage += glossary_resp.usage;
            verbose_detail_kv("Glossary extraction", "success");
            (glossary_resp.new_glossary_terms, None)
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            verbose_detail_kv(
                "Glossary extraction",
                format!("failed: {}. Chapter kept without new terms.", msg),
            );
            (Vec::new(), Some(msg))
        }
    };

    ProviderTranslationResult {
        response: AcceptedTranslation {
            chapter,
            new_glossary_terms,
            glossary_extraction_error,
        },
        usage,
    }
}

fn print_usage_info(usage: &TranslationUsage) {
    print_usage_info_with_label("Usage", usage);
}

pub(crate) fn print_usage_info_with_label(label: &str, usage: &TranslationUsage) {
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
        .map(|term| {
            Ok(ChapterGlossaryTerm {
                key: glossary_term_key(term),
                fingerprint: glossary_term_prompt_fingerprint(term)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

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

pub(crate) struct ChapterPaths<'a> {
    pub raw_path: &'a Path,
    pub out_path: &'a Path,
    pub chapter_path: &'a str,
}

impl<'a> ChapterPaths<'a> {
    pub fn new(raw_path: &'a Path, out_path: &'a Path, chapter_path: &'a str) -> Self {
        Self {
            raw_path,
            out_path,
            chapter_path,
        }
    }
}

pub(crate) struct ChapterContext<'a> {
    pub translators: &'a Translators,
    pub style_guide: &'a Option<String>,
    pub output_config: &'a OutputConfig,
    pub injection_mode: InjectionMode,
    pub glossary_path: &'a Path,
    pub book_dir: &'a Path,
}

impl<'a> ChapterContext<'a> {
    pub fn new(
        translators: &'a Translators,
        style_guide: &'a Option<String>,
        output_config: &'a OutputConfig,
        injection_mode: InjectionMode,
        glossary_path: &'a Path,
        book_dir: &'a Path,
    ) -> Self {
        Self {
            translators,
            style_guide,
            output_config,
            injection_mode,
            glossary_path,
            book_dir,
        }
    }
}

pub(crate) async fn translate_single_chapter(
    ctx: &ChapterContext<'_>,
    paths: &ChapterPaths<'_>,
    overwrite: bool,
    rerun_chapters_enabled: bool,
    previous_chapter_state: Option<&ChapterState>,
    rerun_decision: Option<&RerunDecision>,
    glossary: &mut Vec<GlossaryTerm>,
) -> Result<ChapterResult> {
    let prev_artifacts = previous_chapter_artifacts(previous_chapter_state);

    let output_exists = paths.out_path.exists();
    if !overwrite && output_exists && rerun_decision.is_none() {
        let source_text_hash = skipped_chapter_source_hash(
            paths.raw_path,
            previous_chapter_state,
            rerun_chapters_enabled,
        )?;

        return Ok(build_skipped_chapter_result(
            paths.chapter_path,
            None,
            None,
            prev_artifacts,
            source_text_hash,
        ));
    }

    let chapter_text = std::fs::read_to_string(paths.raw_path)
        .map_err(|e| Error::io(format!("Failed to read {}", paths.raw_path.display()), e))?;
    let source_text_hash = normalized_source_text_hash(&chapter_text);

    if chapter_text.trim().is_empty() {
        stderr_status(format!("Skip {}: chapter is empty", paths.chapter_path));
        return Ok(build_skipped_chapter_result(
            paths.chapter_path,
            Some(EMPTY_CHAPTER_SKIP_REASON.to_string()),
            None,
            prev_artifacts,
            Some(source_text_hash),
        ));
    }

    if let Some(decision) = rerun_decision {
        verbose_detail_kv("Rerun reason", &decision.reason);
    }

    let start = Instant::now();
    let selection = select_terms_for_text(glossary, &chapter_text, ctx.injection_mode);
    print_glossary_info(&selection, ctx.injection_mode);

    let (response, last_error, failed_usage) = attempt_translation(
        ctx.translators,
        &chapter_text,
        &selection,
        glossary,
        ctx.style_guide,
        ctx.output_config,
    )
    .await;

    let duration = start.elapsed();

    if let Some(resp) = response {
        print_usage_info(&resp.usage);

        if output_exists {
            let backup_path = create_backup(ctx.book_dir, paths.out_path)?;
            verbose_detail_kv("Backup", backup_path.display());
        }

        let rendered_translation =
            render_chapter_markdown(&resp.response.chapter, ctx.output_config);

        io::atomic_write(paths.out_path, &rendered_translation)?;

        let (new_terms_added, exported_terms) = merge_new_glossary_terms(
            glossary,
            resp.response.new_glossary_terms,
            ctx.glossary_path,
        )?;

        verbose_detail_kv("Result", "success");
        return Ok(build_success_chapter_result(
            paths.chapter_path,
            duration.as_millis() as u64,
            resp.usage,
            build_chapter_glossary_usage(&selection, ctx.injection_mode)?,
            exported_terms,
            source_text_hash,
            new_terms_added,
            resp.response.glossary_extraction_error,
        ));
    }

    let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
    verbose_detail_kv("Result", "failed");
    verbose_detail_kv("Error", &error_msg);
    let failed_source_text_hash =
        failed_chapter_source_hash(previous_chapter_state, &source_text_hash);
    Ok(build_failed_chapter_result(
        paths.chapter_path,
        error_msg,
        duration.as_millis() as u64,
        failed_usage,
        prev_artifacts,
        failed_source_text_hash,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GlobalConfig;

    use crate::translate::providers::Provider;

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
            Err(Error::Provider {
                kind: "test".to_string(),
                detail: "extractor unavailable".to_string(),
            })
        }
    }

    struct SucceedingProvider;

    #[async_trait::async_trait]
    impl Provider for SucceedingProvider {
        async fn translate(
            &self,
            _req: crate::translate::TranslationRequest,
        ) -> Result<ProviderTextResult> {
            Ok(ProviderTextResult {
                chapter: StructuredChapter {
                    chapter_number: None,
                    chapter_title: None,
                    content: "translated".to_string(),
                },
                usage: TranslationUsage::default(),
            })
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
            unreachable!()
        }
    }

    fn profile_options(
        profile: Option<&str>,
        repair_profile: Option<&str>,
        glossary_profile: Option<&str>,
    ) -> (Option<String>, Option<String>, Option<String>) {
        (
            profile.map(str::to_string),
            repair_profile.map(str::to_string),
            glossary_profile.map(str::to_string),
        )
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
        let (profile, repair, glossary) = profile_options(None, None, None);

        let profiles = resolve_translate_profiles(
            &config,
            &book_config,
            profile.as_deref(),
            repair.as_deref(),
            glossary.as_deref(),
        )
        .unwrap();

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
        let (profile, repair, glossary) =
            profile_options(Some("cli"), Some("cli-repair"), Some("cli-glossary"));

        let profiles = resolve_translate_profiles(
            &config,
            &book_config,
            profile.as_deref(),
            repair.as_deref(),
            glossary.as_deref(),
        )
        .unwrap();

        assert_eq!(profiles.translation_name, "cli");
        assert_eq!(profiles.repair_name, "cli-repair");
        assert_eq!(profiles.glossary_name, "cli-glossary");
    }

    #[test]
    fn test_skipped_chapter_source_hash_backfills_legacy_hash_during_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        let chapter_text = "# Chapter 1\n\nSource text\n";
        std::fs::write(&raw_path, chapter_text).unwrap();

        let previous_chapter_state = previous_chapter_state_with_hash(None);

        let source_text_hash =
            skipped_chapter_source_hash(&raw_path, Some(&previous_chapter_state), true).unwrap();

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

        let source_text_hash =
            skipped_chapter_source_hash(&raw_path, Some(&previous_chapter_state), false).unwrap();

        assert_eq!(source_text_hash, None);
    }

    #[test]
    fn test_retry_delay_secs_backs_off_exponentially() {
        assert_eq!(retry_delay_secs(1), 2);
        assert_eq!(retry_delay_secs(2), 4);
        assert_eq!(retry_delay_secs(3), 8);
    }

    #[tokio::test]
    async fn test_call_translation_with_retry_returns_first_success() {
        let translator = Translator {
            provider: Box::new(SucceedingProvider),
        };
        let output_config = OutputConfig::default();

        let (response, error, attempt) =
            call_translation_with_retry(&translator, "text", &[], &None, &output_config).await;

        assert!(response.is_some());
        assert!(error.is_none());
        assert_eq!(attempt, 1);
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
    fn test_skipped_chapter_source_hash_backfills_legacy_hash_during_combined_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        let chapter_text = "# Chapter 1\n\nSource text\n";
        std::fs::write(&raw_path, chapter_text).unwrap();

        let previous_chapter_state = previous_chapter_state_with_hash(None);

        let source_text_hash =
            skipped_chapter_source_hash(&raw_path, Some(&previous_chapter_state), true).unwrap();

        assert_eq!(
            source_text_hash,
            Some(normalized_source_text_hash(chapter_text))
        );
    }
}
