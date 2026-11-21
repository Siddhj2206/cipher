use cipher_book::*;

fn book(dir: &tempfile::TempDir) -> Book {
    Book::create(
        dir.path().join("merge.cipher"),
        BookMeta {
            title: "Test".into(),
            source_lang: "ko".into(),
            target_lang: "en".into(),
        },
    )
    .unwrap()
}

fn term(book: &mut Book, source: &str, rendering: &str, status: TermStatus) -> TermId {
    TermId(
        book.apply(Change::UpsertTerm {
            id: None,
            fields: TermFields {
                source: source.into(),
                rendering: rendering.into(),
                note: Some(format!("Note for {rendering}")),
                ..Default::default()
            },
            status,
            first_chapter: None,
        })
        .unwrap()
        .created[0],
    )
}

fn forbid(book: &mut Book, term: TermId, rendering: &str) {
    book.apply(Change::AddForbidden {
        term,
        rendering: rendering.into(),
    })
    .unwrap();
}

#[test]
fn merge_deduplicates_existing_forms_and_forbidden_renderings_and_undo_restores_both() {
    use TermStatus::*;
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let keep = term(&mut book, "포혈공", "Blood Duke", Accepted);
    let drop = term(&mut book, "포혈공", "Duke of Blood", Pending);
    let alias = term(&mut book, "포혈공(怖血公)", "Duke of Blood", Pending);
    let preview = book.preview_term_merge(drop, alias).unwrap();
    assert!(
        preview.forbidden().is_empty(),
        "same rendering must not forbid itself"
    );
    book.apply(Change::MergeTerms(Box::new(preview))).unwrap();
    forbid(&mut book, keep, "Blood Lord");
    forbid(&mut book, drop, "blood lord");
    forbid(&mut book, drop, "Ancient Duke");
    forbid(&mut book, drop, "BLOOD DUKE");
    let before = book.glossary().unwrap();
    let revision_count = book.revisions(100).unwrap().len();

    let preview = book.preview_term_merge(keep, drop).unwrap();
    assert_eq!(preview.aliases(), ["포혈공(怖血公)"]);
    assert_eq!(
        preview.forbidden(),
        ["Ancient Duke", "Blood Lord", "Duke of Blood"]
    );
    let rev = book
        .apply(Change::MergeTerms(Box::new(preview)))
        .unwrap()
        .revision;
    let after = book.glossary().unwrap();
    assert_eq!(after.terms.len(), 1);
    let survivor = &after.terms[0];
    assert_eq!(survivor.id, keep);
    assert_eq!(survivor.status, Accepted);
    assert_eq!(survivor.fields, before.terms[0].fields);
    assert_eq!(
        survivor.source_forms().collect::<Vec<_>>(),
        ["포혈공", "포혈공(怖血公)"]
    );
    assert_eq!(
        survivor.forbidden,
        ["Ancient Duke", "Blood Lord", "Duke of Blood"]
    );
    assert_eq!(book.revisions(100).unwrap().len(), revision_count + 1);

    let mut reopened = Book::open(book.path()).unwrap();
    let undo = reopened.preview_undo(rev).unwrap();
    assert_eq!(undo.revision.kind, RevisionKind::TermMerge);
    assert!(undo.conflicts.is_empty());
    assert!(
        undo.changes
            .iter()
            .all(|(entity, _)| ["term", "term_alias", "forbidden_rendering"].contains(entity))
    );
    assert!(matches!(
        reopened.undo(rev, UndoMode::Whole).unwrap(),
        UndoOutcome::Undone { .. }
    ));
    assert_eq!(reopened.glossary().unwrap(), before);
}

#[test]
fn stale_merges_and_partial_undo_do_not_write_anything() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = book(&dir);
    let keep = term(&mut book, "가나다", "Tower", TermStatus::Accepted);
    let duplicate = term(&mut book, "라마바", "Spire", TermStatus::Pending);
    assert!(book.preview_term_merge(keep, keep).is_err());
    let stale = book.preview_term_merge(keep, duplicate).unwrap();
    forbid(&mut book, duplicate, "Obelisk");
    let before = book.glossary().unwrap();
    let revisions = book.revisions(100).unwrap();
    assert!(book.apply(Change::MergeTerms(Box::new(stale))).is_err());
    assert_eq!(book.glossary().unwrap(), before);
    assert_eq!(book.revisions(100).unwrap(), revisions);

    let merge = book.preview_term_merge(keep, duplicate).unwrap();
    let rev = book
        .apply(Change::MergeTerms(Box::new(merge)))
        .unwrap()
        .revision;
    book.apply(Change::RemoveForbidden {
        term: keep,
        rendering: "Spire".into(),
    })
    .unwrap();
    let after = book.glossary().unwrap();
    assert!(!book.preview_undo(rev).unwrap().conflicts.is_empty());
    assert!(matches!(
        book.undo(rev, UndoMode::SkipConflicts).unwrap(),
        UndoOutcome::Conflict { .. }
    ));
    assert_eq!(book.glossary().unwrap(), after);
}
