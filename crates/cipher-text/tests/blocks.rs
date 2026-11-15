#![allow(clippy::type_complexity)] // table tests use tuple-of-slices cases

use cipher_text::blocks::{BlockKind, Style, kind, render, split};

const CHAPTER: &str = include_str!("fixtures/chapter_ko.md");

#[test]
fn splits_a_real_shaped_chapter() {
    let blocks = split(CHAPTER);
    assert_eq!(
        blocks,
        [
            "1화",
            "테셀리드가 문을 열었다. 안에는 아무도 없었다.",
            "“누구냐?”",
            "***",
            "한은 대답하지 않았다. 한국의 겨울은 길었다.",
            "- 첫째 문",
            "- 둘째 문",
            // Glued lines stay one Block, joined by a newline.
            "*쾅!*\n[<시스템> 경고. 진입 불가.]",
        ]
    );
}

#[test]
fn split_table() {
    let cases: &[(&str, &[&str])] = &[
        ("", &[]),
        ("  \n\n \n", &[]),
        ("one", &["one"]),
        ("a\r\n\r\nb\r\n", &["a", "b"]),
        ("a\n\n\n\n\nb", &["a", "b"]),
        // Scene breaks and headings are their own Block even without blank lines.
        ("a\n***\nb", &["a", "***", "b"]),
        ("a\n* * *\nb", &["a", "* * *", "b"]),
        ("a\n---\nb", &["a", "---", "b"]),
        ("# T\nbody", &["# T", "body"]),
        ("text\n## Sub\nmore", &["text", "## Sub", "more"]),
        // `#` needs a space; a hashtag is text.
        ("#hash tag", &["#hash tag"]),
        // List items split; continuation lines stay with their item.
        ("- a\n- b\n  more\n\ntail", &["- a", "- b\nmore", "tail"]),
        ("1. one\n2. two", &["1. one", "2. two"]),
        // Dialogue dashes are not list items.
        ("-……\n-You must", &["-……\n-You must"]),
        // `*Boom!*` is emphasis, not a bullet.
        ("*Boom!*\nnext", &["*Boom!*\nnext"]),
        ("  indented  ", &["indented"]),
    ];
    for (input, want) in cases {
        assert_eq!(&split(input), want, "input {input:?}");
    }
}

#[test]
fn kinds() {
    let cases = [
        ("plain", BlockKind::Paragraph),
        ("# Title", BlockKind::Heading(1)),
        ("### Deep", BlockKind::Heading(3)),
        ("***", BlockKind::SceneBreak),
        ("> quoted", BlockKind::Quote),
        ("- item", BlockKind::ListItem),
        ("12. item", BlockKind::ListItem),
        ("*Whoosh!*", BlockKind::Paragraph),
    ];
    for (input, want) in cases {
        assert_eq!(kind(input), want, "input {input:?}");
    }
}

fn spans(input: &str) -> (String, Vec<(String, Style)>) {
    let r = render(input);
    let s = r
        .spans
        .iter()
        .map(|s| (r.text[s.range.clone()].to_string(), s.style))
        .collect();
    (r.text, s)
}

#[test]
fn render_table() {
    use Style::{Emphasis as E, Strong as S};
    let cases: Vec<(&str, &str, Vec<(&str, Style)>)> = vec![
        ("plain", "plain", vec![]),
        ("*Whoosh!*", "Whoosh!", vec![("Whoosh!", E)]),
        ("_soft_ word", "soft word", vec![("soft", E)]),
        ("a **bold** b", "a bold b", vec![("bold", S)]),
        ("***both***", "both", vec![("both", E), ("both", S)]),
        (
            "**bold *and em* text**",
            "bold and em text",
            vec![("bold and em text", S), ("and em", E)],
        ),
        ("*a* and *b*", "a and b", vec![("a", E), ("b", E)]),
        // Unmatched markers stay literal.
        ("*open", "*open", vec![]),
        ("close*", "close*", vec![]),
        ("a * b * c", "a * b * c", vec![]),
        ("**x", "**x", vec![]),
        ("***", "***", vec![]),
        ("snake_case_name", "snake_case_name", vec![]),
        // `*` may open and close inside a word (CommonMark), `_` may not.
        ("2*3*4", "234", vec![("3", E)]),
        // Escapes.
        (r"\*not em\*", "*not em*", vec![]),
        // Korean text and quotes next to markers.
        ("*쾅!* 그가 말했다", "쾅! 그가 말했다", vec![("쾅!", E)]),
        ("*“Boom!”*", "“Boom!”", vec![("“Boom!”", E)]),
        // Block-level prefixes.
        ("# Chapter 227: ", "Chapter 227:", vec![]),
        ("## *Title*", "Title", vec![("Title", E)]),
        ("> quoted *x*", "quoted x", vec![("x", E)]),
        ("43. Heretic Inquisitor", "43. Heretic Inquisitor", vec![]),
        ("one\n*two*", "one\ntwo", vec![("two", E)]),
    ];
    for (input, text, want) in cases {
        let (got_text, got_spans) = spans(input);
        let want: Vec<(String, Style)> = want
            .into_iter()
            .map(|(s, st)| (s.to_string(), st))
            .collect();
        assert_eq!(got_text, text, "text of {input:?}");
        assert_eq!(got_spans, want, "spans of {input:?}");
    }
}

#[test]
fn span_ranges_are_char_boundaries() {
    let r = render("가나다 *라마* **바사**");
    for s in &r.spans {
        assert!(r.text.is_char_boundary(s.range.start));
        assert!(r.text.is_char_boundary(s.range.end));
    }
    assert_eq!(r.spans.len(), 2);
}
