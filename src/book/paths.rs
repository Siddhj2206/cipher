use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BookPaths {
    pub root: PathBuf,
    pub config_json: PathBuf,
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
            config_json: root.join("config.json"),
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
    pub config_json: bool,
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
            config_json: paths.config_json.is_file(),
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
