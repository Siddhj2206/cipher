#![allow(clippy::type_complexity)] // table tests use tuple-of-slices cases

use cipher_text::rename::{RenameSpec, derive_partials, rename};

fn run(text: &str, old: &str, new: &str) -> Option<String> {
    rename(text, &RenameSpec::new(old, new)).map(|r| r.text)
}

#[test]
fn boundaries_and_basic_replacement() {
    let cases: &[(&str, &str, &str, Option<&str>)] = &[
        ("Reed left.", "Reed", "Tess", Some("Tess left.")),
        (
            "Reeds, Breed and Reed.",
            "Reed",
            "Tess",
            Some("Reeds, Breed and Tess."),
        ),
        (
            "\"Reed!\" said Reed",
            "Reed",
            "Tess",
            Some("\"Tess!\" said Tess"),
        ),
        ("Reed-like grin", "Reed", "Tess", Some("Tess-like grin")),
        ("Nothing here.", "Reed", "Tess", None),
        ("Reedy", "Reed", "Tess", None),
        // Multi-word and curly apostrophes.
        (
            "The Assassin’s Guild met.",
            "Assassin's Guild",
            "Killers' Guild",
            Some("The Killers' Guild met."),
        ),
        // New contains old: replaced once, not recursively.
        ("Reed ran", "Reed", "Reed Argent", Some("Reed Argent ran")),
        // Same text in, same text out: no change.
        ("Reed ran", "Reed", "Reed", None),
        // Blank inputs.
        ("Reed ran", "", "Tess", None),
        ("Reed ran", "Reed", "  ", None),
        // Whole words in any script: Hangul is alphanumeric, so 한 is not inside 한국.
        ("한국의 한", "한", "칸", Some("한국의 칸")),
    ];
    for (text, old, new, want) in cases {
        assert_eq!(
            run(text, old, new).as_deref(),
            *want,
            "{old} -> {new} in {text:?}"
        );
    }
}

#[test]
fn case_follows_the_match() {
    let cases: &[(&str, &str, &str, Option<&str>)] = &[
        // Names: exact and SHOUTING only. A lowercase variant is a common word.
        (
            "REED stood. Reed sat. reed grew.",
            "Reed",
            "Tess",
            Some("TESS stood. Tess sat. reed grew."),
        ),
        (
            "The magic tower and the Magic Tower and MAGIC TOWER.",
            "Magic Tower",
            "Mage Tower",
            Some("The magic tower and the Mage Tower and MAGE TOWER."),
        ),
        // Phrases: exact, sentence-initial capital, Title Case, UPPER.
        (
            "He let out a breath. Let out a breath, he did. Let Out A Breath! LET OUT A BREATH!",
            "let out a breath",
            "exhaled slowly",
            Some("He exhaled slowly. Exhaled slowly, he did. Exhaled Slowly! EXHALED SLOWLY!"),
        ),
        // A phrase is not matched in other mixed cases.
        ("let OUT a breath", "let out a breath", "sighed", None),
    ];
    for (text, old, new, want) in cases {
        assert_eq!(
            run(text, old, new).as_deref(),
            *want,
            "{old} -> {new} in {text:?}"
        );
    }
}

#[test]
fn possessives() {
    let cases: &[(&str, &str, &str, &str)] = &[
        ("Tesselid's sword", "Tesselid", "Reed", "Reed's sword"),
        ("Tesselid’s sword", "Tesselid", "Reed", "Reed’s sword"),
        // New name ends in s: bare apostrophe.
        (
            "Tesselid's sword",
            "Tesselid",
            "Moriphas",
            "Moriphas' sword",
        ),
        (
            "Tesselid’s sword",
            "Tesselid",
            "Moriphas",
            "Moriphas’ sword",
        ),
        // Old name ends in s.
        ("Moriphas' sword", "Moriphas", "Reed", "Reed's sword"),
        ("Moriphas’ sword", "Moriphas", "Reed", "Reed’s sword"),
        ("Moriphas' sword", "Moriphas", "Gus", "Gus' sword"),
        // The 's style after an s-name is kept.
        ("Moriphas's sword", "Moriphas", "Reed", "Reed's sword"),
        ("Moriphas's sword", "Moriphas", "Gus", "Gus's sword"),
        // Shouting.
        ("TESSELID'S SWORD", "Tesselid", "Reed", "REED'S SWORD"),
        // Contractions are left alone.
        ("Tesselid'll go", "Tesselid", "Reed", "Reed'll go"),
        // A closing quote after an s-name is not a possessive.
        ("'Moriphas' he said", "Moriphas", "Reed", "'Reed' he said"),
        ("said 'Tesselid'.", "Tesselid", "Reed", "said 'Reed'."),
    ];
    for (text, old, new, want) in cases {
        assert_eq!(
            run(text, old, new).as_deref(),
            Some(*want),
            "{old} -> {new} in {text:?}"
        );
    }
}

