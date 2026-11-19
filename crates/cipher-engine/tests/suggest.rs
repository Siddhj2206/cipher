//! Suggestions through `reach`, `suggest` and `Worker::step`, with a scripted model.

use cipher_book::*;
use cipher_engine::{Models, Selection, SuggestionRequest, Worker, reach, suggest};
use cipher_llm::{Fake, FakeReply};

struct Setup {
    _dir: tempfile::TempDir,
    book: Book,
    chapters: Vec<ChapterId>,
}

/// Three Chapters. 마탑주 is rendered "Magic Tower Lord" four times and once as
/// "Tower Lord of Magic". 한숨 ("a sigh") is a one-off phrase.
fn setup() -> Setup {
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
    let text: [&[(&str, &str)]; 3] = [
        &[
            ("마탑주가 왔다.", "The Magic Tower Lord came."),
            ("바람이 불었다.", "The wind blew."),
        ],
        &[
            ("마탑주는 웃었다.", "The Magic Tower Lord smiled."),
            (
                "마탑주의 지팡이가 빛났다.",
                "The Magic Tower Lord's staff shone.",
            ),
        ],
        &[
            ("마탑주가 떠났다.", "The Tower Lord of Magic left."),
            ("그는 한숨을 쉬었다.", "He let out a breath."),
            ("마탑주가 돌아왔다.", "The Magic Tower Lord returned."),
        ],
    ];
    let mut chapters = Vec::new();
    for (n, pairs) in text.iter().enumerate() {
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
                .zip(pairs.iter())
                .map(|(b, (_, t))| NewPair {
                    source: vec![b.id],
                    translation: vec![t.to_string()],
                    flagged: None,
                })
                .collect(),
        })
        .unwrap();
        chapters.push(ch);
    }
    book.apply(Change::UpsertTerm {
        id: None,
        fields: TermFields {
            source: "마탑주".into(),
            rendering: "Magic Tower Lord".into(),
            ..Default::default()
        },
        status: TermStatus::Accepted,
        first_chapter: None,
    })
    .unwrap();
    book.apply(Change::AddStoryNote {
        chapter: Some(chapters[0]),
        text: "The Magic Tower Lord arrives.".into(),
    })
    .unwrap();
    Setup {
        _dir: dir,
        book,
        chapters,
    }
}

fn selection(book: &Book, ch: ChapterId, i: usize, text: &str) -> Selection {
    Selection {
        chapter: ch,
        block: book.chapter(ch).unwrap().translation[i].id,
        text: text.into(),
    }
}

fn worker(book: &Book, fake: &Fake) -> Worker {
    Worker::new(
        Book::open(book.path()).unwrap(),
        Models::new(Box::new(fake.clone())),
    )
    .unwrap()
}

fn run_all(w: &mut Worker) {
    while w.step(&mut |_| {}).unwrap() {}
}

fn only_suggestion(book: &Book) -> Suggestion {
    let mut all = book.suggestions(SuggestionFilter::default()).unwrap();
    assert_eq!(all.len(), 1);
    all.remove(0)
}

#[test]
fn reach_counts_both_sides() {
    let Setup {
        book,
        chapters,
        _dir,
    } = setup();
    let sel = selection(&book, chapters[0], 0, "Magic Tower Lord");
    let r = reach(&book, &sel, Scope::Everywhere).unwrap();
    assert_eq!(r.source_expr.as_deref(), Some("마탑주"));
    assert!(r.term.is_some());
    assert_eq!(
        (r.blocks, r.chapters, r.exact),
        (5, 3, 4),
        "four exact plus the one rendered otherwise"
    );
    let here = reach(&book, &sel, Scope::Here).unwrap();
    assert_eq!((here.blocks, here.chapters), (1, 1));
}

#[test]
fn a_rename_is_local_except_where_the_name_was_rendered_otherwise() {
    let Setup {
        mut book,
        chapters,
        _dir,
    } = setup();
    let fake = Fake::new([FakeReply::Text("[1] The Tower Master left.".into())]);
    let sel = selection(&book, chapters[0], 0, "Magic Tower Lord");
    suggest(
        &mut book,
        SuggestionRequest {
            selection: sel,
            said: "Tower Master".into(),
            scope: Scope::Everywhere,
            rename: true,
        },
    )
    .unwrap();

    let s = only_suggestion(&book);
    assert_eq!(s.state, SuggestionState::Working);
    assert_eq!(s.edits.len(), 4);
    assert!(s.edits.iter().all(|e| e.origin == EditOrigin::Rename));
    assert!(
        s.edits
            .iter()
            .any(|e| e.after.as_deref() == Some("The Tower Master's staff shone."))
    );

    let mut w = worker(&book, &fake);
    run_all(&mut w);
    let req = &fake.requests()[0];
    assert!(
        req.input.contains("SOURCE: 마탑주가 떠났다."),
        "{}",
        req.input
    );
    assert!(
        req.input
            .contains("translate the source expression: 마탑주")
    );
    assert!(
        !req.input.contains("[2]"),
        "only the Block rendered otherwise goes to the model"
    );

    let s = only_suggestion(&book);
    assert_eq!(s.state, SuggestionState::Ready);
    let model: Vec<_> = s
        .edits
        .iter()
        .filter(|e| e.origin == EditOrigin::Model)
        .collect();
    assert_eq!(model[0].after.as_deref(), Some("The Tower Master left."));

    // Accept everything: one Review Revision.
    let decisions = s.edits.iter().map(|e| (e.id, true)).collect();
    book.apply(Change::DecideEdits { decisions }).unwrap();
    let all: Vec<String> = chapters
        .iter()
        .flat_map(|&c| book.chapter(c).unwrap().translation)
        .map(|b| b.text)
        .collect();
    assert!(
        all.iter()
            .all(|t| !t.contains("Magic Tower Lord") && !t.contains("Tower Lord of Magic")),
        "{all:?}"
    );
    assert_eq!(only_suggestion(&book).state, SuggestionState::Done);
}

