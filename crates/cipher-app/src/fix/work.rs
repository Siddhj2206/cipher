//! The worker thread and its Events, and Reach computed off the UI thread.
//!
//! One worker runs when an Endpoint is current: `cipher_engine::run` on a thread with its own
//! Book handle (WAL lets it write beside the app's). Its Events cross an mpsc channel to the
//! app, which drains them in Rune's `poll`; a forwarding thread wakes the event loop.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use cipher_book::{Book, ChapterId, JobKind, Scope};
use cipher_engine::{Event, Models, Reach, Selection, StopFlag};
use cipher_llm::{Endpoints, Keychain};
use rune::{Tx, Waker};

/// What the worker can do now.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// No Endpoint is set up: Renames work, model work waits.
    NoEndpoint,
    /// The current Endpoint's name.
    Running(String),
    /// An Endpoint is set but could not be connected.
    Unavailable(String),
}

pub enum Msg {
    Event(Event),
    Reach {
        selection: Selection,
        reach: Reach,
    },
    /// An Endpoint test call came back: what it said, or why it failed.
    Tested(Result<String, String>),
    /// A sign-in needs this URL opened in the browser.
    OpenUrl(String),
    /// A sign-in finished: the account line and the plan's models (best first), or why it
    /// failed.
    SignedIn(Result<(String, Vec<String>), String>),
    /// The open Book's opening lines and Term numbers are counted.
    Scanned(Result<crate::library::Scan, String>),
    /// The models an Endpoint's plan offers: (Endpoint id, slugs).
    ModelsListed(String, Result<Vec<String>, String>),
    /// An API-key Endpoint's model list, for the form `id` asking that protocol and base URL.
    ModelsFetched(String, crate::settings::Source, Result<Vec<String>, String>),
    /// How many Chapters an old import's Pairs were re-paired in.
    Realigned(Result<usize, String>),
    /// How the Book names a Term (`Library::evidence`), for the Library generation it was
    /// asked in.
    Evidence(
        cipher_book::TermId,
        u64,
        Result<cipher_engine::Evidence, String>,
    ),
    /// A folder import finished: the new Book and what it holds, or why it failed.
    Imported(Result<(std::path::PathBuf, cipher_engine::ImportReport), String>),
    ShowInFiles {
        source: crate::home::FilesSource,
        result: Result<(), String>,
    },
}

/// Sends to the app from any thread and wakes its event loop.
#[derive(Clone)]
pub struct Outbox {
    tx: Sender<Msg>,
    waker: Option<Waker>,
}

impl Outbox {
    /// `false` once the app has gone.
    pub fn send(&self, msg: Msg) -> bool {
        let sent = self.tx.send(msg).is_ok();
        if sent && let Some(w) = &self.waker {
            w();
        }
        sent
    }
}

pub struct Background {
    inbox: Receiver<Msg>,
    outbox: Sender<Msg>,
    waker: Option<Waker>,
    stop: StopFlag,
    /// The running worker thread, joined before a new one starts.
    worker: Option<JoinHandle<()>>,
    pub status: Status,
}

impl Background {
    pub fn new() -> Background {
        let (outbox, inbox) = channel();
        Background {
            inbox,
            outbox,
            waker: None,
            stop: StopFlag::default(),
            worker: None,
            status: Status::NoEndpoint,
        }
    }

    pub fn outbox(&self) -> Outbox {
        Outbox {
            tx: self.outbox.clone(),
            waker: self.waker.clone(),
        }
    }

    /// Runs `f` on its own thread and sends what it returns to the app.
    pub fn spawn(&self, f: impl FnOnce() -> Msg + Send + 'static) {
        let outbox = self.outbox();
        std::thread::spawn(move || {
            outbox.send(f());
        });
    }

    /// The waker the app's event loop gave; set before anything is spawned.
    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    /// Stops the worker after the call it is making. A later [`Background::restart`] waits for
    /// it, so it never overlaps the next one.
    pub fn stop(&mut self) {
        self.stop.stop();
        self.stop = StopFlag::default();
        self.status = Status::NoEndpoint;
    }

