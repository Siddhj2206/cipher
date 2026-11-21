use cipher_book::{
    Book, BookMeta, Change, ChapterId, ChapterState, EditState, JobKind, JobSpec, MetadataEditId,
    NewPair, Side, SuggestionFilter, TermFields, TermStatus,
};
use cipher_text::drift::DriftKind;

use super::*;

fn book(dir: &tempfile::TempDir) -> Book {
    Book::create(
        dir.path().join("t.cipher"),
        BookMeta {
            title: "Test".into(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        },
    )
    .unwrap()
}

#[test]
fn notification_chapter_uses_reading_position_not_database_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    chapter(&mut book, 40, &["First"]);
    for title in [None, Some("Unnumbered title".to_string())] {
        book.apply(Change::AddChapter {
            number: None,
            title,
            source: vec!["An unnumbered chapter".into()],
        })
        .unwrap();
    }
    chapter(&mut book, 10, &["Inserted before the others"]);
    let lib = Library::from_book(book).unwrap();
    assert_eq!(
        lib.notification_chapter(lib.chapters[1].id).as_deref(),
        Some("Chapter 40")
    );
    for chapter in &lib.chapters[2..] {
        assert_ne!(chapter.id.0, chapter.number);
        let expected = if chapter.number == 3 {
            "Chapter 3"
        } else {
            "Chapter 4: Unnumbered title"
        };
        assert_eq!(
            lib.notification_chapter(chapter.id).as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn notification_queue_uses_persisted_translation_states() {
    use cipher_book::JobState;

    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::from_book(book(&dir)).unwrap();
    assert!(!lib.translations_busy().unwrap());
    let id = lib
        .book
        .enqueue(JobSpec {
            kind: JobKind::Translate,
            parent: None,
            chapter: None,
            suggestion: None,
            input: None,
        })
        .unwrap();
    let mut job = lib.book.job(id).unwrap();
    for state in [JobState::Queued, JobState::Running, JobState::Checking] {
        job.state = state;
        lib.book.save_job(&job).unwrap();
        assert!(lib.translations_busy().unwrap());
    }
    for state in [JobState::Done, JobState::Paused, JobState::Failed] {
        job.state = state;
        lib.book.save_job(&job).unwrap();
        assert!(!lib.translations_busy().unwrap());
    }
    lib.book
        .enqueue(JobSpec {
            kind: JobKind::SuggestBatch,
            parent: None,
            chapter: None,
            suggestion: None,
            input: None,
        })
        .unwrap();
    assert!(!lib.translations_busy().unwrap());
    assert_eq!(lib.notification_chapter(ChapterId(999)), None);
}

fn term(book: &mut Book, source: &str, rendering: &str, status: TermStatus) -> TermId {
    let applied = book
        .apply(Change::UpsertTerm {
            id: None,
            fields: TermFields {
                source: source.into(),
                rendering: rendering.into(),
                ..TermFields::default()
            },
            status,
            first_chapter: None,
        })
        .unwrap();
    TermId(applied.created[0])
}

fn chapter(book: &mut Book, number: i64, source: &[&str]) -> ChapterId {
    let applied = book
        .apply(Change::AddChapter {
            number: Some(number),
            title: None,
            source: source.iter().map(|s| s.to_string()).collect(),
        })
        .unwrap();
    ChapterId(applied.created[0])
}

/// Translates a Chapter one Pair per source Block.
fn translate(book: &mut Book, ch: ChapterId, translation: &[&str], flag: Option<&str>) {
    let source = book.chapter(ch).unwrap().source;
    let pairs = source
        .iter()
        .zip(translation)
        .map(|(s, t)| NewPair {
            source: vec![s.id],
            translation: vec![t.to_string()],
            flagged: flag.map(String::from),
        })
        .collect();
    book.apply(Change::SetTranslation { chapter: ch, pairs })
        .unwrap();
}

const LORD: &str = "마탑주";

#[test]
fn chapter_menu_blocks_cross_chapter_fix_jobs() {
    use cipher_book::{NewSuggestion, Scope, SuggestionId, SuggestionKind, SuggestionState};

    let (_dir, mut lib) = history_fixture();
    let origin = lib.chapters[0].id;
    let block = lib.book().chapter(origin).unwrap().translation[0].id;
    let suggestion = SuggestionId(
        lib.book_mut()
            .apply(Change::AddSuggestion(NewSuggestion {
                chapter: origin,
                block,
                quote: "opening".into(),
                said: "Fix everywhere".into(),
                scope: Scope::Everywhere,
                is_rename: false,
                source_expr: None,
                state: SuggestionState::Working,
                kind: SuggestionKind::Reader,
                edits: vec![],
            }))
            .unwrap()
            .created[0],
    );
    assert!(!lib.chapter_has_active_job(1));
    let id = lib
        .book_mut()
        .enqueue(JobSpec {
            kind: JobKind::SuggestBatch,
            parent: None,
            chapter: None,
            suggestion: Some(suggestion),
            input: None,
        })
        .unwrap();
    assert!(lib.chapter_has_active_job(1));
    assert!(lib.delete_chapter(1).is_err());
    let mut job = lib.book().job(id).unwrap();
    job.state = cipher_book::JobState::Failed;
    lib.book_mut().save_job(&job).unwrap();
    assert!(!lib.chapter_has_active_job(1));
    lib.delete_chapter(1).unwrap();
}

#[test]
fn delete_reopen_undo_chapter() {
    let (_dir, mut lib) = history_fixture();
    let original = lib.book().chapters().unwrap();
    let index = 1;
    lib.delete_chapter(index).unwrap();
    assert_eq!(lib.chapters[index].id, original[index + 1].id);
    let revision = lib.revisions().unwrap()[0].id;
    let path = lib.book().path().to_owned();
    drop(lib);
    let mut lib = Library::open(&path).unwrap();
    assert!(matches!(
        lib.undo_revision(revision).unwrap(),
        cipher_book::UndoOutcome::Undone { .. }
    ));
    assert_eq!(lib.book().chapters().unwrap(), original);
    assert_eq!(lib.chapters.len(), original.len());
    assert!(lib.chapters.iter().any(|c| c.id == original[index].id));
}

#[test]
fn delete_last_chapters_and_undo_empty_library() {
    let (_dir, mut lib) = history_fixture();
    while !lib.chapters.is_empty() {
        let index = lib.chapters.len() - 1;
        assert!(!lib.chapter_has_active_job(index));
        lib.delete_chapter(index).unwrap();
        if index > 0 {
            assert_eq!(lib.current(), index - 1);
        }
    }
    assert!(lib.delete_chapter(0).is_err());
    let revision = lib.revisions().unwrap()[0].id;
    assert!(matches!(
        lib.undo_revision(revision).unwrap(),
        cipher_book::UndoOutcome::Undone { .. }
    ));
    assert_eq!(lib.chapters.len(), 1);
    assert_eq!(lib.current(), 0);
}

fn history_fixture() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quiet.cipher");
    cipher_engine::import(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/the-quiet-tower"),
        &path,
    )
    .unwrap();
    let lib = Library::open(&path).unwrap();
    (dir, lib)
}

#[test]
fn history_undo_preserves_retried_jobs_with_saved_progress() {
    for (input, partial) in [(Some("Saved request"), ""), (None, "Partial translation")] {
        let mut lib = Library::blank();
        let intake = lib.intake("# Chapter 1\n\nSource paragraph").unwrap();
        lib.add_chapter(&intake).unwrap();
        let revision = lib.revisions().unwrap()[0].id;
        let mut job = lib.book_mut().claim_next_job().unwrap().unwrap();
        job.state = cipher_book::JobState::Failed;
        job.input = input.map(str::to_owned);
        job.partial = partial.into();
        lib.book_mut().save_job(&job).unwrap();
        lib.book_mut().retry_job(job.id).unwrap();
        let saved = lib.book().job(job.id).unwrap();
        assert_eq!(saved.attempts, 0);
        assert_eq!(saved.state, cipher_book::JobState::Queued);
        let preview = lib.preview_undo(revision).unwrap();
        assert!(preview.conflicts.iter().any(|c| c.entity == "job"));
        assert!(matches!(
            lib.undo_revision(revision).unwrap(),
            cipher_book::UndoOutcome::Conflict { .. }
        ));
        let after = lib.book().job(job.id).unwrap();
        assert_eq!(after.input, saved.input);
        assert_eq!(after.partial, saved.partial);
        assert_eq!(lib.chapters.len(), 1);
    }
}

#[test]
fn history_undo_of_added_chapter_removes_unstarted_jobs_but_refuses_running_jobs() {
    for running in [false, true] {
        let mut lib = Library::blank();
        let intake = lib.intake("# Chapter 1\n\nSource paragraph").unwrap();
        lib.add_chapter(&intake).unwrap();
        let revision = lib.revisions().unwrap()[0].id;
        let chapter = lib.chapters[0].id;
        assert_eq!(lib.book().jobs(false).unwrap().len(), 1);
        if running {
            lib.book_mut().claim_next_job().unwrap().unwrap();
        }
        let preview = lib.preview_undo(revision).unwrap();
        assert_eq!(lib.book().jobs(false).unwrap().len(), 1);
        if running {
            assert!(preview.conflicts.iter().any(|c| c.entity == "job"));
            assert!(
                preview
                    .conflict_chapters
                    .iter()
                    .any(|(c, ch)| c.entity == "job" && *ch == chapter)
            );
            assert!(matches!(
                lib.undo_revision(revision).unwrap(),
                cipher_book::UndoOutcome::Conflict { .. }
            ));
            assert_eq!(lib.chapters.len(), 1);
            assert_eq!(lib.book().jobs(false).unwrap().len(), 1);
        } else {
            assert!(preview.conflicts.is_empty(), "{:?}", preview.conflicts);
            assert!(matches!(
                lib.undo_revision(revision).unwrap(),
                cipher_book::UndoOutcome::Undone { .. }
            ));
            assert!(lib.chapters.is_empty());
            assert!(lib.book().jobs(false).unwrap().is_empty());
        }
    }
}

#[test]
fn history_translation_reloads_page_and_whole_conflicts_are_located() {
    use cipher_book::UndoOutcome;
    let (_dir, mut lib) = history_fixture();
    let ch = lib.chapters[0].id;
    let before = lib.book().chapter(ch).unwrap();
    let pairs = before
        .source
        .iter()
        .map(|b| NewPair {
            source: vec![b.id],
            translation: vec!["New translation".into()],
            flagged: None,
        })
        .collect();
    let id = lib
        .book_mut()
        .apply(Change::SetTranslation { chapter: ch, pairs })
        .unwrap()
        .revision;
    assert!(matches!(
        lib.undo_revision(id).unwrap(),
        UndoOutcome::Undone { .. }
    ));
    assert_eq!(
        lib.page()
            .translation
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>(),
        before
            .translation
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
    );
    let pairs = before
        .source
        .iter()
        .map(|b| NewPair {
            source: vec![b.id],
            translation: vec!["Another translation".into()],
            flagged: None,
        })
        .collect();
    let id = lib
        .book_mut()
        .apply(Change::SetTranslation { chapter: ch, pairs })
        .unwrap()
        .revision;
    let block = lib.book().chapter(ch).unwrap().translation[0].id;
    lib.book_mut()
        .apply(Change::EditBlock {
            block,
            text: "Later hand edit".into(),
            hand: true,
        })
        .unwrap();
    let before = lib.book().chapter(ch).unwrap();
    let revisions = lib.revisions().unwrap();
    let preview = lib.preview_undo(id).unwrap();
    assert!(
        preview
            .conflict_chapters
            .iter()
            .any(|(c, chapter)| c.id == block.0 && *chapter == ch)
    );
    assert!(matches!(
        lib.undo_revision(id).unwrap(),
        UndoOutcome::Conflict { .. }
    ));
    assert_eq!(lib.book().chapter(ch).unwrap(), before);
    assert_eq!(lib.revisions().unwrap(), revisions);
}

#[test]
fn history_review_survives_reopening() {
    use cipher_book::{
        EditOrigin, NewProposedEdit, NewSuggestion, Scope, SuggestionId, SuggestionKind,
        SuggestionState, UndoOutcome,
    };
    let (_dir, mut lib) = history_fixture();
    let ch = lib.chapters[0].id;
    let block = lib.book().chapter(ch).unwrap().translation[0].clone();
    let suggestion = lib
        .book_mut()
        .apply(Change::AddSuggestion(NewSuggestion {
            chapter: ch,
            block: block.id,
            quote: block.text.clone(),
            said: "A clearer opening".into(),
            scope: Scope::Here,
            is_rename: false,
            source_expr: None,
            state: SuggestionState::Ready,
            kind: SuggestionKind::Reader,
            edits: vec![NewProposedEdit {
                block: block.id,
                before: block.text.clone(),
                after: Some("Reviewed opening".into()),
                origin: EditOrigin::Model,
                skip_reason: None,
            }],
        }))
        .unwrap();
    let edit = lib
        .book()
        .suggestions(Default::default())
        .unwrap()
        .into_iter()
        .find(|s| s.id == SuggestionId(suggestion.created[0]))
        .unwrap()
        .edits[0]
        .id;
    let revision = lib
        .book_mut()
        .apply(Change::DecideEdits {
            decisions: vec![(edit, true)],
        })
        .unwrap()
        .revision;
    let path = lib.path().to_owned();
    drop(lib);
    let mut lib = Library::open(&path).unwrap();
    assert!(matches!(
        lib.undo_revision(revision).unwrap(),
        UndoOutcome::Undone { .. }
    ));
    assert_eq!(lib.page().translation[0].text, block.text);
    assert_eq!(lib.open_suggestions, 1);
}

#[test]
fn history_skip_conflicts_empty_and_missing_revision() {
    use cipher_book::{RevisionId, RevisionKind, UndoOutcome};
    let (_dir, mut lib) = history_fixture();
    lib.stats[1].counts = vec![987654];
    let untouched = format!("{:?}", lib.stats[1]);
    let ch = lib.chapters[0].id;
    let blocks = lib.book().chapter(ch).unwrap().translation;
    let revision = lib
        .book_mut()
        .apply(Change::Group {
            kind: RevisionKind::HandEdit,
            label: "Two edits".into(),
            changes: blocks[..2]
                .iter()
                .map(|b| Change::EditBlock {
                    block: b.id,
                    text: "Edited".into(),
                    hand: true,
                })
                .collect(),
        })
        .unwrap()
        .revision;
    lib.book_mut()
        .apply(Change::EditBlock {
            block: blocks[0].id,
            text: "Keep later work".into(),
            hand: true,
        })
        .unwrap();
    let outcome = lib.undo_revision(revision).unwrap();
    assert_eq!(format!("{:?}", lib.stats[1]), untouched);
    let UndoOutcome::Undone { skipped, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(skipped.len(), 1);
    assert_eq!(lib.page().translation[0].text, "Keep later work");
    assert_eq!(lib.page().translation[1].text, shown(&blocks[1].text));
    let before = lib.book().chapter(ch).unwrap();
    assert!(matches!(
        lib.undo_revision(revision).unwrap(),
        UndoOutcome::AlreadyUndone { .. }
    ));
    assert!(lib.undo_revision(RevisionId(i64::MAX)).is_err());
    assert_eq!(lib.book().chapter(ch).unwrap(), before);
    let mut empty = Library::blank();
    assert!(empty.revisions().unwrap().is_empty());
    assert!(empty.preview_undo(RevisionId(1)).is_err());
}

#[test]
fn history_repair_import_and_added_chapter_undo() {
    use cipher_book::UndoOutcome;
    let (_dir, mut lib) = history_fixture();
    let ch = lib.chapters[0].id;
    let pairs = lib.book().chapter(ch).unwrap().pairs;
    for pair in pairs {
        lib.book_mut()
            .apply(Change::FlagPair {
                pair: pair.id,
                reason: Some("imported".into()),
            })
            .unwrap();
    }
    let before = lib.book().chapter(ch).unwrap();
    assert_eq!(cipher_engine::realign_imported(lib.book_mut()).unwrap(), 1);
    let revision = lib.revisions().unwrap()[0].id;
    assert!(lib.revisions().unwrap()[0].label.starts_with("Re-paired"));
    assert_eq!(
        undo_mode(&lib.preview_undo(revision).unwrap().revision),
        cipher_book::UndoMode::Whole
    );
    assert!(matches!(
        lib.undo_revision(revision).unwrap(),
        UndoOutcome::Undone { .. }
    ));
    assert_eq!(lib.book().chapter(ch).unwrap(), before);
    let revision = lib
        .book_mut()
        .apply(Change::AddChapter {
            number: Some(99),
            title: None,
            source: vec!["Extra".into()],
        })
        .unwrap()
        .revision;
    assert!(matches!(
        lib.undo_revision(revision).unwrap(),
        UndoOutcome::Undone { .. }
    ));
    assert!(!lib.chapters.iter().any(|c| c.number == 99));
    assert!(!lib.scanned());
    let scan = lib.scan()().unwrap();
    lib.apply_scan(scan);
    assert!(lib.scanned());
}

#[test]
fn history_metadata_review_survives_reopening_and_skips_later_term_edits() {
    for conflict in [false, true] {
        let (_dir, mut lib) = history_fixture();
        let original = lib.book().glossary().unwrap().terms[0].clone();
        lib.book_mut()
            .apply(Change::AddStoryNote {
                chapter: None,
                text: format!("{} watches the tower.", original.fields.rendering),
            })
            .unwrap();
        let glossary = lib.book().glossary().unwrap();
        let notes = lib.book().story_notes().unwrap();
        let at = lib.terms.iter().position(|t| t.id == original.id).unwrap();
        let sid = lib.use_wording(at, "Tower Keeper", &[]).unwrap().unwrap();
        let decisions = lib
            .suggestion(sid)
            .unwrap()
            .metadata
            .iter()
            .map(|e| (e.id, true))
            .collect::<Vec<_>>();
        assert_eq!(decisions.len(), 2);
        let revision = lib.decide_metadata(decisions).unwrap().revision;
        let path = lib.path().to_owned();
        drop(lib);
        let mut lib = Library::open(&path).unwrap();
        assert_ne!(lib.book().glossary().unwrap(), glossary);
        assert_ne!(lib.book().story_notes().unwrap(), notes);
        if conflict {
            let mut fields = original.fields.clone();
            fields.rendering = "Later choice".into();
            lib.book_mut()
                .apply(Change::UpsertTerm {
                    id: Some(original.id),
                    fields,
                    status: original.status,
                    first_chapter: original.first_chapter,
                })
                .unwrap();
        }
        assert!(lib.revisions().unwrap().iter().any(|r| r.id == revision));
        let preview = lib.preview_undo(revision).unwrap();
        assert!(
            preview
                .revision
                .label
                .contains("Accepted glossary change for")
        );
        assert!(preview.changes.contains(&("proposed_metadata", 2)));
        assert!(!preview.chapters.is_empty());
        assert_eq!(
            super::undo_mode(&preview.revision),
            cipher_book::UndoMode::SkipConflicts
        );
        assert_eq!(
            preview.conflicts.iter().any(|c| c.entity == "term"),
            conflict
        );
        if conflict {
            assert!(
                preview
                    .conflict_chapters
                    .iter()
                    .any(|(c, _)| c.entity == "term")
            );
        }
        let cipher_book::UndoOutcome::Undone { skipped, .. } = lib.undo_revision(revision).unwrap()
        else {
            panic!("metadata undo refused");
        };
        assert_eq!(skipped.iter().any(|c| c.entity == "term"), conflict);
        assert_eq!(lib.book().story_notes().unwrap(), notes);
        if conflict {
            assert_eq!(
                lib.terms
                    .iter()
                    .find(|t| t.id == original.id)
                    .unwrap()
                    .rendering,
                "Later choice"
            );
        } else {
            assert_eq!(lib.book().glossary().unwrap(), glossary);
            assert_eq!(
                lib.terms
                    .iter()
                    .find(|t| t.id == original.id)
                    .unwrap()
                    .rendering,
                original.fields.rendering
            );
        }
        for edit in &lib.suggestion(sid).unwrap().metadata {
            let expected =
                if conflict && matches!(edit.change, cipher_book::MetadataChange::Term { .. }) {
                    EditState::Stale
                } else {
                    EditState::Pending
                };
            assert_eq!(edit.state, expected);
        }
        assert!(!lib.scanned());
        let scan = lib.scan()().unwrap();
        lib.apply_scan(scan);
        assert!(lib.scanned());
    }
}

#[test]
fn chapter_labels_use_saved_titles_and_preserve_untitled_fallbacks() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 1, &["바람이 불었다."]);
    let mut lib = Library::from_book(b).unwrap();
    for title in [None, Some(""), Some("  \n"), Some("A lantern in the mist")] {
        let expected = title.filter(|t| !t.trim().is_empty());
        lib.chapters[0].title = title.map(str::to_owned);
        lib.page.title = title.map(str::to_owned);
        assert_eq!(lib.page().title(), expected);
        for opening in ["The wind blew.", ""] {
            lib.chapters[0].opening = opening.into();
            assert_eq!(
                lib.chapters[0].label(),
                expected.unwrap_or(if opening.is_empty() {
                    "Chapter 1"
                } else {
                    opening
                })
            );
        }
    }
}

#[test]
fn draft_title_reaches_reader_and_contents_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::from_book(book(&dir)).unwrap();
    let intake = lib
        .intake("# Chapter 1: A lantern in the mist\n\n그녀는 안개 속에서 등불을 들었다.")
        .unwrap();
    let at = lib.add_chapter(&intake).unwrap();
    lib.open_chapter(at).unwrap();
    assert_eq!(lib.page().title(), Some("A lantern in the mist"));
    assert_eq!(lib.chapters[at].label(), "A lantern in the mist");
    drop(lib);

    let lib = Library::from_book(Book::open(dir.path().join("t.cipher")).unwrap()).unwrap();
    assert_eq!(lib.page().title(), Some("A lantern in the mist"));
    assert_eq!(lib.chapters[0].label(), "A lantern in the mist");
    assert_eq!(lib.chapters[0].state, ChapterState::Queued);
    assert_eq!(
        lib.page().source[0].text,
        "그녀는 안개 속에서 등불을 들었다."
    );
}

