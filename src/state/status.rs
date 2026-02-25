use crate::book::BookLayout;
use crate::state::RunState;
use anyhow::Result;
use std::path::Path;

pub fn show_status(book_dir: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);

    println!("Book: {}", layout.paths.root.display());

    match RunState::load(book_dir)? {
        Some(state) => {
            print_run_state(&state);
        }
        None => {
            println!();
            println!("No translation runs recorded yet.");
            println!();
            println!("To translate this book, run:");
            println!("  cipher translate {}", book_dir.display());
        }
    }

    Ok(())
}

fn print_run_state(state: &RunState) {
    // Profile and model info
    println!("- Profile: {}", state.profile);
    println!("- Provider: {}", state.provider);
    println!("- Model: {}", state.model);
    println!();

    // Summary line
    let summary = state.get_summary();
    println!(
        "Chapters: {} total, {} translated, {} skipped, {} failed, {} pending",
        summary.total, summary.success, summary.skipped, summary.failed, summary.pending
    );
    println!();

    // Failed chapters
    let failed = state.get_failed_chapters();
    if !failed.is_empty() {
        println!("Failed chapters:");
        for (filename, chapter) in failed {
            if let Some(ref error) = chapter.error {
                let error_preview = if error.chars().count() > 60 {
                    format!("{}...", error.chars().take(60).collect::<String>())
                } else {
                    error.clone()
                };
                println!("  {}: {}", filename, error_preview);
            } else {
                println!("  {}", filename);
            }
        }
        println!();
    }
}
