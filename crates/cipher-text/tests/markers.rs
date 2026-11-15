#![allow(clippy::type_complexity)] // table tests use tuple-of-slices cases

use cipher_text::markers::{NewTerm, Parsed, parse, write};

const CLEAN: &str = include_str!("fixtures/response_clean.txt");

/// (first, last, text) of every accepted pair.
fn pairs(p: &Parsed) -> Vec<(u32, u32, &str)> {
    p.pairs
        .iter()
        .map(|p| (p.first, p.last, p.text.as_str()))
        .collect()
}

#[test]
fn write_flattens_and_separates() {
    assert_eq!(
        write(&[(1, "one"), (2, "two\nlines\r\n  here"), (10, ""), (11, "x")]),
        "[1] one\n\n[2] two lines here\n\n[10]\n\n[11] x"
    );
    assert_eq!(write(&[]), "");
}

#[test]
fn write_then_parse_roundtrips() {
    let blocks = [(5, "첫째"), (6, "둘째 줄\n셋째 줄"), (7, "넷째")];
    let p = parse(&write(&blocks), 5..=7, true);
    assert_eq!(
        pairs(&p),
        [(5, 5, "첫째"), (6, 6, "둘째 줄 셋째 줄"), (7, 7, "넷째")]
    );
    assert!(p.missing.is_empty() && p.duplicates.is_empty() && p.out_of_range.is_empty());
}

#[test]
fn clean_response_with_merge_split_and_trailer() {
    let p = parse(CLEAN, 1..=7, true);
    assert_eq!(
        pairs(&p),
        [
            (1, 1, "Chapter 1"),
            (2, 2, "Tesselid opened the door. Nobody was inside."),
            (3, 3, "\"Who's there?\""),
            (4, 4, "***"),
            (5, 6, "Han did not answer. Korea's winter was long."),
            // A split: the line break inside the entry becomes a paragraph break.
            (7, 7, "- First door\n\nSecond door"),
        ]
    );
    assert!(p.missing.is_empty());
    assert_eq!(
        p.new_terms,
        [
            NewTerm {
                source: "테셀리드".into(),
                rendering: "Tesselid".into(),
                kind: Some("name".into()),
                gender: Some("male".into()),
            },
            NewTerm {
                source: "한".into(),
                rendering: "Han".into(),
                kind: Some("name".into()),
                gender: None,
            },
        ]
    );
    assert_eq!(p.notes.as_deref(), Some("Tesselid enters the empty hall."));
}

#[test]
fn marker_formats() {
    let cases: &[(&str, &[(u32, u32, &str)])] = &[
        ("[12] a", &[(12, 12, "a")]),
        ("[ 12 ] a", &[(12, 12, "a")]),
        ("  [12]   a  ", &[(12, 12, "a")]),
        ("[12-14] a", &[(12, 14, "a")]),
        ("[12–14] a", &[(12, 14, "a")]), // en dash
        ("[12—14] a", &[(12, 14, "a")]), // em dash
        ("[12 - 14] a", &[(12, 14, "a")]),
        ("[14-12] a", &[(12, 14, "a")]), // reversed
        ("12. a", &[(12, 12, "a")]),
        ("12) a", &[(12, 12, "a")]),
        ("【12】a", &[(12, 12, "a")]),
        ("［12］ a", &[(12, 12, "a")]),
        ("**[12]** a", &[(12, 12, "a")]),
        ("**12.** a", &[(12, 12, "a")]),
        ("[12]: a", &[(12, 12, "a")]),
        ("[１２] a", &[(12, 12, "a")]), // fullwidth digits
        ("`[12]` a", &[(12, 12, "a")]),
        // Not markers: stay as text of the previous entry or are ignored.
        ("[12 seconds] a", &[]),
        ("[<System> done]", &[]),
        ("preamble\n[12] a", &[(12, 12, "a")]),
        // `13.` is a marker only when 13 is expected.
        ("[12] a\n13. b", &[(12, 12, "a"), (13, 13, "b")]),
        ("[12] a\n99. b", &[(12, 12, "a\n\n99. b")]),
        ("[12] a\n3 apples", &[(12, 12, "a\n\n3 apples")]),
    ];
    for (input, want) in cases {
        let p = parse(input, 12..=20, true);
        assert_eq!(&pairs(&p), want, "input {input:?}");
    }
}

#[test]
fn text_formatting_survives_markers() {
    let p = parse("[1] *Whoosh!* he said\n[2] \"[System] ok\"", 1..=2, true);
    assert_eq!(
        pairs(&p),
        [(1, 1, "*Whoosh!* he said"), (2, 2, "\"[System] ok\"")]
    );
}

#[test]
fn fenced_response_is_unwrapped() {
    let p = parse("```text\n[1] a\n\n[2] b\n```", 1..=2, true);
    assert_eq!(pairs(&p), [(1, 1, "a"), (2, 2, "b")]);
}

#[test]
fn gaps_duplicates_and_out_of_range() {
    // 2 missing, 3 repeated (first wins), 0 and 9 out of range.
    let p = parse("[0] x\n[1] a\n[3] c\n[3] c2\n[4] d\n[9] z", 1..=5, true);
    assert_eq!(pairs(&p), [(1, 1, "a"), (3, 3, "c"), (4, 4, "d")]);
    assert_eq!(p.missing, [2, 5]);
    assert_eq!(p.duplicates, [3]);
    assert_eq!(p.out_of_range, [0, 9]);
}

#[test]
fn overlapping_merge_is_dropped_whole() {
    let p = parse("[1-2] a\n[2-3] b", 1..=3, true);
    assert_eq!(pairs(&p), [(1, 2, "a")]);
    assert_eq!(p.duplicates, [2]);
    assert_eq!(p.missing, [3]);
}