#[test]
fn successive_review_accepts_keep_every_revision_for_undo() {
    for accept_prose in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut b = book(&dir);
        let tid = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
        let ch = chapter(&mut b, 1, &["마탑주가 웃었다."]);
        translate(&mut b, ch, &["The Magic Tower Lord smiled."], None);
        b.apply(Change::AddStoryNote {
            chapter: None,
            text: "The Magic Tower Lord smiled.".into(),
        })
        .unwrap();
        let glossary = b.glossary().unwrap();
        let notes = b.story_notes().unwrap();
        let original_chapter = b.chapter(ch).unwrap();
        let path = b.path().to_owned();
        drop(b);
        let mut lib = Library::open(&path).unwrap();
        let at = lib.terms.iter().position(|t| t.id == tid).unwrap();
        let sid = lib.use_wording(at, "Tower Master", &[]).unwrap().unwrap();
        let metadata: Vec<_> = lib
            .suggestion(sid)
            .unwrap()
            .metadata
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(metadata.len(), 2);
        let mut state = crate::state::State::new(lib);
        for eid in metadata {
            state.review_metadata(sid, vec![(eid, true)]);
        }
        assert_ne!(state.lib.book().glossary().unwrap(), glossary);
        assert_ne!(state.lib.book().story_notes().unwrap(), notes);
        if accept_prose {
            state.accept_all(sid);
            assert_ne!(state.lib.book().chapter(ch).unwrap(), original_chapter);
        }
        assert_eq!(state.fix.finished.len(), 1);
        let finished = &state.fix.finished[0];
        let count = if accept_prose { 3 } else { 2 };
        assert_eq!(finished.revisions.len(), count);
        assert!(finished.revisions.windows(2).all(|r| r[0].0 < r[1].0));
        assert_eq!(finished.applied, count);
        assert_eq!((finished.stale, finished.skipped), (0, 0));
        assert_eq!(
            finished.chapters,
            if accept_prose { vec![ch] } else { vec![] }
        );
        state.undo(sid);
        assert_eq!(state.lib.book().glossary().unwrap(), glossary);
        assert_eq!(state.lib.book().story_notes().unwrap(), notes);
        assert_eq!(state.lib.book().chapter(ch).unwrap(), original_chapter);
        assert!(state.fix.finished.is_empty());
        let pending = state.lib.suggestion(sid).unwrap();
        assert!(
            pending
                .metadata
                .iter()
                .all(|e| e.state == EditState::Pending)
        );
        assert!(pending.edits.iter().all(|e| e.state == EditState::Pending));
    }
}

