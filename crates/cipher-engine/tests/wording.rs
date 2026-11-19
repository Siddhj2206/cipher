//! A Term's wordings: the evidence, choosing one, and the opt-in model step.

use cipher_book::*;

/// One Chapter per line of Pairs. 마력사 is "Magic Scribe" in the Glossary; the translation
/// mostly says "mana threads", once "Magic Scribe" and once nothing. 블레이버 마탑 is its own
/// Term, "Blaiver Magic Tower", and 마탑 inside it is not 마탑.
fn book(dir: &tempfile::TempDir) -> Book {
    let mut book = Book::create(
        dir.path().join("w.cipher"),
        BookMeta {
            title: "W".into(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        },
    )
    .unwrap();
    let mut chapters: Vec<Vec<(&str, &str)>> = vec![
        vec![
            ("마력사가 움직였다.", "The mana threads moved."),
            ("마력사를 당겼다.", "He pulled a mana thread."),
            ("마력사가 끊어졌다.", "Mana Threads snapped."),
        ],
        vec![
            ("마력사가 빛났다.", "The mana threads shone."),
            ("마력사로 묶었다.", "He tied it with the Magic Scribe."),
            ("마력사를 거두었다.", "He drew them back."),
            (
                "블레이버 마탑의 마법사.",
                "A wizard of the Blaiver Magic Tower.",
            ),
            ("마탑이 보였다.", "The Magic Tower came into view."),
        ],
    ];
    // Pairs that don't name it, so "moved" or "pulled" aren't enriched.
    chapters.push(
        (0..12)
            .map(|_| {
                (
                    "바람이 불었다.",
                    "The wind moved, and he pulled and tied and drew.",
                )
            })
            .collect(),
    );
    for (n, pairs) in chapters.iter().enumerate() {
        let ch = ChapterId(
            book.apply(Change::AddChapter {
                number: Some(n as i64 + 1),
                title: None,
                source: pairs.iter().map(|(s, _)| s.to_string()).collect(),
            })
            .unwrap()
            .created[0],
        );
        let src = book.chapter(ch).unwrap().source;
        book.apply(Change::SetTranslation {
            chapter: ch,
            pairs: src
                .iter()
                .zip(pairs)
                .map(|(b, (_, t))| NewPair {
                    source: vec![b.id],
                    translation: vec![t.to_string()],
                    flagged: None,
                })
                .collect(),
        })
        .unwrap();
    }
    for (source, rendering) in [
        ("마력사", "Magic Scribe"),
        ("블레이버 마탑", "Blaiver Magic Tower"),
        ("마탑", "Magic Tower"),
    ] {
        book.apply(Change::UpsertTerm {
            id: None,
            fields: TermFields {
                source: source.into(),
                rendering: rendering.into(),
                ..Default::default()
            },
            status: TermStatus::Accepted,
            first_chapter: None,
        })
        .unwrap();
    }
    book
}

fn term(book: &Book, rendering: &str) -> Term {
    book.glossary()
        .unwrap()
        .terms
        .into_iter()
        .find(|t| t.fields.rendering == rendering)
        .unwrap()
}

#[test]
fn evidence_counts_each_wording_where_the_source_names_the_term() {
    let dir = tempfile::tempdir().unwrap();
    let book = book(&dir);
    let ev = cipher_engine::evidence(&book, term(&book, "Magic Scribe").id).unwrap();
    assert_eq!(ev.naming, 6);
    assert_eq!(ev.rendered.pairs.len(), 1);
    let shown: Vec<(&str, usize)> = ev
        .wordings
        .iter()
        .map(|w| (w.text.as_str(), w.seen.pairs.len()))
        .collect();
    assert_eq!(
        shown,
        [("mana threads", 4)],
        "plural, case and number are one wording"
    );
    assert_eq!(ev.wordings[0].seen.chapters, Some((1, 2)));
    assert_eq!(ev.neither, 1, "He drew them back.");

    let tower = cipher_engine::evidence(&book, term(&book, "Magic Tower").id).unwrap();
    assert_eq!(
        tower.naming, 1,
        "마탑 inside 블레이버 마탑 is the longer Term"
    );
}

#[test]
fn using_a_wording_renames_what_the_user_marked_and_forbids_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let t = term(&book, "Magic Scribe");
    let id = cipher_engine::use_wording(&mut book, t.id, "Magic Scribe", &["mana threads".into()])
        .unwrap()
        .expect("something to rename");
    let s = book
        .suggestions(SuggestionFilter::default())
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap();
    assert_eq!(s.scope, Scope::FromNowOn);
    let afters: Vec<&str> = s.edits.iter().filter_map(|e| e.after.as_deref()).collect();
    assert_eq!(
        afters,
        [
            "The Magic Scribes moved.",
            "He pulled a Magic Scribe.",
            "Magic Scribes snapped.",
            "The Magic Scribes shone.",
        ]
    );
    assert!(term(&book, "Magic Scribe").forbidden.is_empty());
    book.apply(Change::DecideMetadata {
        decisions: s.metadata.iter().map(|e| (e.id, true)).collect(),
    })
    .unwrap();
    assert_eq!(term(&book, "Magic Scribe").forbidden, ["mana thread"]);

    // Adopting the translation's wording renames the Glossary's instead, and spares other Terms.
    let t = term(&book, "Magic Tower");
    let id = cipher_engine::use_wording(&mut book, t.id, "Mage Tower", &[])
        .unwrap()
        .unwrap();
    let all: Vec<String> = book
        .suggestions(SuggestionFilter::default())
        .unwrap()
        .into_iter()
        .flat_map(|s| s.edits.into_iter().filter_map(|e| e.after))
        .collect();
    assert!(all.contains(&"The Mage Tower came into view.".to_string()));
    assert!(!all.iter().any(|a| a.contains("Blaiver Mage Tower")));
    let s = suggestion(&book, id);
    book.apply(Change::DecideMetadata {
        decisions: s.metadata.iter().map(|e| (e.id, true)).collect(),
    })
    .unwrap();
    assert_eq!(term(&book, "Mage Tower").forbidden, ["Magic Tower"]);
}