    /// Stops the worker, if one runs, and starts one on the current Endpoint. The new worker
    /// waits for the old one to finish its call, so two never work on the same Job.
    pub fn restart(&mut self, book: &Path) {
        self.stop.stop();
        self.stop = StopFlag::default();
        let old = self.worker.take();
        self.status = Status::NoEndpoint;
        let Some(endpoints) = Endpoints::default_path().and_then(|p| Endpoints::load(&p).ok())
        else {
            return;
        };
        // The Book's own pick of model, else the default Endpoint's.
        let choice = Book::open(book)
            .ok()
            .and_then(|b| b.model_choice().ok().flatten());
        let Some(endpoint) = endpoints.for_book(
            choice
                .as_ref()
                .map(|c| (c.endpoint.as_str(), c.model.as_str())),
        ) else {
            return;
        };
        let endpoint = &endpoint;
        let connection = match cipher_llm::connect(endpoint, Arc::new(Keychain)) {
            Ok(c) => c,
            Err(e) => {
                self.status = Status::Unavailable(e.to_string());
                return;
            }
        };
        self.status = Status::Running(format!("{} · {}", endpoint.name, endpoint.translate_model));
        let models = Models {
            translate: connection.translate,
            small: Some(connection.small),
            reasoning: endpoint.reasoning,
            name: format!("{} · {}", endpoint.name, endpoint.translate_model),
        };
        let (events, worker_rx) = channel::<Event>();
        let (path, stop) = (book.to_path_buf(), self.stop.clone());
        self.worker = Some(std::thread::spawn(move || {
            if let Some(old) = old {
                let _ = old.join();
            }
            cipher_engine::run(&path, models, events, stop)
        }));
        let outbox = self.outbox();
        std::thread::spawn(move || {
            for event in worker_rx {
                if !outbox.send(Msg::Event(event)) {
                    break;
                }
            }
        });
    }

    /// Works out what `selection` reaches Everywhere on its own thread, with its own Book
    /// handle. The result arrives as a [`Msg::Reach`].
    pub fn reach(&self, book: &Path, selection: Selection) {
        let path = book.to_path_buf();
        self.spawn(move || {
            let reach = Book::open(&path)
                .map_err(cipher_engine::Error::from)
                .and_then(|b| cipher_engine::reach(&b, &selection, Scope::Everywhere));
            Msg::Reach {
                selection,
                reach: reach.unwrap_or_default(),
            }
        });
    }

    pub fn drain(&self) -> Vec<Msg> {
        self.inbox.try_iter().collect()
    }
}

impl Default for Background {
    fn default() -> Self {
        Background::new()
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// The running translation, for the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub chapter: ChapterId,
    pub done: usize,
    pub total: usize,
}

/// The queue is waiting: after a usage limit or a transient error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paused {
    /// Unix seconds.
    pub until: i64,
    pub reason: String,
}