#[test]
fn drift_metadata_review_reloads_the_glossary_and_undo_restores_pending_items() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let tid = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    let ch = chapter(&mut b, 1, &["마탑주가 웃었다."]);
    translate(&mut b, ch, &["The Tower Master smiled."], None);
    let path = b.path().to_owned();
    drop(b);
    let mut lib = Library::open(&path).unwrap();
    let at = lib.terms.iter().position(|t| t.id == tid).unwrap();
    let sid = lib.use_wording(at, "Tower Master", &[]).unwrap().unwrap();
    assert_eq!(lib.terms[at].rendering, "Magic Tower Lord");
    assert!(lib.suggestion(sid).unwrap().edits.is_empty());
    let eid = lib.suggestion(sid).unwrap().metadata[0].id;
    assert!(
        lib.decide_metadata(vec![(MetadataEditId(-1), true)])
            .is_err()
    );
    let empty = lib.decide_metadata(Vec::new()).unwrap();
    assert_eq!((empty.applied, empty.stale), (0, 0));
    let decided = lib.decide_metadata(vec![(eid, true)]).unwrap();
    assert_eq!((decided.applied, decided.stale), (1, 0));
    assert_eq!(lib.terms[at].rendering, "Tower Master");
    assert!(lib.suggestion(sid).is_none());
    lib.undo_review(decided.revision, &[]).unwrap();
    assert_eq!(lib.terms[at].rendering, "Magic Tower Lord");
    assert_eq!(
        lib.suggestion(sid).unwrap().metadata[0].state,
        EditState::Pending
    );
    let result = lib.decide_metadata(vec![(eid, false)]).unwrap();
    assert_eq!(result.applied, 0);
    let s = lib.book().suggestions(SuggestionFilter::default()).unwrap();
    assert_eq!(s[0].metadata[0].state, EditState::Rejected);
}

#[test]
fn term_editing_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let id = term(&mut b, LORD, "Lord", TermStatus::Pending);
    let original = b.glossary().unwrap();
    let mut lib = Library::open_quick(b.path()).unwrap();
    assert_eq!(lib.pending_terms(), 1);
    let (rev, label) = lib.set_term_status(0, TermStatus::Accepted).unwrap();
    assert_eq!(label, "Accepted Lord");
    assert!(!lib.terms[0].provisional);
    lib.undo_term(rev).unwrap();
    assert_eq!(lib.book().glossary().unwrap(), original);
    let (rev, _) = lib.set_term_status(0, TermStatus::Dismissed).unwrap();
    assert!(lib.terms.is_empty());
    lib.undo_term(rev).unwrap();
    assert_eq!(lib.terms[0].id, id);
    assert_eq!(lib.book().glossary().unwrap(), original);
    assert!(lib.edit_term(0, " ", "Lord", "").is_err());
    assert!(lib.edit_term(0, LORD, " ", "").is_err());
    assert!(lib.edit_term(99, LORD, "Lord", "").is_err());
    assert!(lib.edit_term(0, LORD, " Lord ", " ").unwrap().is_none());
    let (rev, label) = lib
        .edit_term(0, " source ", " Keeper ", " note ")
        .unwrap()
        .unwrap();
    assert_eq!(label, "Renamed Lord → Keeper");
    assert_eq!(lib.terms[0].source, "source");
    assert_eq!(lib.terms[0].note.as_deref(), Some("note"));
    assert!(lib.terms[0].provisional);
    assert_eq!(lib.terms[0].forbidden, ["Lord"]);
    lib.undo_term(rev).unwrap();
    assert_eq!(lib.book().glossary().unwrap(), original);
    lib.set_term_status(0, TermStatus::Accepted).unwrap();
    lib.book_mut()
        .apply(Change::UpsertTerm {
            id: Some(id),
            fields: TermFields {
                source: LORD.into(),
                rendering: "Lord".into(),
                kind: Some("title".into()),
                gender: Some("male".into()),
                may_omit: true,
                note: Some("old note".into()),
            },
            status: TermStatus::Accepted,
            first_chapter: None,
        })
        .unwrap();
    lib.reload().unwrap();
    let before = lib.book().glossary().unwrap();
    let (rev, _) = lib.edit_term(0, LORD, "Keeper", " ").unwrap().unwrap();
    assert!(!lib.terms[0].provisional);
    assert_eq!(lib.terms[0].kind.as_deref(), Some("title"));
    assert_eq!(lib.terms[0].gender.as_deref(), Some("male"));
    assert!(lib.terms[0].may_omit);
    assert_eq!(lib.terms[0].note, None);
    lib.undo_term(rev).unwrap();
    assert_eq!(lib.book().glossary().unwrap(), before);
}

