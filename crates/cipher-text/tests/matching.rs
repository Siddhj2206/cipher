use cipher_text::matching::{count_matches, find, find_rendering, terms_in};

fn found<'a>(text: &'a str, sources: &[&str]) -> Vec<(usize, &'a str)> {
    find(text, sources)
        .into_iter()
        .map(|m| (m.index, &text[m.range]))
        .collect()
}

fn rendered<'a>(text: &'a str, rendering: &str) -> Vec<&'a str> {
    find_rendering(text, rendering)
        .into_iter()
        .map(|r| &text[r])
        .collect()
}

#[test]
fn recall_matcher_is_a_substring_search() {
    let cases: &[(&str, &[&str], &[usize])] = &[
        ("한국에서 왔다", &["한", "미국", "한국"], &[0, 2]),
        ("테셀리드가 웃었다", &["테셀리드", "아일렛"], &[0]),
        ("Duke Tesselid", &["duke tesselid", "TESSELID"], &[0, 1]),
        ("anything", &["", "  "], &[]),
        ("", &["a"], &[]),
        ("가나다", &["나다", "가나다"], &[0, 1]),
    ];
    for (text, sources, want) in cases {
        assert_eq!(&terms_in(text, sources), want, "text {text:?}");
    }
}

#[test]
fn short_hangul_needs_a_word_boundary() {
    // (text, number of matches of 한)
    let cases = [
        ("한은 말했다", 1),
        ("한이 웃었다", 1),
        ("한에게는 말했다", 1), // particle chain
        ("그것은 한이었다", 1),
        ("한", 1),
        ("“한!” 그가 외쳤다", 1),
        ("한, 그리고 아일렛", 1),
        ("한 마디 했다", 1), // followed by a space
        ("한국의 겨울", 0),
        ("한의사가 왔다", 0), // 의 is a particle but 사 continues the word
        ("대한민국", 0),
        ("그는 한참 걸었다", 0),
        ("뜻한바", 0),
        ("한한", 0),
    ];
    for (text, n) in cases {
        assert_eq!(find(text, &["한"]).len(), n, "text {text:?}");
    }
}

#[test]
fn two_syllable_sources_follow_the_same_rule() {
    assert_eq!(found("리드는 웃었다", &["리드"]), [(0, "리드")]);
    assert_eq!(found("스리드가 웃었다", &["리드"]), []);
    assert_eq!(found("리드미컬한", &["리드"]), []);
}

#[test]
fn long_or_mixed_sources_are_plain_substrings() {
    // Three syllables: no boundary rule.
    assert_eq!(found("테셀리드가", &["테셀리"]), [(0, "테셀리")]);
    // Latin-only sources are not Hangul-bound.
    assert_eq!(found("SS급 몬스터", &["SS"]), [(0, "SS")]);
    // The Hangul edge of a mixed source is checked, the Latin edge is not.
    assert_eq!(found("SS급은 강했다", &["SS급"]), [(0, "SS급")]);
    assert_eq!(found("우SS급", &["SS급"]), [(0, "SS급")]);
}

#[test]
fn longest_match_wins_and_matches_do_not_overlap() {
    let text = "아르젠트 공작이 아르젠트를 불렀다";
    let sources = ["아르젠트", "아르젠트 공작", "공작"];
    assert_eq!(
        found(text, &sources),
        [(1, "아르젠트 공작"), (0, "아르젠트")]
    );
    assert_eq!(count_matches(text, &sources), [1, 1, 0]);

    // Overlapping candidates: the longer one wins, the shorter loses its overlap.
    assert_eq!(found("ABCD", &["ABC", "BCD"]), [(0, "ABC")]);
    // Ties go to the leftmost.
    assert_eq!(found("ABCD", &["BCD", "ABC"]), [(1, "ABC")]);
    // Same source twice is two matches.
    assert_eq!(found("가나다 가나다", &["가나다"]).len(), 2);
    // Empty sources and empty text.
    assert_eq!(found("abc", &["", "b"]), [(1, "b")]);
    assert!(find("", &["가나다"]).is_empty());
}

#[test]
fn english_finder_table() {
    let cases: &[(&str, &str, &[&str])] = &[
        ("Argent drew his sword.", "Argent", &["Argent"]),
        (
            "Argent's sword and Argent’s horse",
            "Argent",
            &["Argent", "Argent"],
        ),
        ("the argent coin", "Argent", &["argent"]), // first letter case-insensitive
        ("ARGENT shouted", "Argent", &[]),          // the rest is exact
        ("Argental and Reargent", "Argent", &[]),   // whole words only
        ("Argent-like", "Argent", &["Argent"]),
        ("“Argent!” she said", "Argent", &["Argent"]),
        ("O'Argent", "Argent", &[]), // glued to a letter by an apostrophe
        (
            "Duke Tesselid and duke Tesselid",
            "Duke Tesselid",
            &["Duke Tesselid", "duke Tesselid"],
        ),
        (
            "Assassin’s Guild",
            "Assassin's Guild",
            &["Assassin’s Guild"],
        ),
        ("Moriphas' cloak", "Moriphas", &["Moriphas"]),
        ("anything", "", &[]),
        ("한국", "한국", &["한국"]),
    ];
    for (text, rendering, want) in cases {
        assert_eq!(
            &rendered(text, rendering),
            want,
            "{rendering:?} in {text:?}"
        );
    }
}
