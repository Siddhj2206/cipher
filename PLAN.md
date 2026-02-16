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

- During translation, model-suggested terms are saved as `pending`.
- Only `approved` glossary entries are injected into the translation prompt.

Commands:

- `cipher glossary list /path/to/book` (filter by status: approved/pending)
- `cipher glossary review /path/to/book` (interactive approve/edit/reject)
- `cipher glossary approve <id|term>` / `cipher glossary reject <id|term>`
- `cipher glossary export /path/to/book --format <...>` (optional)

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

Use a single canonical glossary file per book (human-editable), with enough metadata to support review.

Recommended fields per entry:

- `term`: translated term to use (English)
- `og_term`: original-language form (optional)
- `definition`: short explanation for the model and the reader
- `status`: `approved` or `pending`
- `notes`: optional user notes (pronunciation, context)
- `first_seen`: chapter/file id (optional)
- `last_seen`: chapter/file id (optional)

Rules:

- Only `approved` entries are injected.
- New terms from the model are appended as `pending` (deduped).
- Deduping should be deterministic. Prefer `og_term` when present, otherwise `term`.

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
- `new_glossary_terms` suggestions (saved as pending)

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
- `cipher glossary list|review|approve|reject|import|export <bookDir>`
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
- pending/approved split
- glossary review/approve/reject commands

4) Provider robustness
- multiple API keys + cooldown/rotation
- fallback chain on failure only
- `doctor` and `profile test`

5) Quality gates
- markdown validation
- overwrite-bad flow
- improved logging + per-chapter summaries

## Open questions

- Should `cipher init` default to `full` glossary injection or `smart`?
- What is the minimum validation strictness that catches bad outputs without false positives?
- Should the canonical glossary entries be addressable by stable ids (recommended) or by term text only?
