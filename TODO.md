# TODO

This file is the current working roadmap for `cipher`.

It is intentionally narrower than the old backlog:

- keep items that still matter
- mark what is already done elsewhere instead of leaving stale tasks around
- separate near-term work from longer-term design questions
- acknowledge that a full rerun-engine rewrite is a later decision, not current work

---

## Current direction

- `smart` glossary injection is the canonical/default mode
- books are initialized from markdown-first scaffolds; EPUB import is removed for now
- reruns should be understandable before they become more ambitious
- `--overwrite` remains the "redo everything under a new regime" option
- repair and glossary extraction should eventually become separate concerns
- orchestration code should get simpler before rerun logic gets more ambitious
- a full rerun-engine rewrite is explicitly deferred for now

What we are not doing right now:

- no worklist/fixpoint rerun engine rewrite yet
- no large glossary/state model rewrite yet
- no broad architectural rewrite just for the sake of flattening modules

---

## Done recently

- `rig-core 0.34`, Gemini provider, extractor retries, usage tracking
- rerun reason text, smart rerun detection, incremental replanning
- TOML config, structured book output, `--dry-run`
- `main()` cleanup, `translate_single_chapter` cleanup
- EPUB import removed
- CLI redesign: short flags (`-p`, `-o`, `-d`, `-q`, `-v`), `--quiet`/`--verbose` output gating, `--json` on display commands, consolidated `--rerun=MODE` flag
- Non-interactive `profile new` flags for scripting
- Progress bar during translation
- Module extraction: preview types to `preview.rs`, status display to `ui/`, rerun planning to `rerun.rs`
- Code-review findings: RerunPlanner removed, duplicated code extracted, alias fixed, ChapterPipeline collapsed, rerun mode preserved in state

---

## Active priorities

### 1. Add chapter source hashing

**Status:** Done

### 2. Implement `--rerun-affected-chapters`

**Status:** Done

### 3. Design `--rerun`

**Status:** Done

### 4. Add rerun preview mode

**Status:** Done

Implemented as `--dry-run` on `translate`.

### 5. Improve tracked/untracked visibility in `status`

**Status:** Done

---

## Active design work

### 6. Redesign validation/repair into a cleaner pipeline

**Status:** Done

### 7. Split glossary extraction from translation response

**Status:** Done

Glossary extraction is a second call after accepted translation. Extraction failure keeps the accepted chapter and skips term capture.

### 8. Narrow repair semantics

**Status:** Done

### 9. Revisit validation strictness after repair redesign

**Status:** Done

Tightened structured-field validation.

### 10. Standardize user config on TOML

**Status:** Done

### 11. Redesign global config schema while switching to TOML

**Status:** Done

### 12. Add book-configured structured output format

**Status:** Done

---

## Simplification and cleanup

These are worthwhile because they reduce cognitive load without changing product direction.

### 13. Refactor `main` into command-specific runners

**Status:** Done

### 14. Simplify `translate_single_chapter`

**Status:** Done

### 16. Simplify interactive profile flows

**Status:** Medium-value cleanup

Targets:

- `select_or_create_provider_sectioned`
- `select_or_create_api_key_sectioned`

Goal:

- separate menu branching from config mutation logic

### 17. Keep polishing `profile new`

**Status:** Follow-up polish

Potential follow-ons:

- clearer defaults
- cleaner summaries before saving
- better distinction between provider creation and provider reuse
- more obvious key-selection flow

### 18. Revisit `translate_book` structure after smaller cleanups

**Status:** Later cleanup

### 19. Do not rewrite the rerun engine yet

**Status:** Deferred intentionally

---

## UX and config follow-ups

### 20. Improve status/reporting for skipped-but-previously-successful chapters

**Status:** Future

### 21. Add more detailed skip output

**Status:** Planned

Useful cases to surface:

- skipped because output exists
- skipped because chapter content is unchanged
- skipped because glossary inputs are unchanged
- skipped because no rerun reason matched
- skipped because the chapter is empty
- skipped because of the current flag combination

### 22. Fix display for empty chapters

**Status:** Done

### 23. Revisit glossary matcher caching only if performance becomes a real issue

**Status:** Deferred unless needed

---

## Product and policy decisions

### 24. Decide the long-term role of `full` mode

**Status:** Open

### 25. Review mode-switch behavior explicitly

**Status:** Open

Need to decide:

- should switching `smart <-> full` trigger reruns?
- should full-mode runs advance canonical baseline?

### 26. Revisit exported-term tracking semantics

**Status:** Open

### 27. Auth system redesign (multi-key + rate-limit switching)

**Status:** Planned — deferred until after other work

**Goals:**

- Split secrets out of `config.toml` into a dedicated `auth.toml` (0o600 perms)
- Support env-var (`{env:VAR}`) and file (`{file:path}`) substitution
- First-class multi-key support with rotation policies (manual, round-robin, priority)
- Automatic key switching on 429 rate limits
- Configurable max key switches per request
- Migration: auto-migrate existing keys from `config.toml` on first load

**Design decisions (confirmed):**

- Per-provider rotation policy with a global fallback option
- Priority policy skips exhausted keys, uses next lowest priority
- No parallel translations (glossary constraint) — no Arc/Mutex needed
- Rate-limit state is runtime-only, never serialized
- Max key switches is configurable (not hardcoded)
- No Retry-After header parsing for now (avoid overcomplication)
- Standard env var names (OPENAI_API_KEY, GEMINI_API_KEY), no CIPHER_ prefix

**Config structure:**

```
~/.config/cipher/
├── config.toml    # Portable, no secrets (profiles, providers, models)
└── auth.toml      # Secrets only, 0o600 perms (keys, rotation policies)
```

**Phases:**

1. Foundation — `AuthConfig`, `AuthKey`, `ProviderKeys`, load/save with 0o600, `{env:}`/`{file:}` resolution, remove `Debug` from `AuthKey`
2. Config split & migration — remove keys from `ProviderConfig`, auto-migrate on first load
3. Key switching logic — `effective_rotation`, `get_next_key`, `mark_exhausted`, `reset_exhausted`, configurable max switches
4. Provider integration — `translate_with_key_switching`, provider-specific env fallbacks
5. CLI — `cipher auth` subcommand group (add, list, remove, use, set-rotation, status)
6. Tests — unit tests for substitution/rotation/migration, integration tests with mock server

**Files touched:**

- `src/config/auth.rs` (new)
- `src/config/mod.rs` (remove keys from ProviderConfig, migration)
- `src/config/profile.rs` (use AuthConfig)
- `src/translate/providers/mod.rs` (key switching logic)
- `src/cli.rs` (auth subcommands)

### 28. Evolve `cipher` beyond novel translation

**Status:** Open

---

## Optional follow-up fixes

These are real but not core roadmap items.

### 29. Flatten structured-output schema for Nvidia / OpenAI-compatible providers

**Status:** Optional

### 30. Surface persisted usage in `status`

**Status:** Nice to have

### 31. Revisit first-class OpenRouter support only if the structured-output story changes

**Status:** Deferred

---

## Suggested order

1. add chapter source hashing
2. implement `--rerun-affected-chapters`
3. design a first useful `--rerun`
4. improve status/reporting for skipped-but-previously-successful chapters
5. add more detailed skip output
6. tighten validation/reporting around structured output
7. simplify interactive profile flows
8. revisit `translate_book` structure after smaller cleanups
9. revisit `full` mode and exported-term policy questions
10. only then consider whether a larger rerun-engine rewrite is still worth it
