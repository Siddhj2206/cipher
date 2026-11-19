//! The translate pipeline through `Worker::step`, with a scripted model.

use cipher_book::*;
use cipher_engine::{Event, Models, Worker};
use cipher_llm::{Fake, FakeReply, LlmError};

struct Setup {
    _dir: tempfile::TempDir,
    worker: Worker,
    fake: Fake,
    chapter: ChapterId,
}

const SOURCE: [&str; 4] = [
    "마탑주가 탑에 올랐다.",
    "바람이 불었다.",
    "\u{201c}이제 시작이군.\u{201d}",
    "아르젠트는 웃었다.",
];

fn setup(script: Vec<FakeReply>) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let mut book = Book::create(
        dir.path().join("t.cipher"),
        BookMeta {
            title: "T".into(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        },
    )
    .unwrap();
    let chapter = ChapterId(
        book.apply(Change::AddChapter {
            number: Some(1),
            title: None,
            source: SOURCE.iter().map(|s| s.to_string()).collect(),
        })
        .unwrap()
        .created[0],
    );
    let term = TermId(
        book.apply(Change::UpsertTerm {
            id: None,
            fields: TermFields {
                source: "마탑주".into(),
                rendering: "Magic Tower Lord".into(),
                kind: Some("title".into()),
                ..Default::default()
            },
            status: TermStatus::Accepted,
            first_chapter: None,
        })
        .unwrap()
        .created[0],
    );
    // Renaming makes "Magic Tower Lord" a Forbidden Rendering.
    book.apply(Change::UpsertTerm {
        id: Some(term),
        fields: TermFields {
            source: "마탑주".into(),
            rendering: "Tower Master".into(),
            kind: Some("title".into()),
            ..Default::default()
        },
        status: TermStatus::Accepted,
        first_chapter: None,
    })
    .unwrap();
    cipher_engine::translate(&mut book, &[chapter]).unwrap();
    let fake = Fake::new(script);
    let worker = Worker::new(book, Models::new(Box::new(fake.clone()))).unwrap();
    Setup {
        _dir: dir,
        worker,
        fake,
        chapter,
    }
}

fn step(w: &mut Worker) -> Vec<Event> {
    let mut events = Vec::new();
    assert!(
        w.step(&mut |e| events.push(e)).unwrap(),
        "a Job should have run"
    );
    events
}

fn translation(w: &Worker, ch: ChapterId) -> Vec<String> {
    w.book()
        .chapter(ch)
        .unwrap()
        .translation
        .into_iter()
        .map(|b| b.text)
        .collect()
}

const CLEAN: &str = "[1] The Tower Master climbed the tower.

[2] The wind blew.

[3] \u{201c}So it begins.\u{201d}

[4] Argent smiled.

NEW_TERMS:
아르젠트 | Argent | name | male
NOTES:
The Tower Master climbs the tower; Argent appears.";

#[test]
fn clean_run_writes_pairs_terms_and_notes() {
    let Setup {
        mut worker,
        fake,
        chapter,
        _dir,
    } = setup(vec![FakeReply::Text(CLEAN.into())]);
    let events = step(&mut worker);
    assert!(
        matches!(events.last(), Some(Event::JobFinished { .. })),
        "{events:?}"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Progress { .. })));

    assert_eq!(
        translation(&worker, chapter),
        [
            "The Tower Master climbed the tower.",
            "The wind blew.",
            "\u{201c}So it begins.\u{201d}",
            "Argent smiled."
        ]
    );
    let book = worker.book();
    let c = book.chapter(chapter).unwrap();
    assert_eq!(c.pairs.len(), 4);
    assert!(c.pairs.iter().all(|p| p.flagged.is_none()));

    let pending: Vec<_> = book
        .glossary()
        .unwrap()
        .terms
        .into_iter()
        .filter(|t| t.status == TermStatus::Pending)
        .collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].fields.rendering, "Argent");
    assert_eq!(pending[0].fields.gender.as_deref(), Some("male"));
    assert_eq!(
        book.story_notes().unwrap()[0].text,
        "The Tower Master climbs the tower; Argent appears."
    );

    // The prompt sent only the Term the Chapter uses, with its Forbidden Rendering.
    let req = &fake.requests()[0];
    assert!(
        req.input
            .contains("마탑주 = Tower Master (title; never: Magic Tower Lord)"),
        "{}",
        req.input
    );
    assert!(req.input.contains("[4] 아르젠트는 웃었다."));

    let job = &book.jobs(false).unwrap()[0];
    assert_eq!(job.state, JobState::Done);
    assert!(job.partial.is_empty() && job.input.is_none());
    assert_eq!(book.chapters().unwrap()[0].state, ChapterState::Translated);
}

