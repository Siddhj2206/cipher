//! The Shelf (`docs/design/desktop-ux.md` › Shelf): the Books Cipher knows about.
//!
//! Books Cipher makes live in one folder, `~/Documents/Cipher` (the XDG documents folder), one
//! `.cipher` file each. Books opened from elsewhere stay where they are and are remembered in
//! `shelf.json` beside `endpoints.json`, with when each Book was last opened.
//!
//! No Rune types here, so all of it is tested against temp folders.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cipher_book::{Book, BookMeta, ChapterState};
use serde::{Deserialize, Serialize};

/// A Book as the Shelf shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfBook {
    pub path: PathBuf,
    pub title: String,
    pub source_lang: String,
    pub chapters: usize,
    pub translated: usize,
    /// Chapters with no text, which never need translating.
    pub empty: usize,
    /// Unix seconds, when Cipher last opened it.
    pub opened: Option<i64>,
    /// Unix seconds, the file's last change.
    pub modified: i64,
    /// Opened from outside the Shelf's folder: removing it forgets it, never deletes it.
    pub elsewhere: bool,
    /// Why it could not be read, if it could not.
    pub problem: Option<String>,
    /// The Chapter reading stopped at, by number.
    pub at_chapter: Option<i64>,
}

/// What `shelf.json` remembers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Recents {
    #[serde(default)]
    elsewhere: Vec<PathBuf>,
    /// Path → unix seconds.
    #[serde(default)]
    opened: BTreeMap<PathBuf, i64>,
    /// Path → where reading stopped.
    #[serde(default)]
    positions: BTreeMap<PathBuf, Position>,
}

/// Where reading stopped in a Book: a Chapter by number, and in it the Block at the top of the
/// window with how far below the window's top it sat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub chapter: i64,
    #[serde(default)]
    pub block: Option<i64>,
    #[serde(default)]
    pub offset: f32,
}

pub struct Shelf {
    pub dir: PathBuf,
    recents_path: Option<PathBuf>,
    recents: Recents,
    /// Most recently opened first, then most recently changed.
    pub books: Vec<ShelfBook>,
}

pub const EXTENSION: &str = "cipher";

fn read_recents(path: &Path) -> Recents {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_recents(path: &Path, recents: &Recents) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(recents).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("Could not save {}: {e}", path.display()))
}

impl Shelf {
    /// The Shelf in `dir`, remembering opened Books in `recents_path` (`None` remembers
    /// nothing, for headless runs). Neither has to exist yet.
    pub fn open(dir: PathBuf, recents_path: Option<PathBuf>) -> Shelf {
        let recents = recents_path
            .as_deref()
            .map(read_recents)
            .unwrap_or_default();
        let mut shelf = Shelf {
            dir,
            recents_path,
            recents,
            books: Vec::new(),
        };
        shelf.scan();
        shelf
    }

