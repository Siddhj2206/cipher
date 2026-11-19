//! The Book through its interface: apply, read, undo, and the Job queue.

use cipher_book::*;

#[test]
fn undo_term_status_and_rename() {
    let mut f = new_book();
    let applied = f
        .book
        .apply(Change::UpsertTerm {
            id: None,
            fields: TermFields {
                source: "탑".into(),
                rendering: "Tower".into(),
                note: Some("A place".into()),
                kind: Some("place".into()),
                gender: Some("neutral".into()),
                may_omit: true,
            },
            status: TermStatus::Pending,
            first_chapter: None,
        })
        .unwrap();
    let id = TermId(applied.created[0]);
    let before = f.book.glossary().unwrap();
    let revision = f
        .book
        .apply(Change::SetTermStatus {
            term: id,
            status: TermStatus::Accepted,
        })
        .unwrap()
        .revision;
    f.book.undo(revision, UndoMode::SkipConflicts).unwrap();
    assert_eq!(f.book.glossary().unwrap(), before);
    let revision = f
        .book
        .apply(Change::UpsertTerm {
            id: Some(id),
            fields: TermFields {
                source: "new".into(),
                rendering: "Spire".into(),
                ..TermFields::default()
            },
            status: TermStatus::Accepted,
            first_chapter: None,
        })
        .unwrap()
        .revision;
    assert_eq!(f.book.glossary().unwrap().terms[0].forbidden, ["Tower"]);
    f.book.undo(revision, UndoMode::SkipConflicts).unwrap();
    assert_eq!(f.book.glossary().unwrap(), before);
}

struct Fixture {
    _dir: tempfile::TempDir,
    book: Book,
}

fn new_book() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let book = Book::create(
        dir.path().join("test.cipher"),
        BookMeta {
            title: "Test".into(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        },
    )
    .unwrap();
    Fixture { _dir: dir, book }
}

#[test]
fn undo_preview_is_read_only_and_reports_conflicts_and_undone() {
    let mut f = new_book();
    let book = &mut f.book;
    let chapter = add_chapter(book, 1, &["Source"]);
    let translation = translate_one_to_one(book, chapter).revision;
    let before = seen(book);
    let revisions = book.revisions(200).unwrap();
    let preview = book.preview_undo(translation).unwrap();
    assert_eq!(preview.chapters, vec![chapter]);
    assert!(preview.conflicts.is_empty());
    assert_eq!(seen(book), before);
    assert_eq!(book.revisions(200).unwrap(), revisions);
    let block = book.chapter(chapter).unwrap().translation[0].id;
    book.apply(Change::EditBlock {
        block,
        text: "Later edit".into(),
        hand: true,
    })
    .unwrap();
    let before = seen(book);
    let revisions = book.revisions(200).unwrap();
    let preview = book.preview_undo(translation).unwrap();
    assert!(
        preview
            .conflicts
            .iter()
            .any(|c| c.entity == "block" && c.id == block.0)
    );
    assert!(
        preview.conflicts.iter().any(|c| c.entity == "pair"),
        "a later-edited block prevents removing its pair"
    );
    assert!(
        preview
            .conflict_chapters
            .iter()
            .any(|(c, ch)| c.id == block.0 && *ch == chapter)
    );
    assert_eq!(seen(book), before);
    assert_eq!(book.revisions(200).unwrap(), revisions);
    let result = book.undo(translation, UndoMode::SkipConflicts).unwrap();
    let UndoOutcome::Undone { revision, .. } = result else {
        panic!("{result:?}")
    };
    assert_eq!(
        book.preview_undo(translation).unwrap().already_undone_by,
        Some(revision)
    );
    let revisions = book.revisions(200).unwrap();
    assert!(book.preview_undo(RevisionId(i64::MAX)).is_err());
    assert_eq!(book.revisions(200).unwrap(), revisions);
}

