use crate::book::paths::{chapter_output_path, chapter_state_key};
use crate::output::{stderr_detail_kv, stderr_status, verbose_detail_kv};
use crate::translate::rerun::{
    GlossaryRerunPlan, RerunDecision, SourceRerunPlan, combine_rerun_decisions,
};
use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

pub(crate) const EMPTY_CHAPTER_SKIP_REASON: &str = "Chapter is empty";
pub(crate) const OUTPUT_EXISTS_SKIP_REASON: &str = "Output exists and no rerun reason matched";
pub(crate) const OUTPUT_MISSING_REASON: &str = "No output exists yet";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewAction {
    Translate,
    Rerun,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChapterPreview {
    pub chapter_path: String,
    pub action: PreviewAction,
    pub reason: String,
    pub approximate: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewSummary {
    pub translate: usize,
    pub rerun: usize,
    pub skip: usize,
    pub approximate_reruns: usize,
    pub exact_reruns: usize,
    pub empty_skips: usize,
    pub output_exists_skips: usize,
    pub output_missing: usize,
}

pub(crate) fn preview_translation_run(
    chapters: &VecDeque<PathBuf>,
    raw_dir: &Path,
    out_dir: &Path,
    options: &crate::translate::TranslateOptions,
    glossary_rerun_plan: &GlossaryRerunPlan,
    source_rerun_plan: &SourceRerunPlan,
) -> Result<i32> {
    let previews = build_chapter_previews(
        chapters,
        raw_dir,
        out_dir,
        options,
        glossary_rerun_plan,
        source_rerun_plan,
    )?;
    let summary = summarize_previews(&previews);

    stderr_status("Planned actions");
    for preview in &previews {
        stderr_status(preview_display_line(preview));
        verbose_detail_kv("Reason", &preview.reason);
    }

    stderr_status("Preview summary");
    stderr_detail_kv("Translate", summary.translate);
    stderr_detail_kv("Rerun", summary.rerun);
    stderr_detail_kv("Skip", summary.skip);
    if summary.exact_reruns > 0 {
        stderr_detail_kv("Exact reruns", summary.exact_reruns);
    }
    if summary.approximate_reruns > 0 {
        stderr_detail_kv("Approximate reruns", summary.approximate_reruns);
    }
    if summary.output_missing > 0 {
        stderr_detail_kv("Missing outputs", summary.output_missing);
    }
    if summary.output_exists_skips > 0 {
        stderr_detail_kv("Existing-output skips", summary.output_exists_skips);
    }
    if summary.empty_skips > 0 {
        stderr_detail_kv("Empty chapters", summary.empty_skips);
    }

    Ok(0)
}

fn build_chapter_previews(
    chapters: &VecDeque<PathBuf>,
    raw_dir: &Path,
    out_dir: &Path,
    options: &crate::translate::TranslateOptions,
    glossary_rerun_plan: &GlossaryRerunPlan,
    source_rerun_plan: &SourceRerunPlan,
) -> Result<Vec<ChapterPreview>> {
    let mut previews = Vec::with_capacity(chapters.len());

    for chapter_file in chapters {
        let chapter_path = chapter_state_key(raw_dir, chapter_file)?;
        let output_exists = chapter_output_path(out_dir, chapter_file)?.exists();
        let rerun_decision = combine_rerun_decisions(
            glossary_rerun_plan.decision_for(&chapter_path),
            source_rerun_plan.decision_for(&chapter_path),
        );

        previews.push(preview_for_chapter(
            chapter_file,
            chapter_path,
            output_exists,
            options,
            rerun_decision.as_ref(),
        )?);
    }

    Ok(previews)
}

pub(crate) fn preview_for_chapter(
    raw_path: &Path,
    chapter_path: String,
    output_exists: bool,
    options: &crate::translate::TranslateOptions,
    rerun_decision: Option<&RerunDecision>,
) -> Result<ChapterPreview> {
    let chapter_text = std::fs::read_to_string(raw_path)
        .with_context(|| format!("Failed to read {}", raw_path.display()))?;

    if chapter_text.trim().is_empty() {
        return Ok(ChapterPreview {
            chapter_path,
            action: PreviewAction::Skip,
            reason: EMPTY_CHAPTER_SKIP_REASON.to_string(),
            approximate: false,
        });
    }

    if options.overwrite {
        return Ok(ChapterPreview {
            chapter_path,
            action: if output_exists {
                PreviewAction::Rerun
            } else {
                PreviewAction::Translate
            },
            reason: if output_exists {
                "Overwrite requested".to_string()
            } else {
                OUTPUT_MISSING_REASON.to_string()
            },
            approximate: false,
        });
    }

    if let Some(decision) = rerun_decision {
        return Ok(ChapterPreview {
            chapter_path,
            action: PreviewAction::Rerun,
            reason: decision.reason.clone(),
            approximate: decision.is_approximate,
        });
    }

    if !output_exists {
        return Ok(ChapterPreview {
            chapter_path,
            action: PreviewAction::Translate,
            reason: OUTPUT_MISSING_REASON.to_string(),
            approximate: false,
        });
    }

    Ok(ChapterPreview {
        chapter_path,
        action: PreviewAction::Skip,
        reason: OUTPUT_EXISTS_SKIP_REASON.to_string(),
        approximate: false,
    })
}

pub(crate) fn preview_display_line(preview: &ChapterPreview) -> String {
    let action = match preview.action {
        PreviewAction::Translate => "Translate",
        PreviewAction::Rerun => "Rerun",
        PreviewAction::Skip => "Skip",
    };

    if preview.action == PreviewAction::Skip && preview.reason == EMPTY_CHAPTER_SKIP_REASON {
        format!("{} {}: chapter is empty", action, preview.chapter_path)
    } else {
        format!("{} {}", action, preview.chapter_path)
    }
}

pub(crate) fn summarize_previews(previews: &[ChapterPreview]) -> PreviewSummary {
    let mut summary = PreviewSummary::default();

    for preview in previews {
        match preview.action {
            PreviewAction::Translate => {
                summary.translate += 1;
                if preview.reason == OUTPUT_MISSING_REASON {
                    summary.output_missing += 1;
                }
            }
            PreviewAction::Rerun => {
                summary.rerun += 1;
                if preview.approximate {
                    summary.approximate_reruns += 1;
                } else {
                    summary.exact_reruns += 1;
                }
            }
            PreviewAction::Skip => {
                summary.skip += 1;
                if preview.reason == EMPTY_CHAPTER_SKIP_REASON {
                    summary.empty_skips += 1;
                }
                if preview.reason == OUTPUT_EXISTS_SKIP_REASON {
                    summary.output_exists_skips += 1;
                }
            }
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::rerun::RerunDecision;
    use crate::translate::test_helpers::translate_options;

    #[test]
    fn test_preview_for_chapter_skips_empty_chapter() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        std::fs::write(&raw_path, " \n\t").unwrap();

        let preview = preview_for_chapter(
            &raw_path,
            "chapter1.md".to_string(),
            false,
            &translate_options(None),
            None,
        )
        .unwrap();

        assert_eq!(preview.action, PreviewAction::Skip);
        assert_eq!(preview.reason, EMPTY_CHAPTER_SKIP_REASON);
        assert_eq!(
            preview_display_line(&preview),
            "Skip chapter1.md: chapter is empty"
        );
    }

    #[test]
    fn test_preview_for_chapter_marks_approximate_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("chapter1.md");
        std::fs::write(&raw_path, "content").unwrap();
        let rerun_decision = RerunDecision {
            reason: "Glossary selection changed (approx): hero".to_string(),
            is_approximate: true,
        };

        let preview = preview_for_chapter(
            &raw_path,
            "chapter1.md".to_string(),
            true,
            &translate_options(Some(crate::RerunMode::Glossary)),
            Some(&rerun_decision),
        )
        .unwrap();

        assert_eq!(preview.action, PreviewAction::Rerun);
        assert!(preview.approximate);
    }

    #[test]
    fn test_summarize_previews_counts_categories() {
        let previews = vec![
            ChapterPreview {
                chapter_path: "chapter1.md".to_string(),
                action: PreviewAction::Translate,
                reason: "No output exists yet".to_string(),
                approximate: false,
            },
            ChapterPreview {
                chapter_path: "chapter2.md".to_string(),
                action: PreviewAction::Rerun,
                reason: "Chapter source changed".to_string(),
                approximate: false,
            },
            ChapterPreview {
                chapter_path: "chapter3.md".to_string(),
                action: PreviewAction::Rerun,
                reason: "Glossary selection changed (approx): hero".to_string(),
                approximate: true,
            },
            ChapterPreview {
                chapter_path: "chapter4.md".to_string(),
                action: PreviewAction::Skip,
                reason: OUTPUT_EXISTS_SKIP_REASON.to_string(),
                approximate: false,
            },
            ChapterPreview {
                chapter_path: "chapter5.md".to_string(),
                action: PreviewAction::Skip,
                reason: EMPTY_CHAPTER_SKIP_REASON.to_string(),
                approximate: false,
            },
        ];

        let summary = summarize_previews(&previews);

        assert_eq!(summary.translate, 1);
        assert_eq!(summary.rerun, 2);
        assert_eq!(summary.skip, 2);
        assert_eq!(summary.output_missing, 1);
        assert_eq!(summary.exact_reruns, 1);
        assert_eq!(summary.approximate_reruns, 1);
        assert_eq!(summary.output_exists_skips, 1);
        assert_eq!(summary.empty_skips, 1);
    }
}