#[test]
fn counts_and_order_of_first_appearance() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let lord = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    let hero = term(&mut b, "테실리드", "Tesselid", TermStatus::Accepted);
    let c = chapter(
        &mut b,
        1,
        &["# 1화", "테실리드가 마탑주를 보았다.", "마탑주가 웃었다."],
    );
    translate(
        &mut b,
        c,
        &[
            "# Chapter 1",
            "Tesselid saw the **Magic Tower Lord**.",
            "The Magic Tower Lord laughed, and Tesselid's face fell.",
        ],
        None,
    );
    let lib = Library::from_book(b).unwrap();
    let (li, hi) = (
        lib.terms.iter().position(|t| t.id == lord).unwrap(),
        lib.terms.iter().position(|t| t.id == hero).unwrap(),
    );
    let s = lib.stats(0);
    assert_eq!(s.counts[li], 2);
    assert_eq!(s.counts[hi], 2);
    assert_eq!(s.present, vec![hi, li], "Tesselid comes first in the text");
    assert_eq!(lib.usage(li).total, 2);
    assert_eq!(lib.usage(li).chapters, 1);
    assert!(s.drift.is_empty());
}

#[test]
fn page_renders_blocks_and_finds_terms_on_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let lord = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    let c = chapter(&mut b, 5, &["# 5화", "마탑주가 말했다."]);
    translate(
        &mut b,
        c,
        &["# Chapter 5: Tower", "The *Magic Tower Lord* spoke."],
        None,
    );
    let lib = Library::from_book(b).unwrap();
    let page = lib.page();
    assert_eq!(page.number, 5);
    assert!(page.hides_first_translated());
    let t = &page.translation[1];
    assert_eq!(t.text, "The Magic Tower Lord spoke.");
    assert_eq!(t.spans.len(), 1, "the emphasis survives as a span");
    assert_eq!(&t.text[t.terms[0].range.clone()], "Magic Tower Lord");
    let s = &page.source[1];
    assert_eq!(&s.text[s.terms[0].range.clone()], LORD);
    assert_eq!(lib.terms[s.terms[0].term].id, lord);
    assert_eq!(lib.first_use(s.terms[0].term), Some(t.id));
}

#[test]
fn deleting_drifting_chapters_rebuilds_usage_and_cast_summary() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let lord = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    b.apply(Change::AddForbidden {
        term: lord,
        rendering: "Tower Master".into(),
    })
    .unwrap();
    for number in 1..=2 {
        let c = chapter(&mut b, number, &["마탑주가 왔다."]);
        translate(&mut b, c, &["The Tower Master came."], None);
    }
    let mut state = crate::state::State::new(Library::from_book(b).unwrap());
    let li = state.lib.terms.iter().position(|t| t.id == lord).unwrap();
    assert_eq!(state.lib.usage(li).drift_chapters, vec![0, 1]);
    assert_eq!(state.drift_summary(li).forbidden_chapters, 2);

    state.lib.delete_chapter(0).unwrap();
    assert_eq!(state.lib.usage(li).drift_chapters, vec![0]);
    assert_eq!(state.drift_summary(li).forbidden_chapters, 1);
    assert_eq!(state.lib.drifting_terms(), 1);
    assert_eq!(state.lib.usage(li).total, 0);

    state.lib.delete_chapter(0).unwrap();
    assert_eq!(*state.lib.usage(li), TermUsage::default());
    assert_eq!(
        state.drift_summary(li),
        crate::drift::DriftSummary::default()
    );
    assert_eq!(state.lib.cast_order(), vec![li]);
    assert!(state.lib.scanned);
}

#[test]
fn forbidden_rendering_present_is_drift_and_is_marked_on_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let lord = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    b.apply(Change::AddForbidden {
        term: lord,
        rendering: "Tower Master".into(),
    })
    .unwrap();
    let c1 = chapter(&mut b, 1, &["마탑주가 왔다."]);
    translate(&mut b, c1, &["The Magic Tower Lord came."], None);
    let c2 = chapter(&mut b, 2, &["마탑주가 갔다."]);
    translate(&mut b, c2, &["The Tower Master left."], None);
    let mut lib = Library::from_book(b).unwrap();
    let li = lib.terms.iter().position(|t| t.id == lord).unwrap();
    assert_eq!(lib.usage(li).drift_chapters, vec![1]);
    assert_eq!(lib.drifting_terms(), 1);
    assert!(matches!(
        lib.stats(1).drift[0].kind,
        DriftKind::ForbiddenPresent { count: 1, .. }
    ));
    assert!(lib.stats(0).drift.is_empty());

    lib.open_chapter(1).unwrap();
    let block = &lib.page().translation[0];
    assert_eq!(block.forbidden.len(), 1);
    assert_eq!(
        &block.text[block.forbidden[0].range.clone()],
        "Tower Master"
    );
}

#[test]
fn a_term_the_source_names_three_times_but_the_translation_never_renders_drifts() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let lord = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    let c = chapter(&mut b, 1, &["마탑주, 마탑주, 마탑주.", "마탑주."]);
    translate(&mut b, c, &["He, he, he.", "Him."], None);
    let c2 = chapter(&mut b, 2, &["마탑주, 마탑주.", "끝."]);
    translate(&mut b, c2, &["The two of them.", "End."], None);
    let lib = Library::from_book(b).unwrap();
    let li = lib.terms.iter().position(|t| t.id == lord).unwrap();
    assert_eq!(
        lib.usage(li).drift_chapters,
        vec![0],
        "two mentions are not enough"
    );
    assert!(matches!(
        lib.stats(0).drift[0].kind,
        DriftKind::NeverRendered { source_count: 4 }
    ));
}

#[test]
fn imported_pairs_are_quiet_and_other_flags_are_problems() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let c1 = chapter(&mut b, 1, &["하나", "둘"]);
    translate(&mut b, c1, &["One", "Two"], Some("imported"));
    let c2 = chapter(&mut b, 2, &["하나", "둘"]);
    translate(&mut b, c2, &["One", "Two"], Some("omitted a sentence"));
    let mut lib = Library::from_book(b).unwrap();
    assert_eq!(lib.page().problems(), 0);
    assert!(
        lib.page()
            .pairs
            .iter()
            .all(|p| p.flag == Some(PairFlag::Imported))
    );
    lib.open_chapter(1).unwrap();
    assert_eq!(lib.page().problems(), 2);
    assert_eq!(
        lib.page().pairs[0].flag,
        Some(PairFlag::Problem("omitted a sentence".into()))
    );
}

#[test]
fn pairs_resolve_to_positions_and_keep_the_position_across_sides() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let c = chapter(&mut b, 1, &["가", "나", "다"]);
    let src = b.chapter(c).unwrap().source;
    b.apply(Change::SetTranslation {
        chapter: c,
        pairs: vec![
            NewPair {
                source: vec![src[0].id, src[1].id],
                translation: vec!["A and B".into()],
                flagged: None,
            },
            NewPair {
                source: vec![src[2].id],
                translation: vec!["C".into()],
                flagged: None,
            },
        ],
    })
    .unwrap();
    let lib = Library::from_book(b).unwrap();
    let page = lib.page();
    let pair = page.translation[0].pair.unwrap();
    assert_eq!(page.pair(pair).unwrap().source_text, "가\n나");
    assert_eq!(page.first_block(Side::Source, pair), Some(src[0].id));
    assert_eq!(page.pair_of(Side::Source, src[1].id), Some(pair));
    assert!(page.first_of_pair(0));
}

#[test]
fn chapter_states_come_from_the_book_and_the_queue() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let done = chapter(&mut b, 1, &["하나"]);
    translate(&mut b, done, &["One"], None);
    let waiting = chapter(&mut b, 2, &["둘"]);
    let queued = chapter(&mut b, 3, &["셋"]);
    let second = chapter(&mut b, 4, &["넷"]);
    for ch in [queued, second] {
        b.enqueue(JobSpec {
            kind: JobKind::Translate,
            parent: None,
            chapter: Some(ch),
            suggestion: None,
            input: None,
        })
        .unwrap();
    }
    let _ = waiting;
    let mut lib = Library::from_book(b).unwrap();
    let states: Vec<_> = lib.chapters.iter().map(|c| c.state).collect();
    assert_eq!(
        states,
        [
            ChapterState::Translated,
            ChapterState::Waiting,
            ChapterState::Queued,
            ChapterState::Queued
        ]
    );
    assert_eq!(lib.chapters[2].queue_place, Some(1));
    assert_eq!(lib.chapters[3].queue_place, Some(2));
    assert_eq!(lib.translated(), 1);
    lib.open_chapter(1).unwrap();
    assert!(lib.page().translation.is_empty());
    assert_eq!(lib.page().source.len(), 1);
    assert_eq!(
        lib.chapters[1].opening, "",
        "a short source line is not an opening"
    );
}

#[test]
fn refresh_chapter_recomputes_that_chapter_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let lord = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    let c1 = chapter(&mut b, 1, &["마탑주가 왔다."]);
    translate(&mut b, c1, &["The Magic Tower Lord came."], None);
    let c2 = chapter(&mut b, 2, &["마탑주가 갔다."]);
    translate(&mut b, c2, &["The Magic Tower Lord left."], None);
    let mut lib = Library::from_book(b).unwrap();
    let li = lib.terms.iter().position(|t| t.id == lord).unwrap();
    assert_eq!(lib.usage(li).total, 2);

    // Another Book handle on the same file, as the engine's worker would be.
    let mut other = Book::open(dir.path().join("t.cipher")).unwrap();
    let ch = other.chapter(c2).unwrap();
    other
        .apply(Change::EditBlock {
            block: ch.translation[0].id,
            text: "He left.".into(),
            hand: true,
        })
        .unwrap();
    assert_eq!(lib.usage(li).total, 2, "nothing changes until asked");
    lib.refresh_chapters(&[1]).unwrap();
    assert_eq!(lib.usage(li).total, 1);
    assert_eq!(lib.counts(li).collect::<Vec<_>>(), vec![1, 0]);
    assert_eq!(
        lib.chapters[1].opening, "",
        "too short to be an opening line"
    );
}

