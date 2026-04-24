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

- upgrade to `rig-core 0.34`
- add built-in Gemini provider
- add usage-returning extractor flow and per-chapter usage persistence
- add run token usage summary output
- add extractor retries
- improve rerun reason text and general translate CLI wording
- fix smart-mode rerun detection for newly relevant glossary terms
- move rerun planning away from one-shot startup planning with forward-only incremental replanning

These are done and should not be treated as open backlog items anymore.

---

## Active priorities

### 1. Add chapter source hashing
**Status:** Done

Why it matters:
- glossary changes are only one source of stale output
- content changes need a first-class rerun path

Scope for v1:
- store a normalized raw markdown hash in chapter state
- do not include provider/model/prompt/style fingerprints yet

Expected follow-ons:
- `--rerun-affected-chapters`
- a simpler future `--rerun`

### 2. Implement `--rerun-affected-chapters`
**Status:** Done

Why it matters:
- users need a predictable way to rerun chapters whose source changed

Acceptance ideas:
- changed raw chapter reruns
- unchanged raw chapter skips
- works independently of glossary rerun checks

### 3. Design `--rerun`
**Status:** Done

Desired meaning:
- rerun if chapter source changed
- rerun if glossary-relevant inputs changed

Notes:
- keep `--rerun-affected-glossary` and `--rerun-affected-chapters` for advanced use
- keep `--overwrite` separate

### 4. Add rerun preview mode
**Status:** Planned

Likely shape:
- `--dry-run` on `translate`
- possibly related visibility in `status`

Useful output:
- affected chapters
- reason text per chapter
- exact vs approximate decision source
- totals by category

### 5. Improve tracked/untracked visibility in `status`
**Status:** Done

Why it matters:
- the current rerun model mixes exact tracking and approximation
- users should be able to see which chapters are fully tracked

Potential output:
- tracked smart selection
- tracked full selection
- approximate legacy fallback
- exported terms recorded
- source hash recorded

---

## Active design work

### 6. Redesign validation/repair into a cleaner pipeline
**Status:** Done

Current problem:
- the initial translation response mixes accepted text and glossary extraction
- repair can blur ownership of `new_glossary_terms`

Target pipeline:
1. generate translation
2. validate translation
3. repair translation only if needed
4. extract glossary terms after translation is accepted

Why this matters:
- clearer state semantics
- easier testing
- narrower repair behavior

### 7. Split glossary extraction from translation response
**Status:** Under discussion

This is the concrete follow-on to the repair redesign.

Current implementation note:
- the code currently does glossary extraction in a separate follow-up request after translation is accepted
- this is intentionally not settled yet and may be reverted back to a combined translation + glossary response

Questions to answer:
- should glossary extraction stay in a second call after accepted translation
- should glossary extraction be folded back into the main translation response
- if split, should glossary extraction failure invalidate chapter success or only skip term capture
- if combined, how should repair avoid taking ownership of `new_glossary_terms`

### 8. Narrow repair semantics
**Status:** Done

Desired repair contract:
- fix structure only
- preserve meaning as much as possible
- do not invent glossary terms
- do not rewrite style unless required for validity

### 9. Revisit validation strictness after repair redesign
**Status:** Open

Follow-up areas:
- separate hard failures from warnings
- identify cases that can be auto-cleaned locally
- move toward book-configured output structure instead of hardcoded heading assumptions
- validate structured fields separately from rendered markdown

### 10. Standardize user config on TOML
**Status:** Planned

Decision:
- use `~/.config/cipher/config.toml` for global config
- use `cipher.toml` in each book directory
- keep TOML for user-authored config and JSON for machine-managed glossary/state data
- no migration compatibility is required right now

Why it matters:
- current user-facing config is split across JSON and Markdown in a way that is harder to read and edit
- TOML is a better fit for nested config and comments
- this is a good opportunity to simplify the schema instead of only changing file extensions

### 11. Redesign global config schema while switching to TOML
**Status:** Planned

Direction:
- keep `default_profile`
- keep named `profiles`
- keep named `providers`
- nest provider keys under each provider instead of keeping a separate top-level `keys` map
- remove provider `extras` for now

Notes:
- global config path should be `~/.config/cipher/config.toml`
- prefer a cleaner typed schema over arbitrary passthrough blobs

### 12. Add book-configured structured output format
**Status:** Planned

Direction:
- define output structure per book in `cipher.toml`
- keep provider-facing structured output schemas simple and flat
- prefer a small TOML field + render-template model over a custom DSL
- render final markdown locally from structured fields

Initial shape:
- fields like `chapter_number`, `chapter_title`, and `content`
- per-field `required` and `description`
- a render template for the final markdown heading/body layout

