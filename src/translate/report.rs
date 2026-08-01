//! Machine-readable JSON report for `cipher translate --json`.
//!
//! The report is the stdout contract for scripting: per-chapter results
//! (path, status, tokens, timing, errors), a run summary, and a typed error
//! envelope for fatal failures. Human-readable progress stays on stderr.

use crate::error::Error;
use crate::state::{ChapterStatus, RunMetadata};
use crate::translate::TranslationUsage;
use crate::translate::orchestrate::ChapterResult;
use crate::translate::preview::{ChapterPreview, PreviewAction, PreviewSummary};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct RunReport {
    book: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<RunMeta>,
    chapters: Vec<ChapterEntry>,
    summary: Summary,
    usage: TranslationUsage,
    cancelled: bool,
    exit_code: i32,
}

#[derive(Debug, Serialize)]
struct RunMeta {
    profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    glossary_profile: Option<String>,
    provider: String,
    model: String,
    started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChapterEntry {
    chapter: String,
    status: ChapterStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    glossary_extraction_error: Option<String>,
    new_terms_added: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<TranslationUsage>,
}

#[derive(Debug, Serialize)]
struct Summary {
    total: usize,
    translated: usize,
    skipped: usize,
    failed: usize,
    new_glossary_terms: usize,
}

/// Data needed to build a run report; collected by `translate_book`.
pub(crate) struct ReportData {
    pub book: String,
    pub run: Option<RunMetadata>,
    pub chapters: Vec<ChapterResult>,
    pub total: usize,
    pub translated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub new_glossary_terms: usize,
    pub usage: TranslationUsage,
    pub cancelled: bool,
    pub exit_code: i32,
}

impl ReportData {
    /// Report for a run that processed no chapters (empty book or dry run).
    pub(crate) fn empty(book: String) -> Self {
        ReportData {
            book,
            run: None,
            chapters: Vec::new(),
            total: 0,
            translated: 0,
            skipped: 0,
            failed: 0,
            new_glossary_terms: 0,
            usage: TranslationUsage::default(),
            cancelled: false,
            exit_code: 0,
        }
    }
}

pub(crate) fn build_run_report(data: &ReportData) -> RunReport {
    RunReport {
        book: data.book.clone(),
        run: data.run.as_ref().map(run_meta),
        chapters: data.chapters.iter().map(chapter_entry).collect(),
        summary: Summary {
            total: data.total,
            translated: data.translated,
            skipped: data.skipped,
            failed: data.failed,
            new_glossary_terms: data.new_glossary_terms,
        },
        usage: data.usage.clone(),
        cancelled: data.cancelled,
        exit_code: data.exit_code,
    }
}

/// Machine-readable dry-run report: per-chapter planned actions and a summary.
#[derive(Debug, Serialize)]
pub(crate) struct PreviewReport {
    book: String,
    dry_run: bool,
    chapters: Vec<PreviewChapterEntry>,
    summary: PreviewSummaryEntry,
    exit_code: i32,
}

#[derive(Debug, Serialize)]
struct PreviewChapterEntry {
    chapter: String,
    action: &'static str,
    reason: String,
    approximate: bool,
}

#[derive(Debug, Serialize)]
struct PreviewSummaryEntry {
    translate: usize,
    rerun: usize,
    skip: usize,
    approximate_reruns: usize,
    exact_reruns: usize,
    empty_skips: usize,
    output_exists_skips: usize,
    output_missing: usize,
}

pub(crate) fn build_preview_report(
    book: String,
    previews: &[ChapterPreview],
    summary: &PreviewSummary,
) -> PreviewReport {
    PreviewReport {
        book,
        dry_run: true,
        chapters: previews
            .iter()
            .map(|p| PreviewChapterEntry {
                chapter: p.chapter_path.clone(),
                action: preview_action_str(p.action),
                reason: p.reason.clone(),
                approximate: p.approximate,
            })
            .collect(),
        summary: PreviewSummaryEntry {
            translate: summary.translate,
            rerun: summary.rerun,
            skip: summary.skip,
            approximate_reruns: summary.approximate_reruns,
            exact_reruns: summary.exact_reruns,
            empty_skips: summary.empty_skips,
            output_exists_skips: summary.output_exists_skips,
            output_missing: summary.output_missing,
        },
        exit_code: 0,
    }
}

fn preview_action_str(action: PreviewAction) -> &'static str {
    match action {
        PreviewAction::Translate => "translate",
        PreviewAction::Rerun => "rerun",
        PreviewAction::Skip => "skip",
    }
}

fn run_meta(metadata: &RunMetadata) -> RunMeta {
    RunMeta {
        profile: metadata.profile.clone(),
        repair_profile: metadata.repair_profile.clone(),
        glossary_profile: metadata.glossary_profile.clone(),
        provider: metadata.provider.clone(),
        model: metadata.model.clone(),
        started_at: metadata.started_at.clone(),
        finished_at: metadata.finished_at.clone(),
    }
}

