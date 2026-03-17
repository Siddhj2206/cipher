use crate::book::BookLayout;
use crate::state::{
    failed_chapters, load_all_chapter_states, load_run_metadata, summarize_chapters,
};
use anyhow::Result;
use std::path::Path;

pub fn show_status(book_dir: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);

    println!("Book: {}", layout.paths.root.display());

    let metadata = load_run_metadata(book_dir)?;
    let chapters = load_all_chapter_states(book_dir)?;

    match metadata {
        Some(metadata) => {
            print_run_state(&metadata, &chapters);
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

fn print_run_state(
    metadata: &crate::state::RunMetadata,
    chapters: &std::collections::BTreeMap<String, crate::state::ChapterState>,
) {
    println!("- Profile: {}", metadata.profile);
    println!("- Provider: {}", metadata.provider);
    println!("- Model: {}", metadata.model);
    println!();

    let summary = summarize_chapters(chapters);
    println!(
        "Chapters: {} total, {} translated, {} skipped, {} failed, {} pending",
        summary.total, summary.success, summary.skipped, summary.failed, summary.pending
    );
    println!();

    let failed = failed_chapters(chapters);
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
