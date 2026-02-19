pub mod status;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunState {
    pub version: u32,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub profile: String,
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<RunOptions>,
    pub chapters: BTreeMap<String, ChapterState>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunOptions {
    pub overwrite: bool,
    pub overwrite_bad: bool,
    pub backup: bool,
    pub fail_fast: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChapterState {
    pub status: ChapterStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempted: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChapterStatus {
    Pending,
    Success,
    Failed,
    Skipped,
}

impl RunState {
    pub fn new(
        profile: String,
        provider: String,
        model: String,
        options: Option<RunOptions>,
    ) -> Self {
        Self {
            version: 1,
            started_at: chrono::Local::now().to_rfc3339(),
            finished_at: None,
            profile,
            provider,
            model,
            options,
            chapters: BTreeMap::new(),
        }
    }

    pub fn state_path(book_dir: &Path) -> PathBuf {
        book_dir.join(".cipher").join("run_state.json")
    }

    pub fn load(book_dir: &Path) -> Result<Option<Self>> {
        let path = Self::state_path(book_dir);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let state: RunState = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    pub fn save(&self, book_dir: &Path) -> Result<()> {
        let path = Self::state_path(book_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn set_chapter(
        &mut self,
        filename: &str,
        status: ChapterStatus,
        error: Option<String>,
        duration_ms: Option<u64>,
    ) {
        let now = chrono::Local::now().to_rfc3339();
        self.chapters.insert(
            filename.to_string(),
            ChapterState {
                status,
                error,
                translation_time_ms: duration_ms,
                last_attempted: Some(now),
            },
        );
    }

    /// Merge a previous run state, carrying forward chapters that weren't processed
    pub fn merge_previous(&mut self, previous: Option<RunState>) {
        if let Some(prev) = previous {
            // Carry forward any chapters not in current run
            for (filename, state) in prev.chapters {
                self.chapters.entry(filename).or_insert(state);
            }
        }
    }

    pub fn mark_finished(&mut self) {
        self.finished_at = Some(chrono::Local::now().to_rfc3339());
    }

    pub fn get_summary(&self) -> RunSummary {
        let mut success = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut pending = 0;

        for state in self.chapters.values() {
            match state.status {
                ChapterStatus::Success => success += 1,
                ChapterStatus::Failed => failed += 1,
                ChapterStatus::Skipped => skipped += 1,
                ChapterStatus::Pending => pending += 1,
            }
        }

        RunSummary {
            total: self.chapters.len(),
            success,
            failed,
            skipped,
            pending,
        }
    }

    pub fn get_failed_chapters(&self) -> Vec<(&String, &ChapterState)> {
        self.chapters
            .iter()
            .filter(|(_, state)| state.status == ChapterStatus::Failed)
            .collect()
    }

    pub fn format_duration(&self) -> String {
        match (&self.started_at, &self.finished_at) {
            (start, Some(end)) => {
                if let (Ok(start_dt), Ok(end_dt)) = (
                    chrono::DateTime::parse_from_rfc3339(start),
                    chrono::DateTime::parse_from_rfc3339(end),
                ) {
                    let duration = end_dt.signed_duration_since(start_dt);
                    if duration.num_hours() > 0 {
                        format!("{}h {}m", duration.num_hours(), duration.num_minutes() % 60)
                    } else if duration.num_minutes() > 0 {
                        format!(
                            "{}m {}s",
                            duration.num_minutes(),
                            duration.num_seconds() % 60
                        )
                    } else {
                        format!("{}s", duration.num_seconds())
                    }
                } else {
                    "unknown".to_string()
                }
            }
            _ => "in progress".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pending: usize,
}