#[test]
fn unsupported_revision_changes_remain_listable_and_preview_writes_nothing() {
    let mut f = new_book();
    let id = f
        .book
        .apply(Change::Group {
            kind: RevisionKind::Review,
            label: "Newer metadata change".into(),
            changes: vec![],
        })
        .unwrap()
        .revision;
    let conn = rusqlite::Connection::open(f.book.path()).unwrap();
    conn.execute(
        "INSERT INTO revision_change (revision_id, seq, entity, entity_id, before, after) VALUES (?1, 0, 'future_unknown_entity', 1, NULL, '{}')",
        [id.0],
    ).unwrap();
    let revisions = f.book.revisions(200).unwrap();
    assert!(revisions.iter().any(|r| r.id == id));
    let error = f.book.preview_undo(id).unwrap_err().to_string();
    assert!(error.contains("unknown entity"));
    assert_eq!(f.book.revisions(200).unwrap(), revisions);
    conn.execute("DELETE FROM revision_change WHERE revision_id = ?1", [id.0])
        .unwrap();
    assert!(f.book.preview_undo(id).is_ok());
}

#[test]
fn undo_preview_of_an_empty_revision_is_empty() {
    let mut f = new_book();
    let id = f
        .book
        .apply(Change::Group {
            kind: RevisionKind::Review,
            label: "No decisions".into(),
            changes: Vec::new(),
        })
        .unwrap()
        .revision;
    let revisions = f.book.revisions(200).unwrap();
    let preview = f.book.preview_undo(id).unwrap();
    assert!(preview.chapters.is_empty());
    assert!(preview.changes.is_empty());
    assert!(preview.conflicts.is_empty());
    assert_eq!(f.book.revisions(200).unwrap(), revisions);
}

/// Everything a reader can see, for comparing before and after an undo.
#[derive(Debug, PartialEq)]
struct Seen {
    chapters: Vec<ChapterSummary>,
    content: Vec<Chapter>,
    glossary: Glossary,
    notes: Vec<StoryNote>,
    suggestions: Vec<Suggestion>,
}

fn seen(book: &Book) -> Seen {
    let chapters = book.chapters().unwrap();
    Seen {
        content: chapters
            .iter()
            .map(|c| book.chapter(c.id).unwrap())
            .collect(),
        chapters,
        glossary: book.glossary().unwrap(),
        notes: book.story_notes().unwrap(),
        suggestions: book.suggestions(SuggestionFilter::default()).unwrap(),
    }
}

fn add_chapter(book: &mut Book, number: i64, source: &[&str]) -> ChapterId {
    let applied = book
        .apply(Change::AddChapter {
            number: Some(number),
            title: None,
            source: source.iter().map(|s| s.to_string()).collect(),
        })
        .unwrap();
    ChapterId(applied.created[0])
}

/// One Pair per source Block, translated as `T:<source>`.
fn translate_one_to_one(book: &mut Book, chapter: ChapterId) -> Applied {
    let c = book.chapter(chapter).unwrap();
    let pairs = c
        .source
        .iter()
        .map(|b| NewPair {
            source: vec![b.id],
            translation: vec![format!("T:{}", b.text)],
            flagged: None,
        })
        .collect();
    book.apply(Change::SetTranslation { chapter, pairs })
        .unwrap()
}

fn term(source: &str, rendering: &str) -> TermFields {
    TermFields {
        source: source.into(),
        rendering: rendering.into(),
        kind: Some("name".into()),
        gender: None,
        note: None,
        may_omit: false,
    }
}

/// Applies `change`, checks it changed something, undoes it, and checks the Book reads
/// exactly as before. Returns the Book state just after the change for further asserts.
fn round_trip(book: &mut Book, change: Change) -> (Applied, Seen) {
    let before = seen(book);
    let applied = book.apply(change).unwrap();
    let after = seen(book);
    assert_ne!(before, after, "the change should be visible");
    let outcome = book.undo(applied.revision, UndoMode::Whole).unwrap();
    assert!(matches!(outcome, UndoOutcome::Undone { .. }), "{outcome:?}");
    assert_eq!(before, seen(book), "undo should restore everything");
    (applied, after)
}

#[test]
fn create_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.cipher");
    let meta = BookMeta {
        title: "S-Class".into(),
        source_lang: "ko".into(),
        target_lang: "en".into(),
    };
    {
        let mut book = Book::create(&path, meta.clone()).unwrap();
        add_chapter(&mut book, 1, &["가", "나"]);
    }
    assert!(matches!(
        Book::create(&path, meta.clone()),
        Err(Error::Exists(_))
    ));
    let book = Book::open(&path).unwrap();
    assert_eq!(book.meta().unwrap(), meta);
    assert_eq!(book.chapters().unwrap().len(), 1);
}

