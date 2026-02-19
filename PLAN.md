# cipher

This document describes how the rebuilt tool (renamed from `btranslate` to `cipher`) should work for end users, how books are initialized and managed, and how config/providers/keys/glossary are handled. The later "Engineering implementation plan" section is allowed to be implementation-specific.

## Goals

- Make translating a new book feel like a guided, repeatable workflow: init -> translate -> review glossary -> iterate.
- Keep secrets out of book folders by default (books stay portable and safe to commit/share).
- Allow fast switching between providers/models without editing every book.
- Support multiple API keys and automatic key rotation/cooldowns.
- Provide reliable reruns: easy overwrite, backups, and "overwrite only bad outputs".
- Keep glossary consistent: the book glossary is the source of truth; the model may return new glossary terms and `cipher` merges them deterministically (deduped) after successful chapter writes.
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

- `cipher profile new` (interactive; adds providers/keys; creates a profile)
- `cipher profile new` (interactive; can reuse existing providers/keys; creates a profile)
- `cipher profile list`
- `cipher profile show <name>`
- `cipher profile set-default <name>`
- `cipher profile test [name]`

### 2) Create a new book

`cipher init /path/to/book` creates a minimal, usable scaffold:

- Book config (no secrets)
- Input folder (e.g. `raw/`)
- Output folder (default `tl/`)
- Glossary file (empty, but valid)
- Optional style/voice guide template (user editable)

Suggested flags:

- `--profile <name>` (defaults to global default profile)
- `--from <existingBook>` (copy style + glossary structure; optional)
- `--import-glossary <path>` (accept canonical JSON; copies into the book if missing)

### 3) Translate ✓ DONE

Primary command:

- `cipher translate /path/to/book`

Behavior:

- Reads chapters from input folder, sorts in predictable order (numeric-first is fine).
- Writes translated markdown to output folder.
- Skips already translated chapters by default.
- Records per-chapter status and errors so the run is resumable.
- **Retry logic**: Failed translations (API errors or validation failures) retry up to 3 times total.
- **Summary output**: Prints final counts (translated, skipped, failed, new glossary terms) at end of run.

CLI output should follow Book-Translator-Go style:

- Sentence case, no bracket tags like `[SKIP]`.
- Use `- ` for sub-messages under a chapter.
- Print the effective profile before the run:
  - `Using profile <name>`
  - `- Provider: <provider>`
  - `- Model: <model>`
- Show glossary usage counts when translating a chapter (e.g. `- Using smart glossary: N/M terms`).

Important rerun controls:

- `--overwrite` overwrite outputs even if present
- `--overwrite-bad` overwrite only chapters whose existing output fails the full validator
- `--backup` (default on overwrite): keep timestamped backups of replaced outputs
- `--fail-fast` stop on first error (default should continue and report failures)

Companion commands:

- `cipher status /path/to/book` (progress, failures, last run details)

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
- optional key label to select which API key to use for that provider
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

The glossary has no approval workflow: there is no `id` and no `status` field (no pending/approved states). The glossary is the enforced source of truth.

### Glossary injection strategy

The tool should support modes (book-configurable):

- `full`: inject all glossary terms
- `smart`: inject a relevant, deterministic subset (with a clear fallback to `full` when uncertain)

Default mode is `smart`.

If `smart` is used, it must be predictable and tuneable (same inputs => same injected subset), with a clear fallback to `full` when uncertain. The algorithm should match Book-Translator-Go:

- Sliding window candidates over the chapter text (window sizes 3..=6), skipping ASCII-only windows
- Fuzzy match each candidate to the closest `og_term` (ngram bag sizes 3 and 4)
- Accept a fuzzy match only if the matched `og_term` is present as an exact substring in the chapter
- Always include entries with empty `og_term`
- Fallback to full glossary when fewer than 5 glossary entries match

### Migration and import

`cipher glossary import` accepts canonical JSON arrays and merges into the book glossary. Duplicate entries (by normalized key) are skipped.

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
- glossary context (full or smart subset)
- the chapter text

