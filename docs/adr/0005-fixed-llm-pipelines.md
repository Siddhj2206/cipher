# Fixed LLM pipelines, not an agent

Every LLM use in Cipher is a fixed pipeline: local preparation, one call (or a small fixed number of calls) with a known input and output shape, then local checks. There are no agent loops or tool calls, and no free-form chat. Anything a substring search, a counter or a diff can do runs locally first and costs nothing.

## Decisions

- **Translation:** one call per Chapter.
  - Input and output are lines numbered `[n]`, with `[n-m]` for merged Blocks. This gives Pairs directly and survives a truncated stream.
  - New names and two or three sentences of Story Notes come back in a trailer on the same response, so there is no separate extraction or summary call.
- **Selective context:** only Terms whose source form occurs in the Chapter are sent. On the test Book that is about 200 tokens, against about 10k for the whole Glossary.
- **Checks are local**, for example looking for Forbidden Renderings. Repair calls go out only for what cannot be fixed by a replace, and only with the failing Pairs.
- **Renames are applied locally,** with no LLM call. Only Blocks that rendered the expression some other way go to the model.
- **Order:** Chapters translate one at a time in order, because Story Notes and Pending Terms depend on earlier Chapters.

The full design is `docs/design/translation-pipeline.md`.

## Considered Options

- **An agent with tools (search the Book, edit Blocks, update the Glossary):** rejected. It is far more expensive per fix, behaves differently every run, and hides what changed. Here a Suggestion's reach is known before spending anything.
- **JSON output:** rejected. On the o200k tokenizer, JSON keys cost about 720 tokens per Chapter against 576 for `[n]` markers, and a truncated JSON response is unrecoverable.
- **Sending the whole Glossary to use prefix caching:** rejected. The stable prompt prefix is too short to benefit, and 224 Terms dilute cheap models.
- **Batch-API backlog mode:** rejected. Chapters translated in parallel each invent their own rendering for the same new name, and only OpenAI's own API offers it.

## Consequences

- Prompt and parser formats are a compatibility surface: a format change must keep old Jobs resumable or fail them clearly.
- Weak models will drift from the marker format, so the parser is lenient and missing ranges are re-requested on their own.
- New capabilities must be phrased as a pipeline step with a known input and output, never as "let the model figure it out".
