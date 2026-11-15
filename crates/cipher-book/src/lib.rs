//! The Book: one SQLite file holding a book's Chapters, Translation, Glossary, Suggestions,
//! history and Job queue (ADR 0004). See `docs/design/architecture.md` › cipher-book and
//! `docs/design/storage.md`.
//!
//! The interface in one breath:
//! - [`Book::create`] / [`Book::open`] (which migrates);
//! - reads return plain owned values;
//! - every write is a [`Change`] passed to [`Book::apply`], which records one Revision;
//!   [`Book::undo`] reverses a Revision unless later work touched the same rows;
//! - the Job queue: [`Book::enqueue`], [`Book::claim_next_job`], [`Book::save_job`].
//!
//! Several `Book`s may be open on the same file (the app and the engine's worker): WAL mode
//! lets reads run alongside a write, and SQLite's lock serialises writes.

mod change;
mod jobs;
mod read;
mod revision;
mod schema;
mod types;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

pub use change::Change;
pub use types::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("a book already exists at {0}")]
    Exists(PathBuf),
    #[error("{0} {1} not found")]
    NotFound(&'static str, i64),
    #[error("invalid change: {0}")]
    Invalid(String),
    #[error("this book was written by a newer Cipher (schema {0})")]
    NewerSchema(i64),
    #[error("corrupt book: {0}")]
    Corrupt(String),
    #[error("corrupt history: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) fn now() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub struct Book {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Book").field("path", &self.path).finish()
    }
}

impl Book {
    /// Creates a new Book file. Fails if `path` exists.
    pub fn create(path: impl AsRef<Path>, meta: BookMeta) -> Result<Book> {
        let path = path.as_ref();
        if path.exists() {
            return Err(Error::Exists(path.to_owned()));
        }
        let mut book = Book::open_conn(path)?;
        book.write_meta(meta)?;
        Ok(book)
    }

    fn write_meta(&mut self, meta: BookMeta) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (key, value) in [
            ("title", meta.title),
            ("source_lang", meta.source_lang),
            ("target_lang", meta.target_lang),
            ("created_at", now().to_string()),
        ] {
            tx.execute(
                "INSERT INTO book_meta (key, value) VALUES (?1, ?2)",
                (key, value),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// An empty Book that lives only in memory and is gone when dropped. Its path is empty.
    pub fn in_memory(meta: BookMeta) -> Result<Book> {
        let mut conn = Connection::open_in_memory()?;
        schema::configure(&conn)?;
        schema::migrate(&mut conn)?;
        let mut book = Book {
            conn,
            path: PathBuf::new(),
        };
        book.write_meta(meta)?;
        Ok(book)
    }

    /// Sets the Book's title. Not a Revision: renaming a Book is not undone with its text.
    pub fn set_title(&mut self, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::Invalid("a Book needs a title".into()));
        }
        self.conn.execute(
            "UPDATE book_meta SET value = ?1 WHERE key = 'title'",
            [title],
        )?;
        Ok(())
    }

    /// The model this Book translates with, chosen from the app's Endpoints: the Endpoint's id
    /// and the model's name. `None` means the app's default. Not a Revision.
    pub fn model_choice(&self) -> Result<Option<ModelChoice>> {
        let get = |key: &str| -> Result<Option<String>> {
            Ok(self
                .conn
                .query_row("SELECT value FROM book_meta WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .optional()?)
        };
        Ok(match (get("model_endpoint")?, get("model")?) {
            (Some(endpoint), Some(model)) => Some(ModelChoice { endpoint, model }),
            _ => None,
        })
    }

    pub fn set_model_choice(&mut self, choice: Option<&ModelChoice>) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM book_meta WHERE key IN ('model_endpoint', 'model')",
            [],
        )?;
        if let Some(c) = choice {
            tx.execute(
                "INSERT INTO book_meta (key, value) VALUES ('model_endpoint', ?1), ('model', ?2)",
                (&c.endpoint, &c.model),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Opens an existing Book and migrates it to the current schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Book> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::Corrupt(format!("no book at {}", path.display())));
        }
        Book::open_conn(path)
    }

    fn open_conn(path: &Path) -> Result<Book> {
        let mut conn = Connection::open(path)?;
        schema::configure(&conn)?;
        schema::migrate(&mut conn)?;
        Ok(Book {
            conn,
            path: path.to_owned(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Validates `change`, writes it in one transaction and records it as one Revision.
    /// On error nothing is written.
    pub fn apply(&mut self, change: Change) -> Result<Applied> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let applied = change::apply(&tx, change)?;
        tx.commit()?;
        Ok(applied)
    }

    /// Reverses a Revision as a new Revision. Rows changed since are conflicts: see
    /// [`UndoMode`] for what happens to them.
    pub fn undo(&mut self, revision: RevisionId, mode: UndoMode) -> Result<UndoOutcome> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let outcome = revision::undo(&tx, revision, mode)?;
        if matches!(outcome, UndoOutcome::Undone { .. }) {
            tx.commit()?;
        }
        Ok(outcome)
    }
}
