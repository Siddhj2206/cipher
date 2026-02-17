use crate::book::BookLayout;
use crate::state::{ChapterStatus, RunState};
use anyhow::Result;
use std::path::Path;

pub fn show_status(book_dir: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);

    println!("Book: {}", layout.paths.root.display());
    println!();

    match RunState::load(book_dir)? {
        Some(state) => {
            print_run_state(&state);
        }
        None => {
            println!("No translation runs recorded yet.");
            println!();
            println!("To translate this book, run:");
            println!("  cipher translate {}", book_dir.display());
        }
    }

    Ok(())
}

fn print_run_state(state: &RunState) {
    // Run info
    println!("Last Run");
    println!("  Started:  {}", state.started_at);
    if let Some(ref finished) = state.finished_at {
        println!("  Finished: {}", finished);
    }
    println!("  Duration: {}", state.format_duration());
    println!();

    // Configuration
    println!("Configuration");
    println!("  Profile:  {}", state.profile);
    println!("  Provider: {}", state.provider);
    println!("  Model:    {}", state.model);

    if let Some(ref opts) = state.options {
        println!();
        println!("  Options:");
        println!("    overwrite:     {}", opts.overwrite);
        println!("    overwrite_bad: {}", opts.overwrite_bad);
        println!("    backup:        {}", opts.backup);
        println!("    fail_fast:     {}", opts.fail_fast);
    }
    println!();

    // Summary
    let summary = state.get_summary();
    println!("Summary");
    println!("  Total:    {}", summary.total);
    println!("  Success:  {}", summary.success);
    println!("  Failed:   {}", summary.failed);
    println!("  Skipped:  {}", summary.skipped);
    println!();

    // Failed chapters
    let failed = state.get_failed_chapters();
    if !failed.is_empty() {
        println!("Failed Chapters ({}/{}):", failed.len(), summary.total);
        for (filename, chapter) in failed {
            println!("  - {}", filename);
            if let Some(ref error) = chapter.error {
                // Truncate long errors
                let error_preview = if error.len() > 80 {
                    format!("{}...", &error[..80])
                } else {
                    error.clone()
                };
                println!("      Error: {}", error_preview);
            }
        }
        println!();
    }

    // Recent chapters (last 10)
    if !state.chapters.is_empty() {
        println!("Chapter Status (showing last 10):");
        let recent: Vec<_> = state.chapters.iter().rev().take(10).collect();
        for (filename, chapter) in recent.iter().rev() {
            let status_icon = match chapter.status {
                ChapterStatus::Success => "✓",
                ChapterStatus::Failed => "✗",
                ChapterStatus::Skipped => "⊘",
                ChapterStatus::Pending => "○",
            };

            let time_info = if let Some(duration_ms) = chapter.translation_time_ms {
                format!(" ({:.1}s)", duration_ms as f64 / 1000.0)
            } else {
                String::new()
            };

            println!("  {} {}{}", status_icon, filename, time_info);
        }

        if state.chapters.len() > 10 {
            println!("  ... and {} more", state.chapters.len() - 10);
        }
    }
}
