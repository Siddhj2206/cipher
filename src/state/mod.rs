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

        // Atomic write: write to temp file then rename
        let temp_path = path.with_extension("json.tmp");
        std::fs::write(&temp_path, &content)?;
        if let Err(e) = std::fs::rename(&temp_path, &path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(e.into());
        }
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
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pending: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> RunState {
        RunState::new("test-profile".into(), "openai".into(), "gpt-4".into(), None)
    }

    #[test]
    fn test_get_summary_empty_state() {
        let state = make_state();
        let summary = state.get_summary();
        assert_eq!(summary.total, 0);
        assert_eq!(summary.success, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.pending, 0);
    }

    #[test]
    fn test_set_chapter_and_summary() {
        let mut state = make_state();
        state.set_chapter("ch01.md", ChapterStatus::Success, None, Some(1500));
        state.set_chapter(
            "ch02.md",
            ChapterStatus::Failed,
            Some("timeout".into()),
            None,
        );
        state.set_chapter("ch03.md", ChapterStatus::Skipped, None, None);
        state.set_chapter("ch04.md", ChapterStatus::Pending, None, None);

        let summary = state.get_summary();
        assert_eq!(summary.total, 4);
        assert_eq!(summary.success, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.pending, 1);
    }

    #[test]
    fn test_get_failed_chapters() {
        let mut state = make_state();
        state.set_chapter("ch01.md", ChapterStatus::Success, None, None);
        state.set_chapter(
            "ch02.md",
            ChapterStatus::Failed,
            Some("API error".into()),
            None,
        );
        state.set_chapter(
            "ch03.md",
            ChapterStatus::Failed,
            Some("timeout".into()),
            None,
        );

        let failed = state.get_failed_chapters();
        assert_eq!(failed.len(), 2);
        assert!(failed.iter().any(|(name, _)| name.as_str() == "ch02.md"));
        assert!(failed.iter().any(|(name, _)| name.as_str() == "ch03.md"));
    }

    #[test]
    fn test_merge_previous_fills_gaps() {
        let mut prev = make_state();
        prev.set_chapter("ch01.md", ChapterStatus::Success, None, Some(1000));
        prev.set_chapter("ch02.md", ChapterStatus::Success, None, Some(2000));

        let mut current = make_state();
        current.set_chapter(
            "ch02.md",
            ChapterStatus::Failed,
            Some("new error".into()),
            None,
        );

        current.merge_previous(Some(prev));

        // ch01 should be inherited from previous
        assert_eq!(current.chapters["ch01.md"].status, ChapterStatus::Success);
        // ch02 should keep the current (not overwritten by previous)
        assert_eq!(current.chapters["ch02.md"].status, ChapterStatus::Failed);
    }

    #[test]
    fn test_merge_previous_none_is_noop() {
        let mut state = make_state();
        state.set_chapter("ch01.md", ChapterStatus::Success, None, None);
        state.merge_previous(None);
        assert_eq!(state.chapters.len(), 1);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = make_state();
        state.set_chapter("ch01.md", ChapterStatus::Success, None, Some(1234));
        state.set_chapter("ch02.md", ChapterStatus::Failed, Some("err".into()), None);
        state.mark_finished();

        state.save(dir.path()).unwrap();

        let loaded = RunState::load(dir.path()).unwrap().expect("should load");
        assert_eq!(loaded.profile, "test-profile");
        assert_eq!(loaded.provider, "openai");
        assert_eq!(loaded.model, "gpt-4");
        assert!(loaded.finished_at.is_some());
        assert_eq!(loaded.chapters.len(), 2);
        assert_eq!(loaded.chapters["ch01.md"].status, ChapterStatus::Success);
        assert_eq!(loaded.chapters["ch01.md"].translation_time_ms, Some(1234));
        assert_eq!(loaded.chapters["ch02.md"].error.as_deref(), Some("err"));
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = RunState::load(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_atomic_write_no_temp_file_left() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state();
        state.save(dir.path()).unwrap();

        let temp_path = RunState::state_path(dir.path()).with_extension("json.tmp");
        assert!(!temp_path.exists(), "temp file should be cleaned up");
    }

    #[test]
    fn test_set_chapter_overwrites_previous() {
        let mut state = make_state();
        state.set_chapter("ch01.md", ChapterStatus::Failed, Some("err".into()), None);
        state.set_chapter("ch01.md", ChapterStatus::Success, None, Some(500));

        assert_eq!(state.chapters["ch01.md"].status, ChapterStatus::Success);
        assert!(state.chapters["ch01.md"].error.is_none());
        assert_eq!(state.chapters.len(), 1);
    }
}