The model response should include:

- translated markdown
- new glossary terms identified during translation

`cipher` auto-merges `new_glossary_terms` into `glossary.json` only when the chapter output passes validation and is written successfully.

## Validation and safety

Validation is required before accepting output.

Current checks:

- output is not empty
- output starts with a top-level heading and the first line matches `# Chapter X` or `# Chapter X: Title`
- code fences are balanced
- no JSON/schema leakage: detects `{...}`, `[...]`, `"type":`, `"properties":`, `"$ref"`

On validation failure:

- retry once (same model) with a repair instruction including validation errors, failed translation, and original text
- if still failing, mark chapter as failed and continue (unless `--fail-fast`)

## Reruns, state, and backups

### Book state directory

Store run state under `.cipher/` in the book:

- per-chapter status: success/failed/skipped
- error summaries
- per-chapter logs (optional)
- optional metadata for debugging (provider/model used, timing)

This state must never prevent a user from rerunning a chapter; it is informational.

### Output overwrite behavior

When overwriting an existing translated chapter (target behavior):

- create a timestamped backup by default
- write new output atomically

## CLI surface (proposed)

- `cipher init <bookDir>`
- `cipher translate <bookDir>`
- `cipher status <bookDir>`
- `cipher glossary list <bookDir>`
- `cipher glossary import <bookDir> <path>`
- `cipher glossary export <bookDir> <path>`
- `cipher profile new|list|show|set-default|test`
- `cipher doctor [bookDir]` (validate config, paths, glossary parse, provider reachability)

## Implementation milestones

1) Skeleton
- CLI layout + command parsing
- book init scaffold
- global config + interactive profile creation

2) Translation core
- chapter discovery + ordering
- translate command with skip/overwrite options
- structured response handling and output writing

3) Glossary workflow
- canonical glossary format
- glossary list/import/export commands

4) Provider robustness
- smart glossary injection (optional, to control prompt size)
- fallback chain on failure only
- multiple API keys + cooldown/rotation
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
- Default glossary injection: `smart` (book config key `glossary_injection`).

### Feature 1: CLI skeleton + project structure

Scope
- Choose CLI framework (`clap`) and error/reporting conventions.
- Establish module layout so later features don’t require large refactors.

Implementation notes
- Crate layout (current/expected):
  - `src/main.rs` (clap CLI + dispatch)
  - `src/book/*` (book layout + config)
  - `src/config/*` (global config, profiles, key state)
  - `src/glossary/*` (parse, dedupe, render for prompt)
  - `src/translate/*` (chapter discovery, prompting, response parsing)
  - `src/state/*` (.cipher run state)
  - `src/validate/*` (output validation)
  - `src/fs.rs` (atomic writes + backups; optional future module)

Done when
- `cipher --help` lists all planned subcommands (even if some are stubs).
- `cipher doctor` can run and prints a placeholder report.

### Feature 2: Book layout discovery + path resolution

Scope
- Given a `bookDir`, resolve all paths (raw dir, output dir, glossary path, style path, state dir).

Done when
- `cipher doctor <bookDir>` reports the resolved paths and whether each exists.

### Feature 3: `cipher init <bookDir>` scaffold ✓ DONE

Scope
- Create directories and starter files:
  - `raw/`, `tl/`, `.cipher/`
  - `config.json` (portable defaults, includes `"profile": "default"`)
  - `glossary.json` (empty but valid)
  - `style.md` (template)

Done when
- Running `cipher init ./MyBook` creates a ready-to-translate scaffold.
- Re-running init is non-destructive (does not overwrite user-edited files by default).
- New books have `"profile": ""` in config.json (empty string falls back to global default_profile).

### Feature 4: Canonical glossary format + basic commands

Scope
- Define `glossary.json` schema and implement:
  - load/save
  - deterministic dedupe
  - rendering for prompt injection
- Add commands:
  - `cipher glossary list <bookDir>`
  - `cipher glossary import <bookDir> <path>`
  - `cipher glossary export <bookDir> <path>`

