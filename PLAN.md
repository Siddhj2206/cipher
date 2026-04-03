# cipher

`cipher` is a Rust CLI for translating book chapters with LLMs.

Current core workflow:

1. Create or import a book project
2. Configure a profile that selects provider/model credentials
3. Translate chapters from `raw/` into `tl/`
4. Inject glossary terms in `smart` or `full` mode
5. Validate output before accepting it
6. Merge newly discovered glossary terms into `glossary.json`
7. Record run/chapter/glossary state in `.cipher/`

---

## Guiding product direction

The long-term shape of `cipher` should be:

- a reliable CLI for iterative book translation, not just one-shot chapter generation
- glossary-driven by default, with `smart` injection as the canonical mode
- resumable and explainable when work is skipped, rerun, repaired, or failed
- deterministic enough that users can trust rerun decisions
- portable at the book level, with secrets managed outside the book folder
- conservative about accepting output: validate first, persist accepted results only
- modular enough that provider, prompting, glossary logic, validation, and state can evolve independently

A good default mental model for users should be:

- `cipher init` or `cipher import` to prepare a book
- `cipher translate` as the main working loop
- glossary grows over time
- reruns become increasingly reliable as state tracking improves
- `status` and future dry-run tooling explain what happened and what still needs attention

---

## What exists today

## CLI surface

Implemented commands:

- `cipher import <epub_path>`
- `cipher init <book_dir>`
- `cipher translate [book_dir]`
- `cipher status <book_dir>`
- `cipher glossary list <book_dir>`
- `cipher glossary import <book_dir> <path>`
- `cipher glossary export <book_dir> <path>`
- `cipher doctor [book_dir]`
- `cipher profile new`
- `cipher profile list`
- `cipher profile show <name>`
- `cipher profile set-default <name>`
- `cipher profile test [name]`

Current translate flags:

- `--profile <name>`
- `--overwrite`
- `--fail-fast`
- `--rerun-affected-glossary`

## Book layout

Default project layout:

- `config.json`
- `raw/`
- `tl/`
- `glossary.json`
- `style.md`
- `.cipher/`

State currently stored under `.cipher/` includes:

- run metadata
- per-chapter state
- glossary tracking state
- backups created during overwrite flows

## Translation pipeline

Current chapter flow:

1. Discover markdown chapters in `raw/`
2. Skip existing outputs unless overwrite/rerun conditions apply
3. Select glossary terms according to configured injection mode
4. Call the translator
5. Validate the returned translation
6. If validation fails, attempt one repair pass
7. Atomically write accepted output
8. Merge newly added glossary terms into the book glossary
9. Save chapter/run state incrementally

Current behavior already includes:

- deterministic chapter ordering
- retries for API failures
- validation-before-acceptance
- automatic backup when overwriting an existing output
- resumable state updates after each chapter
- summary output at the end of a run

## Glossary system

Current glossary model:

- canonical file is `glossary.json`
- glossary entries are plain JSON objects with:
  - `term`
  - optional `og_term`
  - `definition`
  - optional `notes`

Current glossary behavior:

- deterministic dedupe/merge
- prompt fingerprinting for glossary terms
- `smart` and `full` injection modes
- `smart` mode uses deterministic matching and falls back to `full` when too few terms match
- newly returned glossary terms are merged after successful chapter acceptance

## Validation and repair

Current validation checks include:

- non-empty output
- heading presence/shape
- balanced code fences
- JSON/schema leakage detection
- raw JSON-like output rejection

Current repair behavior:

- first generation is validated
- on validation failure, a repair request is sent
- repaired output is validated again
- accepted repaired output is written as the final output

This works today, but the architecture is still transitional and should be refined further.

## Rerun and state model

Current state tracking already captures:

- per-run metadata
- per-chapter success/failed/skipped status
- chapter glossary usage
- exported glossary term fingerprints
- glossary-state snapshots

Current glossary rerun support:

- `--rerun-affected-glossary` compares current glossary-relevant inputs against previously recorded state
- tracked smart-mode chapters can rerun when previously selected/exported term fingerprints change
- tracked smart-mode chapters can also rerun when the effective smart selection changes
- rerun planning is no longer purely one-shot: remaining chapters are replanned during a run after successful glossary additions

