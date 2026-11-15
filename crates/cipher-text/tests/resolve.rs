use cipher_text::resolve::{candidates, resolve};

#[test]
fn finds_the_multi_word_expression_that_separates_hits_from_others() {
    let hits = [
        "테셀리드 공작이 웃었다.",
        "그는 테셀리드 공작을 보았다.",
        "테셀리드 공작은 말이 없었다.",
        "“가자.” 테셀리드 공작의 목소리였다.",
    ];
    // Plain 테셀리드 also occurs in Blocks that rendered it as a pronoun or
    // with another title, so the longer expression separates better.
    let others = [
        "테셀리드가 문을 열었다.",
        "테셀리드는 걸었다.",
        "비가 내렸다.",
        "공작 부인이 도착했다.",
        "아일렛은 웃었다.",
    ];
    let best = resolve(&hits, &others).unwrap();
    assert_eq!(best.text, "테셀리드 공작");
    assert_eq!(best.hit_blocks, 4);
    assert_eq!(best.other_blocks, 0);
    assert!((best.score - 1.0).abs() < 1e-9);

    // The UI list contains it first, with weaker alternatives after.
    let list = candidates(&hits, &others);
    assert_eq!(list[0].text, "테셀리드 공작");
    assert!(list.len() <= 8);
    assert!(list.windows(2).all(|w| w[0].score >= w[1].score));
}

#[test]
fn particles_are_stripped_when_it_does_not_hurt() {
    let hits = [
        "테셀리드가 웃었다.",
        "그는 테셀리드를 보았다.",
        "테셀리드는 말이 없었다.",
    ];
    let others = ["비가 내렸다.", "그는 문을 열었다."];
    assert_eq!(resolve(&hits, &others).unwrap().text, "테셀리드");

    // All hits carry the same particle: the stem is still preferred when it
    // is no worse (the others do not contain it).
    let hits = ["아일렛이 웃었다.", "저기 아일렛이 있다.", "아일렛이 갔다."];
    assert_eq!(resolve(&hits, &others).unwrap().text, "아일렛");
}

#[test]
fn a_particle_that_helps_is_kept() {
    // 이 here separates hits from others: the others have the bare stem.
    let hits = ["마이가 웃었다.", "저기 마이가 있다.", "마이가 갔다."];
    let others = ["마이는 울었다.", "마이를 보았다."];
    let best = resolve(&hits, &others).unwrap();
    assert_eq!(best.text, "마이가");
}

#[test]
fn needs_three_hits() {
    let others = ["비가 내렸다."];
    assert_eq!(
        resolve(&["테셀리드가 웃었다.", "테셀리드를 보았다."], &others),
        None
    );
    assert!(candidates(&["테셀리드가 웃었다."], &others).is_empty());
    let none: [&str; 0] = [];
    assert_eq!(resolve(&none, &none), None);
}

#[test]
fn no_clear_winner_when_hits_share_nothing() {
    let hits = [
        "비가 내렸다.",
        "문이 열렸다.",
        "밤이 깊었다.",
        "바람이 불었다.",
    ];
    let others = ["아침이 밝았다."];
    assert_eq!(resolve(&hits, &others), None);
}

#[test]
fn an_expression_everywhere_is_not_a_winner() {
    // 그는 occurs in every Block; precision is too low to call it the source.
    let hits = ["그는 웃었다.", "그는 갔다.", "그는 왔다."];
    let others: Vec<String> = (0..20)
        .map(|i| format!("그는 {i}번째 문을 열었다."))
        .collect();
    assert_eq!(resolve(&hits, &others), None);
}

#[test]
fn works_with_owned_strings_and_no_others() {
    let hits: Vec<String> = ["아르젠트가 왔다.", "아르젠트는 갔다.", "아르젠트를 불렀다."]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let others: Vec<String> = Vec::new();
    assert_eq!(resolve(&hits, &others).unwrap().text, "아르젠트");
}

#[test]
fn latin_and_digit_runs_are_candidates_too() {
    let hits = [
        "그는 SS급 몬스터를 잡았다.",
        "SS급 던전이 열렸다.",
        "저건 SS급이다.",
    ];
    let others = ["S급 몬스터였다.", "비가 내렸다."];
    assert_eq!(resolve(&hits, &others).unwrap().text, "SS급");
}
