use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};

use console::Style;

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

fn color_enabled() -> bool {
    console::colors_enabled()
}

fn style_green() -> Style {
    Style::new().green()
}
fn style_red() -> Style {
    Style::new().red()
}
fn style_yellow() -> Style {
    Style::new().yellow()
}
fn style_bold() -> Style {
    Style::new().bold()
}
fn style_dim() -> Style {
    Style::new().dim()
}

fn ok() -> String {
    if color_enabled() {
        style_green().apply_to("\u{2713}").to_string()
    } else {
        "\u{2713}".to_string()
    }
}

fn fail_mark() -> String {
    if color_enabled() {
        style_red().apply_to("\u{2717}").to_string()
    } else {
        "\u{2717}".to_string()
    }
}

fn apply_green(s: &str) -> String {
    style_green().apply_to(s).to_string()
}

fn apply_red(s: &str) -> String {
    style_red().apply_to(s).to_string()
}

fn apply_yellow(s: &str) -> String {
    style_yellow().apply_to(s).to_string()
}

fn apply_bold(s: &str) -> String {
    style_bold().apply_to(s).to_string()
}

fn apply_dim(s: &str) -> String {
    style_dim().apply_to(s).to_string()
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
    println!("{}", apply_bold(&header.to_string()));
}

pub fn status(message: impl Display) {
    if is_quiet() {
        return;
    }
    println!("{}", message);
}

// stderr functions — for action/confirmation/progress output.

pub fn stderr_detail(message: impl Display) {
    eprintln!("{} {}", apply_dim("-"), message);
}

pub fn stderr_detail_kv(label: &str, value: impl Display) {
    eprintln!("{} {}: {}", apply_dim("-"), label, value);
}

pub fn stderr_section(header: impl Display) {
    eprintln!();
    eprintln!("{}", apply_bold(&header.to_string()));
}

pub fn stderr_status(message: impl Display) {
    eprintln!("{}", message);
}

pub fn stderr_warn(message: impl Display) {
    eprintln!("{} {}", apply_yellow("\u{26A0}"), message);
}

pub fn stderr_error(message: impl Display) {
    eprintln!("{} {}", apply_red("\u{2717}"), message);
}

// Verbose-only stderr

pub fn verbose_detail(message: impl Display) {
    if is_quiet() || !is_verbose() {
        return;
    }
    eprintln!("{} {}", apply_dim("-"), message);
}

pub fn verbose_detail_kv(label: &str, value: impl Display) {
    if is_quiet() || !is_verbose() {
        return;
    }
    eprintln!("{} {}: {}", apply_dim("-"), label, value);
}

// ── Unified design components ──────────────────────────────────────

pub fn chapter_line_ok(
    name: impl Display,
    time: impl Display,
    tokens: impl Display,
    tags: &[String],
) {
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!("  {}", tags.join(" "))
    };
    eprintln!(
        "\r\x1b[K  {}  {}  {}  {}{}",
        ok(),
        name,
        apply_dim(&time.to_string()),
        apply_dim(&tokens.to_string()),
        tag_str
    );
}

pub fn chapter_line_fail(
    name: impl Display,
    time: impl Display,
    tokens: impl Display,
    error: impl Display,
) {
    eprintln!(
        "\r\x1b[K  {}  {}  {}  {}  {}",
        fail_mark(),
        name,
        apply_dim(&time.to_string()),
        apply_dim(&tokens.to_string()),
        apply_red(&error.to_string())
    );
}

pub fn cancel_banner(completed: usize, total: usize) {
    eprintln!();
    eprintln!(
        " {} {} after {} chapters",
        apply_yellow("\u{26A0}"),
        apply_yellow("Translation cancelled (Ctrl-C)"),
        apply_dim(&format!("{completed}/{total}"))
    );
    eprintln!();
}

pub fn summary_header() {
    eprintln!(" {}", apply_bold("Summary"));
}

pub fn summary_item(label: impl Display, value: impl Display) {
    eprintln!(
        "  {}  {}  {}",
        apply_dim("\u{2502}"),
        apply_bold(&label.to_string()),
        value
    );
}

pub fn styled_green(s: impl Display) -> String {
    apply_green(&s.to_string())
}

pub fn styled_red(s: impl Display) -> String {
    apply_red(&s.to_string())
}

pub fn styled_yellow(s: impl Display) -> String {
    apply_yellow(&s.to_string())
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

    fn with_colors(enabled: bool, f: impl FnOnce()) {
        let prev = console::colors_enabled();
        console::set_colors_enabled(enabled);
        f();
        console::set_colors_enabled(prev);
    }

    #[test]
    fn styled_green_returns_ansi_when_colors_enabled() {
        with_colors(true, || {
            let result = styled_green("hello");
            assert!(result.contains("hello"));
            assert_ne!(result, "hello");
        });
    }

    #[test]
    fn styled_green_returns_plain_text_when_colors_disabled() {
        with_colors(false, || {
            let result = styled_green("hello");
            assert_eq!(result, "hello");
        });
    }

    #[test]
    fn styled_red_and_yellow_also_use_console() {
        with_colors(true, || {
            assert_ne!(styled_red("x"), "x");
            assert_ne!(styled_yellow("x"), "x");
        });
        with_colors(false, || {
            assert_eq!(styled_red("x"), "x");
            assert_eq!(styled_yellow("x"), "x");
        });
    }
}
