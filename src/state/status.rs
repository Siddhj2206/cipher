use crate::book::BookLayout;
use crate::output::{detail, detail_kv};
use crate::state::{
    failed_chapters, load_all_chapter_states, load_run_metadata, summarize_chapters,
};
use anyhow::Result;
use std::path::Path;

pub fn show_status(book_dir: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);

    let metadata = load_run_metadata(book_dir)?;
    let chapters = load_all_chapter_states(book_dir)?;

    match metadata {
        Some(metadata) => {
            println!("Book status");
            detail_kv("Book", layout.paths.root.display());
            print_run_state(&metadata, &chapters);
        }
        None => {
            println!("No translation runs recorded yet.");
            detail_kv("Book", layout.paths.root.display());
            detail(format!("Run: cipher translate {}", book_dir.display()));
        }
    }

    Ok(())
}

fn print_run_state(
    metadata: &crate::state::RunMetadata,
    chapters: &std::collections::BTreeMap<String, crate::state::ChapterState>,
) {
    detail_kv("Profile", &metadata.profile);
    detail_kv("Provider", &metadata.provider);
    detail_kv("Model", &metadata.model);
    detail_kv("Started", &metadata.started_at);
    detail_kv("Last updated", &metadata.updated_at);
    if let Some(finished_at) = &metadata.finished_at {
        detail_kv("Finished", finished_at);
    }

    let summary = summarize_chapters(chapters);
    println!("Chapter summary");
    detail_kv("Total", summary.total);
    detail_kv("Translated", summary.success);
    detail_kv("Skipped", summary.skipped);
    detail_kv("Failed", summary.failed);
    detail_kv("Pending", summary.pending);

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
                detail(format!("{}: {}", filename, error_preview));
            } else {
                detail(filename);
            }
        }
    }
}