#[test]
fn edited_address_and_story_note_reach_the_next_translation_prompt() {
    let Setup {
        _dir,
        worker,
        fake,
        chapter,
    } = setup(vec![FakeReply::Text(CLEAN.into())]);
    let mut book = Book::open(worker.book().path()).unwrap();
    drop(worker);
    let address = AddressId(
        book.apply(Change::AddFormOfAddress(AddressFields {
            speaker: "Old speaker".into(),
            listener: "Old listener".into(),
            source_form: "Old form".into(),
            rendering: "Old rendering".into(),
        }))
        .unwrap()
        .created[0],
    );
    let note = StoryNoteId(
        book.apply(Change::AddStoryNote {
            chapter: None,
            text: "Old continuity note".into(),
        })
        .unwrap()
        .created[0],
    );
    book.apply(Change::EditFormOfAddress {
        id: address,
        fields: Some(AddressFields {
            speaker: "Argent".into(),
            listener: "Tower Master".into(),
            source_form: "스승님".into(),
            rendering: "Master".into(),
        }),
    })
    .unwrap();
    book.apply(Change::EditStoryNote {
        id: note,
        text: Some("Argent has joined the tower.".into()),
    })
    .unwrap();
    let mut worker = Worker::new(book, Models::new(Box::new(fake.clone()))).unwrap();
    step(&mut worker);
    assert!(!translation(&worker, chapter).is_empty());
    let requests = fake.requests();
    let input = &requests[0].input;
    assert!(
        input.contains("- Argent to Tower Master: 스승님 = Master"),
        "{input}"
    );
    assert!(input.contains("Argent has joined the tower."), "{input}");
    for old in [
        "Old speaker",
        "Old listener",
        "Old form",
        "Old rendering",
        "Old continuity note",
    ] {
        assert!(!input.contains(old), "{input}");
    }
}

#[test]
fn a_cut_off_stream_resumes_with_only_the_missing_numbers() {
    let Setup { mut worker, fake, chapter, _dir } = setup(vec![
        // Cut off inside [3]: [3] can't be trusted, so it is asked for again.
        FakeReply::CutOff("[1] The Tower Master climbed the tower.\n\n[2] The wind blew.\n\n[3] \u{201c}So it".into()),
        FakeReply::Text("[3] \u{201c}So it begins.\u{201d}\n\n[4] Argent smiled.\n\nNEW_TERMS:\nNOTES:\nA start.".into()),
    ]);
    step(&mut worker);
    assert_eq!(translation(&worker, chapter).len(), 4);
    assert_eq!(
        translation(&worker, chapter)[2],
        "\u{201c}So it begins.\u{201d}"
    );

    let second = &fake.requests()[1];
    assert!(second.input.contains("SOURCE:\n[3] "), "{}", second.input);
    assert!(
        !second.input.contains("[1] 마탑주"),
        "already translated Blocks are not resent"
    );
    assert!(
        second.input.contains("[2] The wind blew."),
        "neighbours go along as context"
    );
}

#[test]
fn a_dropped_connection_keeps_the_partial_for_the_retry() {
    let Setup {
        mut worker,
        fake,
        chapter,
        _dir,
    } = setup(vec![
        FakeReply::Fail(LlmError::Network("reset".into())),
        FakeReply::Text(CLEAN.into()),
    ]);
    let events = step(&mut worker);
    assert!(
        matches!(events.last(), Some(Event::JobPaused { .. })),
        "{events:?}"
    );
    let mut job = worker.book().jobs(false).unwrap().remove(0);
    assert_eq!(job.state, JobState::Paused);

    // Let it run now instead of in 10 seconds.
    job.resume_after = Some(0);
    let mut book = Book::open(worker.book().path()).unwrap();
    book.save_job(&job).unwrap();
    step(&mut worker);
    assert_eq!(translation(&worker, chapter).len(), 4);
    assert_eq!(fake.requests().len(), 2);
}

#[test]
fn a_missing_range_is_re_requested() {
    let Setup { mut worker, fake, chapter, _dir } = setup(vec![
        FakeReply::Text("[1] The Tower Master climbed the tower.\n\n[4] Argent smiled.\n\nNEW_TERMS:\nNOTES:\nx".into()),
        FakeReply::Text("[2] The wind blew.\n\n[3] \u{201c}So it begins.\u{201d}".into()),
    ]);
    step(&mut worker);
    assert_eq!(translation(&worker, chapter).len(), 4);
    let second = &fake.requests()[1].input;
    assert!(
        second.contains("SOURCE:\n[2] 바람이 불었다.\n\n[3] "),
        "{second}"
    );
}

#[test]
fn merges_become_one_pair() {
    let Setup { mut worker, chapter, _dir, .. } = setup(vec![FakeReply::Text(
        "[1] The Tower Master climbed the tower.\n\n[2-3] The wind blew. \u{201c}So it begins.\u{201d}\n\n[4] Argent smiled.".into(),
    )]);
    step(&mut worker);
    let c = worker.book().chapter(chapter).unwrap();
    assert_eq!(c.pairs.len(), 3);
    assert_eq!(c.blocks_in(c.pairs[1].id, Side::Source).count(), 2);
}

