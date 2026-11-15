//! The Job queue. Jobs are operational state, outside Revisions: they live in the Book so
//! work survives sleep and restarts, but undo never touches them.

use rusqlite::{OptionalExtension, Row, TransactionBehavior, params};

use crate::types::*;
use crate::{Book, Error, Result, now};

const COLUMNS: &str = "id, kind, parent_id, chapter_id, suggestion_id, state, input, partial, \
                       attempts, tokens_in, tokens_out, error, resume_after, updated_at, model";

impl Book {
    /// Adds a Job in state `queued`.
    pub fn enqueue(&mut self, spec: JobSpec) -> Result<JobId> {
        self.conn.execute(
            "INSERT INTO job (kind, parent_id, chapter_id, suggestion_id, state, input, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6)",
            params![
                spec.kind.as_str(),
                spec.parent.map(|j| j.0),
                spec.chapter.map(|c| c.0),
                spec.suggestion.map(|s| s.0),
                spec.input,
                now(),
            ],
        )?;
        Ok(JobId(self.conn.last_insert_rowid()))
    }

    /// Atomically takes the next runnable Job, marks it `running` and counts the attempt.
    ///
    /// Runnable means `queued`, or `paused` with `resume_after` in the past. Child Jobs
    /// (Repairs) go before top-level Jobs, then the oldest first, so Chapters run in the
    /// order they were queued.
    pub fn claim_next_job(&mut self) -> Result<Option<Job>> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id: Option<i64> = tx
            .query_row(
                "SELECT id FROM job
                 WHERE state = 'queued' OR (state = 'paused' AND resume_after <= ?1)
                 ORDER BY parent_id IS NULL, id LIMIT 1",
                [now()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = id else { return Ok(None) };
        tx.execute(
            "UPDATE job SET state = 'running', attempts = attempts + 1, updated_at = ?2
             WHERE id = ?1",
            params![id, now()],
        )?;
        let job = tx.query_row(
            &format!("SELECT {COLUMNS} FROM job WHERE id = ?1"),
            [id],
            raw,
        )?;
        tx.commit()?;
        Ok(Some(job.parse()?))
    }

    /// Saves a Job's state, input, partial output, usage, error and pause time.
    pub fn save_job(&mut self, job: &Job) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE job SET state = ?2, input = ?3, partial = ?4, tokens_in = ?5,
               tokens_out = ?6, error = ?7, resume_after = ?8, updated_at = ?9, model = ?10
             WHERE id = ?1",
            params![
                job.id.0,
                job.state.as_str(),
                job.input,
                job.partial,
                job.tokens_in,
                job.tokens_out,
                job.error,
                job.resume_after,
                now(),
                job.model,
            ],
        )?;
        if n == 0 {
            return Err(Error::NotFound("job", job.id.0));
        }
        Ok(())
    }

    pub fn job(&self, id: JobId) -> Result<Job> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM job WHERE id = ?1"),
                [id.0],
                raw,
            )
            .optional()?
            .ok_or(Error::NotFound("job", id.0))?
            .parse()
    }

    /// Jobs in id order. With `active_only`, only those not `done`.
    pub fn jobs(&self, active_only: bool) -> Result<Vec<Job>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM job WHERE ?1 = 0 OR state != 'done' ORDER BY id"
        ))?;
        let rows = stmt
            .query_map([active_only], raw)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(RawJob::parse).collect()
    }

    /// Puts a failed or paused Job back in the queue now, with a fresh attempt count. Its
    /// saved progress is kept, so a translation resumes where it stopped.
    pub fn retry_job(&mut self, id: JobId) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE job SET state = 'queued', attempts = 0, error = NULL, resume_after = NULL,
               updated_at = ?2
             WHERE id = ?1 AND state IN ('failed', 'paused')",
            params![id.0, now()],
        )?;
        if n == 0 {
            return Err(Error::Invalid(format!(
                "job {} is not failed or paused",
                id.0
            )));
        }
        Ok(())
    }

    /// Puts Jobs left `running` or `checking` by a worker that died back in the queue,
    /// keeping their partial output. Call once when a worker starts. Returns how many.
    pub fn requeue_interrupted(&mut self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE job SET state = 'queued', updated_at = ?1
             WHERE state IN ('running', 'checking')",
            [now()],
        )?)
    }
}

struct RawJob {
    job: Job,
    kind: String,
    state: String,
}

impl RawJob {
    fn parse(mut self) -> Result<Job> {
        self.job.kind = JobKind::parse(&self.kind)?;
        self.job.state = JobState::parse(&self.state)?;
        Ok(self.job)
    }
}

fn raw(r: &Row<'_>) -> rusqlite::Result<RawJob> {
    Ok(RawJob {
        kind: r.get(1)?,
        state: r.get(5)?,
        job: Job {
            id: JobId(r.get(0)?),
            kind: JobKind::Translate,
            parent: r.get::<_, Option<i64>>(2)?.map(JobId),
            chapter: r.get::<_, Option<i64>>(3)?.map(ChapterId),
            suggestion: r.get::<_, Option<i64>>(4)?.map(SuggestionId),
            state: JobState::Queued,
            input: r.get(6)?,
            partial: r.get(7)?,
            attempts: r.get(8)?,
            tokens_in: r.get(9)?,
            tokens_out: r.get(10)?,
            error: r.get(11)?,
            resume_after: r.get(12)?,
            updated_at: r.get(13)?,
            model: r.get(14)?,
        },
    })
}
