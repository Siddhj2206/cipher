#![allow(clippy::type_complexity)] // table tests use tuple-of-slices cases

use cipher_text::checks::*;
use cipher_text::script::Script;

fn term(source: &str, rendering: &str, forbidden: &[&str]) -> CheckTerm {
    CheckTerm {
        source: source.into(),
        aliases: Vec::new(),
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
fn leftover_script_table() {
    let cases: &[(&str, &[&str], Vec<(&str, Script)>)] = &[
        ("Plain English.", &[], vec![]),
        ("He said 안녕 twice.", &[], vec![("안녕", Script::Hangul)]),
        (
            "A 魔族 and a まぞく.",
            &[],
            vec![("魔族", Script::Han), ("まぞく", Script::Kana)],
        ),
        (
            "カタカナ・ひらがな",
            &[],
            vec![("カタカナ", Script::Kana), ("ひらがな", Script::Kana)],
        ),
        // Script changes break a run even without a space.
        (
            "魔まぞ",
            &[],
            vec![("魔", Script::Han), ("まぞ", Script::Kana)],
        ),
        // CJK punctuation and symbols are not leftover words.
        ("「quoted」 ✠ ※ …", &[], vec![]),
        // Allowed substrings are skipped.
        (
            "The 魔 sign ✠ and 魔族",
            &["魔 sign"],
            vec![("魔族", Script::Han)],
        ),
        (
            "한글 at the end 끝",
            &[],
            vec![("한글", Script::Hangul), ("끝", Script::Hangul)],
        ),
    ];
    for (text, allowed, want) in cases {
        let got: Vec<(&str, Script)> = leftover_script(text, allowed)
            .into_iter()
            .map(|r| (&text[r.range], r.script))
            .collect();
        assert_eq!(&got, want, "text {text:?}");
    }
}

#[test]
fn forbidden_rendering_needs_the_source_to_name_the_term() {
    let terms = [term("테셀리드", "Reed", &["Tesselid"])];
    // Source names the Term and the old rendering is present: flagged.
    let hit = forbidden_in_pair(
        &pair(
            "테셀리드가 웃었다.",
            "Tesselid smiled, and Tesselid's eyes shone.",
        ),
        &terms,
    );
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].term_index, 0);
    assert_eq!(hit[0].rendering, "Tesselid");
    assert_eq!(hit[0].ranges, [0..8, 21..29]);
    // Source does not name the Term: not flagged, whatever the translation says.
    assert!(forbidden_in_pair(&pair("그가 웃었다.", "Tesselid smiled."), &terms).is_empty());
    // The right rendering is not a finding, and absence is not either.
    assert!(forbidden_in_pair(&pair("테셀리드가 웃었다.", "Reed smiled."), &terms).is_empty());
    assert!(forbidden_in_pair(&pair("테셀리드가 웃었다.", "He smiled."), &terms).is_empty());
    // Whole words only.
    assert!(
        forbidden_in_pair(&pair("테셀리드가 웃었다.", "Tesselidan smiled."), &terms).is_empty()
    );
}

#[test]
fn forbidden_inside_another_terms_rendering_is_legit() {
    let terms = [
        term("아르젠트", "Reed", &["Argent"]),
        term("테셀리드 아르젠트", "Tesselid Argent", &[]),
    ];
    // Both names are in the source; "Argent" only appears inside the full name.
    let p = pair(
        "테셀리드 아르젠트가 아르젠트를 불렀다",
        "Tesselid Argent called Reed.",
    );
    // The longest-match rule means the source names the full Term once and the
    // short one once more.
    assert!(forbidden_in_pair(&p, &terms).is_empty());
    // A bare "Argent" elsewhere in the same Pair is still flagged.
    let p = pair(
        "테셀리드 아르젠트가 아르젠트를 불렀다",
        "Tesselid Argent called Argent.",
    );
    let hits = forbidden_in_pair(&p, &terms);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ranges.len(), 1);
    assert_eq!(hits[0].ranges[0], 23..29);
}

#[test]
fn forbidden_equal_to_own_rendering_is_ignored() {
    let terms = [term("가나다", "Reed", &["reed", ""])];
    assert!(forbidden_in_pair(&pair("가나다", "Reed"), &terms).is_empty());
}

