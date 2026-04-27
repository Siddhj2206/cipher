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

**Status:** Under discussion

Questions to answer:

- should glossary extraction stay in a second call after accepted translation
- should glossary extraction be folded back into the main translation response
- if split, should glossary extraction failure invalidate chapter success or only skip term capture
- if combined, how should repair avoid taking ownership of `new_glossary_terms`

### 8. Narrow repair semantics

**Status:** Done

### 9. Revisit validation strictness after repair redesign

**Status:** In progress

Follow-up areas:

- separate hard failures from warnings
- identify cases that can be auto-cleaned locally
- continue tightening the book-configured output path
- decide whether chapter metadata fields need stricter validation than free-form strings
- separate structured-field validation from rendered-markdown warnings where helpful

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

**Status:** Open

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

### 27. Better API key storage

**Status:** Open

Ideas to explore:

- OS keyring / secret service
- env-var indirection
- encrypted local storage

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
