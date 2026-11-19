//! Adding Chapters and driving the Job queue: the paste sheet's reading of the clipboard,
//! "Add and translate", "Translate now" and Retry.

use cipher_book::{Change, ChapterId, Error, JobKind, JobState, Result};
use cipher_text::{blocks, heading};

use super::{ChapterRow, Library, opening_line, stats};

/// Pasted text read as a new Chapter, for the paste sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intake {
    /// The detected Chapter number, from a heading in the first lines.
    pub number: Option<i64>,
    pub title: Option<String>,
    /// The Chapter's Blocks, heading removed.
    pub blocks: Vec<String>,
    /// The first three non-empty lines, as pasted.
    pub preview: Vec<String>,
    /// The number of the Chapter it will follow, if any.
    pub after: Option<i64>,
    /// A Chapter with the same number is already in the Book.
    pub exists: bool,
}

impl Library {
    pub fn translations_busy(&self) -> Result<bool> {
        Ok(self.book.jobs(true)?.iter().any(|job| {
            job.kind == JobKind::Translate
                && matches!(
                    job.state,
                    JobState::Queued | JobState::Running | JobState::Checking
                )
        }))
    }

    pub fn notification_chapter(&self, id: ChapterId) -> Option<String> {
        let chapter = self.book.chapter(id).ok()?;
        Some(match chapter.title.filter(|t| !t.trim().is_empty()) {
            Some(title) => format!("Chapter {}: {}", chapter.number.unwrap_or(id.0), title),
            None => format!("Chapter {}", chapter.number.unwrap_or(id.0)),
        })
    }

    /// Reads pasted text as a Chapter. `None` when it holds nothing but whitespace.
    pub fn intake(&self, text: &str) -> Option<Intake> {
        let mut blocks = blocks::split(text);
        if blocks.is_empty() {
            return None;
        }
        let head = heading::take_heading(&mut blocks);
        let number = head
            .as_ref()
            .and_then(|h| h.number)
            .or_else(|| heading::detect_number(text))
            .map(i64::from);
        let title = head.and_then(|h| h.title);
        let preview = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(3)
            .map(str::to_owned)
            .collect();
        let numbers = self.chapters.iter().map(|c| c.number);
        let after = match number {
            Some(n) => numbers.filter(|&m| m < n).max(),
            None => self.chapters.last().map(|c| c.number),
        };
        Some(Intake {
            exists: number.is_some_and(|n| self.chapters.iter().any(|c| c.number == n)),
            number,
            title,
            blocks,
            preview,
            after,
        })
    }

    /// Adds the Chapter in number order and queues it. Returns its index.
    pub fn add_chapter(&mut self, intake: &Intake) -> Result<usize> {
        if intake.blocks.is_empty() {
            return Err(Error::Invalid(
                "the pasted Chapter has no paragraphs".into(),
            ));
        }
        let applied = self.book.apply(Change::AddChapter {
            number: intake.number,
            title: intake.title.clone(),
            source: intake.blocks.clone(),
        })?;
        let id = ChapterId(
            *applied
                .created
                .first()
                .ok_or_else(|| Error::Invalid("AddChapter created no Chapter".into()))?,
        );
        cipher_engine::translate(&mut self.book, &[id]).map_err(super::engine_error)?;
        self.insert_row(id)
    }

    /// Queues an untranslated Chapter.
    pub fn translate(&mut self, index: usize) -> Result<()> {
        let id = self.chapters[index].id;
        cipher_engine::translate(&mut self.book, &[id]).map_err(super::engine_error)?;
        self.refresh_chapters(&[index])
    }

    /// Queues this Chapter and every one after it that is waiting, in order.
    pub fn translate_from(&mut self, index: usize) -> Result<()> {
        let at: Vec<usize> = (index..self.chapters.len())
            .filter(|&i| self.chapters[i].state == cipher_book::ChapterState::Waiting)
            .collect();
        let ids: Vec<_> = at.iter().map(|&i| self.chapters[i].id).collect();
        cipher_engine::translate(&mut self.book, &ids).map_err(super::engine_error)?;
        self.refresh_chapters(&at)
    }

    /// Puts a Chapter's failed (or paused) translate Job back in the queue, keeping its
    /// progress.
    pub fn retry(&mut self, index: usize) -> Result<()> {
        let id = self.chapters[index].id;
        let job = self
            .book
            .jobs(true)?
            .into_iter()
            .rev()
            .find(|j| {
                j.kind == JobKind::Translate
                    && j.chapter == Some(id)
                    && matches!(j.state, JobState::Failed | JobState::Paused)
            })
            .ok_or_else(|| Error::Invalid("this Chapter has no failed Job".into()))?;
        self.book.retry_job(job.id)?;
        self.refresh_chapters(&[index])
    }

    /// Puts a new Chapter's row and numbers in place, keeping the current page.
    fn insert_row(&mut self, id: ChapterId) -> Result<usize> {
        let summaries = self.book.chapters()?;
        let at = summaries
            .iter()
            .position(|s| s.id == id)
            .ok_or(Error::NotFound("chapter", id.0))?;
        let summary = &summaries[at];
        let ch = self.book.chapter(id)?;
        self.stats
            .insert(at, stats::compute(&ch, &self.terms, &self.check, &self.pre));
        self.chapters.insert(
            at,
            ChapterRow {
                id,
                number: summary.number.unwrap_or(at as i64 + 1),
                state: summary.state,
                outdated: summary.outdated,
                opening: opening_line(&ch),
                failure: None,
                made_by: None,
                queue_place: None,
                pending_edits: 0,
            },
        );
        self.refresh_queue()?;
        self.derive_usage();
        self.near.clear();
        let current = self.page.index + usize::from(at <= self.page.index);
        self.open_chapter(current)?;
        Ok(at)
    }
}
