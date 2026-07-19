// PROTOTYPE — throwaway CLI output design exploration for issue #56.
// Run with: cargo run --bin cli-prototype [A|B|C]
// Shows three approaches to translate output.
// v3: persistent per-chapter lines (survive cancel) + summary at end.

struct ChapterResult {
    name: &'static str,
    status: &'static str,
    reason: &'static str,
    time_ms: u64,
    tokens_in: u32,
    tokens_out: u32,
    repaired: bool,
    glossary_terms_added: u32,
}

const RESULTS: &[ChapterResult] = &[
    ChapterResult { name: "chapter-01-intro.md",    status: "ok",   reason: "",                    time_ms: 12340,  tokens_in: 2340,  tokens_out: 1890, repaired: false, glossary_terms_added: 1 },
    ChapterResult { name: "chapter-02-setup.md",    status: "ok",   reason: "",                    time_ms: 8120,   tokens_in: 1560,  tokens_out: 1200, repaired: false, glossary_terms_added: 0 },
    ChapterResult { name: "chapter-03-advanced.md", status: "skip", reason: "empty chapter",       time_ms: 0,      tokens_in: 0,     tokens_out: 0,    repaired: false, glossary_terms_added: 0 },
    ChapterResult { name: "chapter-04-api.md",      status: "fail", reason: "Validation failed: missing heading", time_ms: 14200, tokens_in: 3100,  tokens_out: 2500, repaired: true,  glossary_terms_added: 0 },
    ChapterResult { name: "chapter-05-fn.md",       status: "ok",   reason: "",                    time_ms: 15100,  tokens_in: 4200,  tokens_out: 3100, repaired: true,  glossary_terms_added: 2 },
    ChapterResult { name: "chapter-06-types.md",    status: "ok",   reason: "",                    time_ms: 10100,  tokens_in: 1980,  tokens_out: 1650, repaired: false, glossary_terms_added: 0 },
    ChapterResult { name: "chapter-07-macros.md",   status: "fail", reason: "API error after 3 retries", time_ms: 45100, tokens_in: 8900,  tokens_out: 0,    repaired: false, glossary_terms_added: 0 },
    ChapterResult { name: "chapter-08-meta.md",     status: "skip", reason: "rerun not needed",    time_ms: 0,      tokens_in: 0,     tokens_out: 0,    repaired: false, glossary_terms_added: 0 },
    ChapterResult { name: "chapter-09-hof.md",      status: "ok",   reason: "",                    time_ms: 22100,  tokens_in: 5600,  tokens_out: 4300, repaired: false, glossary_terms_added: 1 },
    ChapterResult { name: "chapter-10-trait.md",    status: "ok",   reason: "",                    time_ms: 18100,  tokens_in: 4700,  tokens_out: 3500, repaired: false, glossary_terms_added: 1 },
];

fn main() {
    let variant = std::env::args().nth(1).unwrap_or_default().to_uppercase();
    match variant.as_str() {
        "B" => variant_b(),
        "C" => variant_c(),
        _   => variant_a(),
    }
}

fn reset() -> &'static str { "\x1b[0m" }
fn bold(s: &str) -> String { format!("\x1b[1m{s}{r}", r = reset()) }
fn dim(s: &str) -> String  { format!("\x1b[2m{s}{r}", r = reset()) }
fn green(s: &str) -> String { format!("\x1b[32m{s}{r}", r = reset()) }
fn red(s: &str) -> String   { format!("\x1b[31m{s}{r}", r = reset()) }
fn yellow(s: &str) -> String { format!("\x1b[33m{s}{r}", r = reset()) }

fn fmt_time(ms: u64) -> String {
    if ms == 0 { return "\u{2014}".to_string(); }
    let total_s = (ms + 500) / 1000;
    if total_s >= 60 {
        format!("{}m{:02}s", total_s / 60, total_s % 60)
    } else {
        format!("{total_s}s")
    }
}

fn fmt_tokens(n: u32) -> String {
    if n == 0 { return "\u{2014}".to_string(); }
    if n >= 1000 { format!("{}K", (n as f64 / 1000.0 * 10.0).round() / 10.0) }
    else { n.to_string() }
}

fn count_results(up_to: usize) -> (usize, usize, usize, u64, u32, u32) {
    let ok = RESULTS[..up_to].iter().filter(|c| c.status == "ok").count();
    let skip = RESULTS[..up_to].iter().filter(|c| c.status == "skip").count();
    let fail = RESULTS[..up_to].iter().filter(|c| c.status == "fail").count();
    let total_time: u64 = RESULTS[..up_to].iter().map(|c| c.time_ms).sum();
    let total_tokens: u32 = RESULTS[..up_to].iter().map(|c| c.tokens_in + c.tokens_out).sum();
    let total_terms: u32 = RESULTS[..up_to].iter().map(|c| c.glossary_terms_added).sum();
    (ok, skip, fail, total_time, total_tokens, total_terms)
}

