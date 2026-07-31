# cipher

A CLI tool for translating book chapters with LLMs.

`cipher` is built for long-form translation workflows where consistency matters across many chapters. It combines profile-based provider configuration, glossary injection, validation, repair retries, and checkpointed run state so you can translate iteratively instead of treating every run as a one-shot batch job.

It is especially suited for serialized web novels and other chapter-based books, but the workflow also fits any markdown-based long-form source text.

## What cipher does

A `cipher` book project is a directory containing:

- raw source chapters
- translated output
- a canonical glossary
- a style guide
- internal state used for resumability and rerun planning

For each chapter, `cipher`:

1. loads the raw markdown
2. selects glossary terms using `smart` or `full` injection
3. sends the chapter, selected glossary, and style guide to the configured model for translation
4. validates the returned translation
5. attempts one repair pass if validation fails
6. sends accepted output through a separate glossary extraction request
7. writes accepted output atomically
8. merges any newly discovered glossary terms
9. saves run and chapter state under `.cipher/`

This makes later runs safer and more explainable, especially when the glossary grows over time.

## Installation

```bash
cargo install --git https://www.github.com/siddhj2206/cipher.git
```

## Quick start

### 1. Create a profile

`cipher` uses profiles to choose a provider and model.

```bash
cipher profile new
```

This interactive flow lets you:

- create or reuse a provider
- enter or reuse an API key
- choose a model
- optionally set the profile as default

Built-in providers currently include `gemini` and `openai`, and you can also add custom OpenAI-compatible providers.

You can inspect profiles with:

```bash
cipher profile list
cipher profile show myprofile
cipher profile test myprofile
```

### 2. Create a book

From scratch:

```bash
cipher init my-book
```

You can also initialize a book with a profile or imported glossary:

```bash
cipher init my-book --profile myprofile
cipher init my-book --from other-book
cipher init my-book --import-glossary terms.json
```

### 3. Add chapters

Place source markdown files in `raw/`:

```text
my-book/
  raw/
    001.md
    002.md
    003.md
```

### 4. Translate

```bash
cipher translate my-book
```

Translated chapters are written to `tl/`.

### 5. Check status

```bash
cipher status my-book
```

This shows the latest recorded run metadata and chapter summary.

## Book project structure

```text
my-book/
  cipher.toml        # Book configuration
  glossary.json      # Canonical glossary
  style.md           # Style guide injected into prompts
  raw/               # Source chapters
    001.md
    002.md
    ...
  tl/                # Translated output
    001.md
    002.md
    ...
  .cipher/           # Internal run state, chapter state, glossary state, backups
```

## Core commands

### `cipher translate [book_dir]`

Translate a book. If `book_dir` is omitted, the current directory is used.

```bash
cipher translate
cipher translate my-book
cipher translate my-book -p fast
cipher translate my-book -p best --repair-profile fast --glossary-profile cheap
cipher translate my-book -o
cipher translate my-book -d
cipher translate my-book --fail-fast
cipher translate my-book --rerun
cipher translate my-book --rerun=glossary
cipher translate my-book --rerun=source
cipher translate my-book -q
cipher translate my-book -v
```

Current translate flags:

- `-p, --profile <name>`: override the book/global profile for this run
- `--repair-profile <name>`: use a different profile for repair requests
- `--glossary-profile <name>`: use a different profile for glossary extraction requests
- `-o, --overwrite`: retranslate even when output already exists
- `-d, --dry-run`: preview translate/rerun/skip decisions without calling providers or writing state
- `--fail-fast`: stop on the first failed chapter
- `--rerun[=MODE]`: retranslate chapters affected by tracked changes. Modes: `all` (glossary + source, default), `glossary`, or `source`
- `-q, --quiet`: suppress non-essential output (progress bar and detail lines)
- `-v, --verbose`: show detailed per-chapter progress and diagnostics: provider call details (model, endpoint), per-call timing, retry attempts with reasons, validation failures, repair decisions, glossary and usage info

Default behavior:

