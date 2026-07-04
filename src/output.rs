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
