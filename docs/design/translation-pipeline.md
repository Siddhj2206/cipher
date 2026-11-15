# Translation pipeline

How Cipher turns source Chapters into Translations and applies Suggestions. Agreed design; the decision record is ADR 0005, and connections are ADR 0006. Terms follow `CONTEXT.md`.

Principles:
- Fixed pipelines, never an agent loop. Every LLM call has one job, a known input and a known output shape.
- Local work first. Anything a substring search, a counter or a diff can do costs zero tokens and runs before any call.
- Spend tokens where quality comes from: Term notes, gender, and honorific policy. Save them where nothing is lost: irrelevant Terms, repeated work, and hidden reasoning.
- Every step is a resumable job row in the Book's SQLite file. Closing the laptop mid-call loses at most that call.

Sizes from the test Book (S-Class Transmigrator, 94 Chapters):

| | Median | Max |
|---|---|---|
| Source Chapter | 4.9k Korean characters, 144 paragraphs, 3.2k tokens | 3.8k tokens |
| Translated Chapter | 1,750 English words, 2.4k tokens | 5.4k tokens |
| Whole Glossary | 224 Terms, 10k tokens as JSON | |
| Terms whose source form appears in a Chapter | 20, about 200 tokens as `source = rendering` lines | 45, about 430 tokens |
| `[n]` markers per Chapter | 576 tokens | |
| JSON keys or `⟦n⟧` markers instead | 720 / 1,152 tokens | |

Token counts use OpenAI's o200k tokenizer, measured 2026-11-11.

## Terms carry what the checks need

Each Term holds:
- source form;
- rendering;
- kind;
- for characters, gender;
- a short note;
- **Forbidden Renderings**: renderings that must not appear.

Forbidden Renderings fill themselves in. Renaming a Term moves the old rendering there, and rejecting a Proposed Edit's wording can add it too. They make the compliance check precise and give Repair an exact instruction.

Honorifics and forms of address are not Terms. They are one Style Rule for honorific policy plus a small "A addresses B as X" table, both sent with every Chapter.

## Translating a Chapter

### 1. Prepare (local)

- Split the source into numbered Blocks.
- Select Terms whose source form occurs in the Chapter. This matcher favours recall: a wrongly included Term costs about 40 characters.
- Include Pending Terms as provisional renderings, so a new name stays consistent across Chapters even before the user accepts it.
- Gather:
  - Style Rules and the address table;
  - Story Notes, to a fixed budget;
  - the last few translated Blocks of the previous Chapter, for continuity.

Sending only the Terms a Chapter uses is about 200 tokens instead of about 10k for the whole Glossary. It also keeps cheap models from being diluted by 224 entries.

### 2. Translate (one call)

The source goes out as ASCII-numbered lines, and the translation comes back the same way:

```
[12] 세 명의 강자를 보유하고 있던 ...
[13] ...
```

```
[12] The Ragnaype Republic of Magic, which once ...
[13-14] (two source Blocks rendered as one)
```

- **Merges:** `[n-m]` marks a merge. There is no split marker: a split is a line break inside one Block.
- **Pairs:** the numbering gives Pairs directly, which is the only reliable way to get them. Source and translation paragraph counts differ (164 vs 159 in one Chapter).
- **Parsing:** lenient, accepting `[12]`, `12.`, `【12】` and stray spaces.
- **Why ASCII:** plain ASCII markers are cheaper than JSON or rare glyphs, by several hundred tokens per Chapter, mostly on output.

After the body comes a short trailer, which saves a separate extraction call and a separate summary call:

```
NEW_TERMS:
아르젠트 | Argent | name | male
NOTES:
Tesselid's forces enter the gate; the Magic Corps vanguard is swallowed.
```

The response streams into the job row. On a cut-off, the last Block is discarded, because there's no telling whether it finished. The next run asks only for the remaining numbers.

