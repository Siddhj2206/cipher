use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::book::paths::BookPaths;

#[derive(Debug, Clone)]
pub struct InitReport {
    pub book_dir: std::path::PathBuf,
    pub created_dirs: Vec<String>,
    pub created_files: Vec<String>,
    pub skipped_files: Vec<String>,
    pub imported_glossary: Option<std::path::PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BookConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub raw_dir: String,
    pub out_dir: String,
    pub glossary_path: String,
    pub style_path: String,
    #[serde(default = "default_glossary_injection")]
    pub glossary_injection: String,
}

impl Default for BookConfig {
    fn default() -> Self {
        Self {
            profile: Some("default".to_string()),
            raw_dir: "raw".to_string(),
            out_dir: "tl".to_string(),
            glossary_path: "glossary.json".to_string(),
            style_path: "style.md".to_string(),
            glossary_injection: default_glossary_injection(),
        }
    }
}

fn default_glossary_injection() -> String {
    "smart".to_string()
}

impl BookConfig {
    pub fn with_profile(profile: impl Into<String>) -> Self {
        let mut config = Self::default();
        config.profile = Some(profile.into());
        config
    }
}

pub fn init_book(
    book_dir: impl AsRef<Path>,
    profile: Option<&str>,
    from: Option<&Path>,
    import_glossary: Option<&Path>,
) -> anyhow::Result<InitReport> {
    let book_dir = book_dir.as_ref();
    let paths = BookPaths::resolve(book_dir);

    let mut report = InitReport {
        book_dir: book_dir.to_path_buf(),
        created_dirs: Vec::new(),
        created_files: Vec::new(),
        skipped_files: Vec::new(),
        imported_glossary: None,
    };

    // Create directories
    create_dir_if_missing(&paths.raw_dir, &mut report)?;
    create_dir_if_missing(&paths.out_dir, &mut report)?;
    create_dir_if_missing(&paths.state_dir, &mut report)?;

    // Create config.json
    let config = if let Some(p) = profile {
        BookConfig::with_profile(p)
    } else {
        BookConfig::default()
    };

    write_json_if_missing(&paths.config_json, &config, &mut report)?;

    // Create glossary.json
    if import_glossary.is_some() {
        // Copy from provided path
        let src = import_glossary.unwrap();
        copy_file_if_missing(src, &paths.glossary_json, &mut report)?;
        report.imported_glossary = Some(src.to_path_buf());
    } else if from.is_some() {
        // Try to copy from existing book
        let from_dir = from.unwrap();
        let from_glossary = from_dir.join("glossary.json");
        if from_glossary.exists() {
            copy_file_if_missing(&from_glossary, &paths.glossary_json, &mut report)?;
            report.imported_glossary = Some(from_glossary);
        } else {
            write_json_if_missing(
                &paths.glossary_json,
                &Vec::<serde_json::Value>::new(),
                &mut report,
            )?;
        }
    } else {
        // Create empty glossary
        write_json_if_missing(
            &paths.glossary_json,
            &Vec::<serde_json::Value>::new(),
            &mut report,
        )?;
    }

    // Create style.md
    write_file_if_missing(&paths.style_md, STYLE_TEMPLATE, &mut report)?;

    Ok(report)
}

fn create_dir_if_missing(path: &Path, report: &mut InitReport) -> anyhow::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            report.created_dirs.push(name.to_string());
        }
    }
    Ok(())
}

fn write_json_if_missing<T: Serialize>(
    path: &Path,
    value: &T,
    report: &mut InitReport,
) -> anyhow::Result<()> {
    if !path.exists() {
        let json = serde_json::to_string_pretty(value)?;
        let mut file = File::create_new(path)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            report.created_files.push(name.to_string());
        }
    } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        report.skipped_files.push(name.to_string());
    }
    Ok(())
}

fn write_file_if_missing(
    path: &Path,
    content: &str,
    report: &mut InitReport,
) -> anyhow::Result<()> {
    if !path.exists() {
        let mut file = File::create_new(path)?;
        file.write_all(content.as_bytes())?;
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            report.created_files.push(name.to_string());
        }
    } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        report.skipped_files.push(name.to_string());
    }
    Ok(())
}

fn copy_file_if_missing(src: &Path, dst: &Path, report: &mut InitReport) -> anyhow::Result<()> {
    if !dst.exists() {
        fs::copy(src, dst)?;
        if let Some(name) = dst.file_name().and_then(|n| n.to_str()) {
            report.created_files.push(name.to_string());
        }
    } else if let Some(name) = dst.file_name().and_then(|n| n.to_str()) {
        report.skipped_files.push(name.to_string());
    }
    Ok(())
}

const STYLE_TEMPLATE: &str = r#"# Style Guide

## Translation Guidelines
- Maintain the tone and atmosphere of the original text
- Keep dialogue natural and character-appropriate
- Preserve pacing and narrative flow
- Respect cultural context while ensuring readability
- Avoid transliteration unless specifically appropriate

## Formatting
- Use proper markdown
- Preserve paragraph structure
- Maintain dialogue formatting
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_book_config_default() {
        let config = BookConfig::default();
        assert_eq!(config.raw_dir, "raw");
        assert_eq!(config.out_dir, "tl");
        assert_eq!(config.glossary_path, "glossary.json");
        assert_eq!(config.style_path, "style.md");
        assert_eq!(config.glossary_injection, "smart");
        assert_eq!(config.profile, Some("default".to_string()));
    }

    #[test]
    fn test_book_config_with_profile() {
        let config = BookConfig::with_profile("default");
        assert_eq!(config.profile, Some("default".to_string()));
    }
}

pub fn load_book_config(path: &Path) -> anyhow::Result<BookConfig> {
    if !path.exists() {
        return Ok(BookConfig::default());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read book config from {}", path.display()))?;
    let config: BookConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse book config from {}", path.display()))?;
    Ok(config)
}