fn suggestion(book: &Book, id: SuggestionId) -> Suggestion {
    book.suggestions(SuggestionFilter::default())
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
}

fn staged(book: &mut Book) -> SuggestionId {
    book.apply(Change::AddStoryNote {
        chapter: None,
        text: "The Magic Scribe follows Lennok.".into(),
    })
    .unwrap();
    let t = term(book, "Magic Scribe");
    let glossary = book.glossary().unwrap();
    let notes = book.story_notes().unwrap();
    let chapters: Vec<_> = book
        .chapters()
        .unwrap()
        .iter()
        .map(|ch| book.chapter(ch.id).unwrap())
        .collect();
    let id = cipher_engine::use_wording(book, t.id, "mana thread", &[])
        .unwrap()
        .unwrap();
    let reopened = Book::open(book.path()).unwrap();
    assert_eq!(reopened.glossary().unwrap(), glossary);
    assert_eq!(reopened.story_notes().unwrap(), notes);
    for chapter in chapters {
        assert_eq!(reopened.chapter(chapter.id).unwrap(), chapter);
    }
    id
}

#[test]
fn keep_survives_restart_without_writing_content_and_review_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let id = staged(&mut book);
    let glossary = book.glossary().unwrap();
    let notes = book.story_notes().unwrap();
    let path = book.path().to_owned();
    drop(book);
    let mut book = Book::open(path).unwrap();
    assert_eq!(book.glossary().unwrap(), glossary);
    assert_eq!(book.story_notes().unwrap(), notes);
    let s = suggestion(&book, id);
    assert_eq!(s.metadata.len(), 2);
    assert!(s.metadata.iter().all(|e| e.state == EditState::Pending));
    assert_eq!(s.edits.len(), 1);
    book.apply(Change::DecideEdits {
        decisions: vec![(s.edits[0].id, true)],
    })
    .unwrap();
    assert_eq!(
        book.glossary().unwrap(),
        glossary,
        "prose acceptance does not confirm Book changes"
    );
    assert_eq!(book.story_notes().unwrap(), notes);
    assert_eq!(suggestion(&book, id).state, SuggestionState::Ready);
    let applied = book
        .apply(Change::DecideMetadata {
            decisions: s.metadata.iter().map(|e| (e.id, true)).collect(),
        })
        .unwrap();
    assert_eq!(term(&book, "mana thread").forbidden, ["Magic Scribe"]);
    assert_eq!(
        book.story_notes().unwrap()[0].text,
        "The Mana thread follows Lennok."
    );
    assert_eq!(suggestion(&book, id).state, SuggestionState::Done);
    let reopened = Book::open(book.path()).unwrap();
    assert_eq!(term(&reopened, "mana thread").forbidden, ["Magic Scribe"]);
    assert_eq!(
        reopened.story_notes().unwrap()[0].text,
        "The Mana thread follows Lennok."
    );
    book.undo(applied.revision, UndoMode::Whole).unwrap();
    assert_eq!(book.glossary().unwrap(), glossary);
    assert_eq!(book.story_notes().unwrap(), notes);
    assert!(
        suggestion(&book, id)
            .metadata
            .iter()
            .all(|e| e.state == EditState::Pending)
    );
}