The prompt keeps stable instructions first, which lets prefix caching work where it can. The stable part is likely under the 1,024-token minimum, though, so caching is a bonus, not a design goal.

### 3. Check (local)

- **Coverage:** every source number appears exactly once, counting merges. Missing ranges are re-requested with two neighbouring Blocks for context, never the whole Chapter.
- **Leftover script:** no Korean, Japanese or Chinese script remains.
- **Wrong renderings:** a Forbidden Rendering, or a Pending Term's competing variant, appears in a Pair whose source names that Term. This looks for something wrong being present rather than the right rendering being absent. Korean repeats names where English says "he", so an absence check would flag 10–20% of correct Pairs.
- **Missing names (Chapter level only):** the source names a Term three or more times and the translation never renders it. Nothing is repaired automatically; it shows as Drift.
- **Length:** the ratio is checked only for Blocks over about 80 characters, to catch omissions and repetition loops without noise from short dialogue.

The checking matcher favours precision:
- the longest match wins;
- a short Hangul source form must have a non-Hangul character on its left and a particle or non-Hangul character on its right, so 한 doesn't match inside 한국.

### 4. Repair

- **Wrong renderings:** fixed by a local replace, at zero tokens, and shown to the user as an applied fix they can undo.
- **Missing ranges:** re-requested in the translate step itself (up to 4 calls per Chapter), with the two neighbouring translated Blocks as context. Numbers still missing after that leave their source Blocks unpaired and fail the Job with the list.
- **Leftover script and implausible length:** one small-model call with only those Pairs and the exact problem, re-checked; a Pair that still fails is flagged. (So: one repair attempt, then the flag.)
- **Revisions:** the Translation (with its Pending Terms and Story Note) is one Revision; the local fixes, the repairs and the flags are each a Revision of their own, so each can be undone.

### 5. Learn (local)

- NEW_TERMS become Pending Terms. A new one is deduplicated against the Glossary by containment, so "Duke Tesselid" is caught against "Tesselid".
- NOTES are appended to Story Notes after the same wrong-rendering check, so an old name can't re-enter the prompt through notes.
- Every so often, one call compacts Story Notes back to their budget.

### Cost

Estimated per Chapter:
- Input: about 4.5k tokens (source with markers 3.8k, Terms 0.2k, Story Notes and Forms of Address 0.4k, instructions).
- Output: about 3k tokens (translation 2.4k, markers about 0.5k, trailer).

Output costs 2–5× more than input per token, so the output side is what to watch. Input spent on Term notes and gender pays for itself.

**Reasoning tokens are the hidden cost.** A thinking model can spend several thousand unseen output tokens per Chapter, so each Endpoint exposes reasoning effort, defaulting to low or off. This needs testing on the ChatGPT-plan route, which may not allow it to be set.

The UI shows the estimate before a run and real usage after each Chapter.

### Queue

Chapters translate in order, because Story Notes and Pending Terms depend on what came before. There is no parallel or batch-API mode: chapters translated in parallel each invent their own rendering for the same new name.

### Job states

Each Chapter's translation is one Job row: `queued → running → checking → done`, or `failed` with a reason, or `paused` when the Endpoint reports a usage limit. A finished Job records its tokens and the Endpoint and model that did it (`job.model`).
- **Streaming to the page:** as each paragraph completes, the worker sends `Event::Streamed` with every finished entry so far (accepted ones plus the parsed partial, its last incomplete entry dropped), and the page shows it above its source.
- **Saved as it streams:** the current call's raw stream is saved to `job.partial` every 2 seconds; entries already accepted, the trailer and token counts are JSON in `job.input`. Resuming parses the partial with its last entry dropped and asks only for the missing numbers.
- **Repair** runs inline at the end of the Chapter's Job rather than as a child Job: it is one small call, and the Chapter is already written when it starts.
- **Retries:** network and server errors pause the Job for 10 s × attempts, rate limits for the server's Retry-After (30 s if none), up to 4 attempts; then the Job fails. Usage limits pause until the reset time (an hour if unknown) without counting. Bad credentials and rejected requests fail at once.

