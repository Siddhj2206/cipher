# Structured Error Handling Research

## Current State

cipher uses `anyhow` pervasively — every public function returns `anyhow::Result<T>`, errors are
created with `anyhow::anyhow!` / `anyhow::bail!`, and context is added via `with_context`.
The single exit path is `exit_with_error` in `src/lib.rs:389` which prints the error in red to
stderr and exits with code 1. Some display-oriented commands (`status`, `glossary list`,
`profile list/show`) already accept `--json` for machine-readable output via `serde_json`.

## Approach Comparison

### thiserror

[thiserror](https://docs.rs/thiserror/latest/thiserror/) — dtolnay, 2.0.19, MIT/Apache-2.0

A derive macro for `std::error::Error`. You write an enum with `#[error("...")]` on each
variant and the crate generates `Display`, `Error::source()`, and optionally `From` impls.
Deliberately absent from the public API — expands to hand-written impls at compile time.
Zero runtime cost.

**Strengths:**
- De facto standard (42M+ downloads); every Rust engineer recognises it
- Compile-time only — no runtime dependencies beyond `std`
- Trivially wraps `anyhow::Error` via `#[error(transparent)]`
- Plays well with `serde` — just add `#[derive(Serialize)]` on the enum
- Minimal: one attribute per variant, no macros, no traits to import
- Source: <https://github.com/dtolnay/thiserror>

**Weaknesses:**
- No built-in backtrace capture (use `std::backtrace::Backtrace` field on nightly)
- No context selectors — you write wrapping functions or `From` impls manually
- Will not compile on Rust < 1.56 (project is on edition 2024, not an issue)

### snafu

[snafu](https://docs.rs/snafu/latest/snafu/) — shepmaster, 0.9.1, MIT/Apache-2.0

Generates error types and *context selectors* — helper structs used with
`.context(SomeContextSnafu { field })` to wrap underlying errors with context.
Includes `Whatever` for stringly-typed fallback, built-in backtrace support,
and `#[snafu::report]` for pretty-printing in `main`.

**Strengths:**
- Context selectors make wrapping ergonomic (`.context(ConfigFileSnafu { path })`)
- Backtrace capture works on stable Rust
- `snafu::report` attribute gives decent error formatting for free
- `#[snafu(whatever)]` variant gives an escape hatch (like anyhow inside an enum)

**Weaknesses:**
- Heavier conceptual surface: context selectors, `IntoError`, `GenerateImplicitData`, etc.
- Context selectors pollute the module namespace (one extra type per variant)
- Less widely known than thiserror; contributors need to learn SNAFU patterns
- The generated code is harder to inspect/debug
- Source: <https://github.com/shepmaster/snafu>

### Custom error enum without derive

Writing `std::error::Error` by hand for a multi-variant enum (Display, Debug, source,
possibly From impls) is roughly 60–100 lines of boilerplate per variant group.
Feasible only if the error type is 2–3 variants and never grows.

**Verdict:** Not competitive for cipher's scale. The crate already has 37 source files across
7 modules — the error type will have at least 10+ variants. thiserror removes the boilerplate
with no downside.

### Recommendation for cipher

**Use thiserror.** The criteria:

1. **Zero-cost.** thiserror disappears at compile time; binary size and stack traces are
   identical to hand-written impls. snafu adds runtime context-selector overhead.

2. **anyhow coexistence.** `#[error(transparent)]` lets us keep anyhow in intermediate layers
   (providers, IO, parsing) while the typed enum lives at the CLI boundary. Migration can be
   incremental — typed variants replace `anyhow::bail!` one function at a time.

3. **serde integration.** `#[derive(Serialize)]` on the same enum gives JSON output with no
   extra mapping layer.

4. **CLI-appropriate.** cipher is not a library; the typed enum only needs to be stable at the
   user-facing boundary. thiserror's simplicity means fewer surprises when adding variants.

5. **Ecosystem.** The project already uses dtolnay crates (anyhow). Adding thiserror is
   consistent with existing dependencies.

## Error Code Scheme

Error codes are stable strings (not numeric) so they never collide with OS exit codes and can
be grouped mnemonically.

```
E001 — Configuration
E002 — I/O
E003 — Profile
E004 — Glossary
E005 — Translation / Provider
E006 — Validation
E007 — State
```

Each code maps to one enum variant. Codes are documented in a table near the error definition
so they function as a public API contract — they will not be removed or repurposed across
releases (only deprecated + replaced).

### Exit Code Mapping

| Condition              | Exit code |
|------------------------|-----------|
| Success                | 0         |
| User error (bad input) | 1         |
| Config / profile error | 2         |
| I/O error              | 3         |
| Provider / LLM error   | 4         |
| Internal bug           | 70+       |

The mapping lives in a single method `fn exit_code(&self) -> i32` on the error enum.
This is independent of clap's exit code (2 for parse errors) — clap handles its own errors
before our error type is ever constructed.

## Suggested Error Type Hierarchy

```rust
#[derive(Debug, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum Error {
    /// E001
    #[error("config error: {0}")]
    Config(String),

    /// E002
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// E003
    #[error("profile '{name}' not found")]
    ProfileNotFound { name: String },

    /// E004
    #[error("glossary error: {0}")]
    Glossary(String),

    /// E005
    #[error("{kind} request failed: {detail}")]
    Provider { kind: String, detail: String },

    /// E006
    #[error("validation error")]
    Validation { field: String, expected: String, found: String },

    /// E007
    #[error("state error: {0}")]
    State(String),

    /// Catch-all for untyped anyhow errors during migration.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

The `Other` variant is the migration bridge — any function still returning `anyhow::Result<T>`
gets converted via `.map_err(Error::from)` at the boundary. Once migration is complete `Other`
can be removed.

## Integration with stderr Display and JSON Output

### Human display (stderr)

Replace the current `exit_with_error` from `src/lib.rs` with:

```rust
pub fn exit_with_error(err: Error) -> ! {
    if global_json_flag() {
        eprintln!("{}", serde_json::to_string(&err).unwrap());
    } else {
        output::stderr_error(format_args!("[{}] {}", err.code(), err));
        if let Some(suggestion) = err.suggestion() {
            output::stderr_detail(format_args!("did you mean: {suggestion}"));
        }
    }
    std::process::exit(err.exit_code())
}
```

The `code()` and `suggestion()` methods dispatch on the enum variant. The `global_json_flag`
is a new atomic alongside `QUIET` / `VERBOSE` in `output.rs`.

### JSON output

When `--json` is active at the top level, every error is serialised as:

```json
{
  "error": {
    "code": "E003",
    "message": "profile 'og' not found",
    "suggestion": "did you mean: 'org'?",
    "exit_code": 2
  }
}
```

This is structurally identical to the JSON output of successful commands (`status`, `list`,
etc.) — they all produce `{"kind": "...", "data": {...}}` under the same `serde(tag = "kind")`
scheme so consumers parse a uniform envelope.

## Coexistence with anyhow During Migration

**Phase 1 — Define the type, add `Other` variant.** Add `thiserror` to `Cargo.toml`. Define
the error enum in a new module `src/error.rs`. The `Other(#[from] anyhow::Error)` variant
lets every existing `anyhow::Result<T>` become `Result<T, Error>` via `?` in calling
functions.

**Phase 2 — Typed boundary in `main`.** Change `run_command` to return
`Result<i32, Error>` instead of `anyhow::Result<i32>`. Update `exit_with_error` to accept
`Error`. Everything still works because `anyhow::Error` converts into `Error::Other`.

**Phase 3 — Replace `anyhow::bail!` with typed variants.** One module at a time, replace
`anyhow::bail!("profile '{}' not found", name)` with
`return Err(Error::ProfileNotFound { name: name.into() })`. The `anyhow` imports in that
module shrink to zero.

**Phase 4 — Remove `Other` variant.** Once every error site is typed, delete
`Error::Other` and stop depending on `anyhow`. (Optional — retaining `Other` costs one
allocation per conversion but keeps the escape hatch.)

Throughout the migration, intermediate layers (providers, state, IO) can continue using
`anyhow::Result` internally; only the boundary functions convert to `Error`.

## Interaction with clap's Error System

clap's `Error` type has its own `ErrorKind` enum, exit code convention (2 for stderr, 0 for
stdout), and `print()` / `exit()` methods. We do **not** try to fit clap errors into our
enum — clap handles its own errors before our code runs:

```rust
// src/main.rs — conceptual
#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => e.exit(),  // clap prints and exits with its own code
    };
    match run_command(cli.command).await {
        Ok(code) => std::process::exit(code),
        Err(e) => exit_with_error(e),
    };
}
```

If a clap parse error needs to be surfaced in JSON mode, catch it before `e.exit()`:

```rust
let cli = match Cli::try_parse() {
    Ok(cli) => cli,
    Err(e) => {
        if json_mode() {
            exit_with_json_error("E001", &e.to_string(), 2);
        } else {
            e.exit();
        }
    }
};
```

## Primary Sources

| Source | URL |
|--------|-----|
| thiserror crate docs | <https://docs.rs/thiserror/latest/thiserror/> |
| thiserror repository | <https://github.com/dtolnay/thiserror> |
| snafu crate docs | <https://docs.rs/snafu/latest/snafu/> |
| snafu repository | <https://github.com/shepmaster/snafu> |
| clap error module docs | <https://docs.rs/clap/latest/clap/error/index.html> |
| clap Error struct | <https://docs.rs/clap/latest/clap/error/struct.Error.html> |
| clap ErrorKind enum | <https://docs.rs/clap/latest/clap/error/enum.ErrorKind.html> |
| anyhow crate docs | <https://docs.rs/anyhow/latest/anyhow/> |
| anyhow + thiserror pattern | <https://docs.rs/thiserror/latest/thiserror/#error-transparent> |
