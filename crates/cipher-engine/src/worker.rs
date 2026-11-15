//! Claims Jobs and turns model failures into Job states.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cipher_book::{Book, Job, JobKind, JobState};
use cipher_llm::LlmError;

use crate::{Event, Models, Result, StopFlag, translate};

/// Transient failures (network, server, rate limits) are retried this many times in all.
const MAX_ATTEMPTS: i64 = 4;

/// Runs Jobs from one Book.
pub struct Worker {
    pub(crate) book: Book,
    pub(crate) models: Models,
}

/// How a Job's run ended, before it is turned into a Job state.
pub(crate) enum Outcome {
    Done(cipher_llm::Usage),
    Failed(String),
    Model(LlmError),
}

impl Worker {
    /// Opens the worker on a Book. Jobs a previous worker left running go back in the queue.
    pub fn new(mut book: Book, models: Models) -> Result<Worker> {
        book.requeue_interrupted()?;
        Ok(Worker { book, models })
    }

    pub fn book(&self) -> &Book {
        &self.book
    }

    /// Claims and runs one Job. Returns `false` when nothing is runnable.
    pub fn step(&mut self, emit: &mut dyn FnMut(Event)) -> Result<bool> {
        let Some(mut job) = self.book.claim_next_job()? else {
            return Ok(false);
        };
        emit(Event::JobStarted {
            job: job.id,
            kind: job.kind,
            chapter: job.chapter,
        });
        let outcome = match job.kind {
            JobKind::Translate => translate::run(self, &mut job, emit)?,
            JobKind::CompactNotes => translate::compact_notes(self)?,
            JobKind::SuggestBatch => crate::suggest::run_batch(self, &mut job, emit)?,
            JobKind::ResolveSource => crate::suggest::run_resolve(self, &mut job, emit)?,
            JobKind::Repair => Outcome::Failed("repairs run inside translate Jobs".into()),
        };
        self.settle(job, outcome, emit)?;
        Ok(true)
    }

    fn settle(
        &mut self,
        mut job: Job,
        outcome: Outcome,
        emit: &mut dyn FnMut(Event),
    ) -> Result<()> {
        match outcome {
            Outcome::Done(usage) => {
                job.state = JobState::Done;
                if !self.models.name.is_empty() {
                    job.model = Some(self.models.name.clone());
                }
                // A Job that didn't record its own usage (Suggestion batches) takes the call's.
                if job.tokens_in.is_none() && (usage.input > 0 || usage.output > 0) {
                    job.tokens_in = Some(usage.input as i64);
                    job.tokens_out = Some(usage.output as i64);
                }
                job.error = None;
                job.partial.clear();
                self.book.save_job(&job)?;
                emit(Event::JobFinished {
                    job: job.id,
                    chapter: job.chapter,
                    usage,
                });
            }
            Outcome::Failed(error) => self.fail(job, error, emit)?,
            Outcome::Model(e) => {
                let retry = |secs: u64| now() + secs as i64;
                let pause = match &e {
                    LlmError::UsageLimit { resets_at } => Some(
                        resets_at
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map_or(retry(3600), |d| d.as_secs() as i64),
                    ),
                    _ if job.attempts >= MAX_ATTEMPTS => None,
                    LlmError::RateLimited { retry_after } => {
                        Some(retry(retry_after.map_or(30, |d| d.as_secs().clamp(1, 600))))
                    }
                    LlmError::Network(_) | LlmError::Server(_) => {
                        Some(retry(10 * job.attempts as u64))
                    }
                    LlmError::Auth | LlmError::Rejected(_) => None,
                };
                match pause {
                    Some(at) => {
                        job.state = JobState::Paused;
                        job.resume_after = Some(at);
                        job.error = Some(e.to_string());
                        self.book.save_job(&job)?;
                        emit(Event::JobPaused {
                            job: job.id,
                            resumes_at: at,
                            reason: e.to_string(),
                        });
                    }
                    None => self.fail(job, e.to_string(), emit)?,
                }
            }
        }
        Ok(())
    }

    fn fail(&mut self, mut job: Job, error: String, emit: &mut dyn FnMut(Event)) -> Result<()> {
        job.state = JobState::Failed;
        job.error = Some(error.clone());
        self.book.save_job(&job)?;
        emit(Event::JobFailed {
            job: job.id,
            chapter: job.chapter,
            error,
        });
        Ok(())
    }
}

/// The worker thread's body: runs Jobs until `stop` is set, sleeping briefly when the
/// queue is empty. Errors from the Book are reported as failed steps and retried.
pub fn run(book_path: &Path, models: Models, events: Sender<Event>, stop: StopFlag) {
    let mut worker = match Book::open(book_path)
        .map_err(Into::into)
        .and_then(|b| Worker::new(b, models))
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("cipher worker: {e}");
            return;
        }
    };
    let mut emit = |e: Event| {
        let _ = events.send(e);
    };
    while !stop.is_stopped() {
        match worker.step(&mut emit) {
            Ok(true) => {}
            Ok(false) => std::thread::sleep(Duration::from_millis(500)),
            Err(e) => {
                eprintln!("cipher worker: {e}");
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
