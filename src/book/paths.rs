use crate::error::{Error, Result};
use crate::state::normalize_chapter_path;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BookPaths {
    pub root: PathBuf,
    pub config_toml: PathBuf,
    pub raw_dir: PathBuf,
    pub out_dir: PathBuf,
    pub legacy_out_dir: PathBuf,
    pub glossary_json: PathBuf,
    pub style_md: PathBuf,
    pub state_dir: PathBuf,
}

impl BookPaths {
    pub fn resolve(book_dir: impl Into<PathBuf>) -> Self {
        let root = book_dir.into();

        Self {
            config_toml: root.join("cipher.toml"),
            raw_dir: root.join("raw"),
            out_dir: root.join("tl"),
            legacy_out_dir: root.join("translated"),
            glossary_json: root.join("glossary.json"),
            style_md: root.join("style.md"),
            state_dir: root.join(".cipher"),
            root,
        }
    }

    pub fn effective_out_dir(&self) -> &Path {
        if self.out_dir.exists() {
            &self.out_dir
        } else if self.legacy_out_dir.exists() {
            &self.legacy_out_dir
        } else {
            &self.out_dir
        }
    }

    pub fn is_using_legacy_out(&self) -> bool {
        !self.out_dir.exists() && self.legacy_out_dir.exists()
    }

    pub fn run_json(&self) -> PathBuf {
        self.state_dir.join("run.json")
    }

    pub fn glossary_state_json(&self) -> PathBuf {
        self.state_dir.join("glossary_state.json")
    }

    pub fn chapters_dir(&self) -> PathBuf {
        self.state_dir.join("chapters")
    }

    pub fn chapter_state_json(&self, chapter_path: &Path) -> PathBuf {
        let mut path = self.chapters_dir();

        if let Some(parent) = chapter_path.parent() {
            path = path.join(parent);
        }

        let filename = chapter_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "chapter".to_string());

        path.join(format!("{}.json", filename))
    }
}

#[derive(Debug, Clone)]
pub struct BookExists {
    pub root_dir: bool,
    pub config_toml: bool,
    pub raw_dir: bool,
    pub out_dir: bool,
    pub legacy_out_dir: bool,
    pub glossary_json: bool,
    pub style_md: bool,
    pub state_dir: bool,
}