    /// `~/Documents/Cipher`: on Linux following `XDG_DOCUMENTS_DIR` from `user-dirs.dirs`,
    /// elsewhere the system's Documents folder.
    #[cfg(not(target_os = "linux"))]
    pub fn default_dir() -> Option<PathBuf> {
        Some(
            dirs::document_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))?
                .join("Cipher"),
        )
    }

    #[cfg(target_os = "linux")]
    pub fn default_dir() -> Option<PathBuf> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map_or_else(|| home.join(".config"), PathBuf::from);
        let documents = std::fs::read_to_string(config.join("user-dirs.dirs"))
            .ok()
            .and_then(|s| documents_dir(&s, &home))
            .unwrap_or_else(|| home.join("Documents"));
        Some(documents.join("Cipher"))
    }

    /// `shelf.json` beside `endpoints.json`.
    pub fn default_recents() -> Option<PathBuf> {
        Some(
            cipher_llm::Endpoints::default_path()?
                .parent()?
                .join("shelf.json"),
        )
    }

    /// Where reading stopped in the Book at `path`, read from `recents_path` alone.
    pub fn position_in(recents_path: &Path, path: &Path) -> Option<Position> {
        read_recents(recents_path).positions.get(path).copied()
    }

    /// Remembers where reading stopped in the Book at `path`, without reading any Books.
    pub fn remember_position(
        recents_path: &Path,
        path: &Path,
        position: Position,
    ) -> Result<(), String> {
        let mut recents = read_recents(recents_path);
        if recents.positions.get(path) == Some(&position) {
            return Ok(());
        }
        recents.positions.insert(path.to_path_buf(), position);
        write_recents(recents_path, &recents)
    }

    /// Re-reads the folder and the remembered Books.
    pub fn scan(&mut self) {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == EXTENSION))
                    .collect()
            })
            .unwrap_or_default();
        let inside = paths.len();
        for p in &self.recents.elsewhere {
            if !paths.contains(p) {
                paths.push(p.clone());
            }
        }
        self.books = paths
            .into_iter()
            .enumerate()
            .map(|(i, p)| self.read(p, i >= inside))
            .collect();
        self.books.sort_by(|a, b| {
            (b.opened, b.modified, &a.title).cmp(&(a.opened, a.modified, &b.title))
        });
    }

    fn read(&self, path: PathBuf, elsewhere: bool) -> ShelfBook {
        let modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        let mut book = ShelfBook {
            title: path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
            source_lang: String::new(),
            chapters: 0,
            translated: 0,
            empty: 0,
            opened: self.recents.opened.get(&path).copied(),
            modified,
            elsewhere,
            problem: None,
            at_chapter: self.recents.positions.get(&path).map(|p| p.chapter),
            path,
        };
        let read = Book::open(&book.path).and_then(|b| Ok((b.meta()?, b.chapters()?)));
        match read {
            Ok((meta, chapters)) => {
                book.title = meta.title;
                book.source_lang = meta.source_lang;
                book.chapters = chapters.len();
                book.translated = chapters
                    .iter()
                    .filter(|c| c.state == ChapterState::Translated)
                    .count();
                book.empty = chapters
                    .iter()
                    .filter(|c| c.state == ChapterState::Empty)
                    .count();
            }
            Err(e) => book.problem = Some(e.to_string()),
        }
        book
    }

    fn save_recents(&self) -> Result<(), String> {
        match &self.recents_path {
            Some(path) => write_recents(path, &self.recents),
            None => Ok(()),
        }
    }

    /// A free path in the folder for a Book titled `title`.
    pub fn new_path(&self, title: &str) -> PathBuf {
        let stem = file_stem(title);
        (1..)
            .map(|n| match n {
                1 => self.dir.join(format!("{stem}.{EXTENSION}")),
                n => self.dir.join(format!("{stem} {n}.{EXTENSION}")),
            })
            .find(|p| !p.exists())
            .expect("some numbered name is free")
    }

    /// Makes an empty Book in the folder. Returns its path.
    pub fn create(&mut self, title: &str, source_lang: &str) -> Result<PathBuf, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Give the Book a title.".into());
        }
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("Could not make {}: {e}", self.dir.display()))?;
        let path = self.new_path(title);
        Book::create(
            &path,
            BookMeta {
                title: title.into(),
                source_lang: source_lang.into(),
                target_lang: "en".into(),
            },
        )
        .map_err(|e| e.to_string())?;
        self.scan();
        Ok(path)
    }

    /// Where an import of `folder` will write: a free path named after the folder.
    pub fn import_path(&self, folder: &Path) -> Result<PathBuf, String> {
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().replace(['-', '_'], " "))
            .ok_or("Pick a folder with a name.")?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("Could not make {}: {e}", self.dir.display()))?;
        Ok(self.new_path(&name))
    }

    /// Remembers a Book outside the folder. A Book inside it needs nothing.
    pub fn add(&mut self, path: &Path) -> Result<(), String> {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        Book::open(&path).map_err(|e| format!("Not a Cipher Book: {e}"))?;
        if !self.contains(&path) {
            self.recents.elsewhere.push(path);
            self.save_recents()?;
        }
        self.scan();
        Ok(())
    }

    fn contains(&self, path: &Path) -> bool {
        let in_dir = path.parent().is_some_and(|d| {
            std::fs::canonicalize(d).ok() == std::fs::canonicalize(&self.dir).ok()
        });
        in_dir || self.recents.elsewhere.iter().any(|p| p == path)
    }

    /// Notes that a Book was opened now, so it comes first.
    pub fn opened(&mut self, path: &Path) -> Result<(), String> {
        self.recents.opened.insert(path.to_path_buf(), unix_now());
        self.save_recents()
    }

    /// Changes a Book's title. Its file keeps its name.
    pub fn rename(&mut self, path: &Path, title: &str) -> Result<(), String> {
        Book::open(path)
            .and_then(|mut b| b.set_title(title))
            .map_err(|e| e.to_string())?;
        self.scan();
        Ok(())
    }

    /// Takes a Book off the Shelf: one in the folder goes to the trash, one from elsewhere is
    /// only forgotten. The Book must not be open.
    pub fn remove(&mut self, path: &Path) -> Result<(), String> {
        let elsewhere = self.recents.elsewhere.iter().any(|p| p == path);
        if elsewhere {
            self.recents.elsewhere.retain(|p| p != path);
        } else {
            trash(path).map_err(|e| format!("Could not move it to the trash: {e}"))?;
        }
        self.recents.opened.remove(path);
        self.recents.positions.remove(path);
        self.save_recents()?;
        self.scan();
        Ok(())
    }
}