- chapters are discovered from `raw/`
- chapter order is stable and numeric-first
- existing outputs are skipped unless overwrite or rerun logic applies
- output is validated before being accepted
- failed API calls retry with exponential backoff
- validation failures get one repair attempt
- accepted outputs are written atomically
- overwriting creates timestamped backups in `.cipher/backups/`
- a progress bar shows translation progress (hidden with `--quiet`)

### `cipher status <book_dir>`

Show the latest recorded run state for a book.

```bash
cipher status my-book
cipher status --json
```

Status currently includes:

- profile, provider, and model used for the last run
- start/update/finish timestamps
- chapter counts for translated, skipped, failed, and pending
- tracking counts for smart-tracked chapters, smart fallback-to-full chapters, legacy primary full-tracked chapters, approximate legacy fallback, exported-term tracking, and source hashes
- a list of failed chapters with short error previews

### `cipher init <book_dir>`

Create a new book scaffold.

```bash
cipher init my-book
cipher init my-book -p myprofile
cipher init my-book --from other-book
cipher init my-book --import-glossary terms.json
```

### `cipher glossary <subcommand> <book_dir>`

Manage the canonical glossary.

```bash
cipher glossary list my-book
cipher glossary list my-book --json
cipher glossary import my-book --file new-terms.json
cipher glossary export my-book --output backup.json
```

### `cipher profile <subcommand>`

Manage profiles.

```bash
cipher profile new
cipher profile new --name my-profile --provider gemini --model gemini-2.5-flash --api-key-file key.txt
cipher profile new --name my-profile --no-input
cipher profile list
cipher profile list --json
cipher profile show myprofile
cipher profile show myprofile --json
cipher profile set-default myprofile
cipher profile test myprofile
```

Non-interactive profile creation flags (all optional; omit for interactive prompts):

- `--name <name>`: profile name (skips interactive prompt)
- `--provider <name>`: provider name (skips interactive selection)
- `--model <name>`: model name (skips interactive prompt)
- `--key-label <label>`: key label to assign (skips interactive key selection)
- `--api-key-file <path>`: read API key from file (skips key input)
- `--set-default`: set as default profile
- `--no-input`: fail if required flags are missing (for scripting)

### `cipher doctor [book_dir]`

Run diagnostics.

```bash
cipher doctor
cipher doctor my-book
```

Without a book directory, `doctor` checks global configuration.
With a book directory, it checks book layout and effective profile resolution.

## Errors and exit codes

Errors carry a stable code and exit status so scripts can react to failure classes:

| Code | Meaning | Exit |
| ---- | ------- | ---- |
| E001 | Global/book config read or parse failed | 2 |
| E002 | I/O failure | 3 |
| E003 | Profile not found | 2 |
| E004 | Glossary JSON failure | 1 |
| E005 | Provider (API key, client, request) | 4 |
| E006 | Invalid input or usage | 1 |
| E007 | Corrupt `.cipher` state | 70 |
| E099 | Unclassified | 70 |

Errors print as `[E00N] message` (validation errors print as the bare message) with an optional `suggestion:` line. See `docs/adr/0003-structured-error-system.md` for details.

## Configuration

## Global config

Global configuration is stored using XDG config directories. On Linux, the current path resolves to:

```text
~/.config/cipher/config.toml
```

It contains:

- providers
- profiles
- default profile

Provider API keys are nested under each provider.

The current implementation stores API keys as plain text in this config. Improving secret storage is planned.

## Book config

Each book contains a portable `cipher.toml`:

```toml
raw_dir = "raw"
out_dir = "tl"
glossary_path = "glossary.json"
style_path = "style.md"
glossary_injection = "smart"
# Optional profile overrides:
# profile = "best"
# repair_profile = "fast"
# glossary_profile = "cheap"

[output.render]
template = """
# Chapter {chapter_number}: {chapter_title}

{content}
"""
```

Translation profile resolution order:

1. `--profile`
2. book `cipher.toml`
3. global default profile