#[test]
fn chapters_insert_in_number_order() {
    let Fixture { mut book, .. } = new_book();
    add_chapter(&mut book, 3, &["c"]);
    add_chapter(&mut book, 1, &["a"]);
    add_chapter(&mut book, 5, &["e"]);
    add_chapter(&mut book, 2, &["b"]);
    let numbers: Vec<_> = book
        .chapters()
        .unwrap()
        .iter()
        .map(|c| c.number.unwrap())
        .collect();
    assert_eq!(numbers, [1, 2, 3, 5]);
}

#[test]
fn add_chapter_round_trips() {
    let Fixture { mut book, .. } = new_book();
    add_chapter(&mut book, 1, &["first"]);
    let (_, after) = round_trip(
        &mut book,
        Change::AddChapter {
            number: Some(2),
            title: Some("Two".into()),
            source: vec!["하나".into(), "둘".into()],
        },
    );
    let c = &after.chapters[1];
    assert_eq!((c.blocks, c.first_line.as_str()), (2, "하나"));
    assert_eq!(c.state, ChapterState::Waiting);
}

#[test]
fn set_translation_pairs_merges_and_round_trips() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["s1", "s2", "s3", "s4"]);
    let src: Vec<BlockId> = book
        .chapter(ch)
        .unwrap()
        .source
        .iter()
        .map(|b| b.id)
        .collect();
    let pairs = vec![
        NewPair {
            source: vec![src[0]],
            translation: vec!["t1".into()],
            flagged: None,
        },
        NewPair {
            source: vec![src[1], src[2]],
            translation: vec!["t2+3".into()],
            flagged: None,
        },
        // src[3] left untranslated (a missing range)
    ];
    let (_, after) = round_trip(&mut book, Change::SetTranslation { chapter: ch, pairs });
    let c = &after.content[0];
    assert_eq!(c.pairs.len(), 2);
    let merged = c.pairs[1].id;
    assert_eq!(c.blocks_in(merged, Side::Source).count(), 2);
    assert_eq!(
        c.blocks_in(merged, Side::Translation).next().unwrap().text,
        "t2+3"
    );
    assert_eq!(c.source[3].pair, None);
    assert_eq!(after.chapters[0].state, ChapterState::Translated);
}

#[test]
fn set_translation_replaces_the_old_one() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a", "b"]);
    translate_one_to_one(&mut book, ch);
    let src: Vec<BlockId> = book
        .chapter(ch)
        .unwrap()
        .source
        .iter()
        .map(|b| b.id)
        .collect();
    book.apply(Change::SetTranslation {
        chapter: ch,
        pairs: vec![NewPair {
            source: src.clone(),
            translation: vec!["ab".into()],
            flagged: None,
        }],
    })
    .unwrap();
    let c = book.chapter(ch).unwrap();
    assert_eq!(c.pairs.len(), 1);
    assert_eq!(c.translation.len(), 1);
    assert!(c.source.iter().all(|b| b.pair == Some(c.pairs[0].id)));
}

#[test]
fn set_translation_rejects_bad_pairs() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a", "b", "c"]);
    let other = add_chapter(&mut book, 2, &["x"]);
    let src: Vec<BlockId> = book
        .chapter(ch)
        .unwrap()
        .source
        .iter()
        .map(|b| b.id)
        .collect();
    let foreign = book.chapter(other).unwrap().source[0].id;
    let p = |source: Vec<BlockId>, t: &[&str]| NewPair {
        source,
        translation: t.iter().map(|s| s.to_string()).collect(),
        flagged: None,
    };
    let cases = [
        vec![p(vec![src[0]], &[])],                             // no translation
        vec![p(vec![], &["t"])],                                // no source
        vec![p(vec![src[0], src[2]], &["t"])],                  // not contiguous
        vec![p(vec![src[0]], &["t"]), p(vec![src[0]], &["u"])], // twice
        vec![p(vec![foreign], &["t"])],                         // another Chapter
    ];
    for pairs in cases {
        let before = seen(&book);
        let r = book.apply(Change::SetTranslation {
            chapter: ch,
            pairs: pairs.clone(),
        });
        assert!(matches!(r, Err(Error::Invalid(_))), "{pairs:?} → {r:?}");
        assert_eq!(before, seen(&book), "a rejected change writes nothing");
    }
}