impl BookExists {
    pub fn probe(paths: &BookPaths) -> Self {
        Self {
            root_dir: paths.root.is_dir(),
            config_toml: paths.config_toml.is_file(),
            raw_dir: paths.raw_dir.is_dir(),
            out_dir: paths.out_dir.is_dir(),
            legacy_out_dir: paths.legacy_out_dir.is_dir(),
            glossary_json: paths.glossary_json.is_file(),
            style_md: paths.style_md.is_file(),
            state_dir: paths.state_dir.is_dir(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BookLayout {
    pub paths: BookPaths,
    pub exists: BookExists,
}

impl BookLayout {
    pub fn discover(book_dir: impl Into<PathBuf>) -> Self {
        let paths = BookPaths::resolve(book_dir);
        let exists = BookExists::probe(&paths);

        Self { paths, exists }
    }

    pub fn is_valid_book(&self) -> bool {
        self.exists.root_dir && self.exists.raw_dir
    }

    pub fn effective_out_dir(&self) -> &Path {
        self.paths.effective_out_dir()
    }

    pub fn is_using_legacy_out(&self) -> bool {
        self.paths.is_using_legacy_out()
    }
}

pub fn chapter_state_key(raw_dir: &Path, chapter_file: &Path) -> Result<String> {
    let relative_path = chapter_file
        .strip_prefix(raw_dir)
        .map_err(|_| Error::State(format!("Failed to relativize {}", chapter_file.display())))?;
    Ok(normalize_chapter_path(relative_path))
}

pub fn chapter_output_path(out_dir: &Path, chapter_file: &Path) -> Result<PathBuf> {
    let filename = chapter_file.file_name().ok_or_else(|| Error::Validation {
        message: "Invalid chapter filename".to_string(),
    })?;
    Ok(out_dir.join(filename))
}

pub(crate) fn discover_chapters(raw_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut chapters = Vec::new();

    if !raw_dir.exists() {
        return Ok(chapters);
    }

    for entry in std::fs::read_dir(raw_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e == "md").unwrap_or(false) {
            chapters.push(path);
        }
    }

    chapters.sort_by(|a, b| {
        let a_name = a
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let b_name = b
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();

        let a_num = extract_number(&a_name);
        let b_num = extract_number(&b_name);

        match (a_num, b_num) {
            (Some(n1), Some(n2)) => n1.cmp(&n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a_name.cmp(&b_name),
        }
    });

    Ok(chapters)
}

pub(crate) fn extract_number(filename: &str) -> Option<u32> {
    let digits: String = filename
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();

    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_number() {
        assert_eq!(extract_number("chapter01"), Some(1));
        assert_eq!(extract_number("chapter1"), Some(1));
        assert_eq!(extract_number("chapter10"), Some(10));
        assert_eq!(extract_number("01-chapter"), Some(1));
        assert_eq!(extract_number("no-number"), None);
        assert_eq!(extract_number(""), None);
    }

    #[test]
    fn chapter_state_key_returns_normalized_relative_path() {
        let raw = Path::new("/book/raw");
        let file = Path::new("/book/raw/01-intro.md");
        let key = chapter_state_key(raw, file).unwrap();
        assert_eq!(key, "01-intro.md");
    }

    #[test]
    fn chapter_output_path_joins_filename_with_out_dir() {
        let out = Path::new("/book/tl");
        let file = Path::new("/book/raw/01-intro.md");
        let path = chapter_output_path(out, file).unwrap();
        assert_eq!(path, Path::new("/book/tl/01-intro.md"));
    }

    #[test]
    fn test_extract_number_multiple_groups() {
        assert_eq!(extract_number("ch3_part2"), Some(3));
        assert_eq!(extract_number("v2_chapter10"), Some(2));
    }

    #[test]
    fn test_discover_chapters_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = discover_chapters(dir.path()).unwrap();
        assert!(chapters.is_empty());
    }

    #[test]
    fn test_discover_chapters_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let non_existent = dir.path().join("does_not_exist");
        let chapters = discover_chapters(&non_existent).unwrap();
        assert!(chapters.is_empty());
    }

    #[test]
    fn test_discover_chapters_filters_non_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("chapter01.md"), "# Ch 1").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "notes").unwrap();
        std::fs::write(dir.path().join("image.png"), "binary").unwrap();

        let chapters = discover_chapters(dir.path()).unwrap();
        assert_eq!(chapters.len(), 1);
        assert!(chapters[0].file_name().unwrap().to_str().unwrap() == "chapter01.md");
    }

    #[test]
    fn test_discover_chapters_sorted_by_number() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("chapter10.md"), "# Ch 10").unwrap();
        std::fs::write(dir.path().join("chapter2.md"), "# Ch 2").unwrap();
        std::fs::write(dir.path().join("chapter1.md"), "# Ch 1").unwrap();

        let chapters = discover_chapters(dir.path()).unwrap();
        let names: Vec<_> = chapters
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["chapter1.md", "chapter2.md", "chapter10.md"]);
    }

    #[test]
    fn test_discover_chapters_non_numeric_sorted_alpha() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("prologue.md"), "# Prologue").unwrap();
        std::fs::write(dir.path().join("epilogue.md"), "# Epilogue").unwrap();
        std::fs::write(dir.path().join("chapter1.md"), "# Ch 1").unwrap();

        let chapters = discover_chapters(dir.path()).unwrap();
        let names: Vec<_> = chapters
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names[0], "chapter1.md");
        assert_eq!(names[1], "epilogue.md");
        assert_eq!(names[2], "prologue.md");
    }
}
