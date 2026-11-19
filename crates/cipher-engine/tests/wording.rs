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
    assert_eq!(term(&book, "Magic Scribe").forbidden, ["mana thread"]);

    // Adopting the translation's wording renames the Glossary's instead, and spares other Terms.
    let t = term(&book, "Magic Tower");
    cipher_engine::use_wording(&mut book, t.id, "Mage Tower", &[]).unwrap();
    let all: Vec<String> = book
        .suggestions(SuggestionFilter::default())
        .unwrap()
        .into_iter()
        .flat_map(|s| s.edits.into_iter().filter_map(|e| e.after))
        .collect();
    assert!(all.contains(&"The Mage Tower came into view.".to_string()));
    assert!(!all.iter().any(|a| a.contains("Blaiver Mage Tower")));
    assert_eq!(term(&book, "Mage Tower").forbidden, ["Magic Tower"]);
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
