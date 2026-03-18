use crate::book::{BookLayout, load_book_config};
use crate::config::{GlobalConfig, validate_profile};
use crate::glossary::{
    GlossaryTerm, InjectionMode, SelectionResult, glossary_term_key,
    glossary_term_prompt_fingerprint, load_glossary, merge_terms, save_glossary,
    select_terms_for_text,
};
use crate::output::{detail, detail_kv, section, stderr_detail, warn};
use crate::state::{
    ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterState, ChapterStatus, GlossaryInjectionMode,
    GlossaryState, GlossaryStateTerm, RunMetadata, RunOptions, load_all_chapter_states,
    load_glossary_state, normalize_chapter_path, save_chapter_state, save_glossary_state,
    save_run_metadata,
};
use crate::translate::Translator;
use crate::validate::validate_translation;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct TranslateOptions {
    pub profile: Option<String>,
    pub overwrite: bool,
    pub fail_fast: bool,
    pub rerun_affected_glossary: bool,
}

struct ChapterResult {
    translated: bool,
    failed: bool,
    skipped: bool,
    new_terms_added: usize,
    chapter_state: ChapterState,
}

#[derive(Debug, Clone)]
struct GlossaryRerunDecision {
    reason: String,
    injection_mode: InjectionMode,
}

#[derive(Debug, Default)]
struct GlossaryRerunPlan {
    forced_chapters: BTreeMap<String, GlossaryRerunDecision>,
    warnings: Vec<String>,
    changed_term_count: usize,
    approximate_smart_checks: usize,
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

impl GlossaryRerunPlan {
    fn decision_for(&self, filename: &str) -> Option<&GlossaryRerunDecision> {
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
    let injection_mode = InjectionMode::from_str(&book_config.glossary_injection);
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
    let chapters = discover_chapters(&layout.paths.raw_dir)?;
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

    let rerun_plan = if options.rerun_affected_glossary {
        section("Planning glossary-affected chapter reruns");
        let plan = build_glossary_rerun_plan(
            &chapters,
            &layout.paths.raw_dir,
            out_dir,
            previous_glossary_state.as_ref(),
            &previous_chapter_states,
            &glossary,
            injection_mode,
        )?;
        detail_kv("Changed glossary terms", plan.changed_term_count);
        detail_kv("Affected chapters", plan.forced_chapters.len());
        if plan.approximate_smart_checks > 0 {
            detail_kv("Approximate smart checks", plan.approximate_smart_checks);
        }
        for warning in &plan.warnings {
            warn(warning);
        }
        plan
    } else {
        GlossaryRerunPlan::default()
    };

    section("Translating chapters");
    detail_kv("Chapters found", chapters.len());

    // Create run state with options
    let run_options = RunOptions {
        overwrite: options.overwrite,
        fail_fast: options.fail_fast,
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

    // Process each chapter
    for chapter_file in &chapters {
        let chapter_path = chapter_state_key(&layout.paths.raw_dir, &chapter_file)?;
        let out_path = chapter_output_path(out_dir, &chapter_file)?;
        let previous_chapter_state = previous_chapter_states.get(&chapter_path);
        let rerun_decision = rerun_plan.decision_for(&chapter_path);

        let result = translate_single_chapter(
            &translator,
            &chapter_file,
            &out_path,
            &chapter_path,
            &options,
            previous_chapter_state,
            rerun_decision,
            &mut glossary,
            &style_guide,
            injection_mode,
            &layout.paths.glossary_json,
            book_dir,
        )
        .await?;

        checkpoint_chapter_progress(book_dir, &mut run_metadata, &result.chapter_state)?;
        previous_chapter_states.insert(chapter_path, result.chapter_state.clone());

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
        new_glossary_terms += result.new_terms_added;
    }

    let baseline_outcome = finalize_glossary_baseline(
        book_dir,
        &options,
        previous_glossary_state.as_ref(),
        &run_start_glossary_state,
        &chapters,
        &layout.paths.raw_dir,
        out_dir,
        &previous_chapter_states,
        &glossary,
        injection_mode,
        failed,
    )?;

    if baseline_outcome.remaining_forced_chapters > 0 {
        warn(format!(
            "Glossary baseline not updated; {} affected chapter(s) still need reruns.",
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
    detail_kv("New glossary terms", new_glossary_terms);

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
    rerun_decision: Option<&GlossaryRerunDecision>,
    glossary: &mut Vec<GlossaryTerm>,
    style_guide: &Option<String>,
    injection_mode: InjectionMode,
    glossary_path: &Path,
    book_dir: &Path,
) -> Result<ChapterResult> {
    let chapter_injection_mode = rerun_decision
        .map(|decision| decision.injection_mode)
        .unwrap_or(injection_mode);

    // Check if output exists
    let output_exists = out_path.exists();
    if !options.overwrite && output_exists && rerun_decision.is_none() {
        return Ok(ChapterResult {
            translated: false,
            failed: false,
            skipped: true,
            new_terms_added: 0,
            chapter_state: ChapterState::new(
                chapter_path.to_string(),
                ChapterStatus::Skipped,
                None,
                None,
                previous_chapter_state.and_then(|state| state.glossary_usage.clone()),
            ),
        });
    }

    if let Some(decision) = rerun_decision {
        println!("Retranslating {}", chapter_path);
        detail_kv("Reason", &decision.reason);
    } else {
        println!("Translating {}", chapter_path);
    }

    // Read chapter
    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;

    if chapter_text.trim().is_empty() {
        return Ok(ChapterResult {
            translated: false,
            failed: false,
            skipped: true,
            new_terms_added: 0,
            chapter_state: ChapterState::new(
                chapter_path.to_string(),
                ChapterStatus::Skipped,
                Some("Empty file".to_string()),
                None,
                previous_chapter_state.and_then(|state| state.glossary_usage.clone()),
            ),
        });
    }

    // Select glossary terms and display info
    let start = Instant::now();
    let selection = select_terms_for_text(glossary, &chapter_text, chapter_injection_mode);
    print_glossary_info(&selection, chapter_injection_mode);

    // Attempt translation with retries
    let (response, last_error) =
        attempt_translation(translator, &chapter_text, &selection, style_guide).await;

    let duration = start.elapsed();

    if let Some(resp) = response {
        // Backup if overwriting existing file
        if output_exists {
            let backup_path = create_backup(book_dir, out_path)?;
            detail_kv("Backup", backup_path.display());
        }

        // Write output atomically
        atomic_write(out_path, &resp.translation)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;

        // Merge glossary terms
        let new_terms_added =
            merge_new_glossary_terms(glossary, resp.new_glossary_terms, glossary_path)?;

        detail_kv("Result", "translated");
        return Ok(ChapterResult {
            translated: true,
            failed: false,
            skipped: false,
            new_terms_added,
            chapter_state: ChapterState::new(
                chapter_path.to_string(),
                ChapterStatus::Success,
                None,
                Some(duration.as_millis() as u64),
                Some(build_chapter_glossary_usage(
                    &selection,
                    chapter_injection_mode,
                )),
            ),
        });
    }

    let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
    detail_kv(
        "Result",
        format!("failed after {} attempts: {}", MAX_API_RETRIES, error_msg),
    );
    Ok(ChapterResult {
        translated: false,
        failed: true,
        skipped: false,
        new_terms_added: 0,
        chapter_state: ChapterState::new(
            chapter_path.to_string(),
            ChapterStatus::Failed,
            Some(error_msg),
            Some(duration.as_millis() as u64),
            previous_chapter_state.and_then(|state| state.glossary_usage.clone()),
        ),
    })
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

    if !options.rerun_affected_glossary {
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

    let remaining_plan = build_glossary_rerun_plan(
        chapters,
        raw_dir,
        out_dir,
        Some(previous_glossary_state),
        chapter_states,
        glossary,
        injection_mode,
    )?;

    if remaining_plan.forced_chapters.is_empty() {
        save_glossary_state(book_dir, &current_glossary_state)?;
        Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::CommitRunEnd,
            remaining_forced_chapters: 0,
        })
    } else {
        Ok(GlossaryBaselineOutcome {
            advance: GlossaryBaselineAdvance::KeepExisting,
            remaining_forced_chapters: remaining_plan.forced_chapters.len(),
        })
    }
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
    let current_fingerprints = snapshot_fingerprints(&current_glossary_state.terms);

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
        return Ok(plan);
    }

    if changed_term_keys.is_empty() {
        return Ok(plan);
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
                if let Some(decision) =
                    exact_rerun_decision(usage, &changed_term_keys, &current_fingerprints)
                {
                    plan.forced_chapters.insert(chapter_path, decision);
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
                            injection_mode: InjectionMode::Full,
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
    usage: &ChapterGlossaryUsage,
    changed_term_keys: &BTreeSet<String>,
    current_fingerprints: &BTreeMap<String, String>,
) -> Option<GlossaryRerunDecision> {
    let effective_mode = effective_usage_injection_mode(usage);

    if effective_mode == InjectionMode::Full {
        return full_glossary_rerun_reason(changed_term_keys).map(|reason| GlossaryRerunDecision {
            reason,
            injection_mode: InjectionMode::Full,
        });
    }

    let changed_keys: Vec<String> = usage
        .terms
        .iter()
        .filter_map(|term| match current_fingerprints.get(&term.key) {
            Some(fingerprint) if fingerprint == &term.fingerprint => None,
            _ => Some(term.key.clone()),
        })
        .collect();

    if changed_keys.is_empty() {
        None
    } else {
        Some(GlossaryRerunDecision {
            reason: format!(
                "matched changed glossary term(s): {}",
                changed_keys.join(", ")
            ),
            injection_mode: InjectionMode::Smart,
        })
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
                reason: format!(
                    "approximate smart fallback matched full glossary change: {}",
                    reason
                ),
                injection_mode: InjectionMode::Full,
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
                "approximate smart glossary change: {}",
                changed_keys.into_iter().collect::<Vec<_>>().join(", ")
            ),
            injection_mode: InjectionMode::Smart,
        }))
    }
}

fn full_glossary_rerun_reason(changed_term_keys: &BTreeSet<String>) -> Option<String> {
    if changed_term_keys.is_empty() {
        None
    } else {
        Some(format!(
            "full glossary changed: {}",
            changed_term_keys
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ))
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

fn effective_usage_injection_mode(usage: &ChapterGlossaryUsage) -> InjectionMode {
    if usage.used_fallback_to_full || usage.injection_mode == GlossaryInjectionMode::Full {
        InjectionMode::Full
    } else {
        InjectionMode::Smart
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
                        "full (fallback from smart), {}/{} terms",
                        selection.selected_count, selection.total_count
                    ),
                );
            } else {
                detail_kv(
                    "Glossary",
                    format!(
                        "smart, {}/{} terms",
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
) -> (
    Option<crate::translate::TranslationResponse>,
    Option<String>,
) {
    let mut last_error: Option<String> = None;

    for api_attempt in 1..=MAX_API_RETRIES {
        match translator
            .translate_chapter(chapter_text, &selection.terms, style_guide.clone())
            .await
        {
            Ok(resp) => {
                let validation = validate_translation(&resp.translation);
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
                        format!(
                            "failed: {}. Attempting repair.",
                            validation_errors.join(", ")
                        ),
                    );

                    let repair_req =
                        crate::translate::TranslationRequest::new(chapter_text.to_string())
                            .with_glossary_terms(selection.terms.clone())
                            .with_style_guide(style_guide.clone())
                            .with_failed_translation(resp.translation)
                            .with_validation_errors(validation_errors.to_vec());

                    match translator.translate_with_request(&repair_req).await {
                        Ok(repair_resp) => {
                            let repair_validation = validate_translation(&repair_resp.translation);
                            if repair_validation.is_valid() {
                                detail_kv("Repair", "succeeded");
                                return (Some(repair_resp), None);
                            }
                            last_error = Some(format!(
                                "Repair validation failed: {}",
                                repair_validation.errors().join(", ")
                            ));
                            detail_kv("Repair", last_error.as_ref().unwrap());
                        }
                        Err(e) => {
                            last_error = Some(format!("Repair API error: {}", e));
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
                            "{}/{} failed: {}. Retrying in {}s.",
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

fn merge_new_glossary_terms(
    glossary: &mut Vec<GlossaryTerm>,
    new_terms: Vec<GlossaryTerm>,
    glossary_path: &Path,
) -> Result<usize> {
    if new_terms.is_empty() {
        return Ok(0);
    }

    let (merged, added, dupes) = merge_terms(std::mem::take(glossary), new_terms);
    *glossary = merged;

    if added > 0 {
        if dupes > 0 {
            detail(format!(
                "Added {} new glossary {} and skipped {} duplicate{}.",
                added,
                pluralize(added, "term", "terms"),
                dupes,
                pluralize(dupes, "", "s")
            ));
        } else {
            detail(format!(
                "Added {} new glossary {}.",
                added,
                pluralize(added, "term", "terms")
            ));
        }
        save_glossary(glossary_path, glossary)?;
    } else if dupes > 0 {
        detail(format!(
            "No new glossary terms added. Skipped {} duplicate{}.",
            dupes,
            pluralize(dupes, "", "s")
        ));
    }

    Ok(added)
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

    fn translate_options(rerun_affected_glossary: bool) -> TranslateOptions {
        TranslateOptions {
            profile: None,
            overwrite: false,
            fail_fast: false,
            rerun_affected_glossary,
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
            &translate_options(false),
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
            &translate_options(true),
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

        let previous_glossary = smart_glossary("Old hero definition");
        let current_glossary = smart_glossary("New hero definition");
        let previous_state = previous_glossary_state(&previous_glossary, InjectionMode::Smart);
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
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
            ),
        )]);

        let outcome = finalize_glossary_baseline(
            dir.path(),
            &translate_options(true),
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
            &translate_options(true),
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
            &translate_options(false),
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
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Full,
                )),
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
        assert_eq!(decision.injection_mode, InjectionMode::Full);
        assert!(decision.reason.contains("full glossary changed"));
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
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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
        assert_eq!(decision.injection_mode, InjectionMode::Smart);
        assert!(decision.reason.contains("matched changed glossary term"));
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
        assert_eq!(decision.injection_mode, InjectionMode::Smart);
        assert!(
            decision
                .reason
                .contains("approximate smart glossary change")
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
        assert_eq!(decision.injection_mode, InjectionMode::Full);
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
                Some(build_chapter_glossary_usage(
                    &selection,
                    InjectionMode::Smart,
                )),
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
        assert_eq!(decision.injection_mode, InjectionMode::Full);
        assert!(decision.reason.contains("full glossary changed"));
    }
}