Repair and glossary extraction profiles default to the translation profile. They can be overridden with `--repair-profile` / `--glossary-profile` or persistent `repair_profile` / `glossary_profile` values in `cipher.toml`.

## Glossary

The glossary is a JSON array of terms:

```json
[
  {
    "term": "Starship",
    "og_term": "星空舰",
    "definition": "The main character's vessel"
  },
  {
    "term": "River Map",
    "og_term": "山河图",
    "definition": "An ancient artifact containing a sealed dimension",
    "notes": "Sometimes referred to as 'The Map' in casual dialogue"
  }
]
```

Fields:

- `term`: translated term to enforce
- `og_term`: original-language term used for matching
- `definition`: explanation/context
- `notes`: optional extra guidance

Glossary behavior:

- canonical source of truth is `glossary.json`
- merges are deterministic
- duplicate terms are skipped during merge/import
- new terms returned by successful chapters are appended after dedupe

## Glossary injection behavior

`smart` is the canonical/default mode. Legacy `full` config values are treated as `smart`.

Smart-mode behavior:

- matches glossary terms against the chapter text using deterministic selection logic
- always includes terms with empty `og_term`
- falls back to full glossary when too few matches are found
- legacy primary full-tracking state is migrated opportunistically when a successful smart-era run proves it is equivalent to smart fallback tracking

## Style guide

If present, `style.md` is injected into every translation request. Put book-specific formatting guidance there, including how to populate structured output fields for a custom render template.

Use it for:

- tone
- narration style
- dialogue conventions
- recurring translation preferences
- rules that are broader than glossary terms

## Validation and repair

Before output is accepted, `cipher` validates it.

Validation checks include:

- non-empty output
- heading presence/shape
- balanced code fences
- JSON/schema leakage detection
- rejection of raw structured response artifacts leaking into prose

If validation fails:

1. the failure is recorded
2. one repair request is attempted using the original text, failed translation, and validation errors
3. the repaired output is validated again
4. if it still fails, the chapter is marked failed

Glossary extraction now runs only after a translation has passed validation.

- translation requests return translated markdown only
- repair requests return corrected markdown only
- glossary extraction runs as a separate follow-up call against accepted markdown and existing glossary terms
- glossary extraction failure does not invalidate an otherwise accepted chapter; it only skips adding new terms for that chapter

## Reruns and state

`cipher` stores internal state under `.cipher/` so runs are resumable and future rerun decisions can be more informed.

Tracked state includes:

- run metadata
- per-chapter result state
- glossary-state snapshots
- chapter glossary usage
- exported glossary term fingerprints

### Glossary-aware reruns

`--rerun=glossary` uses tracked state to detect when a chapter should be rerun because glossary-relevant inputs changed.

Rerun detection compares saved chapter glossary state against the current expected glossary usage, including changed term fingerprints, smart-selection changes when newly relevant or removed terms alter the effective injected set, and fallback-to-full behavior changes. Forward-only incremental replanning for remaining chapters runs when new glossary terms are discovered mid-run.

### Overwrite vs rerun

These are different tools:

- `--overwrite` means redo outputs regardless of tracked equivalence
- `--rerun` (or `--rerun=all`) means rerun chapters whose tracked source or glossary inputs changed
- `--rerun=glossary` means rerun chapters whose tracked glossary inputs became stale
- `--rerun=source` means rerun chapters whose tracked raw source became stale

## Safety guarantees

Current file-safety behavior:

- accepted outputs are written atomically
- overwriting creates backups in `.cipher/backups/`
- glossary and state are saved incrementally during runs

This keeps runs resumable and reduces the chance of corrupted outputs after interruptions.

## Limitations

- API keys are stored as plain text in global config; a proper secret store is planned
- dry-run preview reports planned actions from the existing rerun rules
- status output does not expose all tracked-vs-approximate rerun details

## Development

Useful commands while working on the project:

```bash
cargo build
cargo check
cargo fmt
cargo test
cargo run -- translate ./test-book
cargo run -- status ./test-book
cargo run -- doctor ./test-book
```
