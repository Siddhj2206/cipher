//! Adding Chapters and driving the Job queue: the paste sheet's reading of the clipboard,
//! "Add and translate", "Translate now" and Retry.

use cipher_book::{Change, ChapterId, Error, JobKind, JobState, Result};
use cipher_text::{blocks, heading};

use super::{ChapterRow, Library, opening_line, stats};

fn marker_heading(block: &str) -> Option<i64> {
    let number = i64::from(heading::chapter_heading(block)?.number?);
    let line = block.trim().trim_start_matches('#').trim();
    let lower = line.to_lowercase();
    let rest = line.trim_start_matches(|c: char| c.is_ascii_digit()).trim();
    (lower.starts_with("chapter")
        || lower.starts_with("ch.")
        || line.starts_with(['제', '第'])
        || (rest.is_empty() && !block.trim_start().starts_with('#'))
        || rest.starts_with(['화', '話', '章', '장', '回']))
    .then_some(number)
}

fn markdown_heading_level(block: &str) -> Option<usize> {
    let heading = heading::chapter_heading(block)?;
    if marker_heading(block).is_some() {
        return None;
    }
    let level = block.trim_start().chars().take_while(|&c| c == '#').count();
    let line = block.trim().trim_start_matches('#').trim();
    let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
    (level > 0
        && (heading.title.is_some()
            || rest
                .chars()
                .all(|c| c.is_whitespace() || matches!(c, '.' | ':' | ')'))))
    .then_some(level)
}

