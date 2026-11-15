//! Pairing an existing translation with its source when nothing says which paragraph is which:
//! an imported Chapter. Local and free.
//!
//! A dynamic program over both paragraph lists in the manner of Gale and Church (1993): each
//! step takes 1–3 source paragraphs and 0–3 translated ones as one Pair, and its cost is how
//! far the group's length is from the Chapter's own length ratio, less what the two sides
//! visibly share. Shared means a Term named in the source with its rendering in the
//! translation, the same shape (both dialogue, both a bracketed system line) and the same
//! numbers. Names are the strong signal: a paragraph that names Tesselid in the source almost
//! always says "Tesselid" in English too.

/// One Pair: source paragraphs `source` and translated paragraphs `translation`, as index
/// ranges. `translation` is empty for a source paragraph the translator left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aligned {
    pub source: std::ops::Range<usize>,
    pub translation: std::ops::Range<usize>,
}

/// A Term to anchor on: its source form and rendering.
pub struct Anchor<'a> {
    pub source: &'a str,
    pub rendering: &'a str,
}

/// The group shapes a step may take: (source paragraphs, translated paragraphs).
const STEPS: [(usize, usize); 9] = [
    (1, 1),
    (1, 2),
    (2, 1),
    (1, 3),
    (3, 1),
    (2, 2),
    (1, 0),
    (2, 3),
    (3, 2),
];

/// Pairs `source` with `translation`. Every translated paragraph lands in exactly one Pair, in
/// order; a source paragraph may be left with none. Returns nothing when either side is empty.
pub fn align(source: &[String], translation: &[String], anchors: &[Anchor]) -> Vec<Aligned> {
    let (n, m) = (source.len(), translation.len());
    if n == 0 || m == 0 {
        return Vec::new();
    }
    let s: Vec<Para> = source.iter().map(|t| Para::source(t, anchors)).collect();
    let t: Vec<Para> = translation
        .iter()
        .map(|t| Para::translation(t, anchors))
        .collect();
    let total = |p: &[Para]| p.iter().map(|p| p.len).sum::<f64>().max(1.0);
    let ratio = total(&t) / total(&s);

    // cost[i][j]: the cheapest way to pair source[..i] with translation[..j].
    let width = m + 1;
    let mut cost = vec![f64::INFINITY; (n + 1) * width];
    let mut back = vec![(0u8, 0u8); (n + 1) * width];
    cost[0] = 0.0;
    // A band around the diagonal keeps long Chapters fast; generous, since translators merge.
    let band = 40 + n.abs_diff(m);
    for i in 0..=n {
        let diag = i * m / n;
        let lo = diag.saturating_sub(band);
        let hi = (diag + band).min(m);
        for j in lo..=hi {
            let here = cost[i * width + j];
            if !here.is_finite() {
                continue;
            }
            for &(a, b) in &STEPS {
                let (ni, nj) = (i + a, j + b);
                if ni > n || nj > m {
                    continue;
                }
                let c = here + step_cost(&s[i..ni], &t[j..nj], ratio);
                let k = ni * width + nj;
                if c < cost[k] {
                    cost[k] = c;
                    back[k] = (a as u8, b as u8);
                }
            }
        }
    }
    if !cost[n * width + m].is_finite() {
        return by_position(n, m);
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let (a, b) = back[i * width + j];
        let (a, b) = (a as usize, b as usize);
        if a == 0 && b == 0 {
            return by_position(n, m);
        }
        out.push(Aligned {
            source: i - a..i,
            translation: j - b..j,
        });
        i -= a;
        j -= b;
    }
    out.reverse();
    out
}

/// The fallback: paragraph i with paragraph i, extras joining the last Pair.
fn by_position(n: usize, m: usize) -> Vec<Aligned> {
    let k = n.min(m);
    let mut out: Vec<Aligned> = (0..k)
        .map(|i| Aligned {
            source: i..i + 1,
            translation: i..i + 1,
        })
        .collect();
    if let Some(last) = out.last_mut() {
        last.source.end = n;
        last.translation.end = m;
    }
    out
}

/// What a paragraph offers the aligner.
struct Para {
    /// Characters, weighted so Hangul counts about as much as the English it becomes.
    len: f64,
    /// Indexes of the anchors it names, sorted.
    names: Vec<u16>,
    dialogue: bool,
    bracket: bool,
    digits: String,
}

impl Para {
    fn source(text: &str, anchors: &[Anchor]) -> Para {
        let names = anchors
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.source.is_empty() && text.contains(a.source))
            .map(|(i, _)| i as u16)
            .collect();
        Para::new(text, names)
    }

    fn translation(text: &str, anchors: &[Anchor]) -> Para {
        let lower = text.to_lowercase();
        let names = anchors
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                let r = a.rendering.trim();
                r.chars().count() >= 2 && lower.contains(&r.to_lowercase())
            })
            .map(|(i, _)| i as u16)
            .collect();
        Para::new(text, names)
    }

    fn new(text: &str, names: Vec<u16>) -> Para {
        let t = text.trim();
        let first = t.chars().next().unwrap_or(' ');
        Para {
            len: t.chars().filter(|c| !c.is_whitespace()).count() as f64,
            names,
            dialogue: matches!(first, '"' | '“' | '「' | '『' | '\''),
            bracket: matches!(first, '[' | '<' | '〈' | '《' | '【' | '(' | '（'),
            digits: t.chars().filter(char::is_ascii_digit).collect(),
        }
    }
}

fn step_cost(s: &[Para], t: &[Para], ratio: f64) -> f64 {
    // Leaving a source paragraph out is allowed but dear: translators rarely drop lines.
    if t.is_empty() {
        return 6.0 + s.iter().map(|p| p.len).sum::<f64>() / 40.0;
    }
    let ls: f64 = s.iter().map(|p| p.len).sum();
    let lt: f64 = t.iter().map(|p| p.len).sum();
    // Length: the log of how far off the expected length the group is, softened for short
    // lines, where "……" can become "…" or "Ugh."
    let off = ((lt + 12.0) / (ls * ratio + 12.0)).ln();
    let mut cost = 4.0 * off * off;

    // Merges cost a little each, so 1–1 wins whenever the evidence is even.
    cost += 0.9 * ((s.len() - 1) + (t.len() - 1)) as f64;

    // Names: each one seen on both sides pulls the group together; one on only one side
    // pushes it apart, mildly, because Korean repeats names where English says "he".
    let names = |ps: &[Para]| {
        let mut v: Vec<u16> = ps.iter().flat_map(|p| p.names.iter().copied()).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let (ns, nt) = (names(s), names(t));
    let shared = ns.iter().filter(|x| nt.binary_search(x).is_ok()).count();
    let only_t = nt.len() - shared;
    let only_s = ns.len() - shared;
    cost -= 1.6 * shared as f64;
    cost += 1.2 * only_t as f64 + 0.4 * only_s as f64;

    // Shape: the first paragraph of each side.
    let (a, b) = (&s[0], &t[0]);
    if a.dialogue != b.dialogue {
        cost += 1.0;
    } else if a.dialogue {
        cost -= 0.3;
    }
    if a.bracket != b.bracket {
        cost += 1.0;
    } else if a.bracket {
        cost -= 0.5;
    }
    let ds: String = s.iter().map(|p| p.digits.as_str()).collect();
    let dt: String = t.iter().map(|p| p.digits.as_str()).collect();
    if !ds.is_empty() || !dt.is_empty() {
        cost += if ds == dt { -0.8 } else { 0.6 };
    }
    cost
}