This is a meaningful step forward, but not the final rerun model.

---

## Architectural principles going forward

## 1. Keep modules sharply separated

The existing structure is good and should remain the baseline:

- `src/main.rs` for CLI entry and dispatch
- `src/book/` for layout, init, doctor, and book config
- `src/config/` for global config and profiles
- `src/glossary/` for glossary parsing, merge, matching, and selection
- `src/import/` for EPUB ingestion
- `src/translate/` for orchestration, prompts, provider handling
- `src/state/` for run/chapter/glossary persistence
- `src/validate/` for output validation
- `src/output.rs` and related UI helpers for CLI presentation

New work should reinforce these boundaries, not blur them.

## 2. Treat accepted translation and glossary extraction as separate concerns

Today, generated output and glossary additions still travel together through the translation flow.

The future design should make this cleaner:

- translation acceptance should stand on its own
- repair should fix translation validity only
- glossary extraction should happen after a translation is accepted

This is one of the most important architectural cleanups still ahead.

## 3. Prefer deterministic tracking over heuristics when possible

Approximate behavior is acceptable as a bridge, but the system should trend toward:

- explicit fingerprints
- explainable rerun causes
- stable skip/rerun semantics
- clearer user-visible status around tracked vs approximate decisions

## 4. Make reruns explainable, not just correct

It is not enough to rerun the right chapters. Users also need to understand why.

Reason strings, dry-run output, status visibility, and skip explanations should all become first-class UX concerns.

## 5. Preserve safe file behavior

The current safety posture is correct and should remain standard:

- write atomically
- avoid partial state updates where possible
- back up overwritten user-visible outputs
- keep book folders portable
- never require state files for ordinary book inspection

---

## Current strengths

The rewritten plan should preserve and build on what is already working well:

- clean Rust module layout
- practical CLI surface already in place
- working EPUB import
- profile-based provider configuration
- deterministic glossary dedupe
- smart glossary selection with fallback
- validation gate before accepting output
- repair pass for malformed generations
- incremental saved state for resumability
- glossary-aware rerun planning with tracked smart selection support
- tests covering glossary selection, rerun planning, validation, state round-trips, and CLI-adjacent logic

These are not “future ideas” anymore; they are now part of the baseline.

---

## Main gaps between current state and desired state

## 1. Rerun model is improved but not complete

What exists now:

- glossary-aware reruns
- tracked smart selection comparison
- forward-only incremental replanning for remaining chapters

What is still missing:

- chapter source-content reruns
- unified `--rerun`
- dry-run preview
- stronger chapter input fingerprints
- richer user-facing skip/replan reasoning
- status visibility around tracked vs approximate state
- eventual convergence model if glossary changes cascade through a run

## 2. Repair flow is still too coupled to translation/glossary semantics

Current repair logic works, but the ownership boundaries are still fuzzy. This risks unclear semantics around glossary additions and accepted outputs.

## 3. Status UX is still basic

Current `status` is useful, but it is still mostly a run summary. It does not yet expose the richer tracking details the rerun system increasingly depends on.

## 4. Secret handling needs a better long-term story

Profiles and provider setup are usable today, but API key handling should move toward safer storage and better portability semantics.

## 5. Plan/docs drift needs to stay under control

This rewrite exists because the old plan lagged behind the codebase. Going forward:

- `PLAN.md` should reflect actual architecture and direction
- `TODO.md` should hold the detailed active backlog
- `README.md` should stay end-user focused

---

## Roadmap

## Stage 1: Solidify rerun correctness and observability

This is the current top priority.

### Goals

- make reruns predictable
- reduce stale outputs caused by glossary evolution
- explain rerun decisions in plain language
- prepare the system for a future unified rerun command

### Deliverables

- continue improving glossary rerun reasoning
- surface better logs for incremental replanning during a run
- distinguish exact tracking from approximation in user-visible output
- design and implement chapter content hashing
- introduce `--rerun-affected-chapters`
- define `--rerun` as the union of glossary-driven and source-driven reruns
- add preview/dry-run support for rerun decisions

### Success criteria

- users can tell why a chapter reran or was skipped
- glossary changes discovered mid-run can affect remaining chapters in that same run
- chapter source edits can be detected independently of glossary changes
- rerun commands feel intentional instead of ad hoc