## Applying a Suggestion

### Here

One call: the Pair (source and current translation) plus the user's words return new text for that Block.

### Everywhere

1. **Resolve the source expression, locally first.**
   - If the Selection is a Term's rendering, the source form is known.
   - Otherwise, take every Pair whose translation contains the Selection. Pick the source n-gram that is frequent in those Pairs and rare elsewhere. This works when the phrase occurs three or more times.
   - Only one-offs need a tiny call (a `resolve_source` Job, small model). Its answer must be a literal substring of the source Block, given without particles, or it is ignored and the Suggestion keeps only its translation-side candidates.
2. **Find candidates (local).** Candidates are Pairs whose source contains the expression plus Pairs whose translation contains the Selection. The second set is free and catches requests like "stop writing 'let out a breath'", which have many source forms.
3. **Show the reach before spending.** The fix box already shows "212 Blocks in 80 chapters", so the user can spot a bad resolution at zero cost.
4. **Renames are deterministic.** The user marks a request as a rename in the fix box; no classifier call. Cipher replaces the old rendering locally with care for:
   - word boundaries and case;
   - possessives;
   - "a"/"an" agreement;
   - partial names (full name renamed, given name used alone).

   Only candidates that rendered the name some other way go to the model.
5. **Batch the rest.** Remaining candidates go out 12 Pairs per call (`suggest_batch` Jobs), since cheap models drop ids in larger batches. Each comes back as `[n] new text` or `[n] SKIP: reason`; a missing answer or an unchanged text is recorded as a skipped Proposed Edit with its reason. The Suggestion turns Ready when its last Job ends.
6. **Review, grouped.** Exact local replacements are one group with a few samples and a single Accept all. Model-written edits are reviewed one by one inline. Three hundred individual inline edits would be unusable.

### From now on

Also writes the Term (moving the old rendering to Forbidden Renderings) or the Style Rule. It applies the same rename to Story Notes. A Rename whose Selection is not yet a Term creates one from the resolved source expression and forbids the old wording; a non-Rename adds a Style Rule `“quote”: what the user said`.

### Stale edits

A Proposed Edit whose Block changed after it was proposed is stale. It is never applied over the newer text.

## Drift (local, free)

Drift is shown on the Cast & terms page:
- a Forbidden Rendering is still present;
- or a Term is named three or more times in a Chapter's source but never rendered. Skipped for one-character sources and for Terms marked `may_omit` (term column, schema V3).

### Fixing Drift

"Fix in N chapters" (`cipher_engine::fix_drift`) makes one Everywhere Suggestion of kind `drift` over the Term's drifting Chapters, touching only Pairs whose source names the Term:
- **A Forbidden Rendering:** replaced word for word, as a Rename edit, with no model call. When the forbidden wording sits inside the rendering itself ("Tower Lord" in "Magic Tower Lord"), the Block goes to the model instead.
- **A Chapter that never renders the Term:** each of its Blocks that leaves the Term out goes to the model in the usual batches.
- **The model's request** is its own, not the reader's words: the glossary renders X as "Y"; name "Y" where the source names X and the translation calls it something else or leaves it out; keep a pronoun where English reads better; SKIP where the paragraph already names it or where X there is not this name (an ordinary word, another person). The last clause matters for short sources that are also common words: 아이 is both the name Ai and "child", and on the test Book the model now skips "treating a child" with that reason.
- **For the reader** the Suggestion says "Name “Y” wherever the source names X", is labelled FIXING DRIFT, and the Review bar reads "Naming “Y”". A skipped edit shows the model's reason in the bar.
- The user reviews it like any Suggestion. When nothing qualifies (the source no longer names the Term there), nothing is recorded. While a fix is open, the Term's panel says what it holds ("A fix is ready: 2 changes to review, 2 the model left as they were") and its button opens it instead of sending another.