Why it matters:
- makes output shape configurable per book without inventing a full schema language
- makes repair more targeted because it can reason about missing or malformed fields
- reduces validator reliance on hardcoded heading heuristics

---

## Simplification and cleanup

These are worthwhile because they reduce cognitive load without changing product direction.

### 13. Refactor `main` into command-specific runners
**Status:** Good cleanup

Goal:
- keep `main()` focused on parse + dispatch + one error path

Likely extraction targets:
- `run_init(...)`
- `run_import(...)`
- `run_translate(...)`
- `run_status(...)`
- `run_doctor(...)`
- `run_profile(...)`

### 14. Simplify `translate_single_chapter`
**Status:** High-value cleanup

Why it matters:
- it repeats similar `ChapterResult` / `ChapterState` construction across skip/success/failure branches

Preferred direction:
- extract small helpers for skipped/success/failed result assembly
- do not redesign the overall flow yet

### 15. Break `import_epub` into clearer phases
**Status:** Medium-value cleanup

Likely phases:
- prepare target
- confirm reimport behavior
- clean existing raw chapters if needed
- import spine chapters

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

Why later:
- it is a real hotspot, but it should be simplified after `translate_single_chapter` and `main`
- avoid moving complexity around before smaller boundaries are clearer

### 19. Do not rewrite the rerun engine yet
**Status:** Deferred intentionally

Current position:
- the current rerun logic is already nuanced and somewhat complex
- a worklist/fixpoint engine may eventually be cleaner, but not yet
- any future rewrite should wait until source hashing and simpler rerun UX exist first

In other words:
- keep improving explainability and tracking now
- postpone full engine replacement until the product shape is clearer

---

## UX and config follow-ups

### 20. Improve status/reporting for skipped-but-previously-successful chapters
**Status:** Future

Why it matters:
- a chapter may be skipped this run while still representing a valid successful prior translation
- status output should make that distinction obvious

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

Why it matters:
- empty chapters should read clearly in CLI/status output instead of looking like an ambiguous failure or generic skip

### 23. Revisit glossary matcher caching only if performance becomes a real issue
**Status:** Deferred unless needed

Only do this if:
- large books show measurable slowdown
- profiling shows matcher rebuild cost actually matters

---

## Product and policy decisions

### 24. Decide the long-term role of `full` mode
**Status:** Open

Current leaning:
- keep `smart` as canonical
- treat `full` as non-canonical or emergency mode unless there is a strong reason not to

### 25. Review mode-switch behavior explicitly
**Status:** Open

Need to decide:
- should switching `smart <-> full` trigger reruns?
- should full-mode runs advance canonical baseline?

### 26. Revisit exported-term tracking semantics
**Status:** Open

Need to decide:
- should `exported_terms` mean only newly added terms from the last success?
- or should it preserve a broader semantic claim by the chapter?

### 27. Better API key storage
**Status:** Open

Ideas to explore:
- OS keyring / secret service
- env-var indirection
- encrypted local storage

### 28. Evolve `cipher` beyond novel translation
**Status:** Open

Questions:
- what is novel-specific today?
- can glossary/style abstractions generalize to docs or other markdown workflows?

---

## Optional follow-up fixes

These are real but not core roadmap items.

### 29. Flatten structured-output schema for Nvidia / OpenAI-compatible providers
**Status:** Optional

Why it matters:
- some OpenAI-compatible endpoints reject schemas that contain `$defs` references

Observed failure:
- `HTTP 400 Bad Request`
- `Grammar error: Pointer '/$defs/GlossaryTerm' does not exist`

Likely direction:
- flatten or inline `$defs` references before sending schemas on OpenAI-compatible paths

### 30. Surface persisted usage in `status`
**Status:** Nice to have

Why it matters:
- usage is now collected and persisted per chapter, but `status` does not expose it yet

### 31. Revisit first-class OpenRouter support only if the structured-output story changes
**Status:** Deferred

Current understanding:
- the native rig OpenRouter provider is not a good fit for `cipher` right now because `cipher` depends on structured extraction
- if that changes in rig, revisit whether OpenRouter should be a first-class provider instead of only going through compatible paths

---

## Suggested order

1. add chapter source hashing
2. implement `--rerun-affected-chapters`
3. design a first useful `--rerun`
4. add rerun preview and better status visibility
5. switch user-facing config to TOML and simplify the global config schema
6. add book-configured structured output and move validation toward structured fields + local rendering
7. simplify `main` and `translate_single_chapter`
8. revisit `full` mode and exported-term policy questions
9. only then consider whether a larger rerun-engine rewrite is still worth it
