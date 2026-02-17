use crate::book::{load_book_config, BookLayout};
use crate::config::{GlobalConfig, validate_profile};
use crate::glossary::{load_glossary, merge_terms, save_glossary};
use crate::state::{ChapterStatus, RunOptions, RunState};
use crate::translate::Translator;
use crate::validate::validate_translation;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct TranslateOptions {
    pub overwrite: bool,
    pub overwrite_bad: bool,
    pub backup: bool,
    pub fail_fast: bool,
}

pub async fn translate_book(
    book_dir: &Path,
    options: TranslateOptions,
) -> Result<()> {
    // Load book layout
    let layout = BookLayout::discover(book_dir);
    
    if !layout.is_valid_book() {
        anyhow::bail!(
            "Invalid book layout. Run 'cipher doctor {}' for details.",
            book_dir.display()
        );
    }

    // Load global config
    let global_config = GlobalConfig::load()
        .context("Failed to load global config")?;

    // Resolve effective profile
    let book_config = load_book_config(&layout.paths.config_json).unwrap_or_default();
    let profile_name = global_config.effective_profile_name(book_config.profile.as_deref());
    
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

    let profile = global_config.resolve_profile(profile_name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", profile_name))?;

    println!("Using profile: {}", profile_name);
    println!("  Provider: {}", profile.provider);
    println!("  Model: {}", profile.model);
    println!();

    // Create translator
    let translator = Translator::from_config(&global_config, profile_name)
        .context("Failed to create translator")?;

    // Discover chapters
    let chapters = discover_chapters(&layout.paths.raw_dir)?;
    if chapters.is_empty() {
        println!("No chapters found in raw/");
        return Ok(());
    }

    println!("Found {} chapter(s)", chapters.len());
    println!();

    // Load existing glossary
    let mut glossary = load_glossary(&layout.paths.glossary_json)?;
    let initial_glossary_count = glossary.len();

    // Load previous run state for merging
    let previous_state = RunState::load(book_dir)?;

    // Create run state with options
    let run_options = RunOptions {
        overwrite: options.overwrite,
        overwrite_bad: options.overwrite_bad,
        backup: options.backup,
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
        let filename: String = chapter_file.file_name().unwrap().to_string_lossy().into();
        let raw_path = chapter_file;
        let out_path = out_dir.join(&filename);

        // Check if output exists
        let output_exists = out_path.exists();
        let should_translate = if options.overwrite {
            true
        } else if options.overwrite_bad && output_exists {
            // Check if existing output is bad
            let existing = std::fs::read_to_string(&out_path)?;
            !validate_translation(&existing).is_valid()
        } else {
            !output_exists
        };

        if !should_translate {
            println!("[SKIP] {}", filename);
            run_state.set_chapter(&filename, ChapterStatus::Skipped, None, None);
            skipped += 1;
            continue;
        }

        println!("[TRANSLATE] {}", filename);

        // Read chapter
        let chapter_text = std::fs::read_to_string(&raw_path)
            .with_context(|| format!("Failed to read {}", raw_path.display()))?;

        if chapter_text.trim().is_empty() {
            println!("  Empty chapter, skipping");
            run_state.set_chapter(&filename, ChapterStatus::Skipped, Some("Empty chapter".to_string()), None);
            skipped += 1;
            continue;
        }

        // Translate
        let start = Instant::now();
        match translator.translate_chapter(&chapter_text, &glossary).await {
            Ok(response) => {
                let duration = start.elapsed();
                
                // Validate translation
                let validation = validate_translation(&response.translation);
                if !validation.is_valid() {
                    println!("  Validation failed:");
                    for error in validation.errors() {
                        println!("    - {}", error);
                    }
                    run_state.set_chapter(
                        &filename,
                        ChapterStatus::Failed,
                        Some(format!("Validation failed: {}", validation.errors().join(", "))),
                        Some(duration.as_millis() as u64),
                    );
                    failed += 1;
                    
                    if options.fail_fast {
                        println!();
                        println!("Stopping due to --fail-fast");
                        break;
                    }
                    continue;
                }

                // Backup if needed
                if options.backup && output_exists {
                    let backup_path = create_backup(&out_path)?;
                    println!("  Backed up to {}", backup_path.display());
                }

                // Write output
                std::fs::write(&out_path, &response.translation)
                    .with_context(|| format!("Failed to write {}", out_path.display()))?;

                // Merge glossary terms
                if !response.new_glossary_terms.is_empty() {
                    let (merged, added, _) = merge_terms(glossary, response.new_glossary_terms);
                    glossary = merged;
                    if added > 0 {
                        new_glossary_terms += added;
                        println!("  Added {} glossary term(s)", added);
                    }
                }

                println!("  Done in {:.2}s", duration.as_secs_f64());
                run_state.set_chapter(&filename, ChapterStatus::Success, None, Some(duration.as_millis() as u64));
                translated += 1;
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                println!("  Error: {}", err_msg);
                let duration = start.elapsed();
                run_state.set_chapter(&filename, ChapterStatus::Failed, Some(err_msg), Some(duration.as_millis() as u64));
                failed += 1;
                
                if options.fail_fast {
                    println!();
                    println!("Stopping due to --fail-fast");
                    break;
                }
            }
        }
    }

    // Save updated glossary
    if glossary.len() > initial_glossary_count {
        let mut glossary_mut = glossary;
        save_glossary(&layout.paths.glossary_json, &mut glossary_mut)?;
        println!();
        println!("Updated glossary: {} new term(s)", new_glossary_terms);
    }

    // Merge previous state and mark finished
    run_state.merge_previous(previous_state);
    run_state.mark_finished();

    // Save run state
    run_state.save(book_dir)?;

    // Print summary
    println!();
    println!("Translation complete:");
    println!("  Translated: {}", translated);
    println!("  Skipped: {}", skipped);
    println!("  Failed: {}", failed);

    if failed > 0 {
        std::process::exit(1);
    }

    Ok(())
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
        let a_name = a.file_stem().unwrap().to_string_lossy();
        let b_name = b.file_stem().unwrap().to_string_lossy();
        
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

fn create_backup(path: &Path) -> Result<PathBuf> {
    use chrono::Local;
    
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let filename = path.file_stem().unwrap().to_string_lossy();
    let backup_name = format!("{}_{}.md.bak", filename, timestamp);
    let backup_path = path.with_file_name(&backup_name);
    
    std::fs::copy(path, &backup_path)?;
    Ok(backup_path)
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
}
