//! Everything the views need from the Book, as view-ready owned values.
//!
//! `Library` opens a Book, reads the Chapter list and the Glossary, and works out once, at load:
//! how often each Term is used in each Chapter and where it drifts. The current Chapter is kept
//! as a [`Page`] (Blocks rendered, Terms found, Pairs resolved). When something changes,
//! [`Library::refresh_chapters`] recomputes those Chapters only.
//!
//! No Rune types appear here, so all of it is tested against a Book in a temp dir.

mod edits;
mod history;
mod page;
mod queue;
mod scan;
mod stats;
mod terms;
#[cfg(test)]
mod tests;

use std::path::Path;

use cipher_book::{
    Book, BookMeta, Chapter, ChapterId, ChapterState, Error, JobKind, JobState, Result, TermId,
    TermStatus,
};

pub use edits::counts;
pub use history::undo_mode;
pub use page::{Page, PageBlock, PairFlag};
pub use queue::{Intake, IntakeIssue, IntakePart};
pub use stats::{ChapterStats, shown};

use scan::SourceFinder;

/// A Term as the views show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermInfo {
    pub id: TermId,
    pub source: String,
    pub aliases: Vec<String>,
    pub rendering: String,
    pub kind: Option<String>,
    pub gender: Option<String>,
    pub note: Option<String>,
    /// A Pending Term: used provisionally until the user accepts or dismisses it.
    pub provisional: bool,
    pub forbidden: Vec<String>,
    /// May go unnamed in the Translation (`TermFields::may_omit`).
    pub may_omit: bool,
}

impl TermInfo {
    pub fn source_forms(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.source.as_str()).chain(self.aliases.iter().map(String::as_str))
    }

    pub fn matches_query(&self, query: &str) -> bool {
        query.is_empty()
            || std::iter::once(self.rendering.as_str())
                .chain(self.source_forms())
                .chain(self.forbidden.iter().map(String::as_str))
                .any(|s| s.to_lowercase().contains(query))
    }
}

/// A row of the Contents list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterRow {
    pub id: ChapterId,
    /// The Chapter's number, or its position when it has none.
    pub number: i64,
    pub title: Option<String>,
    pub state: ChapterState,
    /// Translated Blocks marked Outdated.
    pub outdated: usize,
    /// The line a reader recognises the Chapter by: from the Translation when there is one,
    /// else from the source.
    pub opening: String,
    /// Why the latest translate Job failed, when the state is `Failed`.
    pub failure: Option<String>,
    /// Place among Chapters waiting in the Job queue (1 is next), when `Queued`.
    pub queue_place: Option<usize>,
    /// Proposed Edits waiting for a decision in this Chapter.
    pub pending_edits: usize,
    /// The translate Job that wrote the current Translation, when Cipher made it.
    pub made_by: Option<MadeBy>,
}

/// Who translated a Chapter and what it cost: from its last finished translate Job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeBy {
    /// "ChatGPT · gpt-6-luna"; `None` for Jobs from before Cipher recorded it.
    pub model: Option<String>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    /// Unix seconds.
    pub at: i64,
}

fn chapter_title(title: Option<&str>) -> Option<&str> {
    title.filter(|title| !title.trim().is_empty())
}

impl ChapterRow {
    pub fn label(&self) -> String {
        if let Some(title) = chapter_title(self.title.as_deref()) {
            title.to_owned()
        } else if self.opening.is_empty() {
            format!("Chapter {}", self.number)
        } else {
            self.opening.clone()
        }
    }

    /// Not yet translated, whatever the reason.
    pub fn untranslated(&self) -> bool {
        !matches!(self.state, ChapterState::Translated | ChapterState::Empty)
    }
}

/// Where a Term is used, over every Chapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermUsage {
    pub total: u32,
    /// Chapters that use it.
    pub chapters: usize,
    /// Indices of the Chapters where it drifts, in reading order.
    pub drift_chapters: Vec<usize>,
}