impl crate::state::State {
    /// Called from Rune's `poll`: takes what the worker and the other threads sent and re-reads
    /// what changed. Returns whether anything on screen may differ.
    pub fn drain(&mut self, tx: &mut Tx) -> bool {
        self.notifier
            .configure(self.window_focused, self.prefs.notify_when_done);
        let msgs = self.bg.drain();
        if msgs.is_empty() {
            return false;
        }
        let mut reread = Vec::new();
        let mut sync = false;
        for msg in msgs {
            if let Msg::Event(event) = &msg {
                self.notifier.event(event, || {
                    if !self.prefs.notify_show_chapters {
                        return None;
                    }
                    match event {
                        Event::JobFinished {
                            chapter: Some(id), ..
                        } => self.lib.notification_chapter(*id),
                        _ => None,
                    }
                });
            }
            match msg {
                Msg::Reach { selection, reach } => self.on_reach(selection, reach),
                Msg::Tested(result) => self.on_tested(result),
                Msg::SignedIn(result) => self.on_signed_in(result),
                Msg::OpenUrl(url) => self.open_url(tx, &url),
                Msg::Imported(result) => self.on_imported(result),
                Msg::ShowInFiles { source, result } => self.on_show_in_files(source, result),
                Msg::ModelsListed(id, result) => self.on_models_listed(id, result),
                Msg::ModelsFetched(id, source, result) => {
                    self.on_models_fetched(id, source, result)
                }
                Msg::Realigned(Ok(0)) => {}
                Msg::Realigned(Ok(n)) => self.on_realigned(n),
                Msg::Realigned(Err(e)) => {
                    self.toast = Some(format!("Could not re-pair imported chapters: {e}"))
                }
                Msg::Evidence(term, generation, result) => {
                    self.on_evidence(term, generation, result)
                }
                Msg::Scanned(Ok(scan)) => self.lib.apply_scan(scan),
                Msg::Scanned(Err(e)) => self.toast = Some(format!("Could not count Terms: {e}")),
                Msg::Event(event) => match event {
                    Event::JobStarted {
                        kind: JobKind::Translate,
                        chapter: Some(c),
                        ..
                    } => {
                        self.progress = Some(Progress {
                            chapter: c,
                            done: 0,
                            total: 0,
                        });
                        self.paused = None;
                        reread.push(c);
                    }
                    Event::JobStarted { .. } => self.paused = None,
                    Event::Progress {
                        chapter,
                        done,
                        total,
                        ..
                    } => {
                        self.progress = Some(Progress {
                            chapter,
                            done,
                            total,
                        })
                    }
                    Event::Streamed { chapter, entries } => {
                        self.streamed = Some((chapter, entries));
                    }
                    Event::JobFinished {
                        chapter: Some(c), ..
                    } => {
                        self.end_progress(c);
                        reread.push(c);
                    }
                    Event::JobFailed {
                        chapter: Some(c), ..
                    } => {
                        self.end_progress(c);
                        reread.push(c);
                    }
                    Event::JobPaused {
                        resumes_at, reason, ..
                    } => {
                        self.progress = None;
                        self.paused = Some(Paused {
                            until: resumes_at,
                            reason,
                        });
                        sync = true;
                    }
                    Event::JobFinished { job, .. } => {
                        self.on_notes_job_finished(job);
                        sync = true;
                    }
                    Event::JobFailed { .. } | Event::BookChanged { .. } => sync = true,
                },
            }
        }
        reread.sort();
        reread.dedup();
        let at: Vec<usize> = reread
            .into_iter()
            .filter_map(|c| self.lib.chapter_index(c))
            .collect();
        let result = if at.is_empty() {
            if sync { self.lib.sync() } else { Ok(()) }
        } else {
            self.lib.refresh_chapters(&at)
        };
        if let Err(e) = result {
            self.toast = Some(format!("Could not read the Book: {e}"));
        }
        true
    }

    pub fn flush_notifications(&mut self) {
        self.notifier
            .configure(self.window_focused, self.prefs.notify_when_done);
        if self.notifier.pending() {
            self.notifier.flush(
                self.lib.translations_busy().unwrap_or(true),
                self.prefs.notify_show_chapters,
                &mut self.notification_sink,
            );
        }
    }

    /// An old import's position Pairs were re-paired on another thread: read the Book again,
    /// which counts it afresh.
    pub fn on_realigned(&mut self, n: usize) {
        match self.lib.reload() {
            Ok(()) => {
                self.load_strip(self.lib.current());
                self.toast = Some(format!(
                    "Re-paired {} with the source: paragraphs now line up in Interlinear, Drift and Suggestions.",
                    crate::view::plural(n, "imported chapter")
                ));
            }
            Err(e) => self.toast = Some(format!("Could not read the Book: {e}")),
        }
    }

    fn end_progress(&mut self, chapter: ChapterId) {
        if self.progress.is_some_and(|p| p.chapter == chapter) {
            self.progress = None;
        }
        if self.streamed.as_ref().is_some_and(|x| x.0 == chapter) {
            self.streamed = None;
        }
    }
}
