# Output Crates Research

Date: 2026-07-19
Project: cipher
Context: `src/output.rs` uses hand-rolled ANSI escape codes. Goal is to evaluate mature crates that could replace or complement it.

---

## 1. console

**crates.io**: https://crates.io/crates/console  
**docs**: https://docs.rs/console/latest/console/  
**source**: https://github.com/console-rs/console  
**version**: 0.16.4 (Jul 2026)  
**maintainers**: mitsuhiko, djc (same authors as `indicatif` and `dialoguer`)

### What it provides

- `console::style()` — chainable inline styling: `style("text").cyan().bold()`
- `console::Style` — stored style object, apply later with `style.apply_to(text)`
- `console::Term` — terminal abstraction (size, cursor, clear, read)
- `colors_enabled()` / `colors_enabled_stderr()` — auto-detect TTY and `NO_COLOR`
- `set_colors_enabled()` / `set_colors_enabled_stderr()` — override for `--color` flag support
- `strip_ansi_codes()`, `measure_text_width()`, `truncate_str()` — text utilities
- `Emoji` helper — intelligent emoji rendering (falls back on unsupported terminals)

### Pros for cipher

- **Dialoguer is already a dependency** — console is the same `console-rs` family (same repo). Adding it adds zero ecosystem friction.
- **Automatic `NO_COLOR`** — `colors_enabled()` reads `NO_COLOR` and `FORCE_COLOR` out of the box, matching the current hand-rolled logic in `no_color()`.
- **Separate stdout/stderr color detection** — important because cipher uses stdout for data and stderr for progress/status.
- **Can directly replace every ANSI helper** in `output.rs` (`green()`, `red()`, `bold()`, `dim()`, etc.) with `style(x).green()` etc.
- **No performance concern** — style objects are lightweight wrappers.

### Cons

- Pulls in `unicode-width` and `encode_unicode` as unconditional deps (optional with feature flags, but useful).
- The `style()` API does heap-allocate per invocation (fine for CLI output volume).

### NO_COLOR support