pub struct Library {
    book: Book,
    pub meta: BookMeta,
    pub chapters: Vec<ChapterRow>,
    pub terms: Vec<TermInfo>,
    /// Suggestions not yet `Done`.
    pub open_suggestions: usize,
    /// The open Suggestions with their Proposed Edits, newest first.
    pub suggestions: Vec<cipher_book::Suggestion>,
    /// Blocks of the loaded Book with a pending Proposed Edit.
    edit_blocks: std::collections::HashSet<cipher_book::BlockId>,
    stats: Vec<ChapterStats>,
    usage: Vec<TermUsage>,
    finder: SourceFinder,
    check: Vec<cipher_text::checks::CheckTerm>,
    pre: scan::Prefilter,
    page: Page,
    /// Built pages of Chapters near the current one, for continuous reading, by index.
    near: std::collections::HashMap<usize, Page>,
    /// False until every Chapter's numbers and opening line are in (see [`Library::scan`]).
    scanned: bool,
    /// Which load this is, so a scan started before a reload is never applied after it.
    generation: u64,
}

static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Each Chapter's opening line and numbers, worked out away from the UI thread.
pub struct Scan {
    generation: u64,
    rows: Vec<(ChapterId, String, ChapterStats)>,
}

/// Reads and counts Chapters on every core, each thread with its own connection.
fn scan_chapters(
    path: &Path,
    ids: &[ChapterId],
    terms: &[TermInfo],
    check: &[cipher_text::checks::CheckTerm],
) -> Result<Vec<(ChapterId, String, ChapterStats)>> {
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 8);
    let chunk = ids.len().div_ceil(threads).max(1);
    let pre = &scan::Prefilter::new(terms);
    std::thread::scope(|scope| {
        let handles: Vec<_> = ids
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || -> Result<Vec<_>> {
                    let book = Book::open(path)?;
                    part.iter()
                        .map(|&id| {
                            let ch = book.chapter(id)?;
                            Ok((
                                id,
                                opening_line(&ch),
                                stats::compute(&ch, terms, check, pre),
                            ))
                        })
                        .collect()
                })
            })
            .collect();
        let mut out = Vec::with_capacity(ids.len());
        for h in handles {
            out.extend(
                h.join()
                    .map_err(|_| Error::Invalid("the scan panicked".into()))??,
            );
        }
        Ok(out)
    })
}

/// An engine error as the Book error it nearly always is; anything else as `Invalid`.
fn engine_error(e: cipher_engine::Error) -> Error {
    match e {
        cipher_engine::Error::Book(e) => e,
        other => Error::Invalid(other.to_string()),
    }
}

/// The text a reader recognises a Chapter by: its first line of some length that is not the
/// `Chapter N` heading.
fn opening_line(chapter: &Chapter) -> String {
    let blocks = if chapter.translation.is_empty() {
        &chapter.source
    } else {
        &chapter.translation
    };
    blocks
        .iter()
        .map(|b| shown(&b.text))
        .map(|t| t.trim().to_string())
        .find(|t| t.chars().count() > 12 && !t.starts_with("Chapter"))
        .map(|t| t.chars().take(80).collect())
        .unwrap_or_default()
}

fn term_info(t: cipher_book::Term) -> TermInfo {
    TermInfo {
        id: t.id,
        source: t.fields.source,
        aliases: t.aliases,
        rendering: t.fields.rendering,
        kind: t.fields.kind,
        gender: t.fields.gender,
        note: t.fields.note,
        provisional: t.status == TermStatus::Pending,
        forbidden: t.forbidden,
        may_omit: t.fields.may_omit,
    }
}

impl Library {
    /// Opens a Book with every Chapter's numbers in. Slow on a big Book; the app uses
    /// [`Library::open_quick`].
    pub fn open(path: &Path) -> Result<Library> {
        Library::from_book(Book::open(path)?)
    }

    /// Opens a Book with its Chapter list and the first page only, at once at any size. The
    /// opening lines and Term numbers come from [`Library::scan`], run off the UI thread.
    pub fn open_quick(path: &Path) -> Result<Library> {
        Library::load(Book::open(path)?)
    }

    /// Whether the opening lines and Term numbers are in.
    pub fn scanned(&self) -> bool {
        self.scanned
    }