#[test]
fn editing_source_marks_translation_outdated() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a", "b"]);
    translate_one_to_one(&mut book, ch);
    let first = book.chapter(ch).unwrap().source[0].id;
    let (_, after) = round_trip(
        &mut book,
        Change::EditBlock {
            block: first,
            text: "a2".into(),
            hand: true,
        },
    );
    let c = &after.content[0];
    assert_eq!(c.source[0].text, "a2");
    assert!(c.translation[0].outdated);
    assert!(!c.translation[1].outdated);
    assert_eq!(after.chapters[0].outdated, 1);
}

#[test]
fn hand_edit_marks_block() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    translate_one_to_one(&mut book, ch);
    let t = book.chapter(ch).unwrap().translation[0].id;
    let (_, after) = round_trip(
        &mut book,
        Change::EditBlock {
            block: t,
            text: "mine".into(),
            hand: true,
        },
    );
    assert!(after.content[0].translation[0].hand_edited);

    let (_, after) = round_trip(
        &mut book,
        Change::EditBlock {
            block: t,
            text: "repaired".into(),
            hand: false,
        },
    );
    assert!(!after.content[0].translation[0].hand_edited);
}

#[test]
fn flag_pair_round_trips() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    translate_one_to_one(&mut book, ch);
    let pair = book.chapter(ch).unwrap().pairs[0].id;
    let (_, after) = round_trip(
        &mut book,
        Change::FlagPair {
            pair,
            reason: Some("still has Hangul".into()),
        },
    );
    assert_eq!(
        after.content[0].pairs[0].flagged.as_deref(),
        Some("still has Hangul")
    );
}

#[test]
fn renaming_a_term_forbids_the_old_rendering() {
    let Fixture { mut book, .. } = new_book();
    let id = TermId(
        book.apply(Change::UpsertTerm {
            id: None,
            fields: term("마탑주", "Magic Tower Lord"),
            status: TermStatus::Accepted,
            first_chapter: None,
        })
        .unwrap()
        .created[0],
    );
    let (applied, after) = round_trip(
        &mut book,
        Change::UpsertTerm {
            id: Some(id),
            fields: term("마탑주", "Tower Master"),
            status: TermStatus::Accepted,
            first_chapter: None,
        },
    );
    let t = &after.glossary.terms[0];
    assert_eq!(t.fields.rendering, "Tower Master");
    assert_eq!(t.forbidden, ["Magic Tower Lord"]);
    let label = book
        .revisions(10)
        .unwrap()
        .into_iter()
        .find(|r| r.id == applied.revision)
        .unwrap()
        .label;
    assert_eq!(label, "Renamed Magic Tower Lord → Tower Master");

    // Renaming back un-forbids it and forbids the other.
    book.apply(Change::UpsertTerm {
        id: Some(id),
        fields: term("마탑주", "Tower Master"),
        status: TermStatus::Accepted,
        first_chapter: None,
    })
    .unwrap();
    book.apply(Change::UpsertTerm {
        id: Some(id),
        fields: term("마탑주", "Magic Tower Lord"),
        status: TermStatus::Accepted,
        first_chapter: None,
    })
    .unwrap();
    assert_eq!(
        book.glossary().unwrap().terms[0].forbidden,
        ["Tower Master"]
    );
}

