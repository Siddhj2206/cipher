use crate::book::{BookLayout, load_book_config};
use crate::config::{GlobalConfig, validate_profile};
use crate::glossary::{
    GlossaryTerm, InjectionMode, SelectionResult, load_glossary, merge_terms, save_glossary,
    select_terms_for_text,
};
use crate::state::{ChapterStatus, RunOptions, RunState};
use crate::translate::Translator;
use crate::validate::validate_translation;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct TranslateOptions {
    pub profile: Option<String>,
    pub overwrite: bool,
    pub fail_fast: bool,
}

struct ChapterResult {
    translated: bool,
    failed: bool,
    skipped: bool,
    new_terms_added: usize,
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
        eprintln!("Profile validation failed:");
        for error in &validation.errors {
            eprintln!("  - {}", error);
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
        println!("No chapters found in raw/");
        return Ok(());
    }

    println!("Using profile {}", profile_name);
    println!("- Provider: {}", profile.provider);
    println!("- Model: {}", profile.model);
    println!();
    println!("Translating chapters...");
    println!("Found {} files to translate", chapters.len());

    // Load existing glossary
    let mut glossary = load_glossary(&layout.paths.glossary_json)?;

    // Load style guide if it exists
    let style_guide = if layout.exists.style_md {
        match std::fs::read_to_string(&layout.paths.style_md) {
            Ok(content) if !content.trim().is_empty() => {
                println!("- Using style guide: {}", layout.paths.style_md.display());
                Some(content)
            }
            _ => None,
        }
    } else {
        None
    };

    // Load previous run state for merging
    let previous_state = RunState::load(book_dir)?;

    // Create run state with options
    let run_options = RunOptions {
        overwrite: options.overwrite,
        fail_fast: options.fail_fast,
    };

    let mut run_state = RunState::new(
        profile_name.to_string(),
        profile.provider.clone(),
        profile.model.clone(),
        Some(run_options),
    );

    // Determine output directory
    let out_dir = layout.effective_out_dir();

    // Track stats
    let mut translated = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut new_glossary_terms = 0;

    // Process each chapter
    for chapter_file in chapters {
        let filename = chapter_file
            .file_name()
            .context("Invalid chapter filename")?
            .to_string_lossy()
            .into_owned();
        let out_path = out_dir.join(&filename);

        let result = translate_single_chapter(
            &translator,
            &chapter_file,
            &out_path,
            &filename,
            &options,
            &mut glossary,
            &style_guide,
            injection_mode,
            &layout.paths.glossary_json,
            book_dir,
            &mut run_state,
        )
        .await?;

        if result.translated {
            translated += 1;
        }
        if result.skipped {
            skipped += 1;
        }
        if result.failed {
            failed += 1;
            if options.fail_fast {
                println!("Stopping due to --fail-fast");
                break;
            }
        }
        new_glossary_terms += result.new_terms_added;
    }

    // Merge previous state and mark finished
    run_state.merge_previous(previous_state);
    run_state.mark_finished();

    // Save run state
    run_state.save(book_dir)?;