Automatic via `console::colors_enabled()`. Also has `set_colors_enabled(false)` for explicit override. The crate checks `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, and `TERM=dumb`.

---

## 2. indicatif

**crates.io**: https://crates.io/crates/indicatif  
**docs**: https://docs.rs/indicatif/latest/indicatif/  
**source**: https://github.com/console-rs/indicatif  
**version**: 0.18.6 (Jul 2026)  
**maintainers**: mitsuhiko, djc

### What it provides

- `ProgressBar` — bounded progress bar and unbounded spinner
- `MultiProgress` — manage multiple bars from different threads
- `ProgressStyle` — template-based bar rendering (e.g. `{bar:40.cyan/blue} {pos}/{len} {msg}`)
- `ProgressIterator` / `ParallelProgressIterator` — wrap iterators with progress
- Human formatting: `HumanBytes`, `HumanDuration`, `HumanCount`, `HumanFloatCount`
- Automatically hides when not a TTY (pipes to logfiles correctly)

### Pros for cipher

- **Naturally pairs with console** — indicatif depends on console internally. Styles use console's color format strings.
- **Directly useful in `translate` workflow** — per-chapter progress during translation is exactly what it's built for.
- **Spinner style** useful for LLM calls with indeterminate wait time.
- **`MultiProgress`** could track parallel chapter processing if added later.
- **Auto-hides on non-TTY** — already matches cipher's preference for pipe-friendly output.

### Cons

- Adds async/sync complexity — progress bar must be `finish()`-ed or `finish_and_clear()`-ed.
- Adds ~10 dependencies (console, portable-atomic, unit-prefix, etc.). Not heavy for a CLI app.
- Template DSL is powerful but has a learning curve.

### NO_COLOR support

Delegates to `console` under the hood. Colors in bar templates respect `NO_COLOR`.

---

## 3. owo-colors

**crates.io**: https://crates.io/crates/owo-colors  
**docs**: https://docs.rs/owo-colors/latest/owo_colors/  
**source**: https://github.com/owo-colors/owo-colors  
**version**: 4.3.0 (Jun 2026)  
**maintainers**: jam1garner, sunshowers

### What it provides

- `OwoColorize` extension trait on all types: `"text".red()`, `4.on_cyan()`
- `Style` objects for reuse
- `if_supports_color(Stream::Stdout, |t| t.bright_blue())` — conditional styling
- Zero allocations by default (uses compile-time generics for colors)
- Supports `NO_COLOR` / `FORCE_COLOR` via optional `supports-colors` feature

### Pros for cipher

- **Zero allocation in hot paths** — color wrappers are transparent wrapper types with Display impls, no String allocation.
- **Compile-time color dispatch** — generics over color types mean no runtime branching.
- **Clean inline API** — `"text".red()` reads nicely in format strings.
- **`supports-colors` feature** handles TTY detection and `NO_COLOR`.

### Cons

- **Not from the `console-rs` family** — separate ecosystem from dialoguer (already a dep).
- Duplicates console's styling scope — if you add console, owo-colors is redundant.
- `supports-colors` feature pulls in a few extra deps to detect terminal support.
- API is method-call based, which doesn't compose as cleanly with `console::style()` patterns.

### NO_COLOR support

Via `supports-colors` feature (optional). Respects `NO_COLOR`, `FORCE_COLOR`, and `TERM` detection. Can also be overridden with `set_override()`.

---

## 4. yansi

**crates.io**: https://crates.io/crates/yansi  
**docs**: https://docs.rs/yansi/latest/yansi/  
**source**: https://github.com/SergioBenitez/yansi  
**version**: 1.0.1 (Jun 2026)  
**maintainers**: SergioBenitez (Rocket creator)

### What it provides

- `Paint` trait on all types: `"text".red().bold()`
- `Style` with `const` constructors (store styles in `static`)
- Built-in conditions: `Condition::STDERR_IS_TTY`, `Condition::NO_COLOR`, etc.
- Quirks: masking (hide values when disabled), wrapping (preserve outer style), lingering (don't reset)
- Hyperlinks (experimental)
- Zero dependencies by default, `no_std` compatible

### Pros for cipher

- **Zero-default-dependency** footprint when `default-features = false`.
- **`Condition::NO_COLOR`** is built-in and explicit.
- **Masking** is interesting — could use for the emoji characters and hide them when colors are off.
- **Windows 10+ support** via WinAPI query, works for ~96% of Windows machines.

### Cons

- **Not from the `console-rs` family** — standalone ecosystem.
- Manual wiring needed for global enable/disable with `--quiet` / `--verbose` flags.
- "Quirks" (masking, lingering) are clever but add conceptual surface area that may not be needed.
- Less widely adopted than console/termcolor for CLI styling.
- The `Condition::NO_COLOR` is a separate feature gate (`detect-env`).

### NO_COLOR support

Via `Condition::NO_COLOR` (requires `detect-env` feature). Also `yansi::disable()` for explicit global control.

---

## 5. termcolor

**crates.io**: https://crates.io/crates/termcolor  
**docs**: https://docs.rs/termcolor/latest/termcolor/  
**source**: https://github.com/BurntSushi/termcolor  
**version**: 1.4.1 (Jun 2026)  
**maintainers**: BurntSushi (ripgrep, regex, aho-corasick)

### What it provides

- `WriteColor` trait extending `io::Write` with `set_color()` / `reset()`
- `StandardStream` / `StandardStreamLock` — color-aware stdout/stderr wrappers
- `BufferWriter` + `Buffer` — thread-safe buffered colored output
- `ColorSpec` — foreground, background, bold, underline, etc.
- `ColorChoice` — `Always`, `Auto`, `Never` for `--color` flag
- `Ansi` and `NoColor` wrappers for arbitrary writers
- Hyperlinks (experimental)

### Pros for cipher

- **Extremely mature** — used by ripgrep, cargo, and many other tools. Battle-tested.
- **Windows console API support** — best Windows compatibility of any option.
- **`BufferWriter` design** great for parallel output (if parallelism is added later).
- **`ColorChoice` enum** maps directly to a `--color` flag.
- **Zero extra deps** (only `winapi-util` on Windows).

### Cons

- **Write-oriented API** — more verbose than style-based crates. Requires mutable writer access.
- **No `NO_COLOR` auto-detection** — must be wired manually (unusual for a modern crate).
- **No inline styling** — you write `set_color()` then `write!()`, not `format!("{}", "text".red())`.
- **Does not compose with `indicatif`** — indicatif uses `console` internally, so `termcolor` is an extra styling stack.

### NO_COLOR support

None built-in. Must check `NO_COLOR` yourself and pass `ColorChoice::Never`.

---

## Comparison Matrix

| Criterion | console | indicatif | owo-colors | yansi | termcolor |
|---|---|---|---|---|---|
| **Ecosystem fit** | Already have dialoguer | Pairs with console | Standalone | Standalone | Standalone |
| **NO_COLOR auto** | Yes | Yes (via console) | Yes (feature) | Yes (feature) | No |
| **Windows support** | Good | Good (Win10+) | Good | Good (Win10+) | Excellent |
| **Compile time** | ~10 deps | ~15 deps (+ console) | ~0 deps (bare) | 0 deps (bare) | 0 deps (bare) |
| **Styling API** | `style().cyan()` | Template strings | `.red()` trait | `.red()` trait | `set_color()` |
| **Progress bars** | No | Yes | No | No | No |
| **Term features** | Yes (cursor, size) | No | No | No | No |
| **Maintainer** | mitsuhiko/djc | mitsuhiko/djc | jam1garner | SergioBenitez | BurntSushi |
| **Downloads** | Very high | Very high | High | Moderate | Very high |

---

## Clap `Styles` Interaction

Clap v4+ has `clap::builder::Styles` with its own `Style` type (not the same as any crate's Style). It controls help/error text colors only. It is **independent** of whichever crate you choose for application output:

```rust
// Clap's styling — affects --help output only
let styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default())
    .usage(AnsiColor::Green.on_default())
    .literal(AnsiColor::Green.on_default());