#[test]
fn glossary_changes_round_trip() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    let id = TermId(
        book.apply(Change::UpsertTerm {
            id: None,
            fields: term("아르젠트", "Argent"),
            status: TermStatus::Pending,
            first_chapter: Some(ch),
        })
        .unwrap()
        .created[0],
    );
    round_trip(
        &mut book,
        Change::SetTermStatus {
            term: id,
            status: TermStatus::Accepted,
        },
    );
    round_trip(
        &mut book,
        Change::AddForbidden {
            term: id,
            rendering: "Argento".into(),
        },
    );
    book.apply(Change::AddForbidden {
        term: id,
        rendering: "Argento".into(),
    })
    .unwrap();
    round_trip(
        &mut book,
        Change::RemoveForbidden {
            term: id,
            rendering: "Argento".into(),
        },
    );
    round_trip(&mut book, Change::DeleteTerm { term: id });
    round_trip(
        &mut book,
        Change::UpsertTerm {
            id: None,
            fields: term("테실리드", "Tesselid"),
            status: TermStatus::Accepted,
            first_chapter: None,
        },
    );

    let rule = StyleRuleId(
        round_trip(
            &mut book,
            Change::AddStyleRule {
                text: "Keep -nim as Lord".into(),
            },
        )
        .0
        .created[0],
    );
    let _ = rule;
    let rule = StyleRuleId(
        book.apply(Change::AddStyleRule { text: "r".into() })
            .unwrap()
            .created[0],
    );
    round_trip(
        &mut book,
        Change::EditStyleRule {
            id: rule,
            text: Some("r2".into()),
        },
    );
    round_trip(
        &mut book,
        Change::EditStyleRule {
            id: rule,
            text: None,
        },
    );

    let f = AddressFields {
        speaker: "Tesselid".into(),
        listener: "Argent".into(),
        source_form: "형님".into(),
        rendering: "Brother".into(),
    };
    round_trip(&mut book, Change::AddFormOfAddress(f.clone()));
    let addr = AddressId(
        book.apply(Change::AddFormOfAddress(f.clone()))
            .unwrap()
            .created[0],
    );
    round_trip(
        &mut book,
        Change::EditFormOfAddress {
            id: addr,
            fields: Some(AddressFields {
                rendering: "Big brother".into(),
                ..f
            }),
        },
    );
    round_trip(
        &mut book,
        Change::EditFormOfAddress {
            id: addr,
            fields: None,
        },
    );
}

#[test]
fn story_notes_round_trip_and_compact() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    round_trip(
        &mut book,
        Change::AddStoryNote {
            chapter: Some(ch),
            text: "n".into(),
        },
    );
    let n1 = StoryNoteId(
        book.apply(Change::AddStoryNote {
            chapter: Some(ch),
            text: "one".into(),
        })
        .unwrap()
        .created[0],
    );
    book.apply(Change::AddStoryNote {
        chapter: Some(ch),
        text: "two".into(),
    })
    .unwrap();
    round_trip(
        &mut book,
        Change::EditStoryNote {
            id: n1,
            text: Some("uno".into()),
        },
    );
    round_trip(&mut book, Change::EditStoryNote { id: n1, text: None });
    let (_, after) = round_trip(
        &mut book,
        Change::CompactStoryNotes {
            summary: "all".into(),
        },
    );
    assert_eq!(after.notes.len(), 1);
    assert_eq!(
        (after.notes[0].chapter, after.notes[0].text.as_str()),
        (None, "all")
    );
}

/// A Chapter with a translation and a rename Suggestion over its two translated Blocks.
fn with_suggestion(book: &mut Book) -> (ChapterId, SuggestionId, Vec<ProposedEditId>) {
    let ch = add_chapter(book, 1, &["마탑주가 왔다.", "마탑주는 웃었다."]);
    translate_one_to_one(book, ch);
    let c = book.chapter(ch).unwrap();
    let edits = c
        .translation
        .iter()
        .map(|b| NewProposedEdit {
            block: b.id,
            before: b.text.clone(),
            after: Some(b.text.replace("T:", "Tower Master: ")),
            origin: EditOrigin::Rename,
            skip_reason: None,
        })
        .collect();
    let s = SuggestionId(
        book.apply(Change::AddSuggestion(NewSuggestion {
            chapter: ch,
            block: c.translation[0].id,
            quote: "Magic Tower Lord".into(),
            said: "Tower Master".into(),
            scope: Scope::Everywhere,
            is_rename: true,
            source_expr: Some("마탑주".into()),
            state: SuggestionState::Ready,
            kind: SuggestionKind::Reader,
            edits,
        }))
        .unwrap()
        .created[0],
    );
    let ids = book.suggestions(SuggestionFilter::default()).unwrap()[0]
        .edits
        .iter()
        .map(|e| e.id)
        .collect();
    (ch, s, ids)
}

