use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

pub fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
}

pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

fn no_color() -> bool {
    static NO_COLOR: OnceLock<bool> = OnceLock::new();
    *NO_COLOR.get_or_init(|| std::env::var("NO_COLOR").is_ok_and(|v| !v.is_empty()))
}

fn ok() -> &'static str { if no_color() { "\u{2713}" } else { "\x1b[32m\u{2713}\x1b[0m" } }
fn skip_mark() -> &'static str { if no_color() { "\u{2014}" } else { "\x1b[33m\u{2014}\x1b[0m" } }
fn fail_mark() -> &'static str { if no_color() { "\u{2717}" } else { "\x1b[31m\u{2717}\x1b[0m" } }

fn green(s: &str) -> String {
    if no_color() { s.to_string() } else { format!("\x1b[32m{s}\x1b[0m") }
}
fn red(s: &str) -> String {
    if no_color() { s.to_string() } else { format!("\x1b[31m{s}\x1b[0m") }
}
fn yellow(s: &str) -> String {
    if no_color() { s.to_string() } else { format!("\x1b[33m{s}\x1b[0m") }
}
fn bold(s: &str) -> String {
    if no_color() { s.to_string() } else { format!("\x1b[1m{s}\x1b[0m") }
}
fn dim(s: &str) -> String {
    if no_color() { s.to_string() } else { format!("\x1b[2m{s}\x1b[0m") }
}

// stdout functions — for display/inspection commands where output IS the data.
// These keep ANSI minimal to remain pipe-friendly.

pub fn detail(message: impl Display) {
    if is_quiet() {
        return;
    }
    println!("- {}", message);
}

pub fn detail_kv(label: &str, value: impl Display) {
    if is_quiet() {
        return;
    }
    println!("- {}: {}", label, value);
}

pub fn section(header: impl Display) {
    if is_quiet() {
        return;
    }
    println!();
    println!("{}", bold(&header.to_string()));
}

pub fn status(message: impl Display) {
    if is_quiet() {
        return;
    }
    println!("{}", message);
}

// stderr functions — for action/confirmation/progress output.
// These use ANSI styling per the unified design.

pub fn stderr_detail(message: impl Display) {
    eprintln!("{} {}", dim("-"), message);
}

pub fn stderr_detail_kv(label: &str, value: impl Display) {
    eprintln!("{} {}: {}", dim("-"), label, value);
}

pub fn stderr_section(header: impl Display) {
    eprintln!();
    eprintln!("{}", bold(&header.to_string()));
}

pub fn stderr_status(message: impl Display) {
    eprintln!("{}", message);
}

pub fn stderr_warn(message: impl Display) {
    eprintln!("{} {}", yellow("\u{26A0}"), message);
}

pub fn stderr_error(message: impl Display) {
    eprintln!("{} {}", red("\u{2717}"), message);
}

// Verbose-only stderr

pub fn verbose_detail(message: impl Display) {
    if is_quiet() || !is_verbose() {
        return;
    }
    eprintln!("{} {}", dim("-"), message);
}

pub fn verbose_detail_kv(label: &str, value: impl Display) {
    if is_quiet() || !is_verbose() {
        return;
    }
    eprintln!("{} {}: {}", dim("-"), label, value);
}

// ── Unified design components ──────────────────────────────────────

pub fn progress_bar(current: usize, total: usize, elapsed: impl Display) {
    let width = 20usize;
    let filled = if total > 0 { (current * width).checked_div(total).unwrap_or(0) } else { 0 };
    let bar: String = (0..width).map(|i| if i < filled { '=' } else { '-' }).collect();
    eprintln!(
        " {} {}  {}/{}  {}",
        dim("Progress:"),
        dim(&format!("[{}]", bar)),
        current,
        total,
        dim(&elapsed.to_string()),
    );
}

pub fn chapter_line_ok(name: impl Display, time: impl Display, tokens: impl Display, tags: &[String]) {
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!("  {}", tags.join(" "))
    };
    eprintln!("\r\x1b[K  {}  {}  {}  {}{}", ok(), name, dim(&time.to_string()), dim(&tokens.to_string()), tag_str);
}

pub fn chapter_line_fail(name: impl Display, time: impl Display, tokens: impl Display, error: impl Display) {
    eprintln!("\r\x1b[K  {}  {}  {}  {}  {}", fail_mark(), name, dim(&time.to_string()), dim(&tokens.to_string()), red(&error.to_string()));
}

pub fn chapter_line_skip(name: impl Display, reason: impl Display) {
    eprintln!("\r\x1b[K  {}  {}  {}", skip_mark(), name, dim(&reason.to_string()));
}

pub fn cancel_banner(completed: usize, total: usize) {
    eprintln!();
    eprintln!(" {} {} after {} chapters", yellow("\u{26A0}"), yellow("Translation cancelled (Ctrl-C)"), dim(&format!("{completed}/{total}")));
    eprintln!();
}

pub fn summary_header() {
    eprintln!(" {}", bold("Summary"));
}

pub fn summary_item(label: impl Display, value: impl Display) {
    eprintln!("  {}  {}  {}", dim("\u{2502}"), bold(&label.to_string()), value);
}

pub fn styled_green(s: impl Display) -> String {
    green(&s.to_string())
}

pub fn styled_red(s: impl Display) -> String {
    red(&s.to_string())
}

pub fn styled_yellow(s: impl Display) -> String {
    yellow(&s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_toggle() {
        set_quiet(true);
        assert!(is_quiet());
        set_quiet(false);
        assert!(!is_quiet());
    }

    #[test]
    fn verbose_set_does_not_panic() {
        set_verbose(true);
        set_verbose(false);
    }

    #[test]
    fn quiet_suppresses_stdout_functions() {
        set_quiet(true);
        detail("test");
        detail_kv("k", "v");
        section("test");
        status("test");
        set_quiet(false);
    }

    #[test]
    fn verbose_detail_respects_flags() {
        set_quiet(false);
        set_verbose(true);
        verbose_detail("visible");
        verbose_detail_kv("k", "v");

        set_verbose(false);
        verbose_detail("hidden");
        verbose_detail_kv("k", "v");

        set_quiet(true);
        set_verbose(true);
        verbose_detail("quiet-hidden");
        verbose_detail_kv("k", "v");
        set_quiet(false);
    }

    #[test]
    fn stderr_functions_do_not_panic() {
        stderr_detail("test");
        stderr_detail_kv("k", "v");
        stderr_section("test");
        stderr_status("test");
        stderr_warn("test");
        stderr_error("test");
    }
}
