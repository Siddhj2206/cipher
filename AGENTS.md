# AGENTS.md - Guidelines for cipher Contributors

## Project Overview
`cipher` is a Rust CLI tool for translating book chapters using LLMs via rig.rs. It follows a modular architecture with providers, profiles, and glossary management.

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
- Keep functions focused and under ~50 lines
- Use early returns to reduce nesting
- Document public functions with `///` doc comments
- Use `async fn` for I/O operations (translation, file ops)

### Module Structure
```
src/
  main.rs           # CLI entry point with clap
  config/           # Global config, profiles, providers
  book/             # Book layout, paths, init
  glossary/         # Glossary loading, merging, smart selection
  translate/        # Translation orchestration, providers
  state/            # Run state tracking
  validate/         # Output validation
```

### Provider Pattern
- One provider per file in `src/translate/providers/`
- Implement `Provider` trait for each backend
- Handle API errors with user-friendly messages (401, 404, 429, etc.)

### Dependencies
- Add via `cargo add <crate>`, never edit Cargo.toml directly
- Prefer mature, well-maintained crates
- Minimize dependencies where standard library suffices

### Testing
- Unit tests in same file under `#[cfg(test)] mod tests`
- Test names should describe behavior: `test_select_terms_fallback_when_too_few_matches`
- Use descriptive assertions with context

### CLI Output Style (Follow Book-Translator-Go)
- Use sentence case, no bracket tags like `[SKIP]`
- Prefix sub-messages with `- `
- Print profile info before run:
  ```
  Using profile <name>
  - Provider: <provider>
  - Model: <model>
  ```
- Show glossary usage: `- Using smart glossary: N/M terms`

### Git
- Do not commit unless explicitly asked
- Do not push to remote
- Never commit secrets or API keys

### Versioning
- Follow [Semantic Versioning](https://semver.org/): `MAJOR.MINOR.PATCH`
- Bump `PATCH` for bug fixes and minor internal changes
- Bump `MINOR` for new features, CLI flags, or backward-compatible behavior changes
- Bump `MAJOR` for breaking changes (config format, CLI interface, glossary schema)
- Update the version in `Cargo.toml` when making a release-worthy change
- Keep the version in sync between `Cargo.toml` and any other references

### Configuration
- Global config: `~/.config/cipher/config.json` (XDG)
- Per-book config: `config.json` in book directory
- Use `ProjectDirs` from `directories` crate for paths
- Validate configs in `doctor` command

### Glossary Conventions
- JSON array format
- Terms have `term`, optional `og_term`, `definition`, optional `notes`
- No `id` field, no `status` field
- Deterministic ordering by dedupe key
- Smart selection: fallback to full glossary when < 5 matches