#[test]
fn suggestions_round_trip() {
    let Fixture { mut book, .. } = new_book();
    let (ch, s, _) = with_suggestion(&mut book);
    let block = book.chapter(ch).unwrap().translation[0].id;
    round_trip(
        &mut book,
        Change::AddProposedEdits {
            suggestion: s,
            edits: vec![NewProposedEdit {
                block,
                before: "x".into(),
                after: None,
                origin: EditOrigin::Model,
                skip_reason: Some("already fine".into()),
            }],
        },
    );
    round_trip(
        &mut book,
        Change::SetSuggestionState {
            suggestion: s,
            state: SuggestionState::Working,
            source_expr: Some("마탑".into()),
        },
    );
    assert!(matches!(
        book.apply(Change::AddSuggestion(NewSuggestion {
            chapter: ch,
            block,
            quote: "q".into(),
            said: "  ".into(),
            scope: Scope::Here,
            is_rename: false,
            source_expr: None,
            state: SuggestionState::Working,
            kind: SuggestionKind::Reader,
            edits: vec![],
        })),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn accepting_edits_rewrites_blocks_and_finishes_the_suggestion() {
    let Fixture { mut book, .. } = new_book();
    let (ch, s, edits) = with_suggestion(&mut book);
    let (applied, after) = round_trip(
        &mut book,
        Change::DecideEdits {
            decisions: vec![(edits[0], true), (edits[1], false)],
        },
    );
    let c = &after.content[0];
    assert_eq!(c.translation[0].text, "Tower Master: 마탑주가 왔다.");
    assert_eq!(c.translation[1].text, "T:마탑주는 웃었다.");
    let sug = after.suggestions.iter().find(|x| x.id == s).unwrap();
    assert_eq!(sug.state, SuggestionState::Done);
    assert_eq!(
        sug.edits.iter().map(|e| e.state).collect::<Vec<_>>(),
        [EditState::Accepted, EditState::Rejected]
    );
    let label = book
        .revisions(10)
        .unwrap()
        .into_iter()
        .find(|r| r.id == applied.revision)
        .unwrap()
        .label;
    assert_eq!(label, "Accepted 1 edit, skipped 1");

    // Open-only filter hides Done Suggestions.
    book.apply(Change::DecideEdits {
        decisions: vec![(edits[0], true), (edits[1], false)],
    })
    .unwrap();
    assert!(
        book.suggestions(SuggestionFilter {
            chapter: Some(ch),
            open_only: true
        })
        .unwrap()
        .is_empty()
    );
}

#[test]
fn an_edit_over_changed_text_goes_stale() {
    let Fixture { mut book, .. } = new_book();
    let (ch, _, edits) = with_suggestion(&mut book);
    let block = book.chapter(ch).unwrap().translation[0].id;
    book.apply(Change::EditBlock {
        block,
        text: "hand".into(),
        hand: true,
    })
    .unwrap();

    let read = &book.suggestions(SuggestionFilter::default()).unwrap()[0].edits;
    assert_eq!(
        read[0].state,
        EditState::Stale,
        "reads report it stale before deciding"
    );

    book.apply(Change::DecideEdits {
        decisions: vec![(edits[0], true)],
    })
    .unwrap();
    assert_eq!(
        book.chapter(ch).unwrap().translation[0].text,
        "hand",
        "never applied over newer text"
    );
    let read = &book.suggestions(SuggestionFilter::default()).unwrap()[0].edits;
    assert_eq!(read[0].state, EditState::Stale);
}

#[test]
fn group_is_one_revision() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    let src = book.chapter(ch).unwrap().source[0].id;
    let (applied, after) = round_trip(
        &mut book,
        Change::Group {
            kind: RevisionKind::Translation,
            label: "Translated chapter 1".into(),
            changes: vec![
                Change::SetTranslation {
                    chapter: ch,
                    pairs: vec![NewPair {
                        source: vec![src],
                        translation: vec!["A".into()],
                        flagged: None,
                    }],
                },
                Change::UpsertTerm {
                    id: None,
                    fields: term("아르젠트", "Argent"),
                    status: TermStatus::Pending,
                    first_chapter: Some(ch),
                },
                Change::AddStoryNote {
                    chapter: Some(ch),
                    text: "Argent arrives.".into(),
                },
            ],
        },
    );
    assert_eq!(applied.created.len(), 2, "term and note ids");
    assert_eq!(after.glossary.terms.len(), 1);
    assert_eq!(after.notes.len(), 1);
}

#[test]
fn undo_reports_conflicts_with_later_work() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    translate_one_to_one(&mut book, ch);
    let t = book.chapter(ch).unwrap().translation[0].id;
    let first = book
        .apply(Change::EditBlock {
            block: t,
            text: "one".into(),
            hand: true,
        })
        .unwrap();
    book.apply(Change::EditBlock {
        block: t,
        text: "two".into(),
        hand: true,
    })
    .unwrap();

    let before = seen(&book);
    let outcome = book.undo(first.revision, UndoMode::Whole).unwrap();
    assert_eq!(
        outcome,
        UndoOutcome::Conflict {
            conflicts: vec![Conflict {
                entity: "block",
                id: t.0
            }]
        }
    );
    assert_eq!(before, seen(&book), "a conflicting undo writes nothing");
}

#[test]
fn undo_of_a_chapter_with_later_dependents_conflicts() {
    let Fixture { mut book, .. } = new_book();
    let added = book
        .apply(Change::AddChapter {
            number: Some(1),
            title: None,
            source: vec!["a".into()],
        })
        .unwrap();
    translate_one_to_one(&mut book, ChapterId(added.created[0]));
    let outcome = book.undo(added.revision, UndoMode::Whole).unwrap();
    assert!(
        matches!(outcome, UndoOutcome::Conflict { .. }),
        "{outcome:?}"
    );
}

#[test]
fn undo_twice_and_redo() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    let applied = translate_one_to_one(&mut book, ch);
    let translated = seen(&book);
    let UndoOutcome::Undone { revision: undo, .. } =
        book.undo(applied.revision, UndoMode::Whole).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        book.undo(applied.revision, UndoMode::Whole).unwrap(),
        UndoOutcome::AlreadyUndone { by: undo }
    );
    // Undoing the undo is a redo. Ids are restored too, so reads match exactly.
    assert!(matches!(
        book.undo(undo, UndoMode::Whole).unwrap(),
        UndoOutcome::Undone { .. }
    ));
    assert_eq!(translated, seen(&book));
    let labels: Vec<_> = book
        .revisions(3)
        .unwrap()
        .into_iter()
        .map(|r| r.label)
        .collect();
    assert_eq!(
        labels,
        [
            "Undo: Undo: Translated chapter 1",
            "Undo: Translated chapter 1",
            "Translated chapter 1"
        ]
    );
}

