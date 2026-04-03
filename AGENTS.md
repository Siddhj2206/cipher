# AGENTS.md - Guidelines for cipher Contributors

## Project Overview
`cipher` is a Rust CLI tool for translating book chapters using LLMs via rig.rs. It has moved beyond the initial scaffold stage and now includes:

- book initialization and EPUB import
- profile-based provider configuration
- glossary import/export/list commands
- chapter translation with validation and repair
- smart/full glossary injection
- run, chapter, and glossary state tracking under `.cipher/`
- glossary-aware rerun planning

The codebase follows a modular architecture so providers, glossary logic, translation orchestration, validation, and state can evolve independently.

## Build, Test, Lint Commands

```bash
# Build the project
cargo build

# Build release
cargo build --release

# Run all tests
cargo test

# Run a single test
cargo test test_name_here

# Run tests in a specific module
cargo test module_name::

# Format code (MUST run before committing)
cargo fmt

# Check for errors without building
cargo check

# Run with specific book
cargo run -- translate ./test-book
cargo run -- translate ./test-book --overwrite
cargo run -- translate ./test-book --rerun-affected-glossary
cargo run -- status ./test-book
cargo run -- doctor ./test-book
```

## Code Style Guidelines

### Imports Organization
1. Standard library (`std::`)
2. External crates (alphabetical)
3. Internal modules (`crate::`)
4. Within each group: alphabetical order

```rust
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::GlobalConfig;
```

### Formatting
- **No trailing comments** on struct fields or in function bodies
- Use `cargo fmt` default settings
- Max line length: follow rustfmt defaults
- Always run `cargo fmt` before committing

### Naming Conventions
- `snake_case` for: functions, variables, modules, file names
- `PascalCase` for: structs, enums, traits, types
- `SCREAMING_SNAKE_CASE` for: constants
- `CamelCase` for: acronyms in names (e.g., `ApiKey`, not `APIKey`)

### Types & Structs
- Always derive `Debug` for public types
- Use `#[serde(default, skip_serializing_if = "Option::is_none")]` for optional fields
- Prefer `BTreeMap` over `HashMap` for deterministic ordering
- Use `Option<T>` for truly optional fields; avoid sentinel values

### Error Handling
- Use `anyhow::{Result, Context}` for error propagation
- Provide descriptive context with `.with_context()` including paths/identifiers
- Early returns with `?` operator preferred
- For CLI errors, print to stderr with `eprintln!("Error: {}", e)` then exit 1

### Functions
- Keep functions focused and under ~50 lines when practical
- Use early returns to reduce nesting
- Document public functions with `///` doc comments
- Use `async fn` for I/O-bound work such as translation/provider calls

## Module Structure
Current high-level layout:

```text
src/
  main.rs           # CLI entry point with clap
  book/             # Book layout, init, doctor, path resolution
  config/           # Global config, profiles, providers
  glossary/         # Glossary loading, merging, smart selection
  import/           # EPUB import
  state/            # Run, chapter, and glossary state tracking
  translate/        # Translation orchestration, prompts, providers
  ui/               # Interactive CLI helpers
  validate/         # Output validation
  output.rs         # Shared CLI output helpers
```

Keep concerns separated. New work should reinforce module boundaries instead of pushing unrelated logic into a single layer.

## Translation and Rerun Architecture

### Translation flow
The current chapter flow is:

1. Load raw chapter markdown
2. Select glossary terms (`smart` or `full`)
3. Call the translator/provider
4. Validate the output
5. If needed, send a repair request
6. Write accepted output atomically
7. Merge any new glossary terms
8. Save run/chapter state

### Current rerun model
`cipher` currently supports glossary-aware reruns through `--rerun-affected-glossary`.

Important characteristics:
- smart-mode tracking is the canonical direction
- chapter state records glossary usage and exported term fingerprints
- glossary state records prompt-relevant term fingerprints
- rerun planning can be recomputed mid-run for remaining chapters after glossary additions
- current rerun support is glossary-focused; chapter source hashing is planned but not yet part of the baseline

When changing rerun logic:
- preserve determinism
- preserve explainability
- prefer exact tracked comparisons over approximation
- keep user-visible reason text understandable

## Provider Pattern
- One provider per file in `src/translate/providers/`
- Implement the shared provider abstraction cleanly for each backend
- Handle API errors with user-friendly messages (401, 404, 429, etc.)
- Keep provider-specific response handling out of unrelated modules

## Dependencies
- Add via `cargo add <crate>`, never edit `Cargo.toml` directly
- Prefer mature, well-maintained crates
- Minimize dependencies where standard library suffices

## Testing
- Unit tests in the same file under `#[cfg(test)] mod tests`
- Test names should describe behavior: `test_select_terms_fallback_when_too_few_matches`
- Use descriptive assertions with context
- Add focused tests for rerun/state logic when changing glossary tracking behavior
- Prefer testing root-cause logic directly instead of only testing through high-level command flows

## CLI Output Style
Follow Book-Translator-Go style where practical:
- Use sentence case
- No bracket tags like `[SKIP]`
- Prefix sub-messages with `- `
- Print profile info before a translation run:
  - `Using profile <name>`
  - `- Provider: <provider>`
  - `- Model: <model>`
- Show glossary usage in chapter output
- Favor concise but informative reason text for reruns, repairs, skips, and warnings

## Configuration

### Paths
- Global config is resolved through `ProjectDirs` from the `directories` crate
- Current config path is `~/.config/cipher/cipher/config.json` on Linux/XDG systems
- Per-book config is `config.json` in the book directory
- Validate config/profile issues in `doctor`

### Book config conventions
- Portable only; no secrets in book config
- Default layout uses:
  - `raw/`
  - `tl/`
  - `glossary.json`
  - `style.md`
  - `.cipher/`

### Profiles
- A book references a profile name instead of embedding provider/model details
- CLI profile override should take precedence over book config, which should take precedence over the global default profile

## Glossary Conventions
- JSON array format
- Terms have `term`, optional `og_term`, `definition`, optional `notes`
- No `id` field
- No `status` field
- Deterministic ordering by dedupe key
- Smart selection falls back to full glossary when fewer than 5 terms match
- `smart` is the long-term canonical/default mode

## Validation and Repair
- Validation happens before output is accepted
- Repair is currently used to fix invalid model output after validation failure
- Avoid broadening repair semantics accidentally when making changes
- Long-term direction is to separate translation acceptance from glossary extraction more cleanly

## State Model
State under `.cipher/` currently includes:
- run metadata
- per-chapter state
- glossary tracking state
- backups for overwritten outputs

When changing state:
- keep formats deterministic
- prefer additive evolution where possible
- be explicit about semantics for glossary usage and exported terms
- avoid coupling state changes to unrelated CLI behavior

## Documentation Expectations
- `README.md` is end-user facing
- `PLAN.md` is the high-level architecture/direction document
- `TODO.md` is the detailed backlog
- If behavior changes materially, update the relevant docs in the same change when practical

## Git
- Do not commit unless explicitly asked
- Do not push to remote
- Never commit secrets or API keys

## Versioning
- Follow [Semantic Versioning](https://semver.org/): `MAJOR.MINOR.PATCH`
- Bump `PATCH` for bug fixes and minor internal changes
- Bump `MINOR` for new features, CLI flags, or backward-compatible behavior changes
- Bump `MAJOR` for breaking changes (config format, CLI interface, glossary schema)
- Update the version in `Cargo.toml` when making a release-worthy change
- Keep the version in sync between `Cargo.toml` and any other references