Glossary entry fields
- `term` (string)
- `og_term` (string|null)
- `definition` (string)
- `notes` (string|null)

Done when
- Glossary round-trips cleanly and commands behave deterministically.

### Feature 5: Global config (XDG) + interactive profile creation (no secrets in books)

Scope
- Implement a global config file in the user config directory (`~/.config/cipher/config.json`) containing:
  - providers (endpoint/base_url + provider kind)
  - API keys (multiple per provider)
  - profiles (provider + model + generation knobs)
- Book `config.json` references a profile name.
- Add interactive `cipher profile new`:
  - Select an existing provider, or create a new one (OpenAI or OpenAI-compatible)
  - Enter base URL when creating an OpenAI-compatible provider
  - Select an existing API key for that provider, or add a new key
  - Keys are selected by key label; the profile can pin a specific labeled key
  - Enter model name
  - Optionally set as default
- Provider design is extensible for future provider kinds (rig.rs-native or custom).

Done when
- `cipher profile new|list|show|set-default|test` works.
- `cipher doctor <bookDir>` can resolve the effective profile for that book.

### Feature 6: Provider layer using rig.rs (MVP)

Scope
- Implement a provider abstraction backed by `rig` that can:
  - Send a structured prompt built from Book-Translator-Go's base prompt
  - Parse a structured JSON response with `translation` and `new_glossary_terms`