```

- Clap's `Styles` uses `AnsiColor` from `clap::builder::styling`, not from `console` or `yansi`.
- No conflict — they operate on different output (clap renders help text; your code renders status/progress).
- If you want consistent color schemes, you'd define your app's palette once (e.g. a `ColorTheme` struct) and apply it both to clap's `Styles` and your output functions.

---

## Recommendation

**Use `console` as the styling foundation, and add `indicatif` for progress bar needs.**

### Rationale

1. **Dialoguer is already in Cargo.toml.** `console` is the same `console-rs` ecosystem. Adding it means sharing maintainers, issue trackers, and release cadence. Consistent dependency story.

2. **console + indicatif are designed as a pair.** indicatif's `ProgressStyle` templates use console's color format strings. They share the same TTY/NO_COLOR detection.

3. **console replaces the hand-rolled ANSI directly.** Every function in `output.rs` maps cleanly:
   - `green(s)` → `style(s).green().to_string()`
   - `bold(s)` → `style(s).bold().to_string()`
   - `no_color()` → already handled by `colors_enabled()`

4. **indicatif addresses a real need.** The `translate` command already renders per-chapter progress manually. A `ProgressBar` with a spinner during LLM calls + bounded bar for chapter iteration would be a clear UX improvement.

5. **Avoid adding a separate color crate (owo-colors, yansi, termcolor).** They would duplicate console's scope and add a second styling stack. termcolor's write-oriented API is a poor fit for cipher's `impl Display` parameter style. owo-colors and yansi provide inline styling but miss progress bars and terminal features.

### What about compile-time impact?

- `console` adds ~6 deps (libc, windows-sys, unicode-width, encode_unicode, etc.) — acceptable for a CLI.
- `indicatif` adds ~6 more (portable-atomic, unit-prefix, etc.) — also acceptable.
- Combined, this is a standard "output stack" for Rust CLIs (e.g., `cargo` uses similar crates).

### What if we only need styling and no progress bars?

Even then, `console` is the right choice because it's already the ecosystem match. If the project never adds progress bars, you still get the styling replacement without bringing in indicatif.

---

## Sketch: New Output Module with console

```rust
// src/output.rs — using console as styling foundation

