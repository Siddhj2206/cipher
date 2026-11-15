use cipher_text::diff::{DiffOp, DiffOp::*, diff};

fn e(s: &str) -> DiffOp {
    Equal(s.into())
}
fn d(s: &str) -> DiffOp {
    Delete(s.into())
}
fn i(s: &str) -> DiffOp {
    Insert(s.into())
}

/// The ops must reproduce both inputs exactly.
fn check_roundtrip(before: &str, after: &str) {
    let ops = diff(before, after);
    let mut b = String::new();
    let mut a = String::new();
    for op in &ops {
        match op {
            Equal(s) => {
                b.push_str(s);
                a.push_str(s);
            }
            Delete(s) => b.push_str(s),
            Insert(s) => a.push_str(s),
        }
    }
    assert_eq!(b, before, "before of {ops:?}");
    assert_eq!(a, after, "after of {ops:?}");
}

#[test]
fn table() {
    let cases: Vec<(&str, &str, Vec<DiffOp>)> = vec![
        ("", "", vec![]),
        ("same text", "same text", vec![e("same text")]),
        (
            "the old king",
            "the new king",
            vec![e("the "), d("old"), i("new"), e(" king")],
        ),
        ("a b c", "a c", vec![e("a "), d("b "), e("c")]),
        ("a c", "a b c", vec![e("a "), i("b "), e("c")]),
        ("", "new", vec![i("new")]),
        ("gone", "", vec![d("gone")]),
        // Whole words: adding a possessive replaces the word.
        (
            "Argent drew",
            "Argent's drew",
            vec![d("Argent"), i("Argent's"), e(" drew")],
        ),
        // Punctuation is its own token.
        ("He left", "He left.", vec![e("He left"), i(".")]),
        (
            "Wait, what",
            "Wait what",
            vec![e("Wait"), d(","), e(" what")],
        ),
        // Changed words in the middle, several regions.
        (
            "Tesselid met Moriphas at dawn.",
            "Reed met Gus at dawn.",
            vec![
                d("Tesselid"),
                i("Reed"),
                e(" met "),
                d("Moriphas"),
                i("Gus"),
                e(" at dawn."),
            ],
        ),
        // Whitespace differences between equal words are shown on their own.
        ("a  b", "a b", vec![e("a "), d(" "), e("b")]),
        (" lead", "lead", vec![d(" "), e("lead")]),
        // Korean and quotes.
        (
            "“가자.” 그가 말했다",
            "“가자.” 그는 말했다",
            vec![e("“가자.” "), d("그가"), i("그는"), e(" 말했다")],
        ),
    ];
    for (before, after, want) in cases {
        check_roundtrip(before, after);
        assert_eq!(diff(before, after), want, "{before:?} -> {after:?}");
    }
}

#[test]
fn delete_comes_before_insert_and_neighbours_merge() {
    let ops = diff("one two three four", "five six three seven");
    assert_eq!(
        ops,
        [
            d("one two"),
            i("five six"),
            e(" three "),
            d("four"),
            i("seven")
        ]
    );
    check_roundtrip("one two three four", "five six three seven");
}

#[test]
fn moved_and_repeated_words_roundtrip() {
    for (a, b) in [
        ("a b a b a", "b a b"),
        ("x x x", "x x"),
        ("It's the Duke's.", "It’s the duke’s"),
        ("tab\tand\nnewline", "tab and newline"),
        ("가 나 다", "다 나 가"),
    ] {
        check_roundtrip(a, b);
    }
}

#[test]
fn big_inputs_fall_back_gracefully() {
    let before: String = (0..3000).map(|n| format!("w{n} ")).collect();
    let after: String = (0..3000).map(|n| format!("v{n} ")).collect();
    check_roundtrip(&before, &after);
    // One replacement; the shared trailing space stays outside it.
    assert_eq!(diff(&before, &after).len(), 3);
}