#[test]
fn missing_names_need_three_mentions_and_no_rendering() {
    let terms = [term("테셀리드", "Reed", &[]), term("아일렛", "Ailet", &[])];
    let pairs = [
        pair(
            "테셀리드가 왔다. 테셀리드는 웃었다.",
            "He came. He laughed.",
        ),
        pair(
            "테셀리드를 보았다. 아일렛이 웃었다.",
            "She saw him. She laughed.",
        ),
    ];
    // Reed: named 3 times, never rendered. Ailet: named once, a pronoun is normal.
    assert_eq!(
        missing_names(&pairs, &terms),
        [Finding::MissingName {
            term_index: 0,
            source_count: 3
        }]
    );
    // Rendered once anywhere in the Chapter silences it, possessive included.
    let pairs = [
        pair("테셀리드가 왔다. 테셀리드는 웃었다.", "Reed's coat."),
        pair("테셀리드를 보았다.", "She saw him."),
    ];
    assert!(missing_names(&pairs, &terms).is_empty());
    // Two mentions are not enough.
    let pairs = [pair("테셀리드가 왔다. 테셀리드는 웃었다.", "He came.")];
    assert!(missing_names(&pairs, &terms).is_empty());
    // Empty rendering never reports.
    let t = [term("테셀리드", "", &[])];
    assert!(missing_names(&[pair("테셀리드 테셀리드 테셀리드", "x")], &t).is_empty());
}

#[test]
fn missing_names_use_the_precision_matcher() {
    // 한 inside 한국 does not count as naming 한.
    let terms = [term("한", "Han", &[])];
    let pairs = [pair("한국 한국 한국 한국", "Korea Korea Korea Korea")];
    assert!(missing_names(&pairs, &terms).is_empty());
    assert_eq!(source_counts(&pairs, &terms), [0]);
}

#[test]
fn length_ratio_table() {
    let long_source: String = "가".repeat(100);
    let limits = LengthLimits::default();
    let cases: Vec<(String, String, Option<LengthProblem>)> = vec![
        (long_source.clone(), "x".repeat(200), None),
        (long_source.clone(), "x".repeat(100), None),
        (
            long_source.clone(),
            "x".repeat(60),
            Some(LengthProblem::TooShort),
        ),
        (
            long_source.clone(),
            "x".repeat(450),
            Some(LengthProblem::TooLong),
        ),
        (long_source.clone(), "  ".into(), Some(LengthProblem::Empty)),
        // Short sources are not checked, except an empty translation.
        ("가".repeat(40), "x".repeat(2), None),
        ("가".repeat(40), "".into(), Some(LengthProblem::Empty)),
        ("".into(), "anything".into(), None),
        // Exactly at the source limit: not over it.
        ("가".repeat(80), "x".repeat(2), None),
        (
            "가".repeat(81),
            "x".repeat(2),
            Some(LengthProblem::TooShort),
        ),
    ];
    for (source, translation, want) in cases {
        let got = length_ratio(3, &pair(&source, &translation), &limits).map(|f| match f {
            Finding::LengthRatio { pair, problem, .. } => {
                assert_eq!(pair, 3);
                problem
            }
            other => panic!("unexpected {other:?}"),
        });
        assert_eq!(
            got,
            want,
            "{} source chars, {} translation chars",
            source.chars().count(),
            translation.chars().count()
        );
    }
}

#[test]
fn check_chapter_collects_everything_in_order() {
    let terms = [term("테셀리드", "Reed", &["Tesselid"])];
    let long = "가".repeat(120);
    let pairs = [
        pair("테셀리드가 웃었다.", "Tesselid smiled 웃었다."),
        pair(&long, "short"),
        pair("테셀리드는 갔다. 테셀리드는 왔다.", "He left. He came."),
    ];
    let findings = check_chapter(&pairs, &terms, &CheckOptions::default());
    let kinds: Vec<String> = findings
        .iter()
        .map(|f| match f {
            Finding::LeftoverScript { pair, .. } => format!("script@{pair}"),
            Finding::ForbiddenRendering { pair, .. } => format!("forbidden@{pair}"),
            Finding::LengthRatio { pair, .. } => format!("length@{pair}"),
            Finding::MissingName { term_index, .. } => format!("missing#{term_index}"),
        })
        .collect();
    // Source names 테셀리드 three times in the Chapter and "Reed" never appears.
    assert_eq!(kinds, ["script@0", "forbidden@0", "length@1", "missing#0"]);

    let by = by_pair(&findings);
    assert_eq!(by.keys().copied().collect::<Vec<_>>(), [0, 1]);
    assert_eq!(by[&0].len(), 2);
}

#[test]
fn check_options_allow_script() {
    let opts = CheckOptions {
        allowed_script: vec!["魔".into()],
        ..CheckOptions::default()
    };
    let findings = check_chapter(&[pair("a", "the 魔 sign")], &[], &opts);
    assert!(findings.is_empty());
}
