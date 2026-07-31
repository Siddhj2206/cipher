use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

pub(crate) fn create_backup(book_dir: &Path, path: &Path) -> Result<PathBuf> {
    use chrono::Local;

    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let filename = path
        .file_stem()
        .ok_or_else(|| Error::Validation {
            message: "Cannot determine file stem for backup".to_string(),
        })?
        .to_string_lossy();
    let backup_name = format!("{}_{}.md", filename, timestamp);

    let backup_dir = book_dir.join(".cipher").join("backups");
    std::fs::create_dir_all(&backup_dir)?;

    let backup_path = backup_dir.join(&backup_name);
    std::fs::copy(path, &backup_path)?;
    Ok(backup_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_backup() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("chapter01.md");
        std::fs::write(&source, "# Chapter 1\n\nContent here.").unwrap();

        let backup_path = create_backup(dir.path(), &source).unwrap();
        assert!(backup_path.exists());
        assert!(backup_path.to_str().unwrap().contains("chapter01_"));
        assert!(backup_path.to_str().unwrap().ends_with(".md"));

        let content = std::fs::read_to_string(&backup_path).unwrap();
        assert_eq!(content, "# Chapter 1\n\nContent here.");
    }

    #[test]
    fn test_create_backup_creates_backup_dir() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("test.md");
        std::fs::write(&source, "content").unwrap();

        let backup_dir = dir.path().join(".cipher").join("backups");
        assert!(!backup_dir.exists());

        create_backup(dir.path(), &source).unwrap();
        assert!(backup_dir.exists());
    }
}
