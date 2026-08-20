# Architecture audit — August 2026

Full audit of `src/` (12,645 LOC, 40 files), `Cargo.toml`, `CONTEXT.md`, `README.md`, `TODO.md`, `docs/adr/0001-0003`. 219 tests. Zero `unsafe`, zero TODO/FIXME markers in `src/`.

## Strengths

- Clean module tree: `book/`, `config/`, `glossary/`, `state/`, `translate/{providers,rerun}/`, `validate/`, `ui/`; one provider per file.
- 219 tests, strong coverage on the sensitive logic (decisions 18, rerun/glossary 10, baseline 9, orchestrate 17).
- ADR-0003 error discipline holds (E001-E007, exit codes tested).
- stdout/stderr discipline: JSON to stdout, human to stderr.
- Rerun determinism preserved: exact fingerprint comparisons, `is_approximate` flags, concise reasons.
- All 14 dependencies used and justified. Release profile production-minded.
- Future seams proven: Provider trait + StructuredExtractor (Cohere added without disruption).

## Boundary violations (all circular)

| Location | Problem |
|---|---|
| `state/mod.rs:4` | imports `crate::translate::TranslationUsage` — persisted schema type lives in translate while translate imports state everywhere |
| `error.rs:6` | imports `crate::config::ProviderKind`; config imports error — error module knows a domain enum |
| `config/cli.rs:5`, `glossary/cli.rs` | CLI dispatch handlers live inside domain modules, depend on root clap types — CLI concern spread across 4 modules |

## God modules

- `translate/orchestrate.rs` — 1,430 lines
- `translate/rerun/decisions.rs` — 1,205 lines
- `translate/cmd.rs` — 788 lines, `iterate_translation` ~17 args (`:346`), `finalize_run` ~17 args (`:548`)
- `translate/rerun/baseline.rs` — 729 lines
- 9 x `#[allow(clippy::too_many_arguments)]` total: `cmd.rs:346,548`, `orchestrate.rs:118,206`, `state/mod.rs:165` (14 args), `baseline.rs:17,88`, `config/profile.rs:37`, `lib.rs:277`

## Prioritized problems

| # | Problem | Location | Effort |
|---|---------|----------|--------|
| 1 | `state`<->`translate` circular boundary; `TranslationUsage` schema type lives in translate | `state/mod.rs:4`, `translate/types.rs:42` | Medium |
| 2 | Chapter layout inconsistency: non-recursive discovery, recursive state collection, filename-flattening output path -> nested same-named chapters clobber each other's output (data loss) | `book/paths.rs:136-150`, `state/mod.rs:306-315` | Small |
| 3 | God modules/functions; 9 x too_many_arguments (17-arg functions) | `orchestrate.rs`, `decisions.rs`, `cmd.rs`, `baseline.rs`, `state/mod.rs:165` | Large |
| 4 | State version constants written, never validated; no migration machinery — breaking schema change silently yields garbage | `state/mod.rs:10-11` | Medium |
| 5 | Secrets plaintext at default file permissions (no 0600) | `config/mod.rs` write path, `io.rs:4-20` | Small |
| 6 | `ChapterResult` middle-man duplicating `ChapterState` fields; 12-arg ctor | `orchestrate.rs:104-118` | Small |
| 7 | Global output statics (QUIET/VERBOSE/JSON) + raw ANSI instead of injectable reporter | `output.rs`, `lib.rs:357-359`, `cmd.rs:410,436` | Medium |
| 8 | Exit-code collision: clap usage errors and E001 both exit 2 | `error.rs:18-24` | Small |
| 9 | Error envelope for all commands located in `translate/report.rs` — cross-cutting CLI contract mislocated | `lib.rs:405`, `cmd.rs:59`, `report.rs:213` | Small |
| 10 | Rerun comparison logic duplicated (exact decision vs chapter-matches-glossary) — divergence on most sensitive logic | `decisions.rs:25-110`, `rerun/glossary.rs:147-186` | Medium |
| 11 | `exit(0)` on user cancel + E007 for logic errors in interactive layer — aborted runs look successful to scripts | `ui/interactive.rs:30,59,248` | Small |
| 12 | Doctor silently ignores broken `cipher.toml` (`unwrap_or_default`); returns `()` so it can never fail | `book/doctor.rs:81,167`, `lib.rs:321` | Small |
| 13 | `profile new --set-default` requires `--set-default true`; README documents bare flag | `lib.rs:197`, `config/profile.rs:45,65` | Trivial |
| 14 | `RunOptions` persisted but never read back — dead state surface | `cmd.rs:266`, `state/mod.rs:32` | Trivial |
| 15 | JSON output shapes inconsistent across commands (envelope only on translate failure; bare objects elsewhere) | `report.rs` vs `ui/status.rs:21`, `glossary/cli.rs:28`, `config/profile.rs:205,250` | Medium |

## Honorable mentions

- `InjectionMode::from_str` dead code with stderr side effect (`glossary/mod.rs:29-48`)
- Skip reason stored in `ChapterState.error` never surfaced in `--json`
- `ProviderKind` in config coupled into `error.rs`
- `process::exit(0)` inside library code (`ui/interactive.rs:30`)
- `Error::Clone` drops `raw_os_error` from Io errors (`error.rs`)
- Split validation concern: `validate/mod.rs` + `book/output.rs::validate_structured_chapter` concatenated in `orchestrate.rs:412-418`
- `TranslationOnlyResponse` lenient optional-string fields; `EXTRACTOR_RETRIES = 1`
- OpenAI has `use_completions_api` dual path inside one struct; no HTTP-level mock tests

## Testing gaps

- No CLI integration tests (`lib.rs` has 1)
- status 2, doctor 2, io 2, backup 2 tests
- Interactive `exit(0)` path untestable
- Provider network paths (chat vs completions) only exercised against real endpoints