use cipher_book::{
    Book, BookMeta, Change, ChapterId, ChapterState, JobKind, JobSpec, NewPair, Side, TermFields,
    TermStatus,
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
