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
3. sends the chapter, glossary, and style guide to the configured model
4. validates the returned translation
5. attempts one repair pass if validation fails
6. writes accepted output atomically
7. merges any newly discovered glossary terms
8. saves run and chapter state under `.cipher/`

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

### 2. Create or import a book

From scratch:

```bash
cipher init my-book
```

From an EPUB:

```bash
cipher import my-book.epub
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
  config.json        # Book configuration
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
cipher translate my-book --profile fast
cipher translate my-book --overwrite
cipher translate my-book --fail-fast
cipher translate my-book --rerun
cipher translate my-book --rerun-affected-glossary
cipher translate my-book --rerun-affected-chapters
```

Current translate flags:

- `--profile <name>`: override the book/global profile for this run
- `--overwrite`: retranslate even when output already exists
- `--fail-fast`: stop on the first failed chapter
- `--rerun`: retranslate chapters whose tracked source or glossary-relevant inputs changed
- `--rerun-affected-glossary`: retranslate chapters whose glossary-relevant inputs changed since the tracked baseline
- `--rerun-affected-chapters`: retranslate chapters whose raw markdown changed since the last tracked chapter state

Default behavior:

- chapters are discovered from `raw/`
- chapter order is stable and numeric-first
- existing outputs are skipped unless overwrite or rerun logic applies
- output is validated before being accepted
- failed API calls retry with exponential backoff
- validation failures get one repair attempt
- accepted outputs are written atomically
- overwriting creates timestamped backups in `.cipher/backups/`

### `cipher status <book_dir>`

Show the latest recorded run state for a book.

```bash
cipher status my-book
```

Status currently includes:

- profile, provider, and model used for the last run
- start/update/finish timestamps
- chapter counts for translated, skipped, failed, and pending
- a list of failed chapters with short error previews

### `cipher init <book_dir>`

Create a new book scaffold.

```bash
cipher init my-book
cipher init my-book --profile myprofile
cipher init my-book --from other-book
cipher init my-book --import-glossary terms.json
```

### `cipher import <epub_path>`

Import an EPUB into a new book directory.

```bash
cipher import novel.epub
cipher import novel.epub --force
```

Current import behavior:

- creates a book directory alongside the EPUB
- extracts chapters into `raw/`
- converts HTML to markdown
- skips very small/empty chapters
- initializes the standard book scaffold

### `cipher glossary <subcommand> <book_dir>`

Manage the canonical glossary.

```bash
cipher glossary list my-book
cipher glossary import my-book new-terms.json
cipher glossary export my-book backup.json
```

### `cipher profile <subcommand>`

Manage profiles.

```bash
cipher profile new
cipher profile list
cipher profile show myprofile
cipher profile set-default myprofile
cipher profile test myprofile
```

### `cipher doctor [book_dir]`

Run diagnostics.

```bash
cipher doctor
cipher doctor my-book
```

Without a book directory, `doctor` checks global configuration.
With a book directory, it checks book layout and effective profile resolution.

## Configuration

## Global config

Global configuration is stored using XDG config directories. On Linux, the current path resolves to:

```text
~/.config/cipher/cipher/config.json
```

It contains:

- providers
- API keys
- profiles
- default profile

The current implementation stores API keys as plain text in this config. Improving secret storage is planned.

## Book config

Each book contains a portable `config.json`:

```json
{
  "profile": "",
  "raw_dir": "raw",
  "out_dir": "tl",
  "glossary_path": "glossary.json",
  "style_path": "style.md",
  "glossary_injection": "smart"
}
```

Profile resolution order:

1. `--profile`
2. book `config.json`
3. global default profile

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

## Glossary injection modes

Book config supports two modes:

- `smart` - select relevant glossary terms for the current chapter
- `full` - inject the full glossary every time

`smart` is the default.

Current smart-mode behavior:

- matches glossary terms against the chapter text using deterministic selection logic
- always includes terms with empty `og_term`
- falls back to full glossary when too few matches are found

## Style guide

If present, `style.md` is injected into every translation request.

Use it for:

- tone
- narration style
- dialogue conventions
- recurring translation preferences
- rules that are broader than glossary terms

## Validation and repair

Before output is accepted, `cipher` validates it.

Current checks include:

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

## Reruns and state

`cipher` stores internal state under `.cipher/` so runs are resumable and future rerun decisions can be more informed.

Current tracked state includes:

- run metadata
- per-chapter result state
- glossary-state snapshots
- chapter glossary usage
- exported glossary term fingerprints

### Glossary-aware reruns

`--rerun-affected-glossary` uses tracked state to detect when a chapter should be rerun because glossary-relevant inputs changed.

Current support includes:

- changed glossary fingerprints for previously selected terms
- changed fingerprints for exported terms
- smart-selection changes when newly relevant or removed terms alter the effective injected set
- forward-only incremental replanning for remaining chapters when new glossary terms are discovered mid-run

This rerun model is still evolving, but it is already much better than a pure startup-only plan.

### Overwrite vs rerun

These are different tools:

- `--overwrite` means redo outputs regardless of tracked equivalence
- `--rerun` means rerun chapters whose tracked source or glossary inputs changed
- `--rerun-affected-glossary` means rerun chapters whose tracked glossary inputs became stale
- `--rerun-affected-chapters` means rerun chapters whose tracked raw source became stale

## Safety guarantees

Current file-safety behavior:

- accepted outputs are written atomically
- overwriting creates backups in `.cipher/backups/`
- glossary and state are saved incrementally during runs

This keeps runs resumable and reduces the chance of corrupted outputs after interruptions.

## Current limitations

A few areas are intentionally still evolving:

- API keys are not yet stored in a proper secret store
- dry-run rerun preview is not implemented yet
- status output does not yet expose all tracked-vs-approximate rerun details
- repair and glossary extraction are still more coupled than they should be long-term

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