#[test]
fn pending_terms_are_provisional_and_dismissed_ones_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    term(&mut b, "하나", "One", TermStatus::Accepted);
    term(&mut b, "둘", "Two", TermStatus::Pending);
    term(&mut b, "셋", "Three", TermStatus::Dismissed);
    chapter(&mut b, 1, &["하나 둘 셋"]);
    let lib = Library::from_book(b).unwrap();
    let names: Vec<_> = lib
        .terms
        .iter()
        .map(|t| (t.rendering.as_str(), t.provisional))
        .collect();
    assert_eq!(names, [("One", false), ("Two", true)]);
}

#[test]
fn cast_order_puts_drifting_terms_first_then_by_use() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    term(&mut b, "하나", "One", TermStatus::Accepted);
    term(&mut b, "둘", "Two", TermStatus::Accepted);
    let ghost = term(&mut b, "세번", "Three", TermStatus::Accepted);
    let c = chapter(&mut b, 1, &["하나 둘 둘 세번 세번 세번", "둘"]);
    translate(&mut b, c, &["One Two Two", "Two"], None);
    let lib = Library::from_book(b).unwrap();
    let order: Vec<_> = lib
        .cast_order()
        .into_iter()
        .map(|i| lib.terms[i].rendering.clone())
        .collect();
    assert_eq!(order, ["Three", "Two", "One"]);
    assert_eq!(lib.terms[lib.cast_order()[0]].id, ghost);
}

#[test]
fn searching_finds_chapters_by_number_and_terms_by_either_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    for n in [9, 10, 23] {
        chapter(&mut b, n, &["가"]);
    }
    let lib = Library::from_book(b).unwrap();
    assert_eq!(lib.find_chapters("2"), vec![2]);
    assert_eq!(lib.find_chapters("1"), vec![1]);
    assert_eq!(lib.find_terms("tower").len(), 1);
    assert_eq!(lib.find_terms("마탑").len(), 1);
    assert!(lib.find_terms("").is_empty());
}

#[test]
fn an_empty_book_opens_and_its_first_pasted_chapter_becomes_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::from_book(book(&dir)).unwrap();
    assert!(lib.chapters.is_empty());
    assert!(lib.is_saved());
    lib.open_chapter(3).unwrap();
    let intake = lib.intake("1화\n\n바람이 불었다.").unwrap();
    assert_eq!(intake.after, None);
    assert_eq!(lib.add_chapter(&intake).unwrap(), 0);
    assert_eq!(lib.page().number, 1);
    assert_eq!(lib.page().source.len(), 1);

    let blank = Library::blank();
    assert!(blank.chapters.is_empty() && !blank.is_saved());
}

fn selection(lib: &Library, text: &str) -> cipher_engine::Selection {
    let b = &lib.page().translation[0];
    cipher_engine::Selection {
        chapter: lib.chapters[0].id,
        block: b.id,
        text: text.into(),
    }
}

#[test]
fn a_rename_is_reviewed_applied_and_undone() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let c = chapter(&mut b, 1, &["하나", "둘"]);
    translate(
        &mut b,
        c,
        &["The Holy Scripture spoke.", "Holy Scripture fell."],
        None,
    );
    let mut lib = Library::from_book(b).unwrap();
    let sel = selection(&lib, "Holy Scripture");
    let reach = lib.reach(&sel, cipher_book::Scope::Everywhere).unwrap();
    assert_eq!((reach.blocks, reach.chapters, reach.exact), (2, 1, 2));

    let id = lib
        .suggest(cipher_engine::SuggestionRequest {
            selection: sel,
            said: "Divine Scripture".into(),
            scope: cipher_book::Scope::Everywhere,
            rename: true,
        })
        .unwrap();
    let sug = lib.suggestion(id).unwrap();
    assert_eq!(counts(sug).pending, 2);
    assert_eq!(lib.chapters[0].pending_edits, 2);
    assert!(lib.has_edit(lib.page().translation[0].id));

    let decisions = sug.edits.iter().map(|e| (e.id, true)).collect();
    let done = lib.decide(decisions).unwrap();
    assert_eq!((done.applied, done.stale), (2, 0));
    assert_eq!(
        lib.page().translation[0].text,
        "The Divine Scripture spoke."
    );
    assert!(lib.suggestion(id).is_none(), "every edit is decided");

    lib.undo_review(done.revision, &done.chapters).unwrap();
    assert_eq!(lib.page().translation[0].text, "The Holy Scripture spoke.");
    assert_eq!(counts(lib.suggestion(id).unwrap()).pending, 2);
}

#[test]
fn an_edit_over_newer_text_is_stale_and_never_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let c = chapter(&mut b, 1, &["하나"]);
    translate(&mut b, c, &["The Holy Scripture spoke."], None);
    let mut lib = Library::from_book(b).unwrap();
    let sel = selection(&lib, "Holy Scripture");
    let block = sel.block;
    let id = lib
        .suggest(cipher_engine::SuggestionRequest {
            selection: sel,
            said: "Divine Scripture".into(),
            scope: cipher_book::Scope::Here,
            rename: true,
        })
        .unwrap();
    lib.book_mut()
        .apply(Change::EditBlock {
            block,
            text: "Someone rewrote this.".into(),
            hand: true,
        })
        .unwrap();
    lib.refresh_chapters(&[0]).unwrap();
    let sug = lib.suggestion(id).unwrap();
    assert_eq!((counts(sug).pending, counts(sug).stale), (0, 1));

    let decisions = sug.edits.iter().map(|e| (e.id, true)).collect();
    let done = lib.decide(decisions).unwrap();
    assert_eq!((done.applied, done.stale), (0, 1));
    assert_eq!(lib.page().translation[0].text, "Someone rewrote this.");
}

#[test]
fn a_pasted_chapter_is_read_added_in_number_order_and_queued() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    for n in [1, 2, 4] {
        let c = chapter(&mut b, n, &["바람이 불었다."]);
        translate(&mut b, c, &["The wind blew."], None);
    }
    let mut lib = Library::from_book(b).unwrap();
    lib.open_chapter(2).unwrap(); // Chapter 4

    assert_eq!(lib.intake("  \n\n "), None);
    let pasted = "\n제3화 귀환\n\n그날 밤, 탑 위에서 바람이 불었다.\n\n테실리드는 고개를 들었다.\n";
    let intake = lib.intake(pasted).unwrap();
    assert_eq!(intake.number, Some(3));
    assert_eq!(intake.title.as_deref(), Some("귀환"));
    assert_eq!(intake.blocks.len(), 2);
    assert_eq!(intake.preview[0], "제3화 귀환");
    assert_eq!(intake.after, Some(2));
    assert!(!intake.exists);
    assert!(lib.intake("2화\n\n본문입니다.").unwrap().exists);

    let at = lib.add_chapter(&intake).unwrap();
    assert_eq!(at, 2);
    let numbers: Vec<i64> = lib.chapters.iter().map(|c| c.number).collect();
    assert_eq!(numbers, [1, 2, 3, 4]);
    assert_eq!(lib.chapters[2].state, ChapterState::Queued);
    assert_eq!(lib.chapters[2].queue_place, Some(1));
    // The page stays on Chapter 4, now one row lower.
    assert_eq!(lib.current(), 3);
    assert_eq!(lib.page().number, 4);
}

#[test]
fn an_untranslated_chapter_can_be_queued_and_a_failed_one_retried() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 1, &["바람이 불었다."]);
    let failed = chapter(&mut b, 2, &["바람이 불었다."]);
    let job = b
        .enqueue(JobSpec {
            kind: JobKind::Translate,
            parent: None,
            chapter: Some(failed),
            suggestion: None,
            input: None,
        })
        .unwrap();
    let mut claimed = b.claim_next_job().unwrap().unwrap();
    assert_eq!(claimed.id, job);
    claimed.state = cipher_book::JobState::Failed;
    claimed.error = Some("auth: the key was refused".into());
    b.save_job(&claimed).unwrap();

    let mut lib = Library::from_book(b).unwrap();
    assert_eq!(lib.chapters[0].state, ChapterState::Waiting);
    assert_eq!(lib.chapters[1].state, ChapterState::Failed);
    assert_eq!(
        lib.chapters[1].failure.as_deref(),
        Some("auth: the key was refused")
    );
    assert!(lib.retry(0).is_err());

    lib.translate(0).unwrap();
    assert_eq!(lib.chapters[0].state, ChapterState::Queued);
    lib.retry(1).unwrap();
    assert_eq!(lib.chapters[1].state, ChapterState::Queued);
    assert_eq!(lib.chapters[1].failure, None);
    // A retried Job keeps its id, so it runs before Jobs queued after it.
    assert_eq!(lib.chapters[1].queue_place, Some(1));
    assert_eq!(lib.chapters[0].queue_place, Some(2));
}

#[test]
fn batch_splitting_preserves_single_reading_and_tracks_earlier_parts() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 2, &["Already here."]);
    let lib = Library::from_book(b).unwrap();
    assert!(lib.intakes(" \n\n ").is_empty());
    for text in ["Some prose.\n\nMore prose.", "3화\n\nA chapter."] {
        assert_eq!(lib.intakes(text), vec![lib.intake(text).unwrap()]);
    }
    let parts = lib.intakes("Preamble.\n\n4화\n\nFour.\n\n3화\n\nThree.\n\n4화\n\nAnother four.");
    assert_eq!(
        parts.iter().map(|p| p.number).collect::<Vec<_>>(),
        [None, Some(4), Some(3), Some(4)]
    );
    assert_eq!(parts[0].blocks, ["Preamble."]);
    assert_eq!(parts[1].after, Some(2));
    assert_eq!(parts[3].after, Some(3));
    assert!(parts[3].exists);
    assert!(!parts[1].exists);
}