#[test]
fn job_queue() {
    let Fixture { mut book, .. } = new_book();
    let c1 = add_chapter(&mut book, 1, &["a"]);
    let c2 = add_chapter(&mut book, 2, &["b"]);
    let spec = |chapter, parent| JobSpec {
        kind: JobKind::Translate,
        parent,
        chapter: Some(chapter),
        suggestion: None,
        input: None,
    };
    let j1 = book.enqueue(spec(c1, None)).unwrap();
    let j2 = book.enqueue(spec(c2, None)).unwrap();
    assert_eq!(book.chapters().unwrap()[0].state, ChapterState::Queued);

    let mut job = book.claim_next_job().unwrap().unwrap();
    assert_eq!(
        (job.id, job.state, job.attempts),
        (j1, JobState::Running, 1)
    );
    assert_eq!(book.chapters().unwrap()[0].state, ChapterState::Translating);

    // A child Repair jumps ahead of the next Chapter.
    let repair = book
        .enqueue(JobSpec {
            kind: JobKind::Repair,
            ..spec(c1, Some(j1))
        })
        .unwrap();
    job.partial = "[1] A".into();
    job.state = JobState::Checking;
    book.save_job(&job).unwrap();
    assert_eq!(book.claim_next_job().unwrap().unwrap().id, repair);

    // A worker that dies leaves Jobs running; they go back to the queue with their partial.
    assert_eq!(book.requeue_interrupted().unwrap(), 2);
    let again = book.claim_next_job().unwrap().unwrap();
    assert_eq!(again.id, repair);
    let mut again = book.claim_next_job().unwrap().unwrap();
    assert_eq!(
        (again.id, again.partial.as_str(), again.attempts),
        (j1, "[1] A", 2)
    );

    // Paused until a usage limit resets.
    again.state = JobState::Paused;
    again.resume_after = Some(i64::MAX);
    book.save_job(&again).unwrap();
    assert_eq!(book.claim_next_job().unwrap().unwrap().id, j2);
    assert!(book.claim_next_job().unwrap().is_none());
    again.resume_after = Some(0);
    book.save_job(&again).unwrap();
    assert_eq!(book.claim_next_job().unwrap().unwrap().id, j1);

    let mut failed = book.job(j2).unwrap();
    failed.state = JobState::Failed;
    failed.error = Some("auth".into());
    book.save_job(&failed).unwrap();
    assert_eq!(book.chapters().unwrap()[1].state, ChapterState::Failed);
    assert_eq!(book.jobs(true).unwrap().len(), 3);
}

