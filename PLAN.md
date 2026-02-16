# cipher

This document describes how the rebuilt tool (renamed from `btranslate` to `cipher`) should work for end users, how books are initialized and managed, and how config/providers/keys/glossary are handled. It intentionally avoids committing to specific languages, libraries, or frameworks.

## Goals

- Make translating a new book feel like a guided, repeatable workflow: init -> translate -> review glossary -> iterate.
- Keep secrets out of book folders by default (books stay portable and safe to commit/share).
- Allow fast switching between providers/models without editing every book.
- Support multiple API keys and automatic key rotation/cooldowns.
- Provide reliable reruns: easy overwrite, backups, and "overwrite only bad outputs".
- Keep glossary consistent: only approved terms influence future translations; new terms are collected but not silently canonized.
- Fail loudly and recover well: validation, clear error messages, resumable runs.

## Non-goals

- Not a full ebook pipeline (no PDF/EPUB parsing, no layout conversion) beyond markdown in/out.
- Not a translation memory system. (We focus on book-level consistency via glossary + style guides.)
- Not an auto-formatter/linter for prose; validation is for structural correctness and safety.

## Terminology

- "Book": a directory containing chapters (markdown) plus book-specific config and assets.
- "Profile": a named configuration bundle selecting provider/model/fallback behavior.
- "Provider": a remote or local LLM endpoint that can produce translations.

## End-user workflow

### 1) One-time machine setup

`cipher` should support a global config file in the user's config directory (XDG-style) for:

- Providers/endpoints
- API keys (multiple per provider)
- Profiles (named model + fallback chains)
- Default runtime limits (timeouts, retries, validation strictness)

Commands:

- `cipher configure` (interactive; adds providers/keys; creates a default profile)
- `cipher profile list`
- `cipher profile show <name>`
- `cipher profile set-default <name>`

### 2) Create a new book

`cipher init /path/to/book` creates a minimal, usable scaffold:

- Book config (no secrets)
- Input folder (e.g. `raw/`)
- Output folder (e.g. `translated/`)
- Glossary file (empty, but valid)
- Optional style/voice guide template (user editable)

Suggested flags:

- `--profile <name>` (defaults to global default profile)
- `--from <existingBook>` (copy style + glossary structure; optional)
- `--import-glossary <path>` (accept legacy text or json; converts into canonical glossary format)

### 3) Translate

Primary command:

- `cipher translate /path/to/book`

Behavior:

- Reads chapters from input folder, sorts in predictable order (numeric-first is fine).
- Writes translated markdown to output folder.
- Skips already translated chapters by default.
- Records per-chapter status and errors so the run is resumable.

Important rerun controls:

- `--overwrite` overwrite outputs even if present
- `--overwrite-bad` overwrite only chapters that fail validation
- `--backup` (default on overwrite): keep timestamped backups of replaced outputs
- `--fail-fast` stop on first error (default should continue and report failures)

Companion commands:

- `cipher status /path/to/book` (progress, failures, last run details)
- `cipher retry-failed /path/to/book`

### 4) Review glossary (no extra model calls required)

Glossary updates should be a first-class user step.

- Commands:

- `cipher glossary list /path/to/book`
- `cipher glossary import <bookDir> <path>`
- `cipher glossary export <bookDir> <path>`

## Book layout

Recommended default layout (names configurable):

- `config.json` (book config; no secrets)
- `raw/` (input markdown)
- `tl/` (output markdown)
- `glossary.json` (canonical glossary)
- `style.md` (voice + tone + recurring rules; user maintained)
- `.cipher/` (tool-owned state: progress, logs, caches; safe to delete)

## Configuration design

### Two-layer config

1) Global config (user machine)
- Contains provider connection details, API keys, profiles, and defaults.
- Not meant to be committed.

2) Book config (inside book dir)
- Contains only portable settings: paths, selected profile, book-specific prompt/style settings, glossary location.

Optional: a book-local override file (gitignored) for users who prefer secrets per-book.

### Profiles (provider switching)

A book references a profile name instead of embedding provider/model details.

Profile includes:

- provider id
- primary model
- fallback chain (models; optionally cross-provider)
- stable generation knobs (to reduce chapter-to-chapter drift)
- validation policy (strict/normal)

Book config should be able to override a small subset safely (e.g. output folder).

### Multiple API keys and rotation

Global config supports multiple keys per provider. Runtime behavior:

- Select key by rotation/availability.
- On rate-limit/quota errors, mark key as cooling down until time T.
- Automatically retry with another key when available.
- Only surface key details in logs, not in book outputs.

Persistent per-key state (cooldowns, last used) lives outside the book (global state file).

### Fallback chain (models/providers)

Fallback should not increase routine request volume.

- Default: try primary model; transient retry; then fallback model(s) only if needed.
- Optional: allow cross-provider fallback if configured.
- Provide a strict mode that disables fallback for reproducibility.

## Glossary design

### Canonical glossary file

