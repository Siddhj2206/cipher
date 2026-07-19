# TODO

Working roadmap for `cipher`. Intentionally narrower than the old backlog:
keep items that still matter, mark what is done, separate near-term from deferred.

---

## Direction

- `smart` glossary injection is the canonical/default mode
- books are initialized from markdown-first scaffolds; EPUB import is removed
- reruns should be understandable before they become more ambitious
- `--overwrite` remains the "redo everything under a new regime" option
- repair and glossary extraction should eventually become separate concerns
- orchestration code should get simpler before rerun logic gets more ambitious
- a full rerun-engine rewrite is explicitly deferred

What we are not doing right now:

- no worklist/fixpoint rerun engine rewrite yet
- no large glossary/state model rewrite yet
- no broad architectural rewrite just for the sake of flattening modules

---

## Done

- `rig-core 0.34`, Gemini provider, extractor retries, usage tracking
- rerun reason text, smart rerun detection, incremental replanning
- TOML config, structured book output, `--dry-run`
- `main()` cleanup, `translate_single_chapter` cleanup
- EPUB import removed
- CLI redesign: short flags (`-p`, `-o`, `-d`, `-q`, `-v`), `--quiet`/`--verbose`, `--json`, consolidated `--rerun=MODE`
- Non-interactive `profile new` flags for scripting
- Progress bar during translation
- Module extraction: preview types to `preview.rs`, status display to `ui/`, rerun planning to `rerun.rs`
- Code-review findings: RerunPlanner removed, duplicated code extracted, alias fixed, ChapterPipeline collapsed, rerun mode preserved in state
- Chapter source hashing
- `--rerun` (initially `--rerun-affected-chapters` / `--rerun-affected-glossary`, now consolidated to `--rerun=MODE`)
- Rerun preview as `--dry-run`
- Validation/repair pipeline redesign, glossary extraction split from translation, repair narrowed to re-translate with feedback
- Global config migrated to TOML, structured output format in book config
- `main()` refactored into command-specific runners, `translate_single_chapter` simplified
- Domain glossary (CONTEXT.md) and ADRs created
- README and TODO documented; final cleanup pass

---

## Short-term

### Simplify interactive profile flows

Medium-value cleanup. Targets:
- `select_or_create_provider_sectioned`
- `select_or_create_api_key_sectioned`

Goal: separate menu branching from config mutation logic.

### Keep polishing `profile new`

Follow-on polish:
- clearer defaults
- cleaner summaries before saving
- better distinction between provider creation and provider reuse
- more obvious key-selection flow

### Revisit `translate_book` structure after smaller cleanups

Later cleanup.

### Do not rewrite the rerun engine yet

Deferred intentionally.

---

## Product and policy decisions

### Decide long-term role of `full` mode

Open.

### Review mode-switch behavior

Need to decide:
- should switching `smart <-> full` trigger reruns?
- should full-mode runs advance canonical baseline?

### Revisit exported-term tracking semantics

Open.

### Auth system redesign (multi-key + rate-limit switching)

Planned — deferred until after other work.

**Goals:**
- Split secrets out of `config.toml` into a dedicated `auth.toml` (0o600 perms)
- Support env-var (`{env:VAR}`) and file (`{file:path}`) substitution
- First-class multi-key support with rotation policies
- Automatic key switching on 429 rate limits
- Migrate existing keys from `config.toml` on first load

### Evolve `cipher` beyond novel translation

Open.

---

## Optional

- Flatten structured-output schema for Nvidia / OpenAI-compatible providers
- Surface persisted usage in `status`
- Revisit first-class OpenRouter support if structured-output story changes
- Revisit glossary matcher caching only if performance becomes a real issue
- Improve status/reporting for skipped-but-previously-successful chapters
- Add more detailed skip output display