/// `XDG_DOCUMENTS_DIR="$HOME/Documents"` from a `user-dirs.dirs` file.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn documents_dir(dirs: &str, home: &Path) -> Option<PathBuf> {
    let line = dirs
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("XDG_DOCUMENTS_DIR="))?;
    let value = line["XDG_DOCUMENTS_DIR=".len()..].trim_matches('"');
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    // `$HOME/` alone means the documents folder is turned off.
    (path != home && path.is_absolute()).then_some(path)
}

/// A file name for a title: path separators and control characters become spaces.
fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let stem: String = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches('.')
        .chars()
        .take(80)
        .collect();
    if stem.is_empty() {
        "Untitled".into()
    } else {
        stem
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Moves a Book to the system's Recycle Bin or Trash, with its `-wal` and `-shm` files.
#[cfg(not(target_os = "linux"))]
fn trash(path: &Path) -> std::io::Result<()> {
    let mut all = vec![path.to_path_buf()];
    for suffix in ["-wal", "-shm"] {
        let mut side = path.as_os_str().to_owned();
        side.push(suffix);
        let side = PathBuf::from(side);
        if side.exists() {
            all.push(side);
        }
    }
    trash::delete_all(&all).map_err(std::io::Error::other)
}

/// Moves a Book to the freedesktop trash (`$XDG_DATA_HOME/Trash`), with its `-wal` and `-shm`
/// files, so the file manager can restore it.
#[cfg(target_os = "linux")]
fn trash(path: &Path) -> std::io::Result<()> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
        .ok_or_else(|| std::io::Error::other("HOME is not set"))?;
    trash_into(path, &data.join("Trash"))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn trash_into(path: &Path, trash: &Path) -> std::io::Result<()> {
    let (files, info) = (trash.join("files"), trash.join("info"));
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let path = std::fs::canonicalize(path)?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?
        .to_string_lossy()
        .into_owned();
    let free = (1..)
        .map(|n| match n {
            1 => name.clone(),
            n => format!("{n} {name}"),
        })
        .find(|n| !files.join(n).exists() && !info.join(format!("{n}.trashinfo")).exists())
        .expect("some numbered name is free");
    std::fs::write(
        info.join(format!("{free}.trashinfo")),
        format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            percent_encode(&path.to_string_lossy()),
            utc_datetime(unix_now())
        ),
    )?;
    let to = files.join(&free);
    if std::fs::rename(&path, &to).is_err() {
        // Another filesystem: copy, then remove.
        std::fs::copy(&path, &to)?;
        std::fs::remove_file(&path)?;
    }
    for side in ["-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(side);
        let _ = std::fs::remove_file(PathBuf::from(p));
    }
    Ok(())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// `YYYY-MM-DDThh:mm:ss` in UTC (the spec asks for local time; the trash only shows it).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn utc_datetime(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf(dir: &tempfile::TempDir) -> Shelf {
        Shelf::open(
            dir.path().join("Cipher"),
            Some(dir.path().join("config/shelf.json")),
        )
    }

    #[test]
    fn a_new_book_lands_in_the_folder_and_opening_one_puts_it_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = shelf(&dir);
        assert!(s.books.is_empty(), "a missing folder is an empty Shelf");
        assert!(s.create("  ", "ko").is_err());
        let a = s.create("The Tower: Part 1", "ko").unwrap();
        assert_eq!(a, dir.path().join("Cipher/The Tower Part 1.cipher"));
        let b = s.create("The Tower: Part 1", "ja").unwrap();
        assert_eq!(b.file_name().unwrap(), "The Tower Part 1 2.cipher");
        assert_eq!(s.books.len(), 2);

        s.opened(&a).unwrap();
        let again = shelf(&dir);
        assert_eq!(again.books[0].path, a, "remembered across runs");
        assert_eq!(again.books[0].title, "The Tower: Part 1");
        assert!(!again.books[0].elsewhere);
    }

    #[test]
    fn a_book_from_elsewhere_is_remembered_and_only_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = shelf(&dir);
        let outside = dir.path().join("outside.cipher");
        let meta = BookMeta {
            title: "Outside".into(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        };
        drop(Book::create(&outside, meta).unwrap());
        std::fs::write(dir.path().join("junk.cipher"), "not sqlite").unwrap();
        assert!(s.add(&dir.path().join("junk.cipher")).is_err());

        s.add(&outside).unwrap();
        s.add(&outside).unwrap();
        assert_eq!(s.books.len(), 1);
        assert!(s.books[0].elsewhere);
        assert_eq!(shelf(&dir).books.len(), 1);

        let path = s.books[0].path.clone();
        s.remove(&path).unwrap();
        assert!(s.books.is_empty());
        assert!(outside.exists(), "a Book from elsewhere is never deleted");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rename_changes_the_title_and_remove_moves_to_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = shelf(&dir);
        let p = s.create("Draft", "ko").unwrap();
        s.rename(&p, "Final").unwrap();
        assert_eq!(s.books[0].title, "Final");
        assert!(s.rename(&p, "").is_err());

        let trash = dir.path().join("Trash");
        trash_into(&p, &trash).unwrap();
        assert!(!p.exists());
        assert!(trash.join("files/Draft.cipher").exists());
        let info = std::fs::read_to_string(trash.join("info/Draft.cipher.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=/"), "{info}");
        assert!(info.contains("Draft.cipher\nDeletionDate=20"), "{info}");
        s.scan();
        assert!(s.books.is_empty());
    }

    #[test]
    fn names_dates_and_the_documents_folder() {
        assert_eq!(file_stem("a/b: c?"), "a b c");
        assert_eq!(file_stem(" ..hidden "), "hidden");
        assert_eq!(file_stem("///"), "Untitled");
        assert_eq!(file_stem("전지적 독자 시점"), "전지적 독자 시점");
        assert_eq!(percent_encode("/a b/전"), "/a%20b/%EC%A0%84");
        assert_eq!(utc_datetime(0), "1970-01-01T00:00:00");
        assert_eq!(utc_datetime(1_763_164_800), "2025-11-15T00:00:00");
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let dirs = "# comment\nXDG_DOCUMENTS_DIR=\"$HOME/Docs\"\n";
        assert_eq!(documents_dir(dirs, home), Some(home.join("Docs")));
        assert_eq!(documents_dir("XDG_DOCUMENTS_DIR=\"$HOME/\"", home), None);
        let documents = home.join("data").join("d");
        let dirs = format!("XDG_DOCUMENTS_DIR=\"{}\"", documents.display());
        assert_eq!(documents_dir(&dirs, home), Some(documents));
    }
}
