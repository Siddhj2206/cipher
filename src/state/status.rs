use crate::book::BookLayout;
use crate::output::{detail, detail_kv, section};
use crate::state::{
    ChapterState, GlossaryInjectionMode, failed_chapters, load_all_chapter_states,
    load_run_metadata, summarize_chapters,
};
use anyhow::Result;
use std::collections::BTreeMap;
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
    chapters: &BTreeMap<String, ChapterState>,
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

    let tracking = summarize_tracking(chapters);
    section("Tracking summary");
    detail_kv("Tracked smart selection", tracking.tracked_smart_selection);
    detail_kv(
        "Tracked fallback to full",
        tracking.tracked_fallback_to_full,
    );
    if tracking.legacy_tracked_full_selection > 0 {
        detail_kv(
            "Legacy tracked full selection",
            tracking.legacy_tracked_full_selection,
        );
    }
    detail_kv(
        "Approximate legacy fallback",
        tracking.approximate_legacy_fallback,
    );
    detail_kv("Exported terms recorded", tracking.exported_terms_recorded);
    detail_kv("Source hash recorded", tracking.source_hash_recorded);

    if tracking.approximate_legacy_fallback > 0 {
        detail(format!(
            "{} chapter(s) still rely on approximate glossary rerun checks.",
            tracking.approximate_legacy_fallback
        ));
    }
    if tracking.legacy_tracked_full_selection > 0 {
        detail(format!(
            "{} chapter(s) still use legacy primary full-glossary tracking; an equivalent successful smart-era run can rewrite them as smart fallback state.",
            tracking.legacy_tracked_full_selection
        ));
    }
    if tracking.missing_source_hash > 0 {
        detail(format!(
            "{} chapter(s) are missing source hashes, so chapter-content reruns are not fully tracked.",
            tracking.missing_source_hash
        ));
    }

    let failed = failed_chapters(chapters);
    if !failed.is_empty() {
        section("Failed chapters:");
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

#[derive(Debug, Default, PartialEq, Eq)]
struct TrackingSummary {
    tracked_smart_selection: usize,
    tracked_fallback_to_full: usize,
    legacy_tracked_full_selection: usize,
    approximate_legacy_fallback: usize,
    exported_terms_recorded: usize,
    source_hash_recorded: usize,
    missing_source_hash: usize,
}

fn summarize_tracking(chapters: &BTreeMap<String, ChapterState>) -> TrackingSummary {
    let mut summary = TrackingSummary::default();

    for chapter in chapters.values() {
        if chapter.source_text_hash.is_some() {
            summary.source_hash_recorded += 1;
        } else {
            summary.missing_source_hash += 1;
        }

        match chapter.glossary_usage.as_ref() {
            Some(usage) => {
                summary.exported_terms_recorded += 1;
                match usage.injection_mode {
                    GlossaryInjectionMode::Smart if !usage.used_fallback_to_full => {
                        summary.tracked_smart_selection += 1;
                    }
                    GlossaryInjectionMode::Smart => {
                        summary.tracked_fallback_to_full += 1;
                    }
                    GlossaryInjectionMode::Full => {
                        summary.legacy_tracked_full_selection += 1;
                    }
                }
            }
            None => {
                summary.approximate_legacy_fallback += 1;
            }
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        ChapterGlossaryTerm, ChapterGlossaryUsage, ChapterStatus, GlossaryInjectionMode,
    };

    fn sample_chapter_state(path: &str) -> ChapterState {
        ChapterState {
            chapter_path: path.to_string(),
            status: ChapterStatus::Success,
            error: None,
            translation_time_ms: None,
            last_attempted: None,
            translation_usage: None,
            glossary_usage: None,
            exported_terms: vec![],
            source_text_hash: None,
        }
    }

    #[test]
    fn test_summarize_tracking_categorizes_tracked_and_legacy_chapters() {
        let mut tracked_smart = sample_chapter_state("001.md");
        tracked_smart.glossary_usage = Some(ChapterGlossaryUsage {
            injection_mode: GlossaryInjectionMode::Smart,
            used_fallback_to_full: false,
            terms: vec![ChapterGlossaryTerm {
                key: "hero".into(),
                fingerprint: "fp-1".into(),
            }],
        });
        tracked_smart.source_text_hash = Some("hash-1".into());

        let mut tracked_full = sample_chapter_state("002.md");
        tracked_full.glossary_usage = Some(ChapterGlossaryUsage {
            injection_mode: GlossaryInjectionMode::Full,
            used_fallback_to_full: false,
            terms: vec![],
        });
        tracked_full.source_text_hash = Some("hash-2".into());

        let mut tracked_fallback = sample_chapter_state("003.md");
        tracked_fallback.glossary_usage = Some(ChapterGlossaryUsage {
            injection_mode: GlossaryInjectionMode::Smart,
            used_fallback_to_full: true,
            terms: vec![],
        });
        tracked_fallback.source_text_hash = Some("hash-3".into());

        let legacy_untracked = sample_chapter_state("004.md");

        let chapters = BTreeMap::from([
            ("001.md".into(), tracked_smart),
            ("002.md".into(), tracked_full),
            ("003.md".into(), tracked_fallback),
            ("004.md".into(), legacy_untracked),
        ]);

        assert_eq!(
            summarize_tracking(&chapters),
            TrackingSummary {
                tracked_smart_selection: 1,
                tracked_fallback_to_full: 1,
                legacy_tracked_full_selection: 1,
                approximate_legacy_fallback: 1,
                exported_terms_recorded: 3,
                source_hash_recorded: 3,
                missing_source_hash: 1,
            }
        );
    }

    #[test]
    fn test_summarize_tracking_handles_empty_state_set() {
        let chapters = BTreeMap::new();
        assert_eq!(summarize_tracking(&chapters), TrackingSummary::default());
    }
}