#[test]
fn two_books_on_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.cipher");
    let mut writer = Book::create(
        &path,
        BookMeta {
            title: "t".into(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        },
    )
    .unwrap();
    let reader = Book::open(&path).unwrap();
    add_chapter(&mut writer, 1, &["a"]);
    assert_eq!(reader.chapters().unwrap().len(), 1);
}

#[test]
fn undoing_a_review_skips_blocks_edited_since() {
    let Fixture { mut book, .. } = new_book();
    let (ch, _, edits) = with_suggestion(&mut book);
    let review = book
        .apply(Change::DecideEdits {
            decisions: vec![(edits[0], true), (edits[1], true)],
        })
        .unwrap();
    let blocks: Vec<BlockId> = book
        .chapter(ch)
        .unwrap()
        .translation
        .iter()
        .map(|b| b.id)
        .collect();
    book.apply(Change::EditBlock {
        block: blocks[1],
        text: "mine".into(),
        hand: true,
    })
    .unwrap();

    let outcome = book.undo(review.revision, UndoMode::SkipConflicts).unwrap();
    let UndoOutcome::Undone { skipped, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        skipped,
        [Conflict {
            entity: "block",
            id: blocks[1].0
        }]
    );
    let c = book.chapter(ch).unwrap();
    assert_eq!(
        c.translation[0].text, "T:마탑주가 왔다.",
        "the untouched Block is reverted"
    );
    assert_eq!(c.translation[1].text, "mine", "the edited one is kept");
}

#[test]
fn a_failed_job_can_be_retried() {
    let Fixture { mut book, .. } = new_book();
    let ch = add_chapter(&mut book, 1, &["a"]);
    let id = book
        .enqueue(JobSpec {
            kind: JobKind::Translate,
            parent: None,
            chapter: Some(ch),
            suggestion: None,
            input: Some("progress".into()),
        })
        .unwrap();
    assert!(
        matches!(book.retry_job(id), Err(Error::Invalid(_))),
        "a queued Job is not retried"
    );
    let mut job = book.claim_next_job().unwrap().unwrap();
    job.state = JobState::Failed;
    job.error = Some("auth".into());
    book.save_job(&job).unwrap();
    book.retry_job(id).unwrap();
    let job = book.claim_next_job().unwrap().unwrap();
    assert_eq!(
        (job.id, job.attempts, job.error, job.input.as_deref()),
        (id, 1, None, Some("progress"))
    );
}

#[test]
fn an_in_memory_book_works_like_a_file_and_titles_can_change() {
    let meta = BookMeta {
        title: "Scratch".into(),
        source_lang: "ko".into(),
        target_lang: "en".into(),
    };
    let mut book = Book::in_memory(meta).unwrap();
    assert_eq!(book.path(), std::path::Path::new(""));
    assert!(book.chapters().unwrap().is_empty());
    book.apply(Change::AddChapter {
        number: Some(1),
        title: None,
        source: vec!["바람.".into()],
    })
    .unwrap();
    assert_eq!(book.chapters().unwrap().len(), 1);

    book.set_title("  The Tower  ").unwrap();
    assert_eq!(book.meta().unwrap().title, "The Tower");
    assert!(book.set_title(" ").is_err());
}

#[test]
fn a_book_remembers_its_model_choice_and_can_forget_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.cipher");
    let meta = cipher_book::BookMeta {
        title: "M".into(),
        source_lang: "ko".into(),
        target_lang: "en".into(),
    };
    let mut book = cipher_book::Book::create(&path, meta).unwrap();
    assert_eq!(book.model_choice().unwrap(), None);
    let choice = cipher_book::ModelChoice {
        endpoint: "e1".into(),
        model: "model-b".into(),
    };
    book.set_model_choice(Some(&choice)).unwrap();
    drop(book);
    let mut book = cipher_book::Book::open(&path).unwrap();
    assert_eq!(book.model_choice().unwrap(), Some(choice));
    book.set_model_choice(None).unwrap();
    assert_eq!(book.model_choice().unwrap(), None);
}
