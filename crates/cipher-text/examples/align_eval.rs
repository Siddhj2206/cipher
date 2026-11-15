//! Measures the aligner on a folder laid out like `Test-Books/<name>` (raw/, tl/,
//! glossary.json): for each Chapter, how many of the names a translated paragraph says are
//! also named in its paired source, by position and aligned.
//! `cargo run -p cipher-text --example align_eval -- Test-Books/S-Class-Transmigrator [N]`
use cipher_text::align::{Aligned, Anchor, align};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(&args[1]);
    let show: Option<i64> = args.get(2).and_then(|a| a.parse().ok());
    let g: Vec<serde_like::Entry> = serde_like::read(&dir.join("glossary.json"));
    let anchors: Vec<Anchor> = g
        .iter()
        .map(|e| Anchor {
            source: &e.0,
            rendering: &e.1,
        })
        .collect();
    let mut files: Vec<(i64, std::path::PathBuf)> = std::fs::read_dir(dir.join("raw"))
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            Some((p.file_stem()?.to_str()?.parse().ok()?, p))
        })
        .collect();
    files.sort();
    let (mut tot_p, mut tot_a, mut tot) = (0usize, 0usize, 0usize);
    let start = std::time::Instant::now();
    for (n, raw) in files {
        let Ok(tl) = std::fs::read_to_string(dir.join("tl").join(raw.file_name().unwrap())) else {
            continue;
        };
        let mut s = cipher_text::blocks::split(&std::fs::read_to_string(&raw).unwrap());
        cipher_text::heading::take_heading(&mut s);
        let mut t = cipher_text::blocks::split(&tl);
        cipher_text::heading::take_heading(&mut t);
        let pos: Vec<Aligned> = (0..s.len().min(t.len()))
            .map(|i| Aligned {
                source: i..i + 1,
                translation: i..i + 1,
            })
            .collect();
        let al = align(&s, &t, &anchors);
        let score = |pairs: &[Aligned]| {
            let (mut ok, mut all) = (0, 0);
            for p in pairs {
                let src: String = s[p.source.clone()].join(" ");
                let tr: String = t[p.translation.clone()].join(" ").to_lowercase();
                for a in &anchors {
                    if a.rendering.chars().count() >= 3 && tr.contains(&a.rendering.to_lowercase())
                    {
                        all += 1;
                        if src.contains(a.source) {
                            ok += 1;
                        }
                    }
                }
            }
            (ok, all)
        };
        let (pp, all) = score(&pos);
        let (aa, _) = score(&al);
        tot_p += pp;
        tot_a += aa;
        tot += all;
        if show.is_none() || show == Some(n) {
            println!(
                "{n}: {} src {} tl  names explained: position {pp}/{all}  aligned {aa}/{all}  pairs {}",
                s.len(),
                t.len(),
                al.len()
            );
        }
        if show == Some(n) {
            for p in &al {
                println!("--- {:?} {:?}", p.source, p.translation);
                println!(
                    "  S: {}",
                    s[p.source.clone()]
                        .join(" | ")
                        .chars()
                        .take(90)
                        .collect::<String>()
                );
                println!(
                    "  T: {}",
                    t[p.translation.clone()]
                        .join(" | ")
                        .chars()
                        .take(110)
                        .collect::<String>()
                );
            }
        }
    }
    println!(
        "TOTAL names explained: position {tot_p}/{tot}  aligned {tot_a}/{tot}  in {:?}",
        start.elapsed()
    );
}

/// glossary.json without a JSON dependency: pulls `"term"` and `"og_term"` pairs.
mod serde_like {
    pub struct Entry(pub String, pub String);
    pub fn read(p: &std::path::Path) -> Vec<Entry> {
        let text = std::fs::read_to_string(p).unwrap_or_default();
        let field = |obj: &str, k: &str| -> Option<String> {
            let at = obj.find(&format!("\"{k}\""))? + k.len() + 2;
            let rest = &obj[at..];
            let q = rest.find('"')? + 1;
            let end = rest[q..].find('"')? + q;
            Some(rest[q..end].to_string())
        };
        text.split('{')
            .filter_map(|o| Some(Entry(field(o, "og_term")?, field(o, "term")?)))
            .filter(|e| !e.0.is_empty() && !e.1.is_empty())
            .collect()
    }
}