    /// Which load of a Book this is; a reload is a new one.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The work [`Library::open_quick`] left, as a job for another thread. Hand what it
    /// returns to [`Library::apply_scan`].
    pub fn scan(&self) -> impl FnOnce() -> Result<Scan> + Send + 'static {
        let generation = self.generation;
        let path = self.book.path().to_path_buf();
        let ids: Vec<ChapterId> = self.chapters.iter().map(|c| c.id).collect();
        let terms = self.terms.clone();
        let check = self.check.clone();
        move || {
            let rows = scan_chapters(&path, &ids, &terms, &check)?;
            Ok(Scan { generation, rows })
        }
    }

    /// Re-pairs Chapters an old import paired by position ([`cipher_engine::realign_imported`]),
    /// on its own connection. Returns how many changed; then the Book must be read again.
    pub fn realign(&self) -> impl FnOnce() -> Result<usize> + Send + 'static {
        let path = self.book.path().to_path_buf();
        move || cipher_engine::realign_imported(&mut Book::open(&path)?)
    }

    /// Takes a finished scan in. One for another Book, or from before the Terms changed, is
    /// dropped; Chapters added since keep the numbers they already have.
    pub fn apply_scan(&mut self, scan: Scan) {
        if scan.generation != self.generation || self.scanned {
            return;
        }
        let at: std::collections::HashMap<ChapterId, usize> = self
            .chapters
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect();
        for (id, opening, stats) in scan.rows {
            if let Some(&i) = at.get(&id) {
                if stats.counts.len() != self.terms.len() {
                    return;
                }
                self.chapters[i].opening = opening;
                self.stats[i] = stats;
            }
        }
        self.scanned = true;
        self.derive_usage();
    }

    /// An empty, unsaved Book: what the app holds while the Shelf shows and no Book is open.
    pub fn blank() -> Library {
        let meta = BookMeta {
            title: String::new(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        };
        Book::in_memory(meta)
            .and_then(Library::from_book)
            .expect("an in-memory Book always opens")
    }

    /// The Book has a file: false for [`Library::blank`].
    /// The model this Book translates with, if it chose one over the app's default.
    pub fn model_choice(&self) -> Option<cipher_book::ModelChoice> {
        self.book.model_choice().ok().flatten()
    }

    pub fn set_model_choice(&mut self, choice: Option<&cipher_book::ModelChoice>) -> Result<()> {
        self.book.set_model_choice(choice)
    }

    pub fn is_saved(&self) -> bool {
        !self.book.path().as_os_str().is_empty()
    }

    /// A Book with every Chapter's numbers in, read here and now.
    pub fn from_book(book: Book) -> Result<Library> {
        let mut lib = Library::load(book)?;
        let ids: Vec<ChapterId> = lib.chapters.iter().map(|c| c.id).collect();
        for (i, id) in ids.into_iter().enumerate() {
            let ch = lib.book.chapter(id)?;
            lib.chapters[i].opening = opening_line(&ch);
            lib.stats[i] = stats::compute(&ch, &lib.terms, &lib.check, &lib.pre);
        }
        lib.scanned = true;
        lib.derive_usage();
        Ok(lib)
    }

    /// The Chapter list and the first page; no opening lines or numbers yet.
    fn load(book: Book) -> Result<Library> {
        let meta = book.meta()?;
        let summaries = book.chapters()?;
        let terms: Vec<TermInfo> = book
            .glossary()?
            .terms
            .into_iter()
            .filter(|t| t.status != TermStatus::Dismissed)
            .map(term_info)
            .collect();
        let check = stats::check_terms(&terms);
        let finder = SourceFinder::new(&terms);
        let pre = scan::Prefilter::new(&terms);

        let mut chapters = Vec::with_capacity(summaries.len());
        let blank = ChapterStats {
            counts: vec![0; terms.len()],
            ..ChapterStats::default()
        };
        let all_stats = vec![blank; summaries.len()];
        for (i, s) in summaries.iter().enumerate() {
            chapters.push(ChapterRow {
                id: s.id,
                number: s.number.unwrap_or(i as i64 + 1),
                title: s.title.clone(),
                state: s.state,
                outdated: s.outdated,
                opening: String::new(),
                failure: None,
                made_by: None,
                queue_place: None,
                pending_edits: 0,
            });
        }
        let chapters_empty = chapters.is_empty();
        // The first page is built by `open_chapter` below, once the Library is whole.
        let page = Page::default();
        let mut lib = Library {
            book,
            meta,
            chapters,
            terms,
            open_suggestions: 0,
            suggestions: Vec::new(),
            edit_blocks: Default::default(),
            stats: all_stats,
            usage: Vec::new(),
            finder,
            pre,
            check,
            page,
            near: Default::default(),
            scanned: chapters_empty,
            generation: GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        };
        lib.refresh_queue()?;
        lib.derive_usage();
        if !lib.chapters.is_empty() {
            lib.open_chapter(0)?;
        }
        Ok(lib)
    }

    fn build_page(
        index: usize,
        chapters: &[ChapterRow],
        stats: &[ChapterStats],
        terms: &[TermInfo],
        finder: &SourceFinder,
        book: &Book,
    ) -> Result<Page> {
        let row = &chapters[index];
        let ch = book.chapter(row.id)?;
        let cx = page::Context {
            terms,
            finder,
            stats: &stats[index],
        };
        Ok(page::build(index, row.number, &ch, &cx))
    }

    /// Re-reads the Job queue and the Suggestions after the worker wrote something.
    pub fn sync(&mut self) -> Result<()> {
        self.refresh_queue()
    }

    /// Queue places, failure reasons and the open Suggestion count: what the Job table and the
    /// Suggestions say, which the Chapter summaries do not carry.
    fn refresh_queue(&mut self) -> Result<()> {
        let jobs = self.book.jobs(false)?;
        let mut queued: Vec<ChapterId> = Vec::new();
        let mut made: std::collections::HashMap<ChapterId, MadeBy> = Default::default();
        for job in jobs
            .iter()
            .filter(|j| j.kind == JobKind::Translate && j.state == JobState::Done)
        {
            if let Some(c) = job.chapter {
                made.insert(
                    c,
                    MadeBy {
                        model: job.model.clone(),
                        tokens_in: job.tokens_in,
                        tokens_out: job.tokens_out,
                        at: job.updated_at,
                    },
                );
            }
        }
        for row in &mut self.chapters {
            row.failure = None;
            row.queue_place = None;
            row.made_by = (row.state == ChapterState::Translated)
                .then(|| made.remove(&row.id))
                .flatten();
        }
        for job in jobs.iter().filter(|j| j.kind == JobKind::Translate) {
            let Some(chapter) = job.chapter else { continue };
            match job.state {
                JobState::Queued | JobState::Paused => queued.push(chapter),
                _ => {}
            }
        }
        for row in &mut self.chapters {
            if row.state == ChapterState::Failed {
                row.failure = jobs
                    .iter()
                    .rev()
                    .find(|j| {
                        j.kind == JobKind::Translate
                            && j.chapter == Some(row.id)
                            && j.state == JobState::Failed
                    })
                    .and_then(|j| j.error.clone());
            }
            // A translated Chapter can be queued too: to be translated again.
            if matches!(row.state, ChapterState::Queued | ChapterState::Translated) {
                row.queue_place = queued.iter().position(|&c| c == row.id).map(|p| p + 1);
            }
        }
        self.refresh_suggestions()?;
        Ok(())
    }

    fn derive_usage(&mut self) {
        self.usage = (0..self.terms.len())
            .map(|t| {
                let mut u = TermUsage::default();
                for (c, s) in self.stats.iter().enumerate() {
                    let n = s.counts.get(t).copied().unwrap_or(0);
                    u.total += n;
                    u.chapters += (n > 0) as usize;
                    if s.drifts(t) {
                        u.drift_chapters.push(c);
                    }
                }
                u
            })
            .collect();
    }

    /// Makes `index` the current Chapter.
    /// Does nothing in a Book with no Chapters.
    pub fn open_chapter(&mut self, index: usize) -> Result<()> {
        if self.chapters.is_empty() {
            return Ok(());
        }
        let index = index.min(self.chapters.len() - 1);
        self.near.remove(&index);
        if !self.scanned {
            // The scan hasn't reached it: count this one now, so its page and Terms are whole.
            let ch = self.book.chapter(self.chapters[index].id)?;
            self.chapters[index].opening = opening_line(&ch);
            self.stats[index] = stats::compute(&ch, &self.terms, &self.check, &self.pre);
        }
        self.page = Self::build_page(
            index,
            &self.chapters,
            &self.stats,
            &self.terms,
            &self.finder,
            &self.book,
        )?;
        Ok(())
    }

    /// Re-reads several Chapters after they changed, recomputing only their numbers.
    pub fn refresh_chapters(&mut self, indices: &[usize]) -> Result<()> {
        let summaries = self.book.chapters()?;
        for &index in indices {
            let id = self.chapters[index].id;
            let ch = self.book.chapter(id)?;
            let summary = summaries
                .iter()
                .find(|s| s.id == id)
                .ok_or(Error::NotFound("chapter", id.0))?;
            self.stats[index] = stats::compute(&ch, &self.terms, &self.check, &self.pre);
            let row = &mut self.chapters[index];
            row.state = summary.state;
            row.title = summary.title.clone();
            row.outdated = summary.outdated;
            row.opening = opening_line(&ch);
        }
        self.refresh_queue()?;
        self.derive_usage();
        self.near.retain(|i, _| !indices.contains(i));
        if indices.contains(&self.page.index) {
            self.open_chapter(self.page.index)?;
        }
        Ok(())
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    /// The page of Chapter `index` if it is current or loaded.
    pub fn page_at(&self, index: usize) -> Option<&Page> {
        if index == self.page.index && !self.chapters.is_empty() {
            Some(&self.page)
        } else {
            self.near.get(&index)
        }
    }

    /// Builds the pages of `indices` that are not built yet, and forgets every other page
    /// except the current one.
    pub fn keep_pages(&mut self, indices: &[usize]) -> Result<()> {
        self.near
            .retain(|i, _| indices.contains(i) && *i != self.page.index);
        for &i in indices {
            if i < self.chapters.len() && i != self.page.index && !self.near.contains_key(&i) {
                let page = Self::build_page(
                    i,
                    &self.chapters,
                    &self.stats,
                    &self.terms,
                    &self.finder,
                    &self.book,
                )?;
                self.near.insert(i, page);
            }
        }
        Ok(())
    }

    /// Makes a loaded (or any) Chapter current without rebuilding a page it already has.
    pub fn set_current(&mut self, index: usize) -> Result<()> {
        if index == self.page.index || index >= self.chapters.len() {
            return Ok(());
        }
        let page = match self.near.remove(&index) {
            Some(page) => page,
            None => Self::build_page(
                index,
                &self.chapters,
                &self.stats,
                &self.terms,
                &self.finder,
                &self.book,
            )?,
        };
        let old = std::mem::replace(&mut self.page, page);
        self.near.insert(old.index, old);
        Ok(())
    }

    pub fn current(&self) -> usize {
        self.page.index
    }

    pub fn stats(&self, chapter: usize) -> &ChapterStats {
        &self.stats[chapter]
    }

    pub fn usage(&self, term: usize) -> &TermUsage {
        &self.usage[term]
    }

    /// Uses of `term` in each Chapter, in reading order.
    pub fn counts(&self, term: usize) -> impl Iterator<Item = u32> + '_ {
        self.stats
            .iter()
            .map(move |s| s.counts.get(term).copied().unwrap_or(0))
    }

    pub fn drifting_terms(&self) -> usize {
        self.usage
            .iter()
            .filter(|u| !u.drift_chapters.is_empty())
            .count()
    }

    /// Terms for the Cast page: those drifting in the most Chapters first, then by use.
    pub fn cast_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.terms.len()).collect();
        order.sort_by_key(|&i| {
            (
                // The worst Drift first: in the most Chapters.
                std::cmp::Reverse(self.usage[i].drift_chapters.len()),
                std::cmp::Reverse(self.usage[i].total),
                i,
            )
        });
        order
    }

    /// Chapters with no text at all (`ChapterState::Empty`).
    pub fn empty(&self) -> usize {
        self.chapters
            .iter()
            .filter(|c| c.state == ChapterState::Empty)
            .count()
    }

    pub fn translated(&self) -> usize {
        self.chapters
            .iter()
            .filter(|c| c.state == ChapterState::Translated)
            .count()
    }

    /// Chapters whose number starts with `query`.
    pub fn find_chapters(&self, query: &str) -> Vec<usize> {
        let q = query.trim();
        (0..self.chapters.len())
            .filter(|&i| self.chapters[i].number.to_string().starts_with(q))
            .collect()
    }

    /// Terms whose rendering or source form contains `query`.
    pub fn find_terms(&self, query: &str) -> Vec<usize> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        (0..self.terms.len())
            .filter(|&i| {
                let t = &self.terms[i];
                t.matches_query(&q)
            })
            .collect()
    }

    /// The first translated Block of the current Chapter that uses `term`.
    pub fn first_use(&self, term: usize) -> Option<cipher_book::BlockId> {
        self.page
            .translation
            .iter()
            .find(|b| b.terms.iter().any(|t| t.term == term))
            .map(|b| b.id)
    }
}
