# TODO

This file collects medium-term and long-term work that should be easier to revisit than the running notes in `PLAN.md`.

It combines:
- open items from the bottom of `PLAN.md`
- follow-up work from the recent rerun / retranslate review
- a few product-direction notes so future implementation choices stay coherent

---

## Guiding direction

The desired long-term shape of `cipher` is:

- `smart` glossary injection is the canonical/default mode
- reruns become reliable and explainable
- translation acceptance and glossary extraction are treated as separate concerns
- chapter state captures enough input fingerprints to tell when output is stale
- `--rerun` eventually becomes the common user-facing flow
- narrower flags like `--rerun-affected-*` remain available for advanced use
- `--overwrite` remains the "redo everything under a new regime" option

---

## Priority 1: Rerun correctness and predictability

### 1. Fix smart-mode rerun detection for newly relevant glossary terms
**Status:** Done  
**Why it matters:** Current tracked smart-mode rerun logic catches changed fingerprints for previously recorded terms, but it can miss terms that were not selected before and would be selected now.

**Example case:**
- chapter was translated before a glossary term existed
- later another chapter adds that glossary term
- chapter text would now match the new term
- rerun planning should detect that this chapter is now affected

**Desired outcome:**
- tracked smart-mode chapters rerun when the effective smart selection changes, not only when previously tracked terms change

**Possible implementation directions:**
- compare historical selected-term fingerprints against a recomputed current selection
- or extend exact rerun logic so it can detect selection-set expansion/contraction
- avoid relying only on `usage.terms` and `exported_terms`

**Acceptance ideas:**
- test: new glossary term added later now forces rerun for a previously tracked smart chapter
- test: removed glossary term also forces rerun if selection changes

---

### 2. Move rerun planning away from one-shot startup planning
**Status:** Done  
**Why it matters:** The current rerun plan is built once at the beginning of a run. If a chapter adds glossary terms mid-run, later decisions are not updated during that same invocation.

**Questions to resolve:**
- Should planning be recomputed after every successful chapter that mutates glossary state?
- Should recomputation apply only to remaining chapters, or to all chapters?
- Do we want eventual convergence in a single run, or accept multi-run convergence?

**Implementation options:**

#### Option A: Forward-only incremental replanning
After a chapter succeeds and adds glossary terms:
- recompute rerun decisions for remaining unprocessed chapters
- simple and likely good enough for most books

**Pros:**
- much better than one-shot planning
- easier to implement
- lower risk of loops

**Cons:**
- can still miss earlier chapters affected by terms discovered later

#### Option B: Worklist / fixpoint rerun engine
Maintain a queue of chapters to process:
- translate chapter
- if glossary changes, recompute affected chapters
- enqueue chapters whose effective inputs changed
- stop when queue is empty

**Pros:**
- most correct model
- naturally supports convergence

**Cons:**
- needs stronger state/fingerprinting to avoid repeated pointless reruns
- more complex to explain and implement

**Current recommendation:**
- first implement forward-only incremental replanning
- later evolve to worklist/fixpoint once chapter input fingerprints are stronger

---

### 3. Design `--rerun-affected-chapters`
**Status:** Planned  
**Why it matters:** Glossary changes are only one source of stale output. Chapter source content changes should also have a clean rerun path.

**Core idea:**
- store chapter source-content hash in chapter state
- compare current chapter content against stored hash
- rerun if content changed

**Likely scope for v1:**
- hash normalized raw markdown content only
- do not yet include provider/model/prompt/style-guide fingerprints

**Acceptance ideas:**
- modify chapter content without touching glossary
- rerun flag detects chapter as stale
- unchanged chapter is skipped predictably

---

### 4. Design `--rerun`
**Status:** Planned  
**Why it matters:** Most users will likely want one simple rerun flag rather than many narrow options.

**Proposed meaning:**
`--rerun` = rerun if either:
- chapter content changed
- glossary-relevant inputs changed

**CLI family proposal:**
- `--rerun-affected-chapters`
- `--rerun-affected-glossary`
- `--rerun` = union of both
- keep `--overwrite` separate for "new prompt/model/provider regime"

**Notes:**
- provider changes, model changes, and major prompt-template changes are probably better served by `--overwrite`
- avoid overloading `--rerun` with too many causes in the first version