    // Print summary
    println!();
    println!("Translation complete");
    println!("- Translated: {}", translated);
    println!("- Skipped: {}", skipped);
    println!("- Failed: {}", failed);
    println!("- New glossary terms: {}", new_glossary_terms);

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
    filename: &str,
    options: &TranslateOptions,
    glossary: &mut Vec<GlossaryTerm>,
    style_guide: &Option<String>,
    injection_mode: InjectionMode,
    glossary_path: &Path,
    book_dir: &Path,
    run_state: &mut RunState,
) -> Result<ChapterResult> {
    // Check if output exists
    let output_exists = out_path.exists();
    if !options.overwrite && output_exists {
        println!("Skipping {} (already translated)", filename);
        run_state.set_chapter(filename, ChapterStatus::Skipped, None, None);
        return Ok(ChapterResult {
            translated: false,
            failed: false,
            skipped: true,
            new_terms_added: 0,
        });
    }

    println!("Translating {}", filename);

    // Read chapter
    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;

    if chapter_text.trim().is_empty() {
        println!("Skipping {}: Empty file", filename);
        run_state.set_chapter(
            filename,
            ChapterStatus::Skipped,
            Some("Empty file".to_string()),
            None,
        );
        return Ok(ChapterResult {
            translated: false,
            failed: false,
            skipped: true,
            new_terms_added: 0,
        });
    }

    // Select glossary terms and display info
    let start = Instant::now();
    let selection = select_terms_for_text(glossary, &chapter_text, injection_mode);
    print_glossary_info(&selection, injection_mode);

    // Attempt translation with retries
    let (response, last_error) =
        attempt_translation(translator, &chapter_text, &selection, style_guide).await;

    let duration = start.elapsed();

    if let Some(resp) = response {
        // Backup if overwriting existing file
        if output_exists {
            let backup_path = create_backup(book_dir, out_path)?;
            println!("- Backed up to {}", backup_path.display());
        }

        // Write output atomically
        atomic_write(out_path, &resp.translation)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;

        // Merge glossary terms
        let new_terms_added = merge_new_glossary_terms(glossary, resp.new_glossary_terms, glossary_path)?;

        println!("- Successfully translated {}", filename);
        run_state.set_chapter(
            filename,
            ChapterStatus::Success,
            None,
            Some(duration.as_millis() as u64),
        );
        run_state.save(book_dir)?;

        Ok(ChapterResult {
            translated: true,
            failed: false,
            skipped: false,
            new_terms_added,
        })
    } else {
        let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
        println!(
            "- Failed to translate {} after {} attempts: {}",
            filename, MAX_API_RETRIES, error_msg
        );
        run_state.set_chapter(
            filename,
            ChapterStatus::Failed,
            Some(error_msg),
            Some(duration.as_millis() as u64),
        );
        run_state.save(book_dir)?;

        Ok(ChapterResult {
            translated: false,
            failed: true,
            skipped: false,
            new_terms_added: 0,
        })
    }
}

fn print_glossary_info(selection: &SelectionResult, injection_mode: InjectionMode) {
    match injection_mode {
        InjectionMode::Smart => {
            if selection.used_fallback_to_full {
                println!(
                    "- Using full glossary (fallback from smart): {}/{} terms",
                    selection.selected_count, selection.total_count
                );
            } else {
                println!(
                    "- Using smart glossary: {}/{} terms",
                    selection.selected_count, selection.total_count
                );
            }
        }
        InjectionMode::Full => {
            println!("- Using full glossary: {} terms", selection.total_count);
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
                    println!(
                        "- Validation failed: {}. Attempting repair...",
                        validation_errors.join(", ")
                    );

                    let repair_req =
                        crate::translate::TranslationRequest::new(chapter_text.to_string())
                            .with_glossary_terms(selection.terms.clone())
                            .with_style_guide(style_guide.clone())
                            .with_failed_translation(resp.translation)
                            .with_validation_errors(validation_errors.to_vec());

                    match translator.translate_with_request(&repair_req).await {
                        Ok(repair_resp) => {
                            let repair_validation =
                                validate_translation(&repair_resp.translation);
                            if repair_validation.is_valid() {
                                println!("- Repair succeeded");
                                return (Some(repair_resp), None);
                            }
                            last_error = Some(format!(
                                "Repair validation failed: {}",
                                repair_validation.errors().join(", ")
                            ));
                            println!("- {}", last_error.as_ref().unwrap());
                        }
                        Err(e) => {
                            last_error = Some(format!("Repair API error: {}", e));
                            println!("- {}", last_error.as_ref().unwrap());
                        }
                    }
                }

                break;
            }
            Err(e) => {
                last_error = Some(format!("API error: {}", e));
                if api_attempt < MAX_API_RETRIES {
                    let delay_secs = 2u64.pow(api_attempt as u32);
                    println!(
                        "- Attempt {}/{} failed: {}. Retrying in {}s...",
                        api_attempt,
                        MAX_API_RETRIES,
                        last_error.as_ref().unwrap(),
                        delay_secs
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
            println!(
                "- Added {} new term/s to glossary ({} duplicate/s skipped)",
                added, dupes
            );
        } else {
            println!("- Added {} new term/s to glossary", added);
        }
        save_glossary(glossary_path, glossary)?;
    } else if dupes > 0 {
        println!("- No new terms to add ({} duplicate/s skipped)", dupes);
    }

    Ok(added)
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
}
