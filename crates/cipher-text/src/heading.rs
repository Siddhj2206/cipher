//! Chapter headings: the `227화` or `# Chapter 227: The Gate` line at the top of a Chapter,
//! which carries its number and title and is not a Block to translate.

/// What a heading line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub number: Option<u32>,
    pub title: Option<String>,
}

/// Longest line still taken for a heading; anything longer is prose.
const MAX_CHARS: usize = 60;

/// Reads a Chapter heading from one Block, or `None` if the Block is prose.
///
/// Recognised, with optional leading `#`s:
/// - `227`, `227화`, `제227화`, `第12話`, `第3章`, `3장`, each optionally followed by a title
///   after spaces or `-`, `:`, `.`, `–`, `—`;
/// - `Chapter 12`, `Chapter 12: Title`, `Ch. 12 - Title` (any case).
///
/// A number followed by other Korean (`227명의 병사가`) is prose.
pub fn chapter_heading(block: &str) -> Option<Heading> {
    let line = block.trim();
    if line.is_empty() || line.contains('\n') || line.chars().count() > MAX_CHARS {
        return None;
    }
    let line = line.trim_start_matches('#').trim();

    let lower = line.to_lowercase();
    for prefix in ["chapter", "ch."] {
        if lower.starts_with(prefix) {
            let rest = line[prefix.len()..].trim_start();
            let (number, rest) = leading_number(rest)?;
            return Some(Heading {
                number: Some(number),
                title: title(rest)?,
            });
        }
    }

    let rest = line
        .strip_prefix('제')
        .or_else(|| line.strip_prefix('第'))
        .unwrap_or(line)
        .trim_start();
    let (number, rest) = leading_number(rest)?;
    let rest = rest.trim_start();
    let rest = ["화", "話", "章", "장", "回"]
        .iter()
        .find_map(|m| rest.strip_prefix(m))
        .unwrap_or(rest);
    Some(Heading {
        number: Some(number),
        title: title(rest)?,
    })
}

/// Removes a heading from the front of a Chapter's Blocks and returns it.
pub fn take_heading(blocks: &mut Vec<String>) -> Option<Heading> {
    let heading = blocks.first().and_then(|b| chapter_heading(b))?;
    blocks.remove(0);
    Some(heading)
}

/// The Chapter number of pasted text: the first heading among its first five lines.
pub fn detect_number(text: &str) -> Option<u32> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .find_map(|l| chapter_heading(l)?.number)
}

fn leading_number(s: &str) -> Option<(u32, &str)> {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let n = s[..digits].parse().ok()?;
    Some((n, &s[digits..]))
}

/// What follows the number: nothing, or a separator and a title. `None` means the line
/// continues as prose, so it is not a heading.
fn title(rest: &str) -> Option<Option<String>> {
    if rest.is_empty() {
        return Some(None);
    }
    let starts_sep = rest.starts_with([' ', ':', '-', '.', '–', '—', ')', ']']);
    if !starts_sep {
        return None;
    }
    let t = rest
        .trim_start_matches([' ', ':', '-', '.', '–', '—', ')', ']'])
        .trim();
    Some((!t.is_empty()).then(|| t.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(number: u32, title: Option<&str>) -> Option<Heading> {
        Some(Heading {
            number: Some(number),
            title: title.map(str::to_owned),
        })
    }

    #[test]
    fn headings() {
        let cases: &[(&str, Option<Heading>)] = &[
            ("227화", h(227, None)),
            ("제 12 화", h(12, None)),
            ("제12화 탑의 주인", h(12, Some("탑의 주인"))),
            ("12화 - 귀환", h(12, Some("귀환"))),
            ("第12話", h(12, None)),
            ("第3章 始まり", h(3, Some("始まり"))),
            ("# Chapter 227: ", h(227, None)),
            ("# Chapter 3: The Gate", h(3, Some("The Gate"))),
            ("Ch. 4 - Return", h(4, Some("Return"))),
            ("227", h(227, None)),
            ("227명의 병사가 왔다.", None),
            ("Chapter one began.", None),
            (
                "세 명의 강자를 보유하고 있던 라그네이프 마도 공화국이었으나, 이제 다 옛말이 되었다.",
                None,
            ),
            ("", None),
        ];
        for (input, want) in cases {
            assert_eq!(&chapter_heading(input), want, "{input:?}");
        }
    }

    #[test]
    fn pasted_text() {
        assert_eq!(
            detect_number("\n\n321화\n\n그날 밤, 탑 위에서 바람이 불었다."),
            Some(321)
        );
        assert_eq!(detect_number("그날 밤, 탑 위에서 바람이 불었다."), None);
        let mut blocks = vec!["321화".to_string(), "본문".to_string()];
        assert_eq!(take_heading(&mut blocks), h(321, None));
        assert_eq!(blocks, ["본문"]);
    }
}
