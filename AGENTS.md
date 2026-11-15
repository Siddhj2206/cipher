# AGENTS.md

Cipher is being rebuilt from scratch as a desktop app for reading and translating books with LLMs. The old CLI is in git history only; reuse nothing from it unless a new decision brings it back.

## Where things are

- **Resuming or starting work**: `docs/plan.md`. It holds the current milestone, the next task, open questions and notes. Update it before you stop.
- **Domain words**: `CONTEXT.md`. Use its terms in code, tests and docs.
- **Decisions**: `docs/adr/`. Flag any change that contradicts one.
- **Crates, module interfaces and building**: `docs/design/architecture.md`.
- **SQLite schema**: `docs/design/storage.md`.
- **Prompts, LLM calls, checks, Suggestions or Endpoints**: `docs/design/translation-pipeline.md`.
- **Anything visual or interactive**: `docs/design/desktop-ux.md`, with screenshots in `docs/design/img/`. The prototype in `prototypes/editor-spike/` is the visual reference (`cargo run --release`, or `--snap out.png --scene NAME`). Its code structure is throwaway.
- **Packaging, CI and releases**: `docs/releasing.md` for the process; the files are in `packaging/` and `.github/workflows/`, described in `docs/design/architecture.md` › Packaging and platforms.
- **Test data**: `Test-Books/S-Class-Transmigrator` (gitignored) holds 94 real Chapters (`raw/`, `tl/`) and `glossary.json`. Use it for manual runs and measurements; tests use checked-in synthetic fixtures.

## Keeping the docs true

Each fact has one home: the list above. When work changes a design, a schema, a term or a decision, edit its home in the same change, so the next session starts from the truth.

## Agent skills

- **Issue tracker**: GitHub issues. External PRs are not a triage surface. See `docs/agents/issue-tracker.md`.
- **Triage labels**: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.
- **Domain docs**: Single context — `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.

## Git
- Commit or push only when explicitly asked.
- Never commit secrets or API keys.