#[test]
fn a_an_agreement() {
    let cases: &[(&str, &str, &str, &str)] = &[
        ("an Argent knight", "Argent", "Reed", "a Reed knight"),
        ("a Reed knight", "Reed", "Argent", "an Argent knight"),
        ("An Argent knight", "Argent", "Reed", "A Reed knight"),
        ("AN ARGENT KNIGHT", "Argent", "Reed", "A REED KNIGHT"),
        ("a Tess and an Ede", "Tess", "Idris", "an Idris and an Ede"),
        // Sound, not spelling.
        ("a Reed", "Reed", "Uriel", "an Uriel"),
        ("an Argent", "Argent", "University", "a University"),
        ("a Tess", "Tess", "Hour", "an Hour"),
        ("an SS-rank mage", "SS-rank", "D-rank", "a D-rank mage"),
        ("a D-rank mage", "D-rank", "F-rank", "an F-rank mage"),
        // Only a directly adjacent article is touched.
        ("a famous Argent", "Argent", "Idris", "a famous Idris"),
        ("an old Argent", "Argent", "Idris", "an old Idris"),
        // Already correct: no change to the article.
        ("a Reed", "Reed", "Tess", "a Tess"),
        // "a" as part of other words.
        ("data Argent", "Argent", "Idris", "data Idris"),
        // A new name that leaves the article right in a possessive sentence.
        ("an Argent's blade", "Argent", "Reed", "a Reed's blade"),
    ];
    for (text, old, new, want) in cases {
        assert_eq!(
            run(text, old, new).as_deref(),
            Some(*want),
            "{old} -> {new} in {text:?}"
        );
    }
}

#[test]
fn changed_ranges_point_into_the_new_text() {
    let r = rename(
        "An Argent met Argent's friend.",
        &RenameSpec::new("Argent", "Reed"),
    )
    .unwrap();
    assert_eq!(r.text, "A Reed met Reed's friend.");
    let pieces: Vec<&str> = r.changed.iter().map(|c| &r.text[c.clone()]).collect();
    assert_eq!(pieces, ["A", "Reed", "Reed's"]);
    // Sorted and non-overlapping.
    for w in r.changed.windows(2) {
        assert!(w[0].end <= w[1].start);
    }
}

#[test]
fn partial_names_and_longest_match() {
    let spec = RenameSpec::new("Tesselid Argent", "Reed Argent").with_derived_partials();
    assert_eq!(
        spec.partials,
        [("Tesselid".to_string(), "Reed".to_string())]
    );
    let text = "Tesselid Argent smiled. Tesselid left. Argent stayed. TESSELID shouted.";
    let r = rename(text, &spec).unwrap();
    assert_eq!(
        r.text,
        "Reed Argent smiled. Reed left. Argent stayed. REED shouted."
    );

    // The full name is replaced once; the partial does not re-edit inside it.
    let spec = RenameSpec::new("Duke Tesselid", "Duke Reed").with_partial("Tesselid", "Reed");
    assert_eq!(
        rename("Duke Tesselid said Tesselid.", &spec).unwrap().text,
        "Duke Reed said Reed."
    );

    // Partials carry possessives and articles too.
    let spec =
        RenameSpec::new("Tesselid Argent", "Moriphas Argent").with_partial("Tesselid", "Moriphas");
    assert_eq!(
        rename("Tesselid's sword, Tesselid Argent's coat", &spec)
            .unwrap()
            .text,
        "Moriphas' sword, Moriphas Argent's coat"
    );

    // Without partials the lone given name is untouched.
    assert_eq!(
        run("Tesselid left.", "Tesselid Argent", "Reed Argent"),
        None
    );
}

#[test]
fn derive_partials_table() {
    let cases: &[(&str, &str, &[(&str, &str)])] = &[
        ("Tesselid Argent", "Reed Argent", &[("Tesselid", "Reed")]),
        ("Duke Tesselid", "Duke Reed", &[("Tesselid", "Reed")]),
        (
            "Reed Argent of Seren",
            "Reed Argent of Sera",
            &[("Seren", "Sera")],
        ),
        // Two words differ, or the counts differ: nothing is guessed.
        ("Tesselid Argent", "Reed Hale", &[]),
        ("Tesselid", "Reed", &[]),
        ("Tesselid Argent", "Reed", &[]),
        ("Tesselid Argent", "Tesselid Argent", &[]),
        // The differing word must be a short-name-safe, capitalised word.
        ("Lord of Seren", "Lord of Sera", &[("Seren", "Sera")]),
        ("Ed Argent", "Al Argent", &[]),
    ];
    for (old, new, want) in cases {
        let got = derive_partials(old, new);
        let got: Vec<(&str, &str)> = got.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        assert_eq!(&got, want, "{old} -> {new}");
    }
}

#[test]
fn rename_is_idempotent_on_its_own_output() {
    let spec = RenameSpec::new("Tesselid", "Reed");
    let once = rename("Tesselid met an Argent. Tesselid's end.", &spec)
        .unwrap()
        .text;
    assert_eq!(rename(&once, &spec), None);
}
