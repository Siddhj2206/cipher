use cipher_text::checks::{CheckTerm, PairText};
use cipher_text::drift::{Drift, DriftKind, drift};

fn term(source: &str, rendering: &str, forbidden: &[&str]) -> CheckTerm {
    CheckTerm {
        source: source.into(),
        rendering: rendering.into(),
        forbidden: forbidden.iter().map(|s| s.to_string()).collect(),
        may_omit: false,
    }
}

fn pair<'a>(source: &'a str, translation: &'a str) -> PairText<'a> {
    PairText {
        source,
        translation,
    }
}

#[test]
fn forbidden_present_counts_occurrences_across_pairs() {
    let terms = [term("테셀리드", "Reed", &["Tesselid", "Tess"])];
    let pairs = [
        pair("테셀리드가 웃었다.", "Tesselid smiled. Tesselid's hand."),
        pair("테셀리드는 갔다.", "Reed left."),
        pair("테셀리드를 보았다.", "She saw Tess."),
        // Source does not name the Term: ignored.
        pair("그가 웃었다.", "Tesselid again."),
    ];
    assert_eq!(
        drift(&pairs, &terms),
        [
            Drift {
                term_index: 0,
                kind: DriftKind::ForbiddenPresent {
                    rendering: "Tesselid".into(),
                    count: 2
                }
            },
            Drift {
                term_index: 0,
                kind: DriftKind::ForbiddenPresent {
                    rendering: "Tess".into(),
                    count: 1
                }
            },
        ]
    );
}

#[test]
fn never_rendered_when_named_three_times() {
    let terms = [term("아일렛", "Ailet", &[]), term("리드", "Reed", &[])];
    let pairs = [
        pair("아일렛이 왔다. 아일렛은 웃었다.", "She came. She laughed."),
        pair("아일렛을 보았다. 리드가 갔다.", "He saw her. He left."),
    ];
    assert_eq!(
        drift(&pairs, &terms),
        [Drift {
            term_index: 0,
            kind: DriftKind::NeverRendered { source_count: 3 }
        }]
    );
}

#[test]
fn both_kinds_for_one_term_and_ordering_by_term() {
    let terms = [term("가가가", "A", &[]), term("나나나", "B", &["Old"])];
    let pairs = [pair("가가가 가가가 가가가 나나나", "x Old y")];
    assert_eq!(
        drift(&pairs, &terms),
        [
            Drift {
                term_index: 0,
                kind: DriftKind::NeverRendered { source_count: 3 }
            },
            Drift {
                term_index: 1,
                kind: DriftKind::ForbiddenPresent {
                    rendering: "Old".into(),
                    count: 1
                }
            },
        ]
    );
}

#[test]
fn clean_chapter_has_no_drift() {
    let terms = [term("테셀리드", "Reed", &["Tesselid"])];
    let pairs = [pair(
        "테셀리드가 웃었다. 테셀리드가 갔다. 테셀리드가 왔다.",
        "Reed smiled, left and came.",
    )];
    assert!(drift(&pairs, &terms).is_empty());
    assert!(drift(&[], &terms).is_empty());
    assert!(drift(&pairs, &[]).is_empty());
}

#[test]
fn one_character_sources_and_terms_that_may_go_unnamed_never_drift_by_absence() {
    let pairs = [
        pair(
            "세 명이 왔다. 명이 웃었다.",
            "Three people came. Myeong laughed.",
        ),
        pair("명, 명, 명.", "Him, him, him."),
        pair("마탑주, 마탑주, 마탑주.", "He, he, he."),
    ];
    let counter = term("명", "Myeong", &[]);
    let mut lord = term("마탑주", "Magic Tower Lord", &[]);
    assert_eq!(drift(&pairs, &[counter.clone(), lord.clone()]).len(), 1);
    lord.may_omit = true;
    assert!(drift(&pairs, &[counter, lord.clone()]).is_empty());

    // A Forbidden Rendering is still Drift for a Term that may go unnamed.
    lord.forbidden = vec!["Tower Master".into()];
    let forbidden = [pair("마탑주가 왔다.", "The Tower Master came.")];
    assert!(matches!(
        drift(&forbidden, &[lord])[..],
        [Drift {
            kind: DriftKind::ForbiddenPresent { .. },
            ..
        }]
    ));
}