---

### 5. Plan rerun preview mode (`--dry-run`)
**Status:** Planned  
**Why it matters:** Users should be able to inspect what would rerun before committing time/cost.

**Desired output:**
- changed glossary term count
- chapters affected by glossary
- chapters affected by content changes
- whether decision was exact or approximate
- reason text per chapter
- totals by category

**Open questions:**
- Should preview include "would use smart/full injection mode"?
- Should it display tracked vs untracked status?
- Should preview be available for both `translate` and `status`?

---

### 6. Improve reason text for rerun decisions
**Status:** Done  
**Why it matters:** Current reason strings are useful but still rough, especially when explaining smart-mode changes.

**Possible improvements:**
- distinguish:
  - changed imported terms
  - changed exported terms
  - newly relevant terms
  - removed terms
  - fallback-to-full transitions
- surface mid-run replanning decisions clearly in logs:
  - when remaining chapters are replanned after glossary updates
  - updated affected-chapter counts for the remaining queue
  - warnings produced by incremental replanning
- avoid repetitive or duplicate key names
- make reasons suitable for both logs and future dry-run output

**Goal:**
- a chapter rerun reason should be understandable without reading code

---

### 7. Plan status visibility for tracked vs untracked chapters
**Status:** Planned  
**Why it matters:** Some rerun logic uses exact tracking and some uses approximation. Users should be able to see which chapters are fully tracked.

**Possible status fields:**
- tracked smart selection
- tracked full selection
- approximate fallback used
- exported terms recorded
- source hash recorded
- last effective injection mode

**Possible user-facing output:**
- `tracked`
- `partially tracked`
- `approximate`
- `untracked legacy state`

---

### 8. Plan future verbose mode for detailed skip output
**Status:** Planned  
**Why it matters:** Skip behavior is correct but often opaque.

**Useful verbose details:**
- skipped because output exists
- skipped because chapter content unchanged
- skipped because glossary inputs unchanged
- skipped because no rerun reason matched
- skipped empty chapter
- skipped due to current flag combination

---

## Priority 2: Repair flow redesign

### 9. Redesign validation/repair into a cleaner pipeline
**Status:** Open  
**Why it matters:** The current repair flow works, but it mixes multiple responsibilities and makes state semantics harder to reason about.

**Current problem shape:**
- initial response tries to provide translation and new glossary terms
- if validation fails, a repair request is made
- repaired response becomes the accepted response
- this blurs ownership of `new_glossary_terms`

**Recommended new pipeline:**
1. Generate translation
2. Validate translation
3. If needed, repair translation only
4. After translation is accepted, extract glossary terms separately

**Benefits:**
- translation acceptance becomes independent from glossary updates
- repair can be narrowly scoped to formatting/validation fixes
- easier to test and reason about
- clearer state semantics

---

### 10. Split glossary extraction from translation response
**Status:** Open  
**Why it matters:** Glossary extraction is a different concern from chapter translation.

**Potential design:**
- translation endpoint/prompt returns only accepted chapter content
- glossary extraction runs as:
  - a second model call, or
  - a future deterministic extractor, or
  - an optional post-processing step

**Questions to resolve:**
- Is glossary extraction mandatory for every successful chapter?
- Should extraction be skipped on retries/fallbacks?
- Can glossary extraction run against accepted translation + raw source together?

**Tradeoff:**
- more requests vs much cleaner behavior

---

### 11. Narrow repair semantics
**Status:** Open  
**Why it matters:** Repair should not accidentally behave like a fresh retranslation.

**Potential repair contract:**
- fix structure only
- preserve meaning/content as much as possible
- do not invent glossary terms
- do not rewrite tone/style beyond what is needed for validity

**Future prompt design goals:**
- minimal patch framing
- explicit list of validation failures
- avoid "rewrite the whole chapter" behavior unless absolutely necessary

---

### 12. Revisit validation strictness after repair redesign
**Status:** Open  
**Why it matters:** Validation should catch genuinely bad outputs without fighting legitimate prose.

**Future work:**
- separate hard failures from warnings
- identify cases that can be auto-cleaned locally
- evaluate whether heading checks should remain strict or become configurable

