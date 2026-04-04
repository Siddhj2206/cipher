pub mod status;

use crate::book::paths::BookPaths;
use crate::translate::TranslationUsage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

const RUN_METADATA_VERSION: u32 = 1;
const GLOSSARY_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunMetadata {
    pub version: u32,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    pub updated_at: String,
    pub profile: String,
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<RunOptions>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunOptions {
    pub overwrite: bool,
    pub fail_fast: bool,
    #[serde(default)]
    pub rerun: bool,
    #[serde(default)]
    pub rerun_affected_glossary: bool,
    #[serde(default)]
    pub rerun_affected_chapters: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlossaryState {
    pub version: u32,
    pub updated_at: String,
    pub injection_mode: GlossaryInjectionMode,
    #[serde(default)]
    pub terms: BTreeMap<String, GlossaryStateTerm>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlossaryStateTerm {
    pub term: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub og_term: Option<String>,
    pub definition: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChapterState {
    pub chapter_path: String,
    pub status: ChapterStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_usage: Option<TranslationUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glossary_usage: Option<ChapterGlossaryUsage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exported_terms: Vec<ChapterGlossaryTerm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChapterGlossaryUsage {
    pub injection_mode: GlossaryInjectionMode,
    pub used_fallback_to_full: bool,
    #[serde(default)]
    pub terms: Vec<ChapterGlossaryTerm>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChapterGlossaryTerm {
    pub key: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GlossaryInjectionMode {
    Full,
    Smart,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChapterStatus {
    Pending,
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pending: usize,
}

impl RunMetadata {
    pub fn new(
        profile: String,
        provider: String,
        model: String,
        options: Option<RunOptions>,
    ) -> Self {
        let now = now_rfc3339();

        Self {
            version: RUN_METADATA_VERSION,
            started_at: now.clone(),
            finished_at: None,
            updated_at: now,
            profile,
            provider,
            model,
            options,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }

    pub fn mark_finished(&mut self) {
        let now = now_rfc3339();
        self.updated_at = now.clone();
        self.finished_at = Some(now);
    }
}

impl GlossaryState {
    pub fn new(
        injection_mode: GlossaryInjectionMode,
        terms: BTreeMap<String, GlossaryStateTerm>,
    ) -> Self {
        Self {
            version: GLOSSARY_STATE_VERSION,
            updated_at: now_rfc3339(),
            injection_mode,
            terms,
        }
    }
}

impl ChapterState {
    pub fn new(
        chapter_path: String,
        status: ChapterStatus,
        error: Option<String>,
        translation_time_ms: Option<u64>,
        translation_usage: Option<TranslationUsage>,
        glossary_usage: Option<ChapterGlossaryUsage>,
        exported_terms: Vec<ChapterGlossaryTerm>,
        source_text_hash: Option<String>,
    ) -> Self {
        Self {
            chapter_path,
            status,
            error,
            translation_time_ms,
            last_attempted: Some(now_rfc3339()),
            translation_usage,
            glossary_usage,
            exported_terms,
            source_text_hash,
        }
    }
}

pub fn normalized_source_text_hash(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = normalized.trim_end_matches('\n');
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

pub fn load_run_metadata(book_dir: &Path) -> Result<Option<RunMetadata>> {
    let paths = BookPaths::resolve(book_dir);
    load_json_if_exists(&paths.run_json())
}

pub fn save_run_metadata(book_dir: &Path, metadata: &RunMetadata) -> Result<()> {
    let paths = BookPaths::resolve(book_dir);
    save_json(&paths.run_json(), metadata)
}

pub fn load_glossary_state(book_dir: &Path) -> Result<Option<GlossaryState>> {
    let paths = BookPaths::resolve(book_dir);
    load_json_if_exists(&paths.glossary_state_json())
}

pub fn save_glossary_state(book_dir: &Path, glossary_state: &GlossaryState) -> Result<()> {
    let paths = BookPaths::resolve(book_dir);
    save_json(&paths.glossary_state_json(), glossary_state)
}

#[cfg(test)]
pub fn load_chapter_state(book_dir: &Path, chapter_path: &str) -> Result<Option<ChapterState>> {
    let paths = BookPaths::resolve(book_dir);
    load_json_if_exists(&paths.chapter_state_json(Path::new(chapter_path)))
}

pub fn save_chapter_state(book_dir: &Path, chapter_state: &ChapterState) -> Result<()> {
    let paths = BookPaths::resolve(book_dir);
    save_json(
        &paths.chapter_state_json(Path::new(&chapter_state.chapter_path)),
        chapter_state,
    )
}

pub fn load_all_chapter_states(book_dir: &Path) -> Result<BTreeMap<String, ChapterState>> {
    let paths = BookPaths::resolve(book_dir);
    let mut chapter_files = Vec::new();
    collect_chapter_state_files(&paths.chapters_dir(), &mut chapter_files)?;

    let mut states = BTreeMap::new();
    for chapter_file in chapter_files {
        let state: ChapterState = load_json(&chapter_file)?;
        states.insert(state.chapter_path.clone(), state);
    }

    Ok(states)
}

pub fn summarize_chapters(chapters: &BTreeMap<String, ChapterState>) -> RunSummary {
    let mut success = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut pending = 0;

    for state in chapters.values() {
        match state.status {
            ChapterStatus::Success => success += 1,
            ChapterStatus::Failed => failed += 1,
            ChapterStatus::Skipped => skipped += 1,
            ChapterStatus::Pending => pending += 1,
        }
    }

    RunSummary {
        total: chapters.len(),
        success,
        failed,
        skipped,
        pending,
    }
}

pub fn failed_chapters(chapters: &BTreeMap<String, ChapterState>) -> Vec<(&String, &ChapterState)> {
    chapters
        .iter()
        .filter(|(_, state)| state.status == ChapterStatus::Failed)
        .collect()
}

pub fn normalize_chapter_path(path: &Path) -> String {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    if components.is_empty() {
        path.to_string_lossy().replace('\\', "/")
    } else {
        components.join("/")
    }
}

fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339()
}

fn collect_chapter_state_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read chapter state dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_chapter_state_files(&path, files)?;
            continue;
        }

        if path.extension().map(|ext| ext == "json").unwrap_or(false) {
            files.push(path);
        }
    }

    files.sort();
    Ok(())
}

fn load_json_if_exists<T>(path: &Path) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(None);
    }

    Ok(Some(load_json(path)?))
}

fn load_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(value)
}

fn save_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let content = serde_json::to_string_pretty(value)?;
    let temp_path = path.with_extension("json.tmp");
    std::fs::write(&temp_path, &content)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;

    if let Err(error) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("Failed to rename {}", path.display()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_glossary_usage() -> ChapterGlossaryUsage {
        ChapterGlossaryUsage {
            injection_mode: GlossaryInjectionMode::Smart,
            used_fallback_to_full: false,
            terms: vec![ChapterGlossaryTerm {
                key: "hero".into(),
                fingerprint: "fp-1".into(),
            }],
        }
    }

    fn sample_chapter_state(chapter_path: &str, status: ChapterStatus) -> ChapterState {
        ChapterState::new(
            chapter_path.to_string(),
            status,
            None,
            Some(1234),
            None,
            Some(sample_glossary_usage()),
            vec![],
            Some("hash-1".into()),
        )
    }

    #[test]
    fn test_run_metadata_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut metadata = RunMetadata::new(
            "test-profile".into(),
            "openai".into(),
            "gpt-4".into(),
            Some(RunOptions {
                overwrite: true,
                fail_fast: false,
                rerun: false,
                rerun_affected_glossary: false,
                rerun_affected_chapters: false,
            }),
        );
        metadata.mark_finished();

        save_run_metadata(dir.path(), &metadata).unwrap();

        let loaded = load_run_metadata(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.profile, "test-profile");
        assert_eq!(loaded.provider, "openai");
        assert_eq!(loaded.model, "gpt-4");
        assert!(loaded.finished_at.is_some());
        assert!(loaded.updated_at >= loaded.started_at);
    }

    #[test]
    fn test_glossary_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let glossary_state = GlossaryState::new(
            GlossaryInjectionMode::Smart,
            BTreeMap::from([(
                "hero".into(),
                GlossaryStateTerm {
                    term: "Hero".into(),
                    og_term: Some("勇者".into()),
                    definition: "Main character".into(),
                    fingerprint: "fp-1".into(),
                },
            )]),
        );

        save_glossary_state(dir.path(), &glossary_state).unwrap();

        let loaded = load_glossary_state(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.injection_mode, GlossaryInjectionMode::Smart);
        assert_eq!(loaded.terms.len(), 1);
        assert_eq!(loaded.terms["hero"].definition, "Main character");
    }

    #[test]
    fn test_chapter_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut chapter_state = sample_chapter_state("part1/ch01.md", ChapterStatus::Success);
        chapter_state.translation_usage = Some(TranslationUsage {
            input_tokens: 100,
            output_tokens: 200,
            total_tokens: 300,
            cached_input_tokens: 25,
            cache_creation_input_tokens: 10,
        });

        save_chapter_state(dir.path(), &chapter_state).unwrap();

        let loaded = load_chapter_state(dir.path(), "part1/ch01.md")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.chapter_path, "part1/ch01.md");
        assert_eq!(loaded.status, ChapterStatus::Success);
        assert!(loaded.glossary_usage.is_some());
        assert_eq!(loaded.translation_usage, chapter_state.translation_usage);
        assert_eq!(loaded.source_text_hash, chapter_state.source_text_hash);
    }

    #[test]
    fn test_load_all_chapter_states() {
        let dir = tempfile::tempdir().unwrap();
        let chapter_a = sample_chapter_state("part1/ch01.md", ChapterStatus::Success);
        let chapter_b = sample_chapter_state("part2/ch02.md", ChapterStatus::Failed);

        save_chapter_state(dir.path(), &chapter_a).unwrap();
        save_chapter_state(dir.path(), &chapter_b).unwrap();

        let loaded = load_all_chapter_states(dir.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["part1/ch01.md"].status, ChapterStatus::Success);
        assert_eq!(loaded["part2/ch02.md"].status, ChapterStatus::Failed);
    }

    #[test]
    fn test_summarize_chapters() {
        let chapters = BTreeMap::from([
            (
                "ch01.md".into(),
                sample_chapter_state("ch01.md", ChapterStatus::Success),
            ),
            (
                "ch02.md".into(),
                sample_chapter_state("ch02.md", ChapterStatus::Failed),
            ),
            (
                "ch03.md".into(),
                sample_chapter_state("ch03.md", ChapterStatus::Skipped),
            ),
            (
                "ch04.md".into(),
                sample_chapter_state("ch04.md", ChapterStatus::Pending),
            ),
        ]);

        let summary = summarize_chapters(&chapters);
        assert_eq!(summary.total, 4);
        assert_eq!(summary.success, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.pending, 1);
    }

    #[test]
    fn test_failed_chapters() {
        let chapters = BTreeMap::from([
            (
                "ch01.md".into(),
                sample_chapter_state("ch01.md", ChapterStatus::Success),
            ),
            (
                "ch02.md".into(),
                sample_chapter_state("ch02.md", ChapterStatus::Failed),
            ),
        ]);

        let failed = failed_chapters(&chapters);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0.as_str(), "ch02.md");
    }

    #[test]
    fn test_normalize_chapter_path() {
        assert_eq!(
            normalize_chapter_path(Path::new("part1/ch01.md")),
            "part1/ch01.md"
        );
        assert_eq!(normalize_chapter_path(Path::new("./ch01.md")), "ch01.md");
    }

    #[test]
    fn test_temp_file_cleaned_up_after_save() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = RunMetadata::new("p".into(), "provider".into(), "model".into(), None);
        let path = BookPaths::resolve(dir.path()).run_json();

        save_run_metadata(dir.path(), &metadata).unwrap();

        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn test_normalized_source_text_hash_normalizes_line_endings() {
        let lf = "# Chapter 1\n\nLine 1\nLine 2\n";
        let crlf = "# Chapter 1\r\n\r\nLine 1\r\nLine 2\r\n";

        assert_eq!(
            normalized_source_text_hash(lf),
            normalized_source_text_hash(crlf)
        );
    }

    #[test]
    fn test_normalized_source_text_hash_ignores_trailing_newlines_only() {
        let without_trailing_newline = "# Chapter 1\n\nLine 1";
        let with_trailing_newlines = "# Chapter 1\n\nLine 1\n\n";

        assert_eq!(
            normalized_source_text_hash(without_trailing_newline),
            normalized_source_text_hash(with_trailing_newlines)
        );
    }
}
