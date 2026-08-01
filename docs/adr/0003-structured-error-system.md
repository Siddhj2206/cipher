# Structured error system (E001–E007)

Error handling moves from untyped `anyhow::Error` strings to a typed, code-carrying `Error` enum. Every fallible path in the crate returns `crate::error::Result<T>` (`Error` as the error type), and the CLI boundary prints a stable error code and maps the code to a process exit code.

## Codes

| Code | Variant | Meaning | Exit code |
| ---- | ------- | ------- | --------- |
| E001 | `Config(String)` | Global or book config read/parse/serialize failed | 2 |
| E002 | `Io(#[from] io::Error)` | Filesystem or I/O failure; context carried in the message | 3 |
| E003 | `ProfileNotFound { name }` | A named profile could not be resolved | 5 |
| E004 | `Glossary(String)` | Glossary JSON read/parse/write failure | 6 |
| E005 | `Provider { kind, detail }` | Provider client construction or request failure | 4 |
| E006 | `Validation { message }` | Bad input or unsupported usage; message is the bare display | 1 |
| E007 | `State(String)` | Corrupt or inconsistent `.cipher` run state; also serde_json failures | 70 |

`cipher translate` additionally exits with code **8** (`PARTIAL_FAILURE_EXIT_CODE`)
when the run completes but one or more chapters failed — distinct from every
E00N error exit code so scripts can distinguish a completed-but-partially-failed
run from a fatal error.

## Design

- `Error::io(context, source)` attaches a path/action context to an `io::Error` at construction, preserving the underlying `ErrorKind` on the source so callers can match on failure kinds without parsing strings.
- `From<serde_json::Error>` maps JSON failures to `State` (E007): JSON in `.cipher` state files means corrupt state when it fails to round-trip. Glossary files map explicitly to `Glossary` (E004) instead.
- `E006` displays as the bare message (no `[E006]` prefix) so usage errors read naturally; all other variants prefix `[E00N]`.
- Suggestions are static per variant and rendered by the CLI boundary as `suggestion: ...` on a second line; they are not embedded in the error itself.
- Exit code 70 (EX_SOFTWARE) is reserved for "internal" failures (state corruption). Every error code maps to a distinct exit code so scripts can react to each failure class without ambiguity.
- No `serde` derive on `Error` yet; the JSON envelope for error responses is tracked separately.
- Every error site uses a typed variant; there is no catch-all `anyhow` bridge, and `anyhow` is not a dependency.

## Motivation

The CLI previously printed `Error: <context>: <message>` with no stable identity, making it impossible for users or scripts to react to specific failure classes (missing profile vs. API key vs. corrupt state), and tests could not assert on failure class. The code table gives every error a stable, documented identity; the exit codes give scripts a machine-readable signal.