---

## Priority 3: Smart/full glossary mode strategy

### 13. Decide long-term role of `full` mode
**Status:** Open  
**Why it matters:** `full` currently exists, but the intended future is clearly centered around `smart`.

**Possible strategies:**

#### Option A: Soft-deprecate `full`
- keep it supported
- warn that tracking/rerun quality is optimized for `smart`
- simplest path

#### Option B: One-time migration to smart
- if old baseline is `full` and current config is `smart`, trigger one-time normalization reruns
- after that, book becomes smart-native

#### Option C: Treat `full` as non-canonical/emergency mode
- allow `full` for current run
- do not let `full` redefine canonical tracked baseline
- `smart` remains the only mode that drives long-term rerun state

**Current recommendation:**
- strongly consider Option C if simplification is the priority

---

### 14. Review mode-switch behavior explicitly
**Status:** Open  
**Why it matters:** Even if `full` stays supported, switching modes changes prompt inputs materially.

**Need to decide:**
- should switching `smart <-> full` trigger reruns?
- should only one direction trigger reruns?
- should full-mode runs avoid advancing canonical baseline entirely?

**Note:**
This should be answered intentionally rather than emerging accidentally from state logic.

---

## Priority 4: State model evolution

### 15. Expand chapter state to store chapter content hash
**Status:** Planned  
**Why it matters:** Required for `--rerun-affected-chapters` and foundational for better rerun logic.

**Likely fields:**
- `source_hash`
- maybe later `source_size` or `source_mtime` for diagnostics
- keep hash authoritative, not mtime

---

### 16. Consider future "effective input fingerprint" per chapter
**Status:** Future  
**Why it matters:** If rerun logic becomes worklist/fixpoint-based, the engine needs a stable way to decide whether a chapter has already been translated under equivalent inputs.

**Potential future inputs:**
- chapter source hash
- selected glossary term fingerprints
- style guide hash
- injection mode
- prompt version
- maybe provider/model identity

**Important caution:**
Do not overcomplicate the first version of `--rerun`. Start narrow.

---

### 17. Revisit exported-term tracking semantics after retranslation
**Status:** Partially explored  
**Why it matters:** A chapter may export glossary terms that later get imported through normal smart selection, and retranslation can change which association should be preserved.

**Questions to answer explicitly:**
- Should `exported_terms` represent only terms newly added by the last successful run?
- Or all terms the chapter "claimed" semantically, even if already present?
- If a rerun emits no new terms, should old exported associations be cleared or retained?

**Need:**
- settle semantics before tightening rerun logic further

---

## Priority 5: UX, observability, and product direction

### 18. Better way to store API keys
**Status:** Open  
**Why it matters:** Current storage works, but security and ergonomics can improve.

**Ideas to explore:**
- OS keyring / secret service integration
- env-var indirection
- encrypted local storage
- profile-level references to key labels only
- explicit import/export story that never leaks secret material

**Questions:**
- What should `cipher profile new` feel like if secrets are not stored in plain config?
- How portable should key configuration be across machines?

---

### 19. Better `profile new` styling and layout
**Status:** Mostly done, but keep room for follow-up  
**Why it matters:** First-run UX matters a lot.

**Possible future polish:**
- clearer defaults
- cleaner summaries before saving
- better distinction between provider creation vs provider reuse
- more obvious key-selection flow

---

### 20. Evolve `cipher` beyond novel translation
**Status:** Open  
**Why it matters:** The architecture may eventually support documentation translation or other structured markdown workflows.

**Questions to explore:**
- what assumptions are novel-specific today?
- are glossary/style concepts general enough for docs?
- should chapter discovery generalize to sections/pages/doc trees?
- do validation rules need profiles by content type?

**Goal:**
- keep core abstractions broad enough that future expansion is possible without bloating current UX

---

### 21. Update `rig-core`
**Status:** Done  
**Why it matters:** Keep provider behavior current and reduce future compatibility debt.

**Checklist when doing this:**
- confirm extractor behavior still matches expectations
- confirm OpenAI vs OpenAI-compatible differences remain handled correctly
- rerun schema/structured-output checks
- watch for response API changes

---

## Nice-to-have implementation cleanup