#[test]
fn rejecting_everything_leaves_glossary_notes_and_prose_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let id = staged(&mut book);
    let glossary = book.glossary().unwrap();
    let notes = book.story_notes().unwrap();
    let s = suggestion(&book, id);
    let chapter = book.chapter(s.chapter).unwrap();
    book.apply(Change::Group {
        kind: RevisionKind::Review,
        label: "Rejected Drift changes".into(),
        changes: vec![
            Change::DecideEdits {
                decisions: s.edits.iter().map(|e| (e.id, false)).collect(),
            },
            Change::DecideMetadata {
                decisions: s.metadata.iter().map(|e| (e.id, false)).collect(),
            },
        ],
    })
    .unwrap();
    assert_eq!(book.glossary().unwrap(), glossary);
    assert_eq!(book.story_notes().unwrap(), notes);
    assert_eq!(book.chapter(s.chapter).unwrap(), chapter);
    assert_eq!(suggestion(&book, id).state, SuggestionState::Done);
}

#[test]
fn partial_accept_keeps_other_items_pending_and_cannot_apply_twice() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let id = staged(&mut book);
    let notes = book.story_notes().unwrap();
    let s = suggestion(&book, id);
    let accept = || Change::DecideMetadata {
        decisions: vec![(s.metadata[0].id, true)],
    };
    book.apply(accept()).unwrap();
    assert_eq!(term(&book, "mana thread").forbidden, ["Magic Scribe"]);
    assert_eq!(book.story_notes().unwrap(), notes);
    let s = suggestion(&book, id);
    assert_eq!(s.metadata[1].state, EditState::Pending);
    assert_eq!(s.edits[0].state, EditState::Pending);
    let t = term(&book, "mana thread");
    book.apply(Change::UpsertTerm {
        id: Some(t.id),
        fields: TermFields {
            rendering: "A newer name".into(),
            ..t.fields
        },
        status: t.status,
        first_chapter: None,
    })
    .unwrap();
    book.apply(accept()).unwrap();
    assert_eq!(term(&book, "A newer name").id, t.id);
}

#[test]
fn changed_or_deleted_metadata_is_stale_and_never_clobbered() {
    for delete in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut book = book(&dir);
        let id = staged(&mut book);
        let t = term(&book, "Magic Scribe");
        let note = book.story_notes().unwrap()[0].clone();
        if delete {
            book.apply(Change::DeleteTerm { term: t.id }).unwrap();
        } else {
            book.apply(Change::UpsertTerm {
                id: Some(t.id),
                fields: TermFields {
                    note: Some("a newer note".into()),
                    ..t.fields
                },
                status: t.status,
                first_chapter: None,
            })
            .unwrap();
        }
        book.apply(Change::EditStoryNote {
            id: note.id,
            text: (!delete).then(|| "Newer story facts".into()),
        })
        .unwrap();
        let s = suggestion(&book, id);
        assert!(s.metadata.iter().all(|e| e.state == EditState::Stale));
        let glossary = book.glossary().unwrap();
        let notes = book.story_notes().unwrap();
        book.apply(Change::DecideMetadata {
            decisions: s.metadata.iter().map(|e| (e.id, true)).collect(),
        })
        .unwrap();
        assert_eq!(book.glossary().unwrap(), glossary);
        assert_eq!(book.story_notes().unwrap(), notes);
        assert!(
            suggestion(&book, id)
                .metadata
                .iter()
                .all(|e| e.state == EditState::Stale)
        );
        book.apply(Change::DecideMetadata {
            decisions: s.metadata.iter().map(|e| (e.id, false)).collect(),
        })
        .unwrap();
        assert!(
            suggestion(&book, id)
                .metadata
                .iter()
                .all(|e| e.state == EditState::Rejected)
        );
    }
}

