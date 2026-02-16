use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunState {
    pub version: u32,
    pub run_date: String,
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub chapters: BTreeMap<String, ChapterState>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChapterState {
    pub status: ChapterStatus,
    pub error: Option<String>,
    pub translation_time_ms: Option<u64>,
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
    pub fn new(profile: String, provider: String, model: String) -> Self {
        Self {
            version: 1,
            run_date: chrono::Local::now().to_rfc3339(),
            profile,
            provider,
            model,
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

    pub fn set_chapter(&mut self, filename: &str, status: ChapterStatus, error: Option<String>) {
        self.chapters.insert(
            filename.to_string(),
            ChapterState {
                status,
                error,
                translation_time_ms: None,
            },
        );
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
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pending: usize,
}