#[test]
fn batch_plan_orders_numbers_marks_conflicts_and_assigns_unnumbered_parts() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 10, &["Existing."]);
    let lib = Library::from_book(b).unwrap();
    assert!(lib.intake_plan(&[], "").is_empty());
    let files = vec![
        (
            "first.txt".into(),
            "12화\n\nFirst twelve.\n\n11화\n\nEleven.".into(),
        ),
        ("second.txt".into(), "Unnumbered file.".into()),
    ];
    let plan = lib.intake_plan(
        &files,
        "Preamble.\n\n12화\n\nDuplicate.\n\n10화\n\nIn book.",
    );
    assert_eq!(
        plan.iter()
            .map(|p| p.intake.number.unwrap())
            .collect::<Vec<_>>(),
        [10, 11, 12, 12, 13, 14]
    );
    assert_eq!(
        plan.iter().map(|p| p.issue).collect::<Vec<_>>(),
        [
            Some(IntakeIssue::InBook),
            None,
            None,
            Some(IntakeIssue::Duplicate),
            Some(IntakeIssue::NoNumber),
            Some(IntakeIssue::NoNumber)
        ]
    );
    assert_eq!(
        plan.iter().map(|p| p.include).collect::<Vec<_>>(),
        [false, true, true, false, true, true]
    );
    assert_eq!(plan[2].source.as_deref(), Some("first.txt"));
    assert_eq!(plan[4].source.as_deref(), Some("second.txt"));
    assert_eq!(plan[4].intake.after, Some(12));
    assert_eq!(plan[5].intake.after, Some(13));
    assert_ne!(plan[2].key, plan[3].key);
}

#[test]
fn batch_commit_queues_selected_chapters_in_book_order_after_existing_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let existing = chapter(&mut b, 20, &["Existing."]);
    cipher_engine::translate(&mut b, &[existing]).unwrap();
    let mut lib = Library::from_book(b).unwrap();
    let mut plan = lib.intake_plan(
        &[],
        "23화\n\nTwenty-three.\n\n21화\n\nTwenty-one.\n\n21화\n\nDuplicate.\n\n22화\n\nSkip me.",
    );
    plan.iter_mut()
        .find(|p| p.intake.number == Some(22))
        .unwrap()
        .include = false;
    assert_eq!(lib.add_intakes(&plan).unwrap(), Some(1));
    assert_eq!(
        lib.chapters.iter().map(|c| c.number).collect::<Vec<_>>(),
        [20, 21, 23]
    );
    assert_eq!(
        lib.chapters
            .iter()
            .map(|c| c.queue_place)
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    assert_eq!(
        lib.book.chapter(lib.chapters[1].id).unwrap().source[0].text,
        "Twenty-one."
    );
    let jobs = lib.book.jobs(false).unwrap();
    assert_eq!(
        jobs.iter()
            .filter(|j| j.kind == JobKind::Translate)
            .map(|j| j.chapter.unwrap())
            .collect::<Vec<_>>(),
        lib.chapters.iter().map(|c| c.id).collect::<Vec<_>>()
    );
    for part in &mut plan {
        part.include = false;
    }
    assert_eq!(lib.add_intakes(&plan).unwrap(), None);
    assert_eq!(lib.chapters.len(), 3);
}

#[test]
fn batch_commit_rejects_empty_parts_before_adding_anything_and_allows_opted_in_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::from_book(book(&dir)).unwrap();
    let plan = lib.intake_plan(&[], "1화\n\nOne.\n\n2화");
    assert!(lib.add_intakes(&plan).is_err());
    assert!(lib.book.chapters().unwrap().is_empty());
    let mut plan = lib.intake_plan(&[], "1화\n\nOne.\n\n1화\n\nAnother one.");
    plan[1].include = true;
    lib.add_intakes(&plan).unwrap();
    assert_eq!(
        lib.chapters.iter().map(|c| c.number).collect::<Vec<_>>(),
        [1, 1]
    );
}

#[test]
fn picking_more_files_preserves_the_first_file_and_any_later_paste() {
    let mut draft = crate::draft::Draft {
        text: rune::TextEdit::new("Old draft."),
        back: None,
        files: Vec::new(),
        file_name: None,
        included: Default::default(),
        added: Default::default(),
        numbers: Default::default(),
    };
    draft.pick_file("one.txt".into(), "1화\n\nOne.".into(), false);
    assert_eq!(draft.text.text, "1화\n\nOne.");
    assert!(draft.files.is_empty());
    draft.included.insert((0, 0), false);
    draft.pick_file("two.txt".into(), "2화\n\nTwo.".into(), true);
    assert_eq!(draft.files.len(), 2);
    assert_eq!(draft.files[0].0, "one.txt");
    assert!(draft.text.text.is_empty());
    assert_eq!(draft.included.get(&(0, 0)), Some(&false));
    draft.text = rune::TextEdit::new("3화\n\nPasted three.");
    draft.pick_file("four.txt".into(), "4화\n\nFour.".into(), true);
    assert_eq!(draft.files.len(), 3);
    assert_eq!(draft.text.text, "3화\n\nPasted three.");
    draft.pick_file("empty.txt".into(), "".into(), false);
    assert!(draft.files.is_empty());
    assert!(draft.text.text.is_empty());
}

#[test]
fn partial_batch_failure_queues_created_chapters_in_book_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::from_book(book(&dir)).unwrap();
    let existing = lib.intake("10화\n\nAlready queued.").unwrap();
    lib.add_chapter(&existing).unwrap();
    let mut plan = lib.intake_plan(&[], "1화\n\nOne.\n\n2화\n\nTwo.\n\n3화\n\nThree.");
    plan.swap(0, 1);
    let error = lib
        .add_intakes_with(&plan, |lib, intake| {
            if intake.number == Some(3) {
                return Err(cipher_book::Error::Invalid("creation failed".into()));
            }
            let applied = lib.book.apply(Change::AddChapter {
                number: intake.number,
                title: intake.title.clone(),
                source: intake.blocks.clone(),
            })?;
            Ok(ChapterId(applied.created[0]))
        })
        .unwrap_err();
    assert!(error.to_string().contains("creation failed"));
    assert_eq!(
        lib.chapters
            .iter()
            .map(|c| (c.number, c.queue_place))
            .collect::<Vec<_>>(),
        [(1, Some(2)), (2, Some(3)), (10, Some(1))]
    );
    let error = lib
        .add_intakes_with(&plan, |_, _| {
            Err(cipher_book::Error::Invalid("first failed".into()))
        })
        .unwrap_err();
    assert!(error.to_string().contains("first failed"));
    assert_eq!(lib.book.jobs(false).unwrap().len(), 3);
}

#[test]
fn batch_range_tracks_selection_and_falls_back_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::from_book(book(&dir)).unwrap();
    let mut plan = lib.intake_plan(&[], "1화\n\nOne.\n\n321화\n\nNext.\n\n322화\n\nLast.");
    plan[0].include = false;
    assert_eq!(crate::draft::batch_range(&plan), "321–322");
    plan[1].include = false;
    assert_eq!(crate::draft::batch_range(&plan), "322");
    plan[2].include = false;
    assert_eq!(crate::draft::batch_range(&plan), "1–322");
    assert_eq!(crate::draft::batch_range(&[]), "");
}

#[test]
fn batch_boundaries_do_not_treat_numbered_lists_as_chapters() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::from_book(book(&dir)).unwrap();
    let single = "Chapter 321\n\nA checklist.\n\n1. Check the gate\n\n2. Light the lamp";
    assert_eq!(lib.intakes(single), vec![lib.intake(single).unwrap()]);
    let text = format!("{single}\n\n322화\n\nThe next chapter.");
    let parts = lib.intakes(&text);
    assert_eq!(parts.len(), 2);
    assert!(parts[0].blocks.contains(&"1. Check the gate".into()));
    for marker in [
        "1화",
        "1話",
        "1章",
        "1장",
        "1回",
        "제1",
        "第1",
        "Chapter 1",
        "Ch. 1",
        "# 1",
    ] {
        assert_eq!(
            lib.intakes(&format!("{marker}\n\nFirst.\n\n2\n\nSecond."))
                .len(),
            2,
            "{marker}"
        );
    }
    assert!(lib.intakes("").is_empty());
}

#[test]
fn failed_batch_queue_keeps_every_saved_chapter_visible_and_refreshed() {
    for partial_queue in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::from_book(book(&dir)).unwrap();
        let plan = lib.intake_plan(&[], "1화\n\nOne.\n\n2화\n\nTwo.");
        let error = lib
            .add_intakes_using(
                &plan,
                |lib, intake| {
                    let applied = lib.book.apply(Change::AddChapter {
                        number: intake.number,
                        title: intake.title.clone(),
                        source: intake.blocks.clone(),
                    })?;
                    Ok(ChapterId(applied.created[0]))
                },
                |book, ids| {
                    if partial_queue {
                        cipher_engine::translate(book, &ids[..1]).map_err(super::engine_error)?;
                    }
                    Err(cipher_book::Error::Invalid("job insertion failed".into()))
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("job insertion failed"));
        assert_eq!(lib.book.chapters().unwrap().len(), 2);
        assert_eq!(lib.chapters.len(), 2);
        assert_eq!(
            lib.chapters[0].state,
            if partial_queue {
                ChapterState::Queued
            } else {
                ChapterState::Waiting
            }
        );
        assert_eq!(lib.chapters[1].state, ChapterState::Waiting);
        lib.open_chapter(1).unwrap();
        assert_eq!(lib.page().source[0].text, "Two.");
    }
}