use console::Style;
use std::sync::atomic::{AtomicBool, Ordering};

// Global flags (unchanged)
static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);

// Pre-built styles
mod style {
    use console::Style;

    pub fn dim() -> Style {
        Style::new().dim()
    }
    pub fn bold() -> Style {
        Style::new().bold()
    }
    pub fn green() -> Style {
        Style::new().green()
    }
    pub fn red() -> Style {
        Style::new().red()
    }
    pub fn yellow() -> Style {
        Style::new().yellow()
    }
}

// Styled markers
fn ok() -> String {
    if console::colors_enabled_stderr() {
        style::green().apply_to("\u{2713}").to_string()
    } else {
        "\u{2713}".to_string()
    }
}

fn fail_mark() -> String {
    if console::colors_enabled_stderr() {
        style::red().apply_to("\u{2717}").to_string()
    } else {
        "\u{2717}".to_string()
    }
}

// Console respects NO_COLOR automatically via colors_enabled_stderr().
// If we need a --color flag override:
//   console::set_colors_enabled(false);   // or .set_colors_enabled_stderr()

// stdout functions (pipe-friendly, minimal ANSI) — unchanged pattern
pub fn detail(message: impl std::fmt::Display) { /* same as today */ }

// stderr functions — use pre-built styles
pub fn stderr_detail(message: impl std::fmt::Display) {
    eprintln!("{} {}", style::dim().apply_to("-"), message);
}

pub fn chapter_line_ok(name: impl Display, time: impl Display, tokens: impl Display, tags: &[String]) {
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!("  {}", tags.join(" "))
    };
    eprintln!(
        "\r\x1b[K  {}  {}  {}  {}{}",
        ok(),
        name,
        style::dim().apply_to(time),
        style::dim().apply_to(tokens),
        tag_str
    );
}

// etc.
```

When progress bars are needed, add:
```rust
use indicatif::{ProgressBar, ProgressStyle, ProgressDrawTarget};

let bar = ProgressBar::new(total as u64);
bar.set_draw_target(ProgressDrawTarget::stderr());
bar.set_style(
    ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
        .unwrap()
);
```

### Exposed public API (unchanged)

The existing public functions in `output.rs` would keep their signatures. All the callers in `src/translate/cmd.rs`, `src/translate/orchestrate.rs`, `src/ui/status.rs`, `src/config/cli.rs`, etc. would remain unchanged. The implementation underneath swaps ANSI strings for `console::Style` applications.

---

## Cargo.toml changes

```toml
[dependencies]
# Add:
console = "0.16"
indicatif = "0.18"   # optional — only when progress bars are needed
```

Remove nothing (hand-rolled ANSI is in `src/output.rs`, not a crate).

---

## Primary Sources

- console: https://docs.rs/console/latest/console/ — `fn colors_enabled()`, `fn colors_enabled_stderr()`
- indicatif: https://docs.rs/indicatif/latest/indicatif/ — `ProgressBar`, `ProgressStyle`
- console-rs org: https://github.com/console-rs (console, indicatif, dialoguer)
- owo-colors: https://docs.rs/owo-colors/latest/owo_colors/ — `OwoColorize` trait, `supports-colors` feature
- yansi: https://docs.rs/yansi/latest/yansi/ — `Paint` trait, `Condition::NO_COLOR`
- termcolor: https://docs.rs/termcolor/latest/termcolor/ — `WriteColor`, `ColorChoice`
- Clap Styles: https://docs.rs/clap/latest/clap/builder/struct.Styles.html
- NO_COLOR spec: https://no-color.org/