fn chapter_entry(result: &ChapterResult) -> ChapterEntry {
    ChapterEntry {
        chapter: result.chapter_state.chapter_path.clone(),
        status: result.chapter_state.status.clone(),
        time_ms: result.chapter_state.translation_time_ms,
        error: result.chapter_state.error.clone(),
        glossary_extraction_error: result.glossary_extraction_error.clone(),
        new_terms_added: result.new_terms_added,
        tokens: result.usage.clone(),
    }
}

pub(crate) fn print_run_report<T: Serialize>(report: &T) -> Result<(), Error> {
    println!("{}", serde_json::to_string_pretty(report)?);
    Ok(())
}

pub(crate) fn print_error_report(err: &Error) -> Result<(), Error> {
    println!(
        "{}",
        serde_json::to_string_pretty(&build_error_report(err))?
    );
    Ok(())
}

fn build_error_report(err: &Error) -> ErrorReport {
    ErrorReport {
        error: ErrorEntry {
            code: err.code(),
            message: err.to_string(),
            exit_code: err.exit_code(),
        },
    }
}

#[derive(Debug, Serialize)]
struct ErrorReport {
    error: ErrorEntry,
}

#[derive(Debug, Serialize)]
struct ErrorEntry {
    code: &'static str,
    message: String,
    exit_code: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ChapterState, ChapterStatus};
    use crate::translate::orchestrate::ChapterResult;

    fn usage(input: u64, output: u64) -> TranslationUsage {
        TranslationUsage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }
    }

    fn chapter_result(
        path: &str,
        status: ChapterStatus,
        error: Option<String>,
        time_ms: Option<u64>,
        usage: Option<TranslationUsage>,
        new_terms_added: usize,
    ) -> ChapterResult {
        ChapterResult {
            translated: status == ChapterStatus::Success,
            failed: status == ChapterStatus::Failed,
            skipped: status == ChapterStatus::Skipped,
            new_terms_added,
            usage: usage.clone(),
            chapter_state: ChapterState {
                chapter_path: path.to_string(),
                status,
                error,
                translation_time_ms: time_ms,
                last_attempted: None,
                translation_usage: usage.clone(),
                glossary_usage: None,
                exported_terms: vec![],
                source_text_hash: None,
            },
            glossary_extraction_error: None,
        }
    }

    fn run_metadata() -> RunMetadata {
        RunMetadata {
            version: 1,
            started_at: "2026-07-31T10:00:00Z".to_string(),
            finished_at: Some("2026-07-31T10:05:00Z".to_string()),
            updated_at: "2026-07-31T10:05:00Z".to_string(),
            profile: "default".to_string(),
            repair_profile: None,
            glossary_profile: Some("cheap".to_string()),
            provider: "gemini".to_string(),
            model: "gemini-2.5-flash".to_string(),
            options: None,
        }
    }

    fn sample_report_data() -> ReportData {
        ReportData {
            book: "/books/demo".to_string(),
            run: Some(run_metadata()),
            chapters: vec![
                chapter_result(
                    "raw/001.md",
                    ChapterStatus::Success,
                    None,
                    Some(120_000),
                    Some(usage(100, 200)),
                    2,
                ),
                chapter_result("raw/002.md", ChapterStatus::Skipped, None, None, None, 0),
                chapter_result(
                    "raw/003.md",
                    ChapterStatus::Failed,
                    Some("Validation failed: missing heading".to_string()),
                    Some(5_000),
                    Some(usage(50, 25)),
                    0,
                ),
            ],
            total: 3,
            translated: 1,
            skipped: 1,
            failed: 1,
            new_glossary_terms: 2,
            usage: usage(150, 225),
            cancelled: false,
            exit_code: 2,
        }
    }

    #[test]
    fn run_report_serializes_per_chapter_results() {
        let json = serde_json::to_value(build_run_report(&sample_report_data())).unwrap();

        let chapters = json["chapters"].as_array().unwrap();
        assert_eq!(chapters.len(), 3);

        assert_eq!(chapters[0]["chapter"], "raw/001.md");
        assert_eq!(chapters[0]["status"], "success");
        assert_eq!(chapters[0]["time_ms"], 120_000);
        assert_eq!(chapters[0]["new_terms_added"], 2);
        assert!(chapters[0]["error"].is_null());
        assert_eq!(chapters[0]["tokens"]["total_tokens"], 300);
        assert_eq!(chapters[0]["tokens"]["input_tokens"], 100);

        assert_eq!(chapters[1]["status"], "skipped");
        assert!(chapters[1]["time_ms"].is_null());
        assert!(chapters[1]["tokens"].is_null());

        assert_eq!(chapters[2]["status"], "failed");
        assert_eq!(chapters[2]["error"], "Validation failed: missing heading");
        assert_eq!(chapters[2]["tokens"]["output_tokens"], 25);
    }

    #[test]
    fn run_report_includes_run_metadata_and_summary() {
        let json = serde_json::to_value(build_run_report(&sample_report_data())).unwrap();

        assert_eq!(json["book"], "/books/demo");
        assert_eq!(json["run"]["profile"], "default");
        assert_eq!(json["run"]["provider"], "gemini");
        assert_eq!(json["run"]["model"], "gemini-2.5-flash");
        assert_eq!(json["run"]["glossary_profile"], "cheap");
        assert!(json["run"]["repair_profile"].is_null());
        assert_eq!(json["run"]["started_at"], "2026-07-31T10:00:00Z");
        assert_eq!(json["run"]["finished_at"], "2026-07-31T10:05:00Z");

        assert_eq!(json["summary"]["total"], 3);
        assert_eq!(json["summary"]["translated"], 1);
        assert_eq!(json["summary"]["skipped"], 1);
        assert_eq!(json["summary"]["failed"], 1);
        assert_eq!(json["summary"]["new_glossary_terms"], 2);

        assert_eq!(json["usage"]["total_tokens"], 375);
        assert_eq!(json["cancelled"], false);
        assert_eq!(json["exit_code"], 2);
    }

    #[test]
    fn run_report_without_run_metadata_omits_run_field() {
        let mut data = sample_report_data();
        data.run = None;
        data.chapters.clear();
        data.total = 0;

        let json = serde_json::to_value(build_run_report(&data)).unwrap();

        assert!(json.get("run").is_none());
        assert_eq!(json["chapters"].as_array().unwrap().len(), 0);
        assert_eq!(json["summary"]["total"], 0);
    }

    #[test]
    fn empty_report_data_produces_zeroed_report() {
        let json = serde_json::to_value(build_run_report(&ReportData::empty(
            "/books/demo".to_string(),
        )))
        .unwrap();

        assert_eq!(json["book"], "/books/demo");
        assert!(json.get("run").is_none());
        assert_eq!(json["chapters"].as_array().unwrap().len(), 0);
        assert_eq!(json["summary"]["total"], 0);
        assert_eq!(json["usage"]["total_tokens"], 0);
        assert_eq!(json["cancelled"], false);
        assert_eq!(json["exit_code"], 0);
    }

    #[test]
    fn error_report_carries_code_message_and_exit_code() {
        let err = Error::Provider {
            kind: "gemini".to_string(),
            detail: "timeout".to_string(),
        };
        let json = serde_json::to_value(build_error_report(&err)).unwrap();

        assert_eq!(json["error"]["code"], "E005");
        assert_eq!(json["error"]["message"], "gemini request failed: timeout");
        assert_eq!(json["error"]["exit_code"], 4);
    }

    #[test]
    fn validation_error_report_keeps_bare_message() {
        let err = Error::Validation {
            message: "No profile configured. Run 'cipher profile new' to create one.".to_string(),
        };
        let json = serde_json::to_value(build_error_report(&err)).unwrap();

        assert_eq!(json["error"]["code"], "E006");
        assert_eq!(
            json["error"]["message"],
            "No profile configured. Run 'cipher profile new' to create one."
        );
        assert_eq!(json["error"]["exit_code"], 1);
    }

    #[test]
    fn preview_report_serializes_planned_actions() {
        let previews = vec![
            ChapterPreview {
                chapter_path: "raw/001.md".to_string(),
                action: PreviewAction::Translate,
                reason: "No output exists yet".to_string(),
                approximate: false,
            },
            ChapterPreview {
                chapter_path: "raw/002.md".to_string(),
                action: PreviewAction::Rerun,
                reason: "Chapter source changed".to_string(),
                approximate: true,
            },
            ChapterPreview {
                chapter_path: "raw/003.md".to_string(),
                action: PreviewAction::Skip,
                reason: "Chapter is empty".to_string(),
                approximate: false,
            },
        ];
        let summary = PreviewSummary {
            translate: 1,
            rerun: 1,
            skip: 1,
            approximate_reruns: 1,
            exact_reruns: 0,
            empty_skips: 1,
            output_exists_skips: 0,
            output_missing: 1,
        };

        let json = serde_json::to_value(build_preview_report(
            "/books/demo".to_string(),
            &previews,
            &summary,
        ))
        .unwrap();

        assert_eq!(json["book"], "/books/demo");
        assert_eq!(json["dry_run"], true);
        assert_eq!(json["chapters"].as_array().map(Vec::len), Some(3));
        assert_eq!(json["chapters"][0]["chapter"], "raw/001.md");
        assert_eq!(json["chapters"][0]["action"], "translate");
        assert_eq!(json["chapters"][1]["action"], "rerun");
        assert_eq!(json["chapters"][1]["approximate"], true);
        assert_eq!(json["chapters"][2]["action"], "skip");
        assert_eq!(json["summary"]["translate"], 1);
        assert_eq!(json["summary"]["approximate_reruns"], 1);
        assert_eq!(json["exit_code"], 0);
    }
}