#[test]
fn a_large_batch_fits_between_existing_chapters_in_number_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 0, &["Before."]);
    chapter(&mut b, 1000, &["After."]);
    let mut lib = Library::from_book(b).unwrap();
    let text = (1..=128)
        .map(|n| format!("Chapter {n}\n\nPart {n}."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let plan = lib.intake_plan(&[], &text);
    assert_eq!(lib.add_intakes(&plan).unwrap(), Some(1));
    let expected = (0..=128).chain([1000]).collect::<Vec<_>>();
    assert_eq!(
        lib.book
            .chapters()
            .unwrap()
            .iter()
            .map(|c| c.number.unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        lib.chapters.iter().map(|c| c.number).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        lib.chapters[1..129]
            .iter()
            .map(|c| c.queue_place.unwrap())
            .collect::<Vec<_>>(),
        (1..=128).collect::<Vec<_>>()
    );
}

#[test]
fn a_partial_draft_import_retains_unsaved_parts_and_retries_without_duplicates() {
    for unnumbered_files in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut b = book(&dir);
        chapter(&mut b, 10, &["Existing."]);
        let mut s = crate::state::State::new(Library::from_book(b).unwrap());
        s.open_draft("11화\n\nOne.\n\n12화\n\nTwo.\n\n13화\n\nThree.");
        if unnumbered_files {
            let d = s.draft.as_mut().unwrap();
            d.text = rune::TextEdit::new("");
            d.files = vec![
                ("one.txt".into(), "One.".into()),
                ("two.txt".into(), "Two.".into()),
                ("three.txt".into(), "Three.".into()),
            ];
        }
        let numbers = s
            .draft_plan()
            .iter()
            .map(|p| (p.key, p.intake.number))
            .collect::<Vec<_>>();
        s.add_draft_with(&mut rune::Tx::default(), |lib, plan| {
            lib.add_intakes_with(plan, |lib, intake| {
                if intake.number == Some(11) {
                    return Err(cipher_book::Error::Invalid("creation failed".into()));
                }
                let applied = lib.book.apply(Change::AddChapter {
                    number: intake.number,
                    title: intake.title.clone(),
                    source: intake.blocks.clone(),
                })?;
                Ok(ChapterId(applied.created[0]))
            })
        });
        assert_eq!(s.lib.chapters.len(), 2);
        assert!(
            s.draft.is_some(),
            "unsaved source must remain available for retry"
        );
        assert!(matches!(s.screen, crate::state::Screen::Draft));
        let plan = s.draft_plan();
        assert_eq!(
            plan.iter()
                .map(|p| (p.key, p.intake.number))
                .collect::<Vec<_>>(),
            numbers
        );
        let saved = plan
            .iter()
            .find(|p| p.intake.number == Some(12))
            .unwrap()
            .key;
        assert!(!plan.iter().find(|p| p.key == saved).unwrap().include);
        s.toggle_draft_part(saved);
        assert!(
            !s.draft_plan()
                .iter()
                .find(|p| p.key == saved)
                .unwrap()
                .include
        );
        assert!(
            s.toast
                .as_deref()
                .unwrap()
                .starts_with("Added 1 of 3 chapters:")
        );
        s.add_draft(&mut rune::Tx::default());
        assert!(s.draft.is_none());
        assert!(matches!(s.screen, crate::state::Screen::Reading));
        assert_eq!(
            s.lib.chapters.iter().map(|c| c.number).collect::<Vec<_>>(),
            [10, 11, 12, 13]
        );
    }
}

#[test]
fn a_draft_import_with_nothing_saved_keeps_the_draft() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 10, &["Existing."]);
    let mut s = crate::state::State::new(Library::from_book(b).unwrap());
    s.open_draft("11화\n\nOne.\n\n12화\n\nTwo.");
    s.add_draft_with(&mut rune::Tx::default(), |_, _| {
        Err(cipher_book::Error::Invalid("nothing saved".into()))
    });
    assert!(s.draft.is_some());
    assert!(matches!(s.screen, crate::state::Screen::Draft));
    assert!(s.toast.as_deref().unwrap().contains("nothing saved"));
    assert_eq!(s.lib.chapters.len(), 1);
}

#[test]
fn a_fully_saved_draft_leaves_even_if_queueing_failed() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 10, &["Existing."]);
    let mut s = crate::state::State::new(Library::from_book(b).unwrap());
    s.open_draft("11화\n\nOne.\n\n12화\n\nTwo.");
    s.add_draft_with(&mut rune::Tx::default(), |lib, plan| {
        lib.add_intakes_using(
            plan,
            |lib, intake| {
                let applied = lib.book.apply(Change::AddChapter {
                    number: intake.number,
                    title: intake.title.clone(),
                    source: intake.blocks.clone(),
                })?;
                Ok(ChapterId(applied.created[0]))
            },
            |_, _| Err(cipher_book::Error::Invalid("job insertion failed".into())),
        )
    });
    assert!(s.draft.is_none());
    assert!(matches!(s.screen, crate::state::Screen::Reading));
    assert_eq!(s.lib.chapters[s.lib.current()].number, 11);
    assert!(
        s.toast
            .as_deref()
            .unwrap()
            .starts_with("Added 2 of 2 chapters:")
    );
}

#[test]
fn titled_markdown_headings_split_without_splitting_plain_lists() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::from_book(book(&dir)).unwrap();
    for first in ["# 321: The Gate", "## 321. The Gate", "# 321 - The Gate"] {
        let hashes = first.split_whitespace().next().unwrap();
        let text = format!(
            "{first}\n\nOpening.\n\n1. Check the gate\n\n2. Light the lamp\n\n{hashes} 322: The Tower\n\nNext."
        );
        let parts = lib.intakes(&text);
        assert_eq!(parts.len(), 2, "{first}");
        assert_eq!(parts[0].number, Some(321));
        assert_eq!(parts[0].title.as_deref(), Some("The Gate"));
        assert_eq!(
            parts[0].blocks,
            ["Opening.", "1. Check the gate", "2. Light the lamp"]
        );
        assert_eq!(parts[1].number, Some(322));
    }
}

#[test]
fn titled_markdown_subsections_stay_inside_the_shallowest_chapters() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::from_book(book(&dir)).unwrap();
    let single = "# 321: The Gate\n\nOpening.\n\n## 1. Arrival\n\nAt the gate.";
    assert_eq!(lib.intakes(single), vec![lib.intake(single).unwrap()]);
    let parts = lib.intakes(&format!(
        "{single}\n\n# 322: The Tower\n\n## 2. Climbing\n\nUpstairs."
    ));
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].number, Some(321));
    assert!(parts[0].blocks.contains(&"## 1. Arrival".into()));
    assert!(parts[1].blocks.contains(&"## 2. Climbing".into()));
    assert_eq!(
        lib.intakes("# 321: The Gate\n\nOpening.\n\n### Chapter 322\n\nNext.")
            .len(),
        2
    );
    assert_eq!(
        lib.intakes("# 321: The Gate\n\nOpening.\n\n### 322\n\nNext.")
            .len(),
        1
    );
}

#[test]
fn appending_a_file_after_partial_import_preserves_saved_and_skipped_parts() {
    for sources in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let mut b = book(&dir);
        chapter(&mut b, 10, &["Existing."]);
        let mut s = crate::state::State::new(Library::from_book(b).unwrap());
        s.open_draft("");
        let d = s.draft.as_mut().unwrap();
        if sources == 0 {
            d.text = rune::TextEdit::new("11화\n\nOne.\n\n12화\n\nTwo.\n\n13화\n\nThree.");
        } else {
            d.files = vec![
                ("one.txt".into(), "One.".into()),
                ("two.txt".into(), "Two.".into()),
            ];
            if sources == 1 {
                d.files.push(("three.txt".into(), "Three.".into()));
            } else {
                d.text = rune::TextEdit::new("Three.");
            }
        }
        s.add_draft_with(&mut rune::Tx::default(), |lib, plan| {
            lib.add_intakes_with(plan, |lib, intake| {
                if intake.number == Some(11) {
                    return Err(cipher_book::Error::Invalid("creation failed".into()));
                }
                let applied = lib.book.apply(Change::AddChapter {
                    number: intake.number,
                    title: intake.title.clone(),
                    source: intake.blocks.clone(),
                })?;
                Ok(ChapterId(applied.created[0]))
            })
        });
        let skipped = s
            .draft_plan()
            .iter()
            .find(|p| p.intake.number == Some(13))
            .unwrap()
            .key;
        s.toggle_draft_part(skipped);
        s.draft
            .as_mut()
            .unwrap()
            .pick_file("four.txt".into(), "Four.".into(), true);
        let plan = s.draft_plan();
        let saved = plan.iter().find(|p| p.intake.number == Some(12)).unwrap();
        assert!(saved.added);
        assert!(!saved.include);
        let skipped = plan.iter().find(|p| p.intake.number == Some(13)).unwrap();
        assert!(!skipped.include);
        assert_eq!(skipped.intake.blocks, ["Three."]);
        let skipped_key = skipped.key;
        assert_eq!(
            plan.iter()
                .find(|p| p.intake.blocks == ["Four."])
                .unwrap()
                .intake
                .number,
            Some(14)
        );
        s.toggle_draft_part(skipped_key);
        s.add_draft(&mut rune::Tx::default());
        assert!(s.draft.is_none());
        assert_eq!(
            s.lib.chapters.iter().map(|c| c.number).collect::<Vec<_>>(),
            [10, 11, 12, 13, 14]
        );
    }
}

#[test]
fn leaving_a_partial_draft_returns_to_the_original_chapter_after_insertions() {
    for discard in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut b = book(&dir);
        chapter(&mut b, 10, &["Before."]);
        let original = chapter(&mut b, 20, &["Reading."]);
        chapter(&mut b, 30, &["After."]);
        let mut s = crate::state::State::new(Library::from_book(b).unwrap());
        s.go(&mut rune::Tx::default(), 1);
        s.open_draft("1화\n\nOne.\n\n2화\n\nTwo.\n\n3화\n\nThree.");
        s.add_draft_with(&mut rune::Tx::default(), |lib, plan| {
            lib.add_intakes_with(plan, |lib, intake| {
                if intake.number == Some(1) {
                    return Err(cipher_book::Error::Invalid("creation failed".into()));
                }
                let applied = lib.book.apply(Change::AddChapter {
                    number: intake.number,
                    title: intake.title.clone(),
                    source: intake.blocks.clone(),
                })?;
                Ok(ChapterId(applied.created[0]))
            })
        });
        assert!(s.draft.is_some());
        assert_eq!(s.lib.chapters[0].number, 2);
        s.leave_draft(&mut rune::Tx::default(), discard);
        assert!(matches!(s.screen, crate::state::Screen::Reading));
        assert_eq!(s.lib.chapters[s.lib.current()].id, original);
        assert_eq!(s.draft.is_none(), discard);
    }
}

