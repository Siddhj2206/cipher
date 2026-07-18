use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};

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

fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

// stdout functions — for display/inspection commands where output IS the data

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
    println!("{}", header);
}

pub fn status(message: impl Display) {
    if is_quiet() {
        return;
    }
    println!("{}", message);
}

// stderr functions — for action/confirmation/progress output

pub fn stderr_detail(message: impl Display) {
    eprintln!("- {}", message);
}

pub fn stderr_detail_kv(label: &str, value: impl Display) {
    eprintln!("- {}: {}", label, value);
}

pub fn stderr_section(header: impl Display) {
    eprintln!();
    eprintln!("{}", header);
}

pub fn stderr_status(message: impl Display) {
    eprintln!("{}", message);
}

pub fn stderr_warn(message: impl Display) {
    eprintln!("- Warning: {}", message);
}

pub fn stderr_error(message: impl Display) {
    eprintln!("Error: {}", message);
}

// Verbose-only stderr — for per-chapter details during translate

pub fn verbose_detail(message: impl Display) {
    if is_quiet() || !is_verbose() {
        return;
    }
    eprintln!("- {}", message);
}

pub fn verbose_detail_kv(label: &str, value: impl Display) {
    if is_quiet() || !is_verbose() {
        return;
    }
    eprintln!("- {}: {}", label, value);
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
        // None of these should print anything
        detail("test");
        detail_kv("k", "v");
        section("test");
        status("test");
        // But they also shouldn't panic
        set_quiet(false);
    }

    #[test]
    fn verbose_detail_respects_flags() {
        set_quiet(false);
        set_verbose(true);
        verbose_detail("visible");
        verbose_detail_kv("k", "v");
        // No panic = success

        set_verbose(false);
        verbose_detail("hidden");
        verbose_detail_kv("k", "v");
        // No panic = success

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
