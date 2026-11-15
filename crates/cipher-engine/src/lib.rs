//! The fixed pipelines of ADR 0005, run as Jobs from the Book's queue.
//! See `docs/design/architecture.md` › cipher-engine and `docs/design/translation-pipeline.md`.
//!
//! The interface:
//! - [`import`] makes a Book from a folder of markdown Chapters;
//! - [`translate`] queues Chapters;
//! - [`Worker::step`] claims and runs one Job; [`run`] loops it on a thread until stopped;
//! - [`Event`]s report what happened, for the app to redraw.
//!
//! Prompt building and each pipeline step are internal. Tests drive them through `step`
//! with `cipher_llm::Fake`.

mod import;
mod prompt;
mod suggest;
mod translate;
mod worker;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cipher_book::{Book, ChapterId, JobId, JobKind, JobSpec, RevisionId};
use cipher_llm::{Effort, Model, Usage};

pub use import::{ALIGNED, ImportReport, import, realign_imported};
pub use suggest::{Reach, Selection, SuggestionRequest, fix_drift, reach, suggest};
pub use worker::{Worker, run};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Book(#[from] cipher_book::Error),
    /// A folder to import could not be read.
    #[error("{0}")]
    Import(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The models a worker calls. `small` handles Repair and Story Notes compaction and falls
/// back to `translate`.
pub struct Models {
    pub translate: Box<dyn Model>,
    pub small: Option<Box<dyn Model>>,
    pub reasoning: Effort,
    /// What to record on each Job as having done it: "ChatGPT · gpt-6-luna".
    pub name: String,
}

impl Models {
    pub fn new(translate: Box<dyn Model>) -> Models {
        Models {
            translate,
            small: None,
            reasoning: Effort::ProviderDefault,
            name: String::new(),
        }
    }

    pub(crate) fn small(&self) -> &dyn Model {
        self.small.as_deref().unwrap_or(self.translate.as_ref())
    }
}

/// What the worker reports. Sent from the worker thread; the app drains them in its poll.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    JobStarted {
        job: JobId,
        kind: JobKind,
        chapter: Option<ChapterId>,
    },
    /// Translated Blocks streamed so far, of the Chapter's total.
    Progress {
        job: JobId,
        chapter: ChapterId,
        done: usize,
        total: usize,
    },
    /// The paragraphs of a translating Chapter finished so far, as (first, last, text) over
    /// its source Block numbers (1-based). Sent as each one completes, for the page to show.
    Streamed {
        chapter: ChapterId,
        entries: Vec<(u32, u32, String)>,
    },
    JobFinished {
        job: JobId,
        chapter: Option<ChapterId>,
        usage: Usage,
    },
    /// The Job will not be retried. `error` is for the user.
    JobFailed {
        job: JobId,
        chapter: Option<ChapterId>,
        error: String,
    },
    /// The Job is waiting: until `resumes_at` (unix seconds) after a usage limit or a
    /// transient error.
    JobPaused {
        job: JobId,
        resumes_at: i64,
        reason: String,
    },
    /// A Revision was written; re-read what it touched.
    BookChanged { revision: RevisionId },
}

/// Set to stop [`run`] after the current call.
#[derive(Debug, Clone, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Queues Chapters for translation, in the order given. Chapters translate one at a time
/// in queue order, because Story Notes and Pending Terms depend on what came before.
/// A Chapter that already has an unfinished translate Job is skipped.
pub fn translate(book: &mut Book, chapters: &[ChapterId]) -> Result<()> {
    let busy: Vec<ChapterId> = book
        .jobs(true)?
        .into_iter()
        .filter(|j| j.kind == JobKind::Translate && j.state != cipher_book::JobState::Failed)
        .filter_map(|j| j.chapter)
        .collect();
    for &chapter in chapters {
        if busy.contains(&chapter) {
            continue;
        }
        book.enqueue(JobSpec {
            kind: JobKind::Translate,
            parent: None,
            chapter: Some(chapter),
            suggestion: None,
            input: None,
        })?;
    }
    Ok(())
}