## Stage 2: Redesign translation acceptance vs glossary extraction

This is the next major architectural cleanup.

### Goals

- untangle accepted translation from glossary harvesting
- make repair narrower and safer
- improve reasoning and testing around accepted outputs

### Target pipeline

1. Generate translation
2. Validate translation
3. Repair translation only if needed
4. Accept/write translation
5. Extract glossary terms separately
6. Merge extracted glossary additions

### Benefits

- clearer semantics
- fewer surprising glossary side effects from repair output
- easier targeted testing
- more future flexibility for glossary extraction strategies

## Stage 3: Expand state model carefully

State is already useful; the next step is making it strong enough for better rerun guarantees without overcomplicating the system.

### Likely additions

- chapter `source_hash`
- richer chapter input fingerprinting later
- more explicit tracking of effective injection mode
- clearer semantics for exported-term ownership after reruns

### Important constraint

Do not jump straight to an overdesigned fingerprint system. Add only what is needed to support the next rerun features.

## Stage 4: Improve UX and operational polish

### Areas

- clearer `status` output
- future verbose mode for skip reasons
- dry-run/preview mode
- better interactive profile creation polish
- improved diagnostics around config and provider state
- better API key storage

This stage should improve trust and day-to-day usability without changing core architecture.

---

## Command direction

This section describes the intended command shape after the rerun work matures.

## Stable core commands

These should remain the product backbone:

- `cipher import`
- `cipher init`
- `cipher translate`
- `cipher status`
- `cipher glossary ...`
- `cipher profile ...`
- `cipher doctor`

## Translate flag direction

Current:

- `--profile`
- `--overwrite`
- `--fail-fast`
- `--rerun-affected-glossary`

Planned additions/evolution:

- `--rerun-affected-chapters`
- `--rerun`
- `--dry-run`

Intended semantics:

- `--rerun-affected-glossary`: rerun chapters whose glossary-relevant inputs changed
- `--rerun-affected-chapters`: rerun chapters whose source content changed
- `--rerun`: union of both
- `--overwrite`: redo everything regardless of tracked equivalence

That separation should stay sharp. `--overwrite` is not the same thing as rerunning stale chapters.

---

## State direction

Current state files are already central to resumability and rerun planning.

### Current tracked concepts

- run metadata
- glossary baseline/state
- chapter result status
- chapter glossary usage
- exported term fingerprints

### Future tracked concepts

- chapter source hash
- stronger effective input identity
- tracked vs approximate rerun capability
- richer chapter diagnostics

### Principle

State should improve correctness and visibility, but users should not need to understand internal files just to operate the CLI.

---

## Glossary mode strategy

`smart` is the long-term default and should be treated as the canonical path.

`full` can remain supported, but new design work should optimize around smart-mode tracking and reruns first.

Questions still to answer intentionally:

- what canonical role `full` should have in the long-term model
- whether mode switches should trigger reruns
- whether `full` should ever redefine the tracked baseline in the same way as `smart`

Until those are settled, avoid accidental semantics emerging from unrelated refactors.

---

## Documentation strategy

To keep docs aligned with reality:

- `README.md` should explain installation, user workflow, and command usage
- `PLAN.md` should describe current architecture and medium-term direction
- `TODO.md` should contain specific open items, proposed solutions, and follow-up notes

When the codebase moves significantly, update this file again rather than letting an outdated roadmap linger.

---

## Practical implementation priorities

If choosing what to work on next, prefer this order:

1. rerun correctness and reason text
2. chapter-content rerun support
3. dry-run/preview and status visibility
4. repair/extraction pipeline redesign
5. API key storage and setup UX polish

This ordering fits the current codebase best: it builds directly on the rerun/state foundation that now exists.

---

## Definition of success for the next phase

The next phase of `cipher` should make the tool feel like a reliable iterative translation system rather than a batch generator.

That means:

- reruns are trustworthy
- state is informative
- glossary growth does not silently invalidate work
- users can preview and understand what will happen
- accepted translations and glossary extraction have clean semantics
- the CLI remains simple even as internal tracking gets stronger

That is the direction this project should optimize for from here.