Use a single canonical glossary file per book (human-editable). This is the source of truth for enforced translations.

Fields per entry:

- `term`: translated term to use (English)
- `og_term`: original-language form (optional)
- `definition`: short explanation for the model and the reader
- `notes`: optional user notes (pronunciation, context)

Rules:

- All entries in the glossary are treated as authoritative and injected into translation prompts.
- Deduping should be deterministic. Prefer `og_term` when present, otherwise `term`.
- Importing merges new entries, skipping duplicates.

### Glossary injection strategy

The tool should support modes (book-configurable):

- `full`: inject all approved terms
- `smart`: inject a relevant subset + always include entries marked "always include" (optional)

If `smart` is used, it must be predictable and tuneable, with a clear fallback to `full` when uncertain.

### Migration and import

`cipher glossary import` should accept:

- legacy text format
- existing json arrays

and convert into the canonical glossary format with `status=approved` by default (or user choice).

## Prompting and consistency

### Style guide file

Encourage a `style.md` (or similar) that captures:

- tone, POV, tense, dialogue conventions
- rules for honorific/formality handling
- recurring translation choices that are not purely "terms"

This improves cross-chapter consistency without requiring multi-request chunking.

### Per-chapter prompt assembly

Each chapter request should include:

- base translation instructions
- book style guide
- approved glossary context (full or smart subset)
- the chapter text

The model response should include:

- translated markdown

## Validation and safety

Validation is required before accepting output.

Minimum checks:

- output is valid markdown-ish text (not empty)
- code fences are balanced
- no obvious JSON/schema artifacts leaked into the translation
- required top-level heading is present (configurable)

On validation failure:

- retry once (same model) with a repair instruction
- then use fallback model/provider only if configured
- if still failing, mark chapter as failed and continue (unless `--fail-fast`)

## Reruns, state, and backups

### Book state directory

Store run state under `.cipher/` in the book:

- per-chapter status: pending/success/failed/skipped
- error summaries
- per-chapter logs (optional)
- optional metadata for debugging (provider/model used, timing)

This state must never prevent a user from rerunning a chapter; it is informational.

### Output overwrite behavior

When overwriting an existing translated chapter:

- create a timestamped backup by default
- write new output atomically

## CLI surface (proposed)

- `cipher configure`
- `cipher init <bookDir>`
- `cipher translate <bookDir>`
- `cipher status <bookDir>`
- `cipher retry-failed <bookDir>`
- `cipher glossary list|import|export <bookDir>`
- `cipher profile list|show|set-default|test`
- `cipher doctor [bookDir]` (validate config, paths, glossary parse, provider reachability)

## Implementation milestones

1) Skeleton
- CLI layout + command parsing
- book init scaffold
- global config + profile resolution

2) Translation core
- chapter discovery + ordering
- translate command with skip/overwrite options
- structured response handling and output writing

3) Glossary workflow
- canonical glossary format
- glossary list/import/export commands

4) Provider robustness
- multiple API keys + cooldown/rotation
- fallback chain on failure only
- `doctor` and `profile test`

5) Quality gates
- markdown validation
- overwrite-bad flow
- improved logging + per-chapter summaries

## Engineering implementation plan (Rust + rig.rs)

This section is the step-by-step implementation plan for building `cipher` in Rust, using `rig` (rig.rs) for LLM calls. The goal is to deliver working value in small increments; each feature should land in a runnable state.

### Decisions and defaults (lock these in early)

- Default book layout: `raw/` input, `tl/` output, `glossary.json`, `style.md`, `.cipher/` state.
- Legacy compatibility: accept `translated/` as an output folder name when importing/migrating, but new books default to `tl/`.
- Book config file: `config.json` (portable; no secrets).
- Canonical glossary: JSON format with term/og_term/definition/notes.
- Translation response: structured JSON with `translation` and `new_glossary_terms`.

### Feature 1: CLI skeleton + project structure

Scope
- Choose CLI framework (`clap`) and error/reporting conventions.
- Establish module layout so later features don’t require large refactors.

Implementation notes
- Crate layout (suggested):
  - `src/main.rs` (thin) -> `src/cli.rs`
  - `src/book/*` (book layout + config)
  - `src/config/*` (global config, profiles, key state)
  - `src/glossary/*` (parse, dedupe, render for prompt)
  - `src/translate/*` (chapter discovery, prompting, response parsing)
  - `src/state/*` (.cipher run state)
  - `src/validate.rs` (output validation)
  - `src/fs.rs` (atomic writes + backups)

Done when
- `cipher --help` lists all planned subcommands (even if some are stubs).
- `cipher doctor` can run and prints a placeholder report.

### Feature 2: Book layout discovery + path resolution

Scope
- Given a `bookDir`, resolve all paths (raw dir, output dir, glossary path, style path, state dir).

Done when
- `cipher doctor <bookDir>` reports the resolved paths and whether each exists.

### Feature 3: `cipher init <bookDir>` scaffold