fn insertion_order<'a>(parts: &[&'a IntakePart], ordered: &mut Vec<&'a IntakePart>) {
    if parts.is_empty() {
        return;
    }
    let middle = parts.len() / 2;
    let number = parts[middle].intake.number;
    let first = parts.partition_point(|p| p.intake.number < number);
    let end = parts.partition_point(|p| p.intake.number <= number);
    ordered.extend_from_slice(&parts[first..end]);
    insertion_order(&parts[..first], ordered);
    insertion_order(&parts[end..], ordered);
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeIssue {
    NoNumber,
    Duplicate,
    InBook,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakePart {
    pub key: (usize, usize),
    pub source: Option<String>,
    pub intake: Intake,
    pub issue: Option<IntakeIssue>,
    pub include: bool,
    pub added: bool,
}

impl Library {
    /// Splits only at numbered heading Blocks, leaving single-Chapter reading unchanged.
    pub fn intakes(&self, text: &str) -> Vec<Intake> {
        let blocks = blocks::split(text);
        let level = blocks
            .iter()
            .filter_map(|b| markdown_heading_level(b))
            .min();
        let heads: Vec<_> = blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                (marker_heading(b).is_some()
                    || level.is_some_and(|level| markdown_heading_level(b) == Some(level)))
                .then_some(i)
            })
            .collect();
        if heads.len() < 2 {
            return self.intake(text).into_iter().collect();
        }
        let mut starts = heads;
        if starts[0] != 0 {
            starts.insert(0, 0);
        }
        starts.push(blocks.len());
        let mut numbers: Vec<_> = self.chapters.iter().map(|c| c.number).collect();
        starts
            .windows(2)
            .map(|range| {
                let part = &blocks[range[0]..range[1]];
                let mut intake = self.intake(&part.join("\n\n")).expect("nonempty part");
                intake.number = heading::chapter_heading(&part[0])
                    .and_then(|h| h.number)
                    .map(i64::from);
                intake.after = match intake.number {
                    Some(n) => numbers.iter().copied().filter(|&m| m < n).max(),
                    None => numbers.iter().copied().max(),
                };
                intake.exists = intake.number.is_some_and(|n| numbers.contains(&n));
                if let Some(n) = intake.number {
                    numbers.push(n);
                }
                intake
            })
            .collect()
    }

    /// Files retain pick order as a tie-breaker; unnumbered parts go last.
    pub fn intake_plan(&self, files: &[(String, String)], text: &str) -> Vec<IntakePart> {
        self.intake_plan_with_numbers(files, text, &Default::default())
    }

    pub fn intake_plan_with_numbers(
        &self,
        files: &[(String, String)],
        text: &str,
        restored: &std::collections::BTreeMap<(usize, usize), i64>,
    ) -> Vec<IntakePart> {
        let mut parts = Vec::new();
        for (source, (label, text)) in files
            .iter()
            .map(|(label, text)| (Some(label), text.as_str()))
            .chain(std::iter::once((None, text)))
            .enumerate()
        {
            for (index, mut intake) in self.intakes(text).into_iter().enumerate() {
                let key = (source, index);
                let issue = intake.number.is_none().then_some(IntakeIssue::NoNumber);
                if let Some(&number) = restored.get(&key) {
                    intake.number = Some(number);
                }
                parts.push(IntakePart {
                    key,
                    source: label.cloned(),
                    intake,
                    issue,
                    include: true,
                    added: false,
                });
            }
        }
        let mut next = self
            .chapters
            .iter()
            .map(|c| c.number)
            .chain(parts.iter().filter_map(|p| p.intake.number))
            .max()
            .unwrap_or(0);
        let mut seen = std::collections::HashSet::new();
        for part in &mut parts {
            let n = *part.intake.number.get_or_insert_with(|| {
                next += 1;
                next
            });
            part.intake.exists = self.chapters.iter().any(|c| c.number == n);
            part.issue = if !seen.insert(n) {
                Some(IntakeIssue::Duplicate)
            } else if part.intake.exists {
                Some(IntakeIssue::InBook)
            } else {
                part.issue
            };
            part.include = !matches!(
                part.issue,
                Some(IntakeIssue::Duplicate | IntakeIssue::InBook)
            );
        }
        parts.sort_by_key(|p| p.intake.number);
        let mut numbers: Vec<_> = self.chapters.iter().map(|c| c.number).collect();
        for part in &mut parts {
            let n = part.intake.number.unwrap();
            part.intake.after = numbers.iter().copied().filter(|&m| m < n).max();
            numbers.push(n);
        }
        parts
    }

    /// Adds included parts, then submits one ordered translation request.
    pub fn add_intakes(&mut self, parts: &[IntakePart]) -> Result<Option<usize>> {
        self.add_intakes_with(parts, Self::create_chapter)
    }

    pub(super) fn add_intakes_with(
        &mut self,
        parts: &[IntakePart],
        create: impl FnMut(&mut Self, &Intake) -> Result<ChapterId>,
    ) -> Result<Option<usize>> {
        self.add_intakes_using(parts, create, |book, ids| {
            cipher_engine::translate(book, ids).map_err(super::engine_error)
        })
    }

    pub(super) fn add_intakes_using(
        &mut self,
        parts: &[IntakePart],
        mut create: impl FnMut(&mut Self, &Intake) -> Result<ChapterId>,
        queue: impl FnOnce(&mut cipher_book::Book, &[ChapterId]) -> Result<()>,
    ) -> Result<Option<usize>> {
        let mut included: Vec<_> = parts.iter().filter(|p| p.include).collect();
        if included.iter().any(|p| p.intake.blocks.is_empty()) {
            return Err(Error::Invalid(
                "the pasted Chapter has no paragraphs".into(),
            ));
        }
        let mut ids = Vec::new();
        let mut failure = None;
        included.sort_by_key(|p| p.intake.number);
        let mut ordered = Vec::with_capacity(included.len());
        insertion_order(&included, &mut ordered);
        for part in ordered {
            match create(self, &part.intake) {
                Ok(id) => ids.push(id),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if ids.is_empty() {
            return failure.map_or(Ok(None), Err);
        }
        let chapters = self.book.chapters()?;
        ids.sort_by_key(|id| chapters.iter().position(|c| c.id == *id));
        let queued = queue(&mut self.book, &ids);
        for id in &ids {
            self.insert_row(*id)?;
        }
        let at: Vec<_> = ids
            .iter()
            .map(|id| {
                self.chapters
                    .iter()
                    .position(|c| c.id == *id)
                    .expect("added row")
            })
            .collect();
        self.refresh_chapters(&at)?;
        queued?;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(at.first().copied())
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
        let id = self.create_chapter(intake)?;
        cipher_engine::translate(&mut self.book, &[id]).map_err(super::engine_error)?;
        self.insert_row(id)
    }

    fn create_chapter(&mut self, intake: &Intake) -> Result<ChapterId> {
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
        Ok(id)
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