- Provider design:
  - File-per-provider structure (`src/translate/providers/openai.rs`)
  - `Provider` trait for modularity (easy to add more providers later)
  - OpenAI and OpenAI-compatible providers (both use rig's OpenAI provider with optional base_url)
- Use rig's `Extractor` for typed JSON output with JSON schema derived from Rust types
- Provider API notes (important for compatibility):
  - OpenAI: rig's default extractor targets the Responses API (`POST /responses`).
  - OpenAI-compatible endpoints: many only implement Chat Completions (`POST /chat/completions`), so the provider must use rig's chat-completions extractor (e.g. `completions_api().extractor(...)`) when configured as compatible.
- Base prompt copied from Book-Translator-Go:
  - Tone/atmosphere requirements
  - Dialogue, pacing, cultural nuance guidelines
  - Extremely selective glossary term criteria
  - Strict formatting: must start with `# Chapter X: Title` or `# Chapter X`

Done when
- `Translator::translate_chapter()` returns `TranslationResponse` with translation and new glossary terms.
- Provider can be constructed from global config and profile.

### Feature 7: `cipher translate <bookDir>` (batch translation) ✓ DONE

Scope
- Chapter discovery:
  - List `.md` files under `raw/`
  - Numeric-first stable ordering (files without digits sort last)
- Translation loop:
  - Load global config and resolve effective profile for the book
  - Skip if output exists (default)
  - Translate each chapter using the provider with retry logic:
    - API errors: retry same prompt up to 3 times total (initial + 2 retries)
    - Validation failure on first attempt: single repair retry with error context
    - Repair retry includes: validation errors, failed translation, original text
    - After repair failure: mark chapter as failed, continue
    - Output shows progress: `- Attempt X/3 failed: <error>. Retrying...`
  - Validate output before accepting (current):
    - Non-empty
    - Strict heading: first line must be `# Chapter X: Title` or `# Chapter X`
    - Balanced code fences
  - On final failure (after 3 attempts): mark chapter as failed, continue (unless `--fail-fast`)
  - On validation success:
    - Write translation to `tl/<same-filename>.md`
    - Auto-merge `new_glossary_terms` into `glossary.json` (dedupe by og_term/term)
- Overwrite controls:
  - `--overwrite` - retranslate even if output exists
  - `--overwrite-bad` - only retranslate outputs that fail validation
  - `--backup` (default true) - timestamped backups before overwrite
- State tracking:
  - Store per-chapter status under `.cipher/`
  - Record success/failed/skipped counts
- Final summary output:
  - Translated: N
  - Skipped: N
  - Failed: N
  - New glossary terms: N

Done when
- Translating a folder produces outputs in deterministic order.
- Glossary is updated with new terms only from successfully translated chapters.
- Overwrite-bad, skip, and fail-fast behaviors work correctly.
- CLI output follows Book-Translator-Go style (profile header, per-chapter messages with `- ` sub-lines, glossary usage counts).
- Failed chapters retry up to 3 times before giving up.

### Feature 8: `.cipher/` run state + resumability

Scope
- Persist per-chapter status and last errors under `.cipher/`.
- Record enough metadata for debugging (provider, model, timing) without leaking secrets.

Done when
- `cipher status <bookDir>` shows last run details and counts of success/failed/skipped.
- Re-running translate uses existing outputs + state to skip work predictably.

### Feature 9: Validation + repair retry ✓ DONE

Scope
- Extended validation with JSON/schema leakage detection
- Repair retry on validation failure

Implementation
- `src/validate/mod.rs`:
  - `check_json_leakage()`: Detects raw JSON patterns (`{...}`, `[...]`)
  - `check_schema_leakage()`: Detects schema artifacts (`"type":`, `"properties":`, `"$ref"`)
- `src/translate/types.rs`:
  - Added `repair_instruction: Option<String>` and `failed_translation: Option<String>` to `TranslationRequest`
  - Builder methods for repair context
- `src/translate/prompt.rs`:
  - When repair_instruction is set, prompt includes: previous errors, failed translation, original text
- `src/translate/cmd.rs`:
  - API errors: retry same prompt up to 3 times
  - Validation failure on 1st attempt: 1 repair retry with error context
  - Validation failure on retries 2-3: fail immediately (no repair)

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

### Feature 11: Smart glossary injection ✓ DONE

Scope
- Implement `glossary_injection` in book config (`smart` default, `full` optional).
- Smart mode selects relevant glossary terms per chapter using the Book-Translator-Go algorithm and constants:
  - min matches = 5
  - window sizes = 3..=6
  - ngram bag sizes = 2, 3, and 4
- Keep it deterministic and explainable; fall back to `full` when uncertain.
- Performance requirement: smart selection must be fast on large books.

Implementation
- Custom `ClosestMatch` struct in `src/glossary/closest_match.rs` using inverted index
- Precomputes ngram-to-term-id mapping for O(1) lookups
- Algorithm matches Go's `github.com/schollz/closestmatch`:
  - Build inverted index: `ngram → Vec<term_index>`
  - For each query ngram, look up matching terms (O(1))
  - Aggregate intersection scores only for matching terms
  - Return highest scoring term
- Handles Chinese/short strings with bag size 2

Done when
- Smart mode is the default, reduces prompt size without causing term drift, and remains fast on large books.

#### Fallback: Lazy matcher rebuild (if performance is still lacking)

If the current approach (rebuilding ClosestMatch on every `select_terms_smart` call) proves slow in practice:

- Add `needs_rebuild: bool` flag to glossary state (like Book-Translator-Go's `needsRebuild`)
- Cache `matcher: Option<ClosestMatch>` in the glossary struct
- `ensure_matcher()` rebuilds only when `needs_rebuild == true`
- Set `needs_rebuild = true` whenever terms are added/modified
- This avoids redundant inverted index construction across chapters in the same run

### Feature 12: Multiple keys + cooldown/rotation

Scope
- Store per-key cooldown + last-used state outside books.
- Selection algorithm:
  - choose an available key by rotation
  - on quota/rate-limit errors, cooldown that key until T
  - retry with another available key

Done when
- Sustained batch runs rotate keys automatically and recover from 429/quota errors.

### Feature 13: Fallback chain (models/providers)

Scope
- Implement fallback strictly on failure, not for routine requests.
- Add strict mode to disable fallback for reproducibility.

Done when
- A configured fallback chain is exercised only when needed, and decisions are visible in logs/state.

## Open questions

- Where should the smart-glossary matcher/index live (per run cache vs persisted under `.cipher/`), and when should it rebuild (on glossary file change vs always)?
- What is the minimum validation strictness that catches bad outputs without false positives?
