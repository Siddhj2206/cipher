# AGENTS.md

Project-specific guidance for contributors working on `cipher`.

## Project
- `cipher` is a Rust CLI for translating chapter-based books with LLMs.
- Keep module boundaries clear. Do not mix provider, glossary, state, validation, and CLI concerns in one place.
- One provider per file in `src/translate/providers/`.

## Dependencies
- Add dependencies with `cargo add <crate>`.
- Prefer standard library solutions unless a crate clearly improves the result.

## Translation and Reruns
- Chapter flow is: load raw markdown, select glossary terms, translate, validate, repair if needed, write output atomically, merge glossary terms, save state.
- `smart` glossary injection is the canonical/default direction.
- Current rerun inputs are chapter source hashing and glossary-relevant tracked state.
- When changing rerun logic:
  - preserve determinism
  - preserve explainability
  - prefer exact tracked comparisons over approximation
  - keep rerun reason text concise and understandable

## State and Config
- Book config must stay portable and must not contain secrets.
- Default book layout uses `raw/`, `tl/`, `glossary.json`, `style.md`, and `.cipher/`.
- State changes should be additive where possible and formats should stay deterministic.

## Testing and Docs
- If you change rerun or state behavior, add targeted tests for it.
- Update `README.md` or `TODO.md` when behavior changes materially.

## Git
- Do not commit or push unless explicitly asked.
- Never commit secrets or API keys.