Scope
- Create directories and starter files:
  - `raw/`, `tl/`, `.cipher/`
  - `config.json` (portable defaults)
  - `glossary.json` (empty but valid)
  - `style.md` (template)

Done when
- Running `cipher init ./MyBook` creates a ready-to-translate scaffold.
- Re-running init is non-destructive (does not overwrite user-edited files by default).

### Feature 4: Canonical glossary format + basic commands

Scope
- Define `glossary.json` schema and implement:
  - load/save
  - deterministic dedupe
  - rendering for prompt injection (approved only)
- Add commands:
  - `cipher glossary list <bookDir> [--status approved|pending]`
  - `cipher glossary approve <id|term>`
  - `cipher glossary reject <id|term>`

Glossary entry fields
- `term` (string)
- `og_term` (string|null)
- `definition` (string)
- `notes` (string|null)

Commands
- `cipher glossary list <bookDir>`
- `cipher glossary import <bookDir> <path>`
- `cipher glossary export <bookDir> <path>`

Done when
- Glossary round-trips cleanly and commands behave deterministically.

### Feature 5: Global config (XDG) + profile resolution (no secrets in books)

Scope
- Implement a global config file in the user config directory containing:
  - providers (endpoint/base_url + provider kind)
  - API keys (multiple)
  - profiles (provider + model + fallback chain + generation knobs)
  - defaults (timeouts, retries, validation policy)
- Book `config.json` references a profile name.

Done when
- `cipher profile list|show|set-default` works.
- `cipher doctor <bookDir>` can resolve the effective profile for that book.

### Feature 6: Provider layer using rig.rs (MVP)

Scope
- Implement a provider abstraction backed by `rig` that can:
  - send a prompt
  - parse a structured JSON response into `TranslationResponse`
- Start with an OpenAI-compatible provider (works for OpenAI and Gemini’s compatible endpoint).

Done when
- A small internal smoke test can call the model (behind an env flag) and parse `TranslationResponse`.

### Feature 7: `cipher translate <bookDir>` (single chapter -> batch)

Scope
- Chapter discovery:
  - list `.md` files under `raw/`
  - numeric-first stable ordering (files without digits sort last)
- Translation loop:
  - skip if output exists (default)
  - write translation to `tl/<same-filename>.md`
  

Done when
- Translating a folder produces outputs and updates `glossary.json` with `pending` items.

### Feature 8: `.cipher/` run state + resumability

Scope
- Persist per-chapter status and last errors under `.cipher/`.
- Record enough metadata for debugging (provider, model, timing) without leaking secrets.

Done when
- `cipher status <bookDir>` shows counts of success/failed/skipped/pending.
- Re-running translate uses existing outputs + state to skip work predictably.

### Feature 9: Validation + repair retry

Scope
- Implement minimum output validation:
  - non-empty
  - balanced code fences
  - starts with `#` heading (configurable)
  - does not contain obvious JSON/schema leakage
- On failure: one repair retry with a “repair instruction” prompt.

Done when
- Bad outputs are detected and either repaired or marked failed with a clear reason.

### Feature 10: Overwrite, overwrite-bad, atomic writes, backups

Scope
- Implement:
  - `--overwrite`
  - `--overwrite-bad`
  - atomic output writing
  - timestamped backups on overwrite (default on overwrite)

Done when
- Overwrites never corrupt files (even on crash) and backups are created deterministically.

### Feature 11: Retry failed

Scope
- `cipher retry-failed <bookDir>` retries only failed chapters using the same rules as translate.

Done when
- Failed chapters can be retried without reprocessing successful chapters.

### Feature 12: Interactive workflows (configure + glossary review)

Scope
- `cipher configure` interactive global config creation/editing.
- `cipher glossary review <bookDir>` interactive approve/edit/reject for pending items.

Done when
- A user can go from empty machine state to a configured profile, translate a book, and review glossary without editing JSON manually.

### Feature 13: Multiple keys + cooldown/rotation

Scope
- Store per-key cooldown + last-used state outside books.
- Selection algorithm:
  - choose an available key by rotation
  - on quota/rate-limit errors, cooldown that key until T
  - retry with another available key

Done when
- Sustained batch runs rotate keys automatically and recover from 429/quota errors.

### Feature 14: Fallback chain (models/providers)

Scope
- Implement fallback strictly on failure, not for routine requests.
- Add strict mode to disable fallback for reproducibility.

Done when
- A configured fallback chain is exercised only when needed, and decisions are visible in logs/state.

### Feature 15: Smart glossary injection (optional, later)

Scope
- Add `smart` injection mode to include only relevant approved terms.
- Keep it predictable/tuneable and fall back to `full` when uncertain.

Done when
- Smart mode reduces prompt size without causing term drift; falling back to full is explainable.

## Open questions

- Should `cipher init` default to `full` glossary injection or `smart`?
- What is the minimum validation strictness that catches bad outputs without false positives?

