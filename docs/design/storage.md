# Book storage

The SQLite schema behind `cipher-book` (ADR 0004, `architecture.md`).

## Conventions

- **File:** one file per Book, ending `.cipher`.
- **Settings:** WAL mode, `foreign_keys = ON`, a 10-second busy timeout.
- **Schema:** `crates/cipher-book/src/schema.rs` is the source of truth. The version is `PRAGMA user_version`; `Book::open` runs the migrations. Never edit a migration once a real Book has been written with it; append a new one. V2 (2026-11-15) adds indexes on `chapter_id` for `pair`, `job`, `suggestion` and `story_note`: without them a 1,285-Chapter Book scanned every Pair on each Chapter read. V3 adds `term.may_omit`. V4 (2026-11-16) adds `suggestion.kind` (`reader` or `drift`) and `job.model` ("ChatGPT · gpt-6-luna", recorded when a Job finishes).
- **Ids:** every table has an integer `id`, `forbidden_rendering` included, so a Revision can address any row the same way.
- **Ordering:** `ord` columns are REAL, so items can be inserted between two others without renumbering. Chapters with a number are inserted in number order.
- **Times:** unix seconds, INTEGER.
- **Text:** stored as the user sees it, in markdown. Rendering is `cipher-text`'s job.
- **Derived, not stored:** a Chapter's state (Empty, Waiting, Queued, Translating, Translated, Failed) comes from its source and translated Blocks and its latest translate Job, and a pending Proposed Edit whose Block no longer reads `before` is reported Stale on read.

## Revisions

Every write goes through a `Writer` that snapshots the whole row before and after as a JSON object of its columns, in `revision_change` (`before = NULL`: created; `after = NULL`: deleted). So undo has no per-table logic:
- it walks the Revision's changes backwards and restores each `before`;
- it only does so while the row still equals `after`. Otherwise later work touched it: a Conflict. A foreign key that blocks a restore is also a Conflict;
- `UndoMode::Whole` (structural Revisions: a Translation, an added Chapter) writes nothing if there is any Conflict; `UndoMode::SkipConflicts` (Reviews, made of independent Block edits) reverts the rest and reports the skipped rows, which the UI shows as conflicts;
- an undo is itself a Revision (kind `undo`), and the undone one gets `undone_by`. Undoing an undo is a redo, and restored rows keep their ids.

Covered by Revisions: chapter, pair, block, term, forbidden_rendering, style_rule, form_of_address, story_note, suggestion, proposed_edit. Outside them, as operational state: book_meta, job, publish_state.

Undoing an added Chapter removes its queued, never-started Jobs in the same transaction. Started Jobs or Jobs with dependents block that Undo; previews report those Jobs as conflicts. If the Chapter cannot be removed, its Jobs remain untouched.

`Book::preview_undo` runs the same Undo algorithm in an Immediate transaction using `SkipConflicts`, then always rolls it back. It returns the Revision, sorted affected Chapter ids, per-entity change counts, conflicts (including foreign-key-blocked restores) with Chapter locations, and any `already_undone_by`. It creates no Revision and changes no data.

`book_meta` keys: `title`, `source_lang`, `target_lang`, `created_at`, and, when the Book has chosen a model, `model_endpoint` (an Endpoint id from the user's `endpoints.json`) with `model`.

## Rules the schema does not enforce

`cipher-book` checks these in `apply`:
- Every Pair has at least one Block on each side; a Pair's source Blocks are contiguous and belong to its Chapter; a source Block is in at most one Pair. Source Blocks in no Pair are untranslated (a missing range).
- `SetTranslation` rewrites a Chapter's Translation and Pairing but keeps every translated Block whose text comes back unchanged (matched in order, a few Blocks ahead), and leaves a Pair untouched when it comes back the same. So re-pairing writes little, and Suggestions on unchanged Blocks survive a new Translation. Proposed Edits on a Block that is gone are deleted; a Suggestion made on one moves to the Chapter's first translated Block.
- `pair.flagged` values: `imported` (an import before 2026-11-16 paired by position; re-paired once, see `translation-pipeline.md` › Importing), `aligned` (an import paired locally), or a reason Repair could not fix the Pair. The first two are quiet in the UI.
- An accepted Proposed Edit is applied only while its Block still reads `before`. Otherwise it becomes `stale` and the Block is left alone. A Suggestion whose edits are all decided becomes `done`.
- Renaming a Term moves the old rendering into `forbidden_rendering` and removes the new one from it, in the same Revision.
- Editing a source Block marks the translated Blocks of its Pair `outdated`. Rewriting a translated Block clears `outdated`; a Hand Edit also sets `hand_edited`.