#[test]
fn appended_explicit_numbers_conflict_with_restored_automatic_numbers() {
    for claimed in [13, 12] {
        let dir = tempfile::tempdir().unwrap();
        let mut b = book(&dir);
        chapter(&mut b, 10, &["Existing."]);
        let mut s = crate::state::State::new(Library::from_book(b).unwrap());
        s.open_draft("");
        s.draft.as_mut().unwrap().files = vec![
            ("one.txt".into(), "One.".into()),
            ("two.txt".into(), "Two.".into()),
            ("three.txt".into(), "Three.".into()),
        ];
        s.add_draft_with(&mut rune::Tx::default(), |lib, plan| {
            lib.add_intakes_with(plan, |lib, intake| {
                if intake.number == Some(11) {
                    return Err(cipher_book::Error::Invalid("creation failed".into()));
                }
                let applied = lib.book.apply(Change::AddChapter {
                    number: intake.number,
                    title: intake.title.clone(),
                    source: intake.blocks.clone(),
                })?;
                Ok(ChapterId(applied.created[0]))
            })
        });
        s.draft.as_mut().unwrap().pick_file(
            "claimed.txt".into(),
            format!("Chapter {claimed}\n\nClaimed."),
            true,
        );
        let plan = s.draft_plan();
        let appended = plan
            .iter()
            .find(|p| p.source.as_deref() == Some("claimed.txt"))
            .unwrap();
        assert_eq!(appended.issue, Some(IntakeIssue::Duplicate));
        assert_eq!(appended.intake.exists, claimed == 12);
        assert!(!appended.include);
        assert_eq!(
            plan.iter()
                .filter(|p| p.include && p.intake.number == Some(claimed))
                .count(),
            usize::from(claimed == 13)
        );
        s.add_draft(&mut rune::Tx::default());
        assert_eq!(
            s.lib.chapters.iter().map(|c| c.number).collect::<Vec<_>>(),
            [10, 11, 12, 13]
        );
    }
}

#[test]
fn punctuated_bare_markdown_numbers_split_at_the_same_heading_level() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::from_book(book(&dir)).unwrap();
    for hashes in ["#", "###"] {
        for punctuation in [".", ":", ")"] {
            let text = format!("{hashes} 321\n\nFirst.\n\n{hashes} 322{punctuation}\n\nSecond.");
            let parts = lib.intakes(&text);
            assert_eq!(parts.len(), 2, "{hashes} 322{punctuation}");
            assert_eq!(parts[1].number, Some(322));
            assert_eq!(parts[1].title, None);
            assert_eq!(parts[1].blocks, ["Second."]);
        }
    }
    let plain = "321\n\nFirst.\n\n322.\n\nSecond.";
    assert_eq!(lib.intakes(plain), vec![lib.intake(plain).unwrap()]);
}

#[test]
fn bare_markdown_chapters_set_the_depth_for_titled_subsections() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 1, &["Existing chapter one."]);
    let lib = Library::from_book(b).unwrap();
    for first in ["# 321", "# 321.", "# 321:", "# 321)"] {
        let text = format!("{first}\n\nOpening.\n\n## 1. Arrival\n\nAt the gate.");
        assert_eq!(
            lib.intakes(&text),
            vec![lib.intake(&text).unwrap()],
            "{first}"
        );
        let plan = lib.intake_plan(&[], &text);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].intake.number, Some(321));
        assert!(plan[0].include);
    }
    let parts = lib.intakes("# 321\n\nFirst.\n\n# 322.\n\nSecond.");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1].number, Some(322));
}

#[test]
fn appended_files_cannot_claim_pending_paste_numbers() {
    for explicit in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let mut b = book(&dir);
        chapter(&mut b, 10, &["Existing."]);
        let mut s = crate::state::State::new(Library::from_book(b).unwrap());
        s.open_draft("");
        let d = s.draft.as_mut().unwrap();
        d.files = vec![
            ("one.txt".into(), "One.".into()),
            ("two.txt".into(), "Two.".into()),
        ];
        d.text = rune::TextEdit::new("Pending paste.");
        s.add_draft_with(&mut rune::Tx::default(), |lib, plan| {
            lib.add_intakes_with(plan, |lib, intake| {
                if intake.number == Some(11) {
                    return Err(cipher_book::Error::Invalid("creation failed".into()));
                }
                let applied = lib.book.apply(Change::AddChapter {
                    number: intake.number,
                    title: intake.title.clone(),
                    source: intake.blocks.clone(),
                })?;
                Ok(ChapterId(applied.created[0]))
            })
        });
        s.draft.as_mut().unwrap().pick_file(
            "new.txt".into(),
            if explicit {
                "Chapter 13\n\nNew."
            } else {
                "New."
            }
            .into(),
            true,
        );
        let plan = s.draft_plan();
        let paste = plan
            .iter()
            .find(|p| p.intake.blocks == ["Pending paste."])
            .unwrap();
        assert_eq!(paste.intake.number, Some(13));
        assert_eq!(paste.issue, Some(IntakeIssue::NoNumber));
        assert!(paste.include);
        let new = plan
            .iter()
            .find(|p| p.source.as_deref() == Some("new.txt"))
            .unwrap();
        assert_eq!(new.intake.number, Some(if explicit { 13 } else { 14 }));
        assert_eq!(
            new.issue,
            Some(if explicit {
                IntakeIssue::Duplicate
            } else {
                IntakeIssue::NoNumber
            })
        );
        assert_eq!(new.include, !explicit);
        s.add_draft(&mut rune::Tx::default());
        let expected = if explicit {
            vec![10, 11, 12, 13]
        } else {
            vec![10, 11, 12, 13, 14]
        };
        assert_eq!(
            s.lib.chapters.iter().map(|c| c.number).collect::<Vec<_>>(),
            expected
        );
        assert!(s.draft.is_none());
    }
}

#[test]
fn a_quick_open_and_its_scan_count_what_a_full_open_counts() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    let lord = term(&mut b, LORD, "Magic Tower Lord", TermStatus::Accepted);
    b.apply(Change::AddForbidden {
        term: lord,
        rendering: "Tower Master".into(),
    })
    .unwrap();
    term(&mut b, "탑", "Tower", TermStatus::Accepted);
    let c1 = chapter(
        &mut b,
        1,
        &["마탑주가 왔다. 탑.", "마탑주, 마탑주, 마탑주."],
    );
    translate(
        &mut b,
        c1,
        &["The Magic Tower Lord came. Tower.", "He, he."],
        None,
    );
    let c2 = chapter(&mut b, 2, &["마탑주가 갔다."]);
    translate(&mut b, c2, &["The Tower Master left."], None);
    chapter(&mut b, 3, &["아직."]);
    let path = b.path().to_path_buf();
    let full = Library::from_book(b).unwrap();

    let mut quick = Library::open_quick(&path).unwrap();
    assert!(!quick.scanned());
    assert_ne!(
        quick.chapters[0].opening, "",
        "the open Chapter is counted at once"
    );
    assert_eq!(quick.chapters[1].opening, "", "the rest come with the scan");
    let stale = quick.scan()().unwrap();
    quick.reload().unwrap();
    quick.apply_scan(stale);
    assert!(!quick.scanned(), "a scan from before a reload is dropped");
    let scan = quick.scan()().unwrap();
    quick.apply_scan(scan);
    assert!(quick.scanned());
    for i in 0..3 {
        assert_eq!(quick.chapters[i].opening, full.chapters[i].opening);
        let (a, b) = (quick.stats(i), full.stats(i));
        assert_eq!(
            (&a.counts, &a.present, &a.drift),
            (&b.counts, &b.present, &b.drift)
        );
    }
    for t in 0..full.terms.len() {
        assert_eq!(quick.usage(t), full.usage(t));
    }
    assert_eq!(full.drifting_terms(), 1);
}

#[test]
fn a_chapter_with_no_text_is_empty_not_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = book(&dir);
    chapter(&mut b, 1, &["하나."]);
    chapter(&mut b, 2, &[]);
    let lib = Library::from_book(b).unwrap();
    assert_eq!(lib.chapters[0].state, ChapterState::Waiting);
    assert_eq!(lib.chapters[1].state, ChapterState::Empty);
    assert!(!lib.chapters[1].untranslated());
    assert_eq!(lib.empty(), 1);
}

/// Times opening and scanning a real Book: `CIPHER_BIG_BOOK=path cargo test --release -p
/// cipher-app big_book -- --ignored --nocapture`. Runs on a copy.
#[test]
#[ignore]
fn big_book_timings() {
    let Ok(path) = std::env::var("CIPHER_BIG_BOOK") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("big.cipher");
    std::fs::copy(&path, &copy).unwrap();
    let t = std::time::Instant::now();
    let mut lib = Library::open_quick(&copy).unwrap();
    eprintln!(
        "open_quick: {:?} ({} chapters)",
        t.elapsed(),
        lib.chapters.len()
    );
    let t = std::time::Instant::now();
    let scan = lib.scan()().unwrap();
    eprintln!("scan: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    lib.apply_scan(scan);
    eprintln!("apply: {:?}", t.elapsed());
    assert!(lib.scanned());
    let t = std::time::Instant::now();
    lib.open_chapter(lib.chapters.len() / 2).unwrap();
    eprintln!("open_chapter: {:?}", t.elapsed());
}