#[test]
fn entry_straddling_the_range_is_out_of_range() {
    let p = parse("[1] a\n[2-6] b", 1..=3, true);
    assert_eq!(pairs(&p), [(1, 1, "a")]);
    assert_eq!(p.out_of_range, [4, 5, 6]);
    assert_eq!(p.missing, [2, 3]);
}

#[test]
fn absurd_range_is_not_iterated() {
    let p = parse("[1-4000000000] a\n[2] b", 1..=2, true);
    assert_eq!(pairs(&p), [(2, 2, "b")]);
    assert_eq!(p.missing, [1]);
}

#[test]
fn out_of_order_entries_are_sorted() {
    let p = parse("[3] c\n[1] a\n[2] b", 1..=3, true);
    assert_eq!(pairs(&p), [(1, 1, "a"), (2, 2, "b"), (3, 3, "c")]);
}

#[test]
fn empty_entry_is_kept_and_covers_its_number() {
    let p = parse("[1]\n[2] b", 1..=2, true);
    assert_eq!(pairs(&p), [(1, 1, ""), (2, 2, "b")]);
    assert!(p.missing.is_empty());
}

#[test]
fn cut_off_drops_the_last_entry() {
    let p = parse("[1] a\n[2] b\n[3] c unfinis", 1..=5, false);
    assert_eq!(pairs(&p), [(1, 1, "a"), (2, 2, "b")]);
    assert_eq!(p.missing, [3, 4, 5]);

    // A merge that was cut off loses all its numbers.
    let p = parse("[1] a\n[2-3] b unfin", 1..=3, false);
    assert_eq!(pairs(&p), [(1, 1, "a")]);
    assert_eq!(p.missing, [2, 3]);

    // Cut off before any marker: nothing to drop.
    let p = parse("Sure, here", 1..=2, false);
    assert!(p.pairs.is_empty());
    assert_eq!(p.missing, [1, 2]);

    // The same text with complete = true keeps the last entry.
    let p = parse("[1] a\n[2] b", 1..=2, true);
    assert_eq!(p.pairs.len(), 2);
}

#[test]
fn trailer_proves_the_body_finished() {
    let p = parse(
        "[1] a\n[2] b\nNEW_TERMS:\nx | y | name | male\nz | w | pla",
        1..=2,
        false,
    );
    // Body kept; the unfinished last row is dropped.
    assert_eq!(p.pairs.len(), 2);
    assert_eq!(p.new_terms.len(), 1);
    assert_eq!(p.new_terms[0].source, "x");

    // Once NOTES has started the terms are final.
    let p = parse(
        "[1] a\nNEW_TERMS:\nx | y\nNOTES:\nHalf a sent",
        1..=1,
        false,
    );
    assert_eq!(p.new_terms.len(), 1);
    assert_eq!(p.notes.as_deref(), Some("Half a sent"));
}

#[test]
fn trailer_variants() {
    struct Case {
        input: &'static str,
        terms: Vec<(
            &'static str,
            &'static str,
            Option<&'static str>,
            Option<&'static str>,
        )>,
        notes: Option<&'static str>,
    }
    let cases = vec![
        Case {
            input: "[1] a\nNEW_TERMS:\n아르젠트 | Argent | name | male\nNOTES:\nA fight.",
            terms: vec![("아르젠트", "Argent", Some("name"), Some("male"))],
            notes: Some("A fight."),
        },
        // Reversed order, markdown decoration, bullets, outer pipes.
        Case {
            input: "[1] a\n**NOTES:** Quick.\n\n## New terms:\n- 가 | A | place\n| 나 | B | name | female |",
            terms: vec![
                ("가", "A", Some("place"), None),
                ("나", "B", Some("name"), Some("female")),
            ],
            notes: Some("Quick."),
        },
        // Missing and extra columns, placeholders, header and separator rows.
        Case {
            input: "[1] a\nNEW_TERMS:\nsource | rendering | kind | gender\n---|---|---|---\n가 | A\n나 | B | name | - | extra\n다 | C | - | n/a",
            terms: vec![
                ("가", "A", None, None),
                ("나", "B", Some("name"), None),
                ("다", "C", None, None),
            ],
            notes: None,
        },
        // Arrow separators, "none" rows, rows without a rendering.
        Case {
            input: "[1] a\nNEW_TERMS:\n가 -> A\n나 → B\nnone\n다 |",
            terms: vec![("가", "A", None, None), ("나", "B", None, None)],
            notes: None,
        },
        // Multi-line notes; lowercase header.
        Case {
            input: "[1] a\nnotes:\nLine one.\n\nLine two.",
            terms: vec![],
            notes: Some("Line one.\nLine two."),
        },
        // Empty trailer sections.
        Case {
            input: "[1] a\nNEW_TERMS:\nNOTES:\n",
            terms: vec![],
            notes: None,
        },
    ];
    for c in cases {
        let p = parse(c.input, 1..=1, true);
        let got: Vec<_> = p
            .new_terms
            .iter()
            .map(|t| {
                (
                    t.source.as_str(),
                    t.rendering.as_str(),
                    t.kind.as_deref(),
                    t.gender.as_deref(),
                )
            })
            .collect();
        assert_eq!(got, c.terms, "terms of {:?}", c.input);
        assert_eq!(p.notes.as_deref(), c.notes, "notes of {:?}", c.input);
        assert_eq!(pairs(&p), [(1, 1, "a")], "body of {:?}", c.input);
    }
}

#[test]
fn trailer_words_inside_text_are_not_headers() {
    let p = parse(
        "[1] Notebooks were everywhere.\n[2] Newton's laws",
        1..=2,
        true,
    );
    assert_eq!(p.pairs.len(), 2);
    assert!(p.notes.is_none());
}
