# AGENTS.md

Project-specific guidance for contributors. Domain definitions live in `CONTEXT.md`.

## Project
- Keep module boundaries clear. Do not mix provider, glossary, state, validation, and CLI concerns in one place.
- One provider per file in `src/translate/providers/`.

## Dependencies
- Add dependencies with `cargo add <crate>`.
- Prefer standard library unless a crate clearly improves the result.

## Rerun logic
- preserve determinism
- preserve explainability
- prefer exact tracked comparisons over approximation
- keep rerun reason text concise and understandable

## State and Config
- Book config must stay portable and must not contain secrets.
- State changes should be additive where possible and formats should stay deterministic.

## Testing and Docs
- If you change rerun or state behavior, add targeted tests.
- Update `README.md` or `TODO.md` when behavior changes materially.

## Agent skills

- **Issue tracker**: GitHub issues. External PRs are not a triage surface. See `docs/agents/issue-tracker.md`.
- **Triage labels**: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.
- **Domain docs**: Single context — `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.

## Git
- Do not commit or push unless explicitly asked.
- Never commit secrets or API keys.