// ─── Variant A: Persistent per-chapter lines (progress bar + results below) ─────

fn variant_a() {
    // Header line (persistent)
    eprintln!(" {}  {} chapters  {}", dim("\u{2500}"), RESULTS.len(), dim("Profile: default"));
    eprintln!();

    // Progress bar line (updates in-place — simulates indicatif)
    // In reality this would be an indicatif ProgressBar; here we show the final state.
    eprintln!(" {}", dim("Translating [====================] 10/10"));
    eprintln!();

    // Per-chapter results (persistent — each line printed once)
    for c in RESULTS {
        let mut tags: Vec<String> = Vec::new();
        if c.repaired { tags.push(dim("repaired")); }
        if c.glossary_terms_added > 0 { tags.push(green(&format!("+{} term", c.glossary_terms_added))); }
        let tag_str = if tags.is_empty() { String::new() } else { format!("  {}", tags.join(" ")) };

        let time_str = dim(&fmt_time(c.time_ms));
        let tok_str = dim(&fmt_tokens(c.tokens_in + c.tokens_out));

        match c.status {
            "ok" => eprintln!(
                "  {}  {}  {}  {} tok{}",
                green("\u{2713}"), c.name, time_str, tok_str, tag_str
            ),
            "skip" => eprintln!(
                "  {}  {}  {}",
                yellow("\u{2014}"), c.name, dim(c.reason)
            ),
            "fail" => eprintln!(
                "  {}  {}  {}  {} tok{}",
                red("\u{2717}"), c.name, time_str, tok_str,
                format!("  {}", red(c.reason))
            ),
            _ => {}
        }
    }

    // Summary (persistent)
    eprintln!();
    let (ok, skip, fail, total_time, total_tokens, total_terms) = count_results(RESULTS.len());
    eprintln!(" {}", bold("Summary"));
    eprintln!("  {}  {}  {}  {}", dim("\u{2502}"), bold("Chapters"), RESULTS.len(), summary_line(ok, skip, fail));
    eprintln!("  {}  {}  {}", dim("\u{2502}"), bold("Time"), fmt_time(total_time));
    eprintln!("  {}  {}  {}", dim("\u{2502}"), bold("Token usage"), format!("{total_tokens} (${:.2})", total_tokens as f64 * 0.02 / 1000.0));
    eprintln!("  {}  {}  {}", dim("\u{2502}"), bold("Glossary terms added"), total_terms);
    eprintln!();
    let msg = green("Translation complete.");
    eprintln!(" {}", bold(&msg));
}

// ─── Variant B: Same layout but with tabular per-chapter lines ─────

fn variant_b() {
    eprintln!(" {}  {} chapters  {}", dim("\u{2500}"), RESULTS.len(), dim("Profile: default"));
    eprintln!();
    eprintln!(" {}", dim("Translating [====================] 10/10"));
    eprintln!();

    // Table header
    eprintln!("  {} {:28} {:>7} {:>8} {} {}", bold(" "), bold("Chapter"), bold("Time"), bold("Tokens"), bold("Terms"), bold("Notes"));
    for c in RESULTS {
        let time_str = fmt_time(c.time_ms);
        let tok_str = if c.tokens_in + c.tokens_out > 0 {
            format!("{}", c.tokens_in + c.tokens_out)
        } else { "\u{2014}".to_string() };
        let terms_str = if c.glossary_terms_added > 0 {
            green(&format!("+{}", c.glossary_terms_added))
        } else { String::new() };

        match c.status {
            "ok" => {
                let notes = if c.repaired { dim("repaired") } else { String::new() };
                eprintln!("  {} {:28} {:>7} {:>8} {:>6} {}", green("\u{2713}"), c.name, time_str, tok_str, terms_str, notes);
            }
            "skip" => {
                eprintln!("  {} {:28} {:>7} {:>8} {:>6} {}", yellow("\u{2014}"), c.name, "\u{2014}", "\u{2014}", "", dim(c.reason));
            }
            "fail" => {
                eprintln!("  {} {:28} {:>7} {:>8} {:>6} {}", red("\u{2717}"), c.name, time_str, tok_str, "", red(c.reason));
            }
            _ => {}
        }
    }

    eprintln!();
    let (ok, skip, fail, total_time, total_tokens, total_terms) = count_results(RESULTS.len());
    eprintln!(" {}", bold("Summary"));
    eprintln!("  {}  {}  {}  {}", dim("\u{2502}"), bold("Chapters"), RESULTS.len(), summary_line(ok, skip, fail));
    eprintln!("  {}  {}  {}", dim("\u{2502}"), bold("Time"), fmt_time(total_time));
    eprintln!("  {}  {}  {}", dim("\u{2502}"), bold("Token usage"), format!("{total_tokens} (${:.2})", total_tokens as f64 * 0.02 / 1000.0));
    eprintln!("  {}  {}  {}", dim("\u{2502}"), bold("Glossary terms added"), total_terms);
    eprintln!();
    let msg_b = green("Translation complete.");
    eprintln!(" {}", bold(&msg_b));
}