"It can go unnamed" sets `may_omit`: absence stops counting, and Forbidden Renderings are still checked.

## Importing existing translations

An imported Chapter comes with a translation but no Pairs. `cipher_text::align` pairs them locally, at no token cost: a dynamic program over both paragraph lists in the manner of Gale and Church (1993). Each step takes 1–3 source paragraphs and 0–3 translated ones; its cost is how far the group's length is from the Chapter's own ratio, less what both sides share:
- a glossary Term named in the source with its rendering in the translation (the strongest signal);
- the same shape (both dialogue, both a bracketed system line);
- the same digits.

A source paragraph the translator dropped joins the Pair before it, so no imported Chapter reads as partly untranslated. The Pairs are flagged `aligned`.

Measured on S-Class Transmigrator (94 Chapters, 2026-11-16): of the names a translated paragraph says, 90% are named in its paired source, against 40% when paired by position (`cargo run -p cipher-text --example align_eval -- Test-Books/S-Class-Transmigrator`). About 16 ms a Chapter.

Books imported before this were paired by position (`imported`). The app re-pairs them once, in the background after the Book is counted (`cipher_engine::realign_imported`), as Revisions of 150 Chapters so no write holds the Book long; Genius Wizard's 1,278 Chapters take 21 s. Before re-pairing, Drift fixes and Suggestions on those Books sent the model mismatched paragraphs.

## Endpoints

- **Models:** one required default "translate" model, more models a Book may choose instead, and an optional cheaper "small" model. The small model handles Repair, one-off source resolution and Story Notes compaction.
- **Which a Book uses:** a Book may store a Model Choice (an Endpoint id and one of its models); the worker translates with that Endpoint and model (`Endpoints::for_book`), or with the default Endpoint's default model when the Book chose none or its choice no longer exists.
- **Wire protocols:** Chat Completions and Responses. Both are needed: some providers serve some models only over Responses, and so does a ChatGPT plan.
- **Reasoning effort:** set per Endpoint.
- **Credentials:** keys and tokens live in the OS keychain, never in a Book.

How an Endpoint connects (API key or Sign in with ChatGPT, and why not Claude subscriptions) is ADR 0006.

## Rules settled in code

The exact rules live in the doc comments of `crates/cipher-text`. The ones that differ from a naive reading:
- **Markers:** a bare `12.` counts only when 12 is expected; a duplicate keeps the first entry; an entry straddling the range is dropped whole; a cut-off drops its last entry, or its last trailer row once `NEW_TERMS:` has started.
- **Precision matcher:** a short Hangul form must end at a non-Hangul character or a chain of particles that does (`한에게는` matches, `한의사` doesn't).
- **Forbidden check:** an occurrence inside another named Term's approved rendering is not a hit.
- **Length check:** flags below 0.9 or above 4.0 translation/source characters for sources over 80 characters. Measured median 2.02 (p5–p95 about 1.4–2.6) on the 82 cleanly aligned long Blocks of the test Book; thresholds are untested against real omissions.
- **Rename:** a capitalised old rendering (a name) matches only its exact spelling and ALL CAPS; a lowercase phrase also matches sentence case and Title Case. Partial names are derived only when exactly one capitalised word of three or more letters differs.
- **Source resolution:** character n-grams (2–14) scored by F1 of recall and precision; the winner needs recall ≥ 0.6 and precision ≥ 0.5 and a 0.1 lead. On the test Book it found `마도 공화국` from 22 hits in 2.5 ms.

## Not doing

- **Agent loops or tool calls.**
- **Batch-API backlog mode:** inconsistent new names, and it only works on OpenAI proper.
- **A classifier call to detect renames:** a toggle in the fix box does it.
- **Split markers:** splits are line breaks inside a Block.
- **Model-based repair of glossary violations:** they are deterministic replaces.