#[test]
fn a_forbidden_rendering_is_fixed_locally_as_its_own_revision() {
    let Setup { mut worker, fake, chapter, _dir } = setup(vec![FakeReply::Text(
        "[1] The Magic Tower Lord climbed the tower.\n\n[2] The wind blew.\n\n[3] \u{201c}So it begins.\u{201d}\n\n[4] Argent smiled.\n\nNOTES:\nThe Magic Tower Lord climbs.".into(),
    )]);
    step(&mut worker);
    assert_eq!(fake.requests().len(), 1, "zero tokens: no repair call");
    assert_eq!(
        translation(&worker, chapter)[0],
        "The Tower Master climbed the tower."
    );
    assert_eq!(
        worker.book().story_notes().unwrap()[0].text,
        "The Tower Master climbs.",
        "old names don't enter notes"
    );

    let revisions = worker.book().revisions(2).unwrap();
    assert_eq!(revisions[0].label, "Fixed 1 paragraph with old names");
    // The fix can be undone on its own.
    let mut book = Book::open(worker.book().path()).unwrap();
    assert!(matches!(
        book.undo(revisions[0].id, UndoMode::Whole).unwrap(),
        UndoOutcome::Undone { .. }
    ));
    assert_eq!(
        book.chapter(chapter).unwrap().translation[0].text,
        "The Magic Tower Lord climbed the tower."
    );
}

#[test]
fn leftover_script_is_repaired_by_one_small_call() {
    let Setup { mut worker, fake, chapter, _dir } = setup(vec![
        FakeReply::Text("[1] The Tower Master climbed the tower.\n\n[2] The 바람 blew.\n\n[3] \u{201c}So it begins.\u{201d}\n\n[4] Argent smiled.".into()),
        FakeReply::Text("[2] The wind blew.".into()),
    ]);
    step(&mut worker);
    assert_eq!(translation(&worker, chapter)[1], "The wind blew.");
    let repair = &fake.requests()[1];
    assert!(
        repair
            .input
            .contains("PROBLEM: untranslated text remains: \"바람\""),
        "{}",
        repair.input
    );
    assert!(
        !repair.input.contains("[1]"),
        "only the failing Pair is sent"
    );
    assert!(
        worker
            .book()
            .chapter(chapter)
            .unwrap()
            .pairs
            .iter()
            .all(|p| p.flagged.is_none())
    );
}

#[test]
fn a_pair_that_stays_broken_is_flagged() {
    let bad = "[1] The Tower Master climbed the tower.\n\n[2] The 바람 blew.\n\n[3] \u{201c}So it begins.\u{201d}\n\n[4] Argent smiled.";
    let Setup {
        mut worker,
        chapter,
        _dir,
        ..
    } = setup(vec![
        FakeReply::Text(bad.into()),
        FakeReply::Text("[2] The 바람 blew again.".into()),
    ]);
    step(&mut worker);
    let c = worker.book().chapter(chapter).unwrap();
    assert!(c.pairs[1].flagged.as_deref().unwrap().contains("바람"));
}

#[test]
fn a_usage_limit_pauses_until_the_reset() {
    let reset = std::time::UNIX_EPOCH + std::time::Duration::from_secs(4_000_000_000);
    let Setup {
        mut worker, _dir, ..
    } = setup(vec![FakeReply::Fail(LlmError::UsageLimit {
        resets_at: Some(reset),
    })]);
    let events = step(&mut worker);
    assert!(
        matches!(
            events.last(),
            Some(Event::JobPaused {
                resumes_at: 4_000_000_000,
                ..
            })
        ),
        "{events:?}"
    );
    assert!(
        !worker.step(&mut |_| {}).unwrap(),
        "nothing runs before the reset"
    );
}

#[test]
fn bad_credentials_fail_the_job() {
    let Setup {
        mut worker, _dir, ..
    } = setup(vec![FakeReply::Fail(LlmError::Auth)]);
    let events = step(&mut worker);
    assert!(
        matches!(events.last(), Some(Event::JobFailed { .. })),
        "{events:?}"
    );
    assert_eq!(
        worker.book().chapters().unwrap()[0].state,
        ChapterState::Failed
    );
}

#[test]
fn story_notes_compact_when_they_outgrow_the_budget() {
    let long = "Something happened that matters later. ".repeat(80); // ~3,200 chars
    let Setup {
        mut worker,
        fake,
        _dir,
        ..
    } = setup(vec![
        FakeReply::Text(format!("{CLEAN}\n{long}")),
        FakeReply::Text("Summary: the Magic Tower Lord rose.".into()),
    ]);
    step(&mut worker); // translate, which queues the compaction
    let events = step(&mut worker);
    assert!(
        matches!(events.last(), Some(Event::JobFinished { .. })),
        "{events:?}"
    );
    let notes = worker.book().story_notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].chapter, None);
    assert_eq!(
        notes[0].text, "Summary: the Tower Master rose.",
        "old names are fixed in summaries too"
    );
    assert!(fake.requests()[1].instructions.contains("summary"));
}
