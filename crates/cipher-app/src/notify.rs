use std::collections::BTreeSet;

use cipher_book::{JobId, JobKind};
use cipher_engine::Event;

pub mod desktop;

#[derive(Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

pub trait Sink {
    fn send(&mut self, notice: Notice);
}

#[derive(Default)]
pub struct Notifier {
    translations: BTreeSet<JobId>,
    pending: Vec<Option<String>>,
    enabled: bool,
    focused: bool,
}

impl Notifier {
    pub fn configure(&mut self, focused: bool, enabled: bool) {
        self.focused = focused;
        self.enabled = enabled;
        if focused || !enabled {
            self.pending.clear();
        }
    }

    pub fn event(&mut self, event: &Event, chapter_name: impl FnOnce() -> Option<String>) {
        match event {
            Event::JobStarted { job, kind, .. } => {
                if *kind == JobKind::Translate {
                    self.translations.insert(*job);
                } else {
                    self.translations.remove(job);
                }
            }
            Event::JobFinished { job, .. } => {
                if self.translations.remove(job) && self.enabled && !self.focused {
                    self.pending.push(chapter_name());
                }
            }
            Event::JobFailed { job, .. } | Event::JobPaused { job, .. } => {
                self.translations.remove(job);
            }
            _ => {}
        }
    }

    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn flush(&mut self, queue_busy: bool, show_chapters: bool, sink: &mut impl Sink) {
        if queue_busy || !self.translations.is_empty() || !self.pending() {
            return;
        }
        let count = self.pending.len();
        let mut body = format!(
            "{count} {} translated",
            if count == 1 { "chapter" } else { "chapters" }
        );
        if show_chapters {
            let names: Vec<_> = self
                .pending
                .iter()
                .filter_map(|n| n.as_deref())
                .take(3)
                .collect();
            if !names.is_empty() {
                body.push_str(": ");
                body.push_str(&names.join(", "));
                if count > 3 {
                    body.push_str(", …");
                }
            }
        }
        self.pending.clear();
        sink.send(Notice {
            title: "Translation finished".into(),
            body,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cipher_book::ChapterId;

    #[derive(Default)]
    struct Recording(Vec<Notice>);

    impl Sink for Recording {
        fn send(&mut self, notice: Notice) {
            self.0.push(notice);
        }
    }

    fn start(n: &mut Notifier, id: i64, kind: JobKind) {
        n.event(
            &Event::JobStarted {
                job: JobId(id),
                kind,
                chapter: Some(ChapterId(id)),
            },
            || None,
        );
    }

    fn finish(n: &mut Notifier, id: i64) {
        n.event(
            &Event::JobFinished {
                job: JobId(id),
                chapter: Some(ChapterId(id)),
                usage: Default::default(),
            },
            || Some("Chapter 7: Secret title".into()),
        );
    }

    fn ready() -> (Notifier, Recording) {
        let mut n = Notifier::default();
        n.configure(false, true);
        (n, Recording::default())
    }

    #[test]
    fn single_translation_and_empty_flush() {
        let (mut n, mut sink) = ready();
        n.flush(false, false, &mut sink);
        assert!(sink.0.is_empty());
        start(&mut n, 1, JobKind::Translate);
        finish(&mut n, 1);
        n.flush(false, false, &mut sink);
        n.flush(false, false, &mut sink);
        assert_eq!(
            sink.0,
            vec![Notice {
                title: "Translation finished".into(),
                body: "1 chapter translated".into()
            }]
        );
    }

    #[test]
    fn focused_or_disabled_is_silent() {
        for (focused, enabled) in [(true, true), (false, false)] {
            let (mut n, mut sink) = ready();
            n.configure(focused, enabled);
            start(&mut n, 1, JobKind::Translate);
            finish(&mut n, 1);
            n.flush(false, true, &mut sink);
            assert!(sink.0.is_empty());
        }
    }

    #[test]
    fn sequential_batch_waits_for_persisted_queue_to_drain() {
        let (mut n, mut sink) = ready();
        for id in 1..=3 {
            start(&mut n, id, JobKind::Translate);
            finish(&mut n, id);
            n.flush(id < 3, false, &mut sink);
            assert_eq!(sink.0.len(), usize::from(id == 3));
        }
        assert_eq!(sink.0[0].body, "3 chapters translated");
    }

    #[test]
    fn paused_failed_and_untracked_finishes_are_silent() {
        let (mut n, mut sink) = ready();
        start(&mut n, 1, JobKind::Translate);
        n.event(
            &Event::JobPaused {
                job: JobId(1),
                resumes_at: 1,
                reason: "limit".into(),
            },
            || None,
        );
        start(&mut n, 2, JobKind::Translate);
        n.event(
            &Event::JobFailed {
                job: JobId(2),
                chapter: Some(ChapterId(2)),
                error: "error".into(),
            },
            || None,
        );
        for id in 1..=3 {
            finish(&mut n, id);
        }
        n.flush(false, true, &mut sink);
        assert!(sink.0.is_empty());
    }

    #[test]
    fn non_translation_kinds_never_notify_even_with_a_chapter() {
        let (mut n, mut sink) = ready();
        for kind in [
            JobKind::SuggestBatch,
            JobKind::ResolveSource,
            JobKind::Repair,
            JobKind::CompactNotes,
        ] {
            start(&mut n, 1, kind);
            finish(&mut n, 1);
        }
        n.flush(false, true, &mut sink);
        assert!(sink.0.is_empty());
    }

    #[test]
    fn regained_focus_or_disabled_pref_drops_pending_batch() {
        for (focused, enabled) in [(true, true), (false, false)] {
            let (mut n, mut sink) = ready();
            start(&mut n, 1, JobKind::Translate);
            finish(&mut n, 1);
            n.flush(true, false, &mut sink);
            n.configure(focused, enabled);
            n.configure(false, true);
            n.flush(false, false, &mut sink);
            assert!(sink.0.is_empty());
        }
    }

    #[test]
    fn chapter_names_are_opt_in() {
        for show in [false, true] {
            let (mut n, mut sink) = ready();
            start(&mut n, 1, JobKind::Translate);
            finish(&mut n, 1);
            n.flush(false, show, &mut sink);
            assert_eq!(sink.0[0].body.contains("Secret title"), show);
            assert_eq!(sink.0[0].body.contains("Chapter 7"), show);
        }
    }

    #[test]
    fn missing_names_still_notify_and_long_batches_are_bounded() {
        let (mut n, mut sink) = ready();
        for id in 1..=5 {
            start(&mut n, id, JobKind::Translate);
            finish(&mut n, id);
        }
        n.flush(false, true, &mut sink);
        assert!(sink.0[0].body.ends_with(", …"));
        assert_eq!(sink.0[0].body.matches("Secret title").count(), 3);
        start(&mut n, 6, JobKind::Translate);
        n.event(
            &Event::JobFinished {
                job: JobId(6),
                chapter: None,
                usage: Default::default(),
            },
            || None,
        );
        n.flush(false, true, &mut sink);
        assert_eq!(sink.0[1].body, "1 chapter translated");
    }
}