#[test]
fn from_now_on_renames_the_term_and_story_notes() {
    let Setup {
        mut book,
        chapters,
        _dir,
    } = setup();
    let sel = selection(&book, chapters[0], 0, "Magic Tower Lord");
    suggest(
        &mut book,
        SuggestionRequest {
            selection: sel,
            said: "Tower Master".into(),
            scope: Scope::FromNowOn,
            rename: true,
        },
    )
    .unwrap();
    let term = book.glossary().unwrap().terms.remove(0);
    assert_eq!(term.fields.rendering, "Tower Master");
    assert_eq!(term.forbidden, ["Magic Tower Lord"]);
    assert_eq!(
        book.story_notes().unwrap()[0].text,
        "The Tower Master arrives."
    );
}

#[test]
fn here_without_rename_asks_the_model_about_one_block() {
    let Setup {
        mut book,
        chapters,
        _dir,
    } = setup();
    let fake = Fake::new([FakeReply::Text(
        "[1] SKIP: the wording is already natural".into(),
    )]);
    let sel = selection(&book, chapters[0], 1, "The wind blew.");
    suggest(
        &mut book,
        SuggestionRequest {
            selection: sel,
            said: "too flat".into(),
            scope: Scope::Here,
            rename: false,
        },
    )
    .unwrap();
    run_all(&mut worker(&book, &fake));
    let s = only_suggestion(&book);
    assert_eq!(s.state, SuggestionState::Ready);
    assert_eq!(s.edits.len(), 1);
    assert_eq!(s.edits[0].after, None);
    assert_eq!(
        s.edits[0].skip_reason.as_deref(),
        Some("the wording is already natural")
    );
}

#[test]
fn a_one_off_gets_a_tiny_resolve_call_whose_answer_must_be_in_the_source() {
    let Setup {
        mut book,
        chapters,
        _dir,
    } = setup();
    let fake = Fake::new([
        FakeReply::Text("한숨".into()),
        FakeReply::Text("[1] He sighed.".into()),
    ]);
    let sel = selection(&book, chapters[2], 1, "let out a breath");
    suggest(
        &mut book,
        SuggestionRequest {
            selection: sel,
            said: "just say sighed".into(),
            scope: Scope::Everywhere,
            rename: false,
        },
    )
    .unwrap();
    let mut w = worker(&book, &fake);
    run_all(&mut w);

    let reqs = fake.requests();
    assert!(reqs[0].input.contains("ENGLISH WORDS: let out a breath"));
    let s = only_suggestion(&book);
    assert_eq!(s.source_expr.as_deref(), Some("한숨"));
    assert_eq!(s.state, SuggestionState::Ready);
    assert_eq!(s.edits.len(), 1);
    assert_eq!(s.edits[0].after.as_deref(), Some("He sighed."));
}

#[test]
fn a_resolve_answer_not_in_the_source_is_ignored() {
    let Setup {
        mut book,
        chapters,
        _dir,
    } = setup();
    let fake = Fake::new([
        FakeReply::Text("숨을 내쉬다".into()),
        FakeReply::Text("[1] He sighed.".into()),
    ]);
    let sel = selection(&book, chapters[2], 1, "let out a breath");
    suggest(
        &mut book,
        SuggestionRequest {
            selection: sel,
            said: "sighed".into(),
            scope: Scope::Everywhere,
            rename: false,
        },
    )
    .unwrap();
    run_all(&mut worker(&book, &fake));
    let s = only_suggestion(&book);
    assert_eq!(s.source_expr, None);
    assert_eq!(
        s.edits.len(),
        1,
        "the Selection's own Block is still handled"
    );
}

#[test]
fn an_edit_proposed_before_a_hand_edit_goes_stale() {
    let Setup {
        mut book,
        chapters,
        _dir,
    } = setup();
    let sel = selection(&book, chapters[0], 0, "Magic Tower Lord");
    suggest(
        &mut book,
        SuggestionRequest {
            selection: sel.clone(),
            said: "Tower Master".into(),
            scope: Scope::Here,
            rename: true,
        },
    )
    .unwrap();
    book.apply(Change::EditBlock {
        block: sel.block,
        text: "Someone came.".into(),
        hand: true,
    })
    .unwrap();
    let s = only_suggestion(&book);
    assert_eq!(s.edits[0].state, EditState::Stale);
}