#[test]
fn no_prose_replacements_still_stages_book_changes_for_review() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let t = term(&book, "Magic Scribe");
    for ch in book.chapters().unwrap() {
        for b in book.chapter(ch.id).unwrap().translation {
            if b.text.contains("Magic Scribe") {
                book.apply(Change::EditBlock {
                    block: b.id,
                    text: b.text.replace("Magic Scribe", "mana thread"),
                    hand: true,
                })
                .unwrap();
            }
        }
    }
    let id = staged(&mut book);
    let s = suggestion(&book, id);
    assert!(s.edits.is_empty());
    assert_eq!(s.metadata.len(), 2);
    assert_eq!(term(&book, "Magic Scribe").id, t.id);
    book.apply(Change::DecideMetadata {
        decisions: s.metadata.iter().map(|e| (e.id, true)).collect(),
    })
    .unwrap();
    assert_eq!(term(&book, "mana thread").id, t.id);
    assert_eq!(
        book.story_notes().unwrap()[0].text,
        "The Mana thread follows Lennok."
    );
    assert_eq!(suggestion(&book, id).state, SuggestionState::Done);
}

#[test]
fn keeping_an_unchanged_wording_is_empty_and_unknown_terms_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let t = term(&book, "Magic Scribe");
    let revisions = book.revisions(100).unwrap();
    assert_eq!(
        cipher_engine::use_wording(&mut book, t.id, "Magic Scribe", &[]).unwrap(),
        None
    );
    assert!(cipher_engine::use_wording(&mut book, TermId(-1), "Magic Scribe", &[]).is_err());
    assert_eq!(book.revisions(100).unwrap(), revisions);
}

#[test]
fn asking_the_model_sends_only_pairs_that_name_it_neither_way() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let t = term(&book, "Magic Scribe");
    cipher_engine::ask_model(&mut book, t.id)
        .unwrap()
        .expect("one Pair");
    let inputs: Vec<String> = book
        .jobs(false)
        .unwrap()
        .into_iter()
        .filter(|j| j.kind == JobKind::SuggestBatch)
        .filter_map(|j| j.input)
        .collect();
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].matches(',').count(),
        0,
        "one Block: He drew them back."
    );
}

#[test]
fn a_model_answer_that_rewrites_the_paragraph_is_left_alone_with_the_reason() {
    use cipher_engine::{Models, Worker};
    use cipher_llm::{Fake, FakeReply};
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let t = term(&book, "Magic Scribe");
    let id = cipher_engine::ask_model(&mut book, t.id).unwrap().unwrap();
    // An answer for some other paragraph: it names the Term but keeps none of the words.
    let fake = Fake::new([FakeReply::Text(
        "[1] Lennok scattered a hundred Magic Scribes across the burning sky at once.".into(),
    )]);
    let mut w = Worker::new(
        Book::open(book.path()).unwrap(),
        Models::new(Box::new(fake)),
    )
    .unwrap();
    while w.step(&mut |_| {}).unwrap() {}
    let s = Book::open(book.path())
        .unwrap()
        .suggestions(SuggestionFilter::default())
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap();
    assert_eq!(s.edits.len(), 1);
    assert_eq!(s.edits[0].after, None);
    assert!(
        s.edits[0]
            .skip_reason
            .as_deref()
            .unwrap()
            .contains("rewrote more than the name"),
        "{:?}",
        s.edits[0].skip_reason
    );
}