### 22. Improve status/reporting around skipped-but-previously-successful chapters
**Status:** Future  
**Why it matters:** A chapter may be skipped this run but still represent a successful prior translation. Status output should remain intuitive.

**Question:**
- Should "skipped this run" and "last successful translation state" be represented separately?

---

### 23. Revisit glossary matcher caching only if performance becomes real-world issue
**Status:** Deferred unless needed  
**Why it matters:** Current smart selection is working; do not optimize prematurely.

**Only do this if:**
- large books show measurable slowdown
- profiling confirms matcher rebuild cost matters

---

## Suggested implementation order

1. Fix smart-mode rerun detection for newly relevant terms
2. Move rerun planning to incremental replanning
3. Add chapter source hashing
4. Implement `--rerun-affected-chapters`
5. Implement `--rerun`
6. Add dry-run preview
7. Improve reason text and tracked/untracked visibility
8. Redesign repair flow
9. Decide final long-term role of `full`
10. Revisit exported-term semantics if needed

---

## Notes from previous ad-hoc TODOs

These were carried over from `PLAN.md` and are now represented above:

- [x] Update chapter state checks even when glossary state has not changed
- [ ] Work on a better way to store API keys
- [x] Better `profile new` styling and layout
- [ ] Smart checks chapter by chapter instead of only once at startup
- [ ] Evolve `cipher` beyond novel translation
- [ ] Retranslating chapters with new content
- [x] Update `rig-core`
- [x] Figure out full glossary rerun issue
- [x] Better reason text
- [x] Check saving/comparing exported glossary terms from chapters
- [ ] Plan rerun preview mode (`--dry-run`)
- [ ] Plan status visibility for tracked vs untracked chapters
- [ ] Plan future verbose mode for detailed skip output

---

## Optional follow-up fixes

### 24. Flatten structured-output schema for Nvidia / OpenAI-compatible providers
**Status:** Optional  
**Why it matters:** Some OpenAI-compatible endpoints reject structured-output schemas that contain `$defs` references, which currently breaks glossary extraction for providers like Nvidia.

**Observed failure:**
- `HTTP 400 Bad Request`
- `Grammar error: Pointer '/$defs/GlossaryTerm' does not exist`

**Current understanding:**
- `TranslationResponse` includes `new_glossary_terms: Vec<GlossaryTerm>`
- the generated JSON schema uses a `$ref` to `#/$defs/GlossaryTerm`
- rig's OpenAI-compatible extractor path sends that schema as-is
- Nvidia appears not to support that reference format in this path

**Possible fix:**
- flatten or inline `$defs` references before sending the schema on OpenAI-compatible provider paths
- keep the existing path for providers that already accept the current schema

**Acceptance ideas:**
- regression test: provider schema for `TranslationResponse` contains no `$ref` to `#/$defs/GlossaryTerm` on the Nvidia/OpenAI-compatible path
- manual check: a chapter translate request succeeds on Nvidia without the 400 grammar error

---

### 25. Pass provider `extras` through to rig `additional_params`
**Status:** Optional  
**Why it matters:** `ProviderConfig` already has an `extras` JSON field, but `cipher` currently ignores it. Passing it through would expose newer rig/provider-specific controls without expanding `cipher`'s own config surface first.

**Current understanding:**
- `ProviderConfig.extras` exists in `src/config/mod.rs`
- provider construction currently threads only API key and model into `ProviderParams`
- rig `0.34` extractor builders support `.additional_params(serde_json::Value)`
- both OpenAI/OpenAI-compatible and Gemini provider paths can consume provider-specific raw JSON through that hook

**Possible implementation:**
- add `extras: Option<serde_json::Value>` to `ProviderParams`
- clone `provider_config.extras` into provider construction
- apply `.additional_params(extras)` conditionally on OpenAI/OpenAI-compatible and Gemini extractors
- document this as an advanced provider-specific passthrough with no schema normalization by `cipher`

**Acceptance ideas:**
- config/provider build test: providers still resolve cleanly when `extras` is present
- manual check: OpenAI/OpenAI-compatible extras reach rig Responses/Completions requests
- manual check: Gemini extras such as `generationConfig` are accepted through the extractor path