// ─── Variant C: Compact per-chapter + mini-progress, simulate mid-cancel ─────

fn variant_c() {
    // Simulate a mid-cancel scenario — only 7 of 10 chapters processed
    let processed = 7;

    // Header
    eprintln!(" {} {}", bold("cipher translate"), dim("\u{2014} book: fp-essentials  \u{2022}  10 chapters  \u{2022}  Profile: default"));
    eprintln!(" {}", dim("\u{2500}").repeat(48));
    eprintln!();

    // Mini progress (one line, updates in place)
    eprintln!(" {} {}/{} chapters  {}  {} tok",
        dim("Progress:"),
        processed, RESULTS.len(),
        dim(&format_time(RESULTS[..processed].iter().map(|c| c.time_ms).sum())),
        RESULTS[..processed].iter().map(|c| c.tokens_in + c.tokens_out).sum::<u32>(),
    );
    eprintln!();

    // Per-chapter lines (persistent)
    for (i, c) in RESULTS.iter().enumerate() {
        if i >= processed { break; }
        let mut tags: Vec<String> = Vec::new();
        if c.repaired { tags.push(dim("(repaired)")); }
        if c.glossary_terms_added > 0 { tags.push(green(&format!("+{}", c.glossary_terms_added))); }
        let tag_str = if tags.is_empty() { String::new() } else { format!(" {}", tags.join(" ")) };

        let time_str = dim(&fmt_time(c.time_ms));
        let tok_str = dim(&format_tok(c.tokens_in + c.tokens_out));

        match c.status {
            "ok" => eprintln!("  {} {}  {}  {}{}", green("\u{2713}"), c.name, time_str, tok_str, tag_str),
            "skip" => eprintln!("  {} {}  {}", yellow("\u{2014}"), c.name, dim(c.reason)),
            "fail" => eprintln!("  {} {}  {}  {}  {}", red("\u{2717}"), c.name, time_str, tok_str, red(c.reason)),
            _ => {}
        }
    }

    // Mid-cancel indicator
    eprintln!();
    eprintln!(" {}", yellow("\u{26A0} Translation cancelled (Ctrl-C) after {} chapters").replace("{}", &processed.to_string()));
    eprintln!();

    // Summary of what was completed
    let (ok, skip, fail, total_time, total_tokens, total_terms) = count_results(processed);
    eprintln!(" {}  {}  {}", bold("Completed:"), summary_line(ok, skip, fail), dim(&format!("({processed}/{total})", total = RESULTS.len())));
    eprintln!(" {}  {}  {}", dim("\u{2514}"), dim("Token usage"), format!("{total_tokens}  {t}", t = dim(&fmt_time(total_time))));
    let terms_msg = format!("Glossary terms added: {total_terms}");
    eprintln!(" {}  {}", dim("\u{2514}"), dim(&terms_msg));
}

fn format_time(ms: u64) -> String {
    let total_s = ms / 1000;
    if total_s >= 60 {
        format!("{}m{:02}s", total_s / 60, total_s % 60)
    } else {
        format!("{total_s}s")
    }
}

fn format_tok(n: u32) -> String {
    if n >= 1000 {
        format!("{:.1}K tok", n as f64 / 1000.0)
    } else {
        format!("{n} tok")
    }
}

fn summary_line(ok: usize, skip: usize, fail: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    if ok > 0 { parts.push(format!("{c}{ok} translated{r}", c = "\x1b[32m", r = reset())); }
    if skip > 0 { parts.push(format!("{c}{skip} skipped{r}", c = "\x1b[33m", r = reset())); }
    if fail > 0 { parts.push(format!("{c}{fail} failed{r}", c = "\x1b[31m", r = reset())); }
    parts.join(", ")
}
