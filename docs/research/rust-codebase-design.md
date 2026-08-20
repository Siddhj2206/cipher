# Rust Codebase Design Research

Date: 2026-08-21
Project: cipher
Context: Wayfinder ticket #95 (map #94). Research good Rust codebase design for CLI applications, grounded in cipher's evidence: god modules (`translate/orchestrate.rs` 1,430 lines, `translate/rerun/decisions.rs` 1,205, `translate/cmd.rs` 788, `translate/rerun/baseline.rs` 729), 9× `#[allow(clippy::too_many_arguments)]`, a `state`↔`translate` circular module dependency, and CLI dispatch spread across `config/cli.rs`, `glossary/cli.rs`, and `lib.rs`. This document surfaces options with trade-offs; it does not design cipher's solution (a later grilling ticket owns that).

---

## 1. Deep modules and god-module decomposition

### The principle (primary source: *A Philosophy of Software Design*, Ousterhout)

- **Ch. 4 "Modules Should Be Deep"**: a deep module has a small interface hiding a large implementation (Ousterhout's example: Unix file I/O — five syscalls, enormous hidden complexity). A shallow module has a big interface relative to the implementation it hides. Shallow modules are the symptom of "classitis" — decomposition driven by mechanical rules (one class per letter of the alphabet, split every file over N lines) rather than by hidden abstractions.
- **Ch. 2 "The Nature of Complexity"**: complexity shows up as *change amplification* (one simple change touches many places), *cognitive load* (how much you must know to make a change), and *unknown unknowns*. Complexity is incremental — it accumulates one bad decision at a time, which is why 4 god modules can coexist with a clean top-level module tree.
- **Ch. 5 "Information Hiding (and Leakage)"**: a design decision should be visible in exactly one module. *Leakage* is when the same decision shows up in multiple modules (and is often the smell behind a "circular" module dependency).

**For cipher**: `orchestrate.rs` (1,430 lines) and `decisions.rs` (1,205) are the *opposite* of deep modules — a huge interface (17-arg functions, `orchestrate.rs:346`, `cmd.rs:548`) AND a huge implementation. The audit's "God modules" row and the 9 `too_many_arguments` sites are the mechanical measurement of this; the fix is not "split by line count" but "find the hidden abstraction that is currently leaking through every call site."

### Decomposition toolkit (options)

1. **Parameter objects / context structs.** Collapse N-argument functions into a struct carrying the shared inputs (`RunContext`-style). This is what cargo does with its `Config` object (threaded through nearly every function) and what Ousterhout recommends over passing many individual values ("passing information unnecessarily" is itself a complexity cause — ch. 5). A 17-arg function usually signals that a `struct` is being passed as 17 loose fields.
2. **Submodule extraction by hidden decision, not by length.** Split `orchestrate.rs` along the seams the rerun logic already has (`rerun/baseline.rs`, `rerun/decisions.rs`, `rerun/glossary.rs` exist) — the submodules of `translate/` already demonstrate the intended shape; the god modules are where orchestration + decision logic fused. Ousterhout ch. 4 warns explicitly that arbitrary decomposition produces *more* shallow modules and *more* cognitive load — so each extracted submodule must have a real, defensible interface.
3. **Facades.** A thin public API over a deeper implementation. `translate/mod.rs` already re-exports a facade surface (`pub use crate::translate::cmd::{TranslateOptions, translate_book}`); the gap is that `cmd.rs`/`orchestrate.rs` internals are effectively public-by-convention and take everything as parameters.
4. **Keep the leak visible with tooling.** `cargo-modules` (regexident) renders the crate's module tree and shows which modules are hub nodes — useful for auditing boundary bloat before/after refactors.

---

## 2. Crate layout for CLIs: single binary vs lib+bin vs workspace

### What the ecosystem actually does (primary sources: Cargo.toml + repo trees)

| Project | Layout | Notes |
|---|---|---|
| **ripgrep** (BurntSushi) | Workspace. Root package `ripgrep` is a thin binary shell (bin at `crates/core/main.rs`, `autotests = false`, one integration test `tests/tests.rs`) + library crates: `crates/cli` (flags, pattern, process, writer — the CLI library), `crates/grep`, `crates/searcher`, `crates/matcher`, `crates/regex`, `crates/printer`, `crates/ignore`, `crates/globset`. | The *engine* crates predate the workspace and are published standalone; the split exists so each layer is independently testable and reusable. ripgrep also dropped clap for `lexopt` (14.0) to cut compile time and own its flag UX. |
| **cargo** (rust-lang) | Workspace. Root package `cargo` holds **lib + bin in one crate** (`src/lib.rs` + `src/bin/cargo/`) + `crates/*` (`cargo-util`, `cargo-util-schemas`, `cargo-platform`, `rustfix`, `cargo-test-support`, …) + `credential/*`. | The root package is a library first (rust-analyzer, build-script tooling consume it) with a thin bin. `cargo-util-schemas` is a dedicated crate for schema/config types (see §3). |
| **fd** (sharkdp) | Single crate, **bin-only** (`[[bin]] src/main.rs`, no lib target). Flat modules: `cli.rs`, `config.rs`, `error.rs`, `exit_codes.rs`, `output.rs`, `walk.rs`, `exec/`, `filter/`, `fmt/`. Integration tests in `tests/tests.rs` + `tests/testenv/` fixture harness. | Solo-maintained. No lib, no workspace — yet modules stay ≤ a few hundred lines. |
| **just** (casey) | Single crate with **lib + bin** (`src/lib.rs` + `src/main.rs`, `doctest = false`) + small `crates/*` workspace members. Errors via `snafu`. Integration test `tests/lib.rs`. | The lib target exists chiefly so the code is importable for tests and for the doc/mangen tooling. |
| **zellij** | Single main crate (`src/main.rs`, `src/commands.rs`, modules) + workspace members for `default-plugins/*`. E2E tests live **inside** `src/tests/e2e/` with `insta` snapshots. | The workspace boundary exists for plugin crates, not for the core. |

### Trade-offs for a solo-dev project

| | Single bin-only crate | Single crate, lib+bin | Workspace with core/cli crates |
|---|---|---|---|
| Cargo book support | Default layout | Default layout (`src/lib.rs` + `src/main.rs`) | `[workspace]` with `members`; one `Cargo.lock`, shared `target/`, `workspace.package`/`workspace.dependencies` inheritance (cargo book: Workspaces) |
| Compile/test iteration | Fastest | Fast | Slower per-change (multiple crates); `--workspace` commands needed |
| Module cycles | Legal, silently allowed | Legal, silently allowed | **Impossible across crates** — the only hard boundary Rust gives you |
| Integration tests | Impossible for the bin's own code (Rust book ch. 11: bin-only crates can't `use` their own code in `tests/`) | Possible (`use cipher::…`) | Possible per crate |
| Overhead | None | None | Per-crate `Cargo.toml`s, versioning, feature propagation, `default-members` |
| Established examples | fd | just, cargo (root), zellij (root) | ripgrep, cargo |

### Recommendation for cipher

**Stay a single crate with lib + bin** (which cipher already is: `Cargo.toml` has `[lib] name = "cipher"` + `src/main.rs`). The workspace-with-core/cli-crates shape is what ripgrep grew *into* after the engine became reusable standalone; cargo is a platform, not a CLI. For a solo CLI whose only consumer is the binary, a workspace buys exactly one thing — impossible cross-crate cycles — at the cost of build graph, tooling, and release overhead. The cycle cipher actually has (`state`↔`translate`) is intra-crate and fixable without a workspace (§3). Revisit a workspace only if the provider engine or rerun core ever becomes a library others consume.

---

## 3. Module boundary discipline: schema vs domain

### Cycle facts (primary sources: cargo book package layout, Rust book ch. 7)

- Within one crate, Rust **permits mutually-referencing modules** — the crate compiles as a whole; module cycles are a *coupling smell*, not a compile error. Cycles only become a hard error across crates. So `state`↔`translate` compiles today; it costs you *change amplification* (Ousterhout ch. 2) — every schema change touches both trees, and neither module is comprehensible alone.
- Cargo's package layout convention puts domain files under `src/` per module; nothing in the convention says schema types must live with the logic that writes them.

### The exemplar: cargo's `cargo-util-schemas`

cargo's workspace contains `crates/cargo-util-schemas` (`manifest/`, `lockfile/`, `core/` — `package_id_spec.rs`, `partial_version.rs`, `source_kind.rs`). This is the canonical answer to "where do schema/state types belong": **persistence and schema types live in their own layer, and the domain modules import them — never the reverse.** `cargo-util-schemas` sits under the domain logic in the dependency graph and is the only place manifest/lockfile shapes are defined.

### Options for cipher's `state`↔`translate` cycle

1. **Move the persisted type into the persistence module.** `state/mod.rs:4` imports `crate::translate::TranslationUsage` because the persisted usage record is defined in `translate/types.rs:42`. Options: (a) define `TranslationUsage` in `state/` and have `translate` import it — the dependency becomes one-directional (`state` knows nothing of `translate`); (b) keep schema types in `state` but as a dedicated `state/types` submodule, mirroring cargo's crate-level split at module level. Minimal change, no crate restructure.
2. **Neutral types module.** A shared `types` module that both depend on. Works, but is the weakest option — it tends to become a dumping ground, and Ousterhout ch. 5 (information hiding) prefers each decision visible in exactly one place.
3. **Extract a crate** (workspace). Makes the cycle impossible, but only pays off if the schema becomes reusable (§2).

### Error boundary coupling (audit row: `error.rs:6` ↔ `config`)

`error.rs` imports `crate::config::ProviderKind`; `config` imports `error`. The API-guidelines-aligned shape is: `error.rs` knows **only** `std::error::Error` shapes, and domain enums either move under `error`'s dependents or get duplicated at the boundary. The cheap fix is the same as above — pick a direction and move the enum.

### CLI dispatch spread (audit row: `config/cli.rs`, `glossary/cli.rs`, `lib.rs`)

The Rust book's guidance for CLI organization is that `main.rs` stays a thin shell that calls into library code (ch. 11: "a straightforward `src/main.rs` that calls logic that lives in `src/lib.rs`"). Three observations from primary sources:

- **fd** keeps one `cli.rs` and one `config.rs`; command handlers live in `src/main.rs` dispatching into the small modules.
- **just** keeps `main.rs` thin and puts everything else in lib modules.
- **cargo** keeps *all* subcommand impls in `src/bin/cargo/commands/*.rs` — but note that is the *bin* crate's own directory, still separated from the lib core.

For cipher, the boundary question is whether subcommand handlers belong with their domain (`config/cli.rs`, `glossary/cli.rs`) or in one CLI layer. Both shapes exist in mature tools; the consistent rule to hold is: **one module owns "this is a CLI flag" (clap types) and every other module exposes plain functions** — the current spread leaks clap's `Command` type into four modules (audit's wording: "CLI concern spread across 4 modules"). Options: a single `cli.rs` dispatching to domain functions (fd-style), or per-domain `*_cli.rs` that stay dependent only on root CLI types (not on each other).

---

## 4. Injectable output / reporter patterns

### What mature CLIs do

- **ripgrep — sink trait, swapped at runtime.** The `grep-printer` crate exposes `Standard`, `Summary`, and `JSON` printers, each returning a sink (`StandardSink`, `JSONSink`, `SummarySink`) that the searcher writes match events into. The searcher never formats output; it calls into whichever sink it was given. `--json` selects the JSON sink; colors and writers live in the printer. The crate's docs frame it exactly this way: "the [`Standard`] printer shows results in a human readable format… The [`JSON`] printer shows results in a machine readable format."
- **cargo — context object with terminal access.** Cargo threads a `Config` (with `shell` access) through the whole tree rather than passing terminal handles or output flags per call; the progress/message surface uses `cargo-util-terminal`, `anstream`/`anstyle`. The pattern to steal is the *context object* — output capability travels with the call context, not as separate arguments (see §1, parameter objects).
- **indicatif** (already evaluated for cipher in `output-crates.md`): `ProgressDrawTarget::stderr()` + automatic TTY/`NO_COLOR` handling; pairs with `console`.

### Options

| Pattern | Mechanism | Pros | Cons |
|---|---|---|---|
| **Trait objects** `&dyn Reporter` | vtable dispatch | Runtime swap (human ↔ JSON ↔ quiet) without generics in call sites; testable with a fake reporter; the API-guidelines `C-OBJECT` rule ("traits are object-safe if they may be useful as a trait object") applies | One indirection per call; trait must stay object-safe (no generics in methods) |
| **Generics** `impl Reporter` | monomorphization | Zero-cost; static guarantees | Infests every signature with a type parameter; can't store heterogeneous reporters (e.g. `Vec<Box<dyn …>>` needs object safety anyway) |
| **Event system** (channel/`tracing`) | producers emit events, consumer renders | Producers fully decoupled; concurrency-friendly; trivially captured in tests | Heaviest; async runtime needed; overkill for a sequential CLI |
| **Context object** (cargo-style) | a struct holding writer/reporter/quiet flags | Collapses many args (directly fixes `too_many_arguments`); one place to swap behavior | Everyone takes the context — can become a god parameter |

### For cipher

The audit (#7) flags `output.rs`'s global statics (`QUIET`/`VERBOSE`/`JSON` atomics) as the blocker: progress reporting cannot be redirected, JSON mode is global state, and tests cannot capture output. The established shape for a sequential CLI like cipher is **trait object or context object**: define the reporter interface (e.g. `chapter_started/…` or a lighter `write_progress` surface), implement human (stderr, colors via `console`) and JSON variants, and thread `&dyn Reporter` (or a context struct owning it) from `main` down — deleting the globals. Event systems only pay off if parallelism or a UI layer arrives. Keep the stdout-for-data / stderr-for-progress discipline cipher already documents.

---

## 5. Error architecture: typed at the library, formatted at the CLI boundary

### Primary sources

- **Rust API guidelines, C-GOOD-ERR** ("Error types are meaningful and well-behaved"): every public `Result<T, E>` error should implement `std::error::Error`, be `Send + Sync`, and never be `()` — this is what lets errors compose via `source()` and `?`.
- **anyhow vs thiserror (dtolnay's own framing in the crate docs)**: anyhow is for *applications* that don't care about error identity (message + context is enough); thiserror is for *libraries* that must expose typed errors. A CLI is both — the guidance, mirrored in `docs/research/error-handling.md` (already adopted via ADR-0003): **internal layers may use `anyhow`; the user-facing boundary returns the typed `cipher::error::Error`** (E001–E007), and formatting (exit codes, red stderr, JSON envelope) happens only at the boundary in `main`.
- Ecosystem: **just** uses `snafu` for a large typed error surface with context selectors; **cargo** uses `anyhow` broadly in the bin and typed `thiserror` errors in the util/schema crates.

### Envelope convention

The audit (#9) notes the CLI error envelope currently lives inside `translate/report.rs` — a domain module owning a cross-cutting CLI contract. The boundary pattern in all mature CLIs: `main` maps typed error → exit code + message (+ JSON envelope in `--json` mode); domain modules return typed errors without formatting. The envelope type belongs next to the error type (`error.rs`), not in a translation submodule. (cipher already has exit-code mapping in ADR-0003; this is about where the type lives, not its shape.)

---

## 6. State versioning and migration (light pass — separate ticket covers this)

Primary sources: serde attributes reference; cargo's config handling.

- **Additive schema changes** are free with `#[serde(default)]` on new fields — old state files deserialize with defaults.
- **Forward-compat detection**: `#[serde(deny_unknown_fields)]` makes unknown fields an error (strict), while `serde_ignored` (cargo itself depends on it to warn about unknown config keys) ignores-but-reports. Either beats silent acceptance.
- **Breaking changes**: the canonical minimal pattern is a version field on the state root + a load-time `migrate(version)` dispatch; the persisted file is never written in the old shape again, and old shapes are one-way migrated.
- **cipher's current gap** (audit #4): `state/mod.rs:10-11` writes version constants that are *never read back* and there is no validation — a breaking schema change silently yields garbage. The minimal fix is: version field in the serialized root, check on load, and a `migrate` step before use. `schemars` (already a dependency) can validate shape at the boundary. No migration tooling (e.g. `sqlite`-style migrations) is warranted for a JSON state file.

---

## 7. Testing seams: unit vs integration, fixtures, and network mocks

### Primary sources

- **Rust book ch. 11 (Test Organization)**: unit tests live in `src/` next to code (can touch private items); integration tests live in `tests/`, are separate crates, and can only use the *public library API* — which is exactly why the book recommends bin projects put logic in `src/lib.rs` so `tests/` can drive it. Cipher already has a lib target; it simply has no `tests/` directory (audit: "No CLI integration tests — lib.rs has 1").
- **Fixture harnesses in the ecosystem**:
  - **fd** — `tests/testenv/mod.rs` builds a throwaway directory tree with tempfiles; `tests/tests.rs` runs the actual binary against it. A test-helper module under `tests/` (subdirectory, so it isn't its own test target — Rust book ch. 11's `tests/common/mod.rs` convention).
  - **cargo** — `cargo-test-support` crate: a full harness (project builders, mocked registry) used by every integration test.
  - **ripgrep** — one `tests/tests.rs` target (`autotests = false`) driving the binary end-to-end.
  - **just** — `tests/lib.rs`; **zellij** — e2e tests *inside* `src/tests/e2e/` with `insta` snapshots.
- **Network mocking**: the standard seam is to inject the HTTP client behind a trait (the API-guidelines `C-GENERIC`/`C-OBJECT` family) and hand tests a fake or `wiremock` (a purpose-built HTTP mock server for Rust). Audit notes cipher's provider paths are "only exercised against real endpoints" and there is "no HTTP-level mock test" — the `Provider` trait is already the seam; wrapping the client (`rig`) or adding a test provider makes the chat/completions paths testable offline.

### Current state in cipher (as of this research, 2026-08-21)

The `tests/` layer already exists — the architecture audit's "No CLI integration tests (lib.rs has 1)" predates it:

- `tests/cli_integration_tests.rs` — 24 end-to-end tests driving the real CLI (init, doctor, glossary, profile, translate with typed-error envelopes, JSON output, quiet/verbose parsing, provider-failure exit codes).
- `tests/translator_integration_tests.rs` — 4 tests using a `MockProvider` in `tests/helpers/mod.rs` implementing the existing `Provider` trait with scripted translate/repair/extract results (exactly the fd-style fixture-helper convention from Rust book ch. 11).
- `tests/helpers/mod.rs` — the shared fixture module (mock provider + default results).

Remaining gaps: no HTTP-level mock of the real provider network paths (the chat vs completions dual path in the OpenAI provider is still only exercised against live endpoints), no end-to-end *rerun* flow test through the CLI, and no scripted failure-recovery scenario for backup/repair.

### Options for cipher

1. **Extend the existing integration layer** (highest value per effort): an end-to-end rerun test — translate with the `MockProvider`, mutate a raw chapter, rerun, assert the decision + report. The seams (`lib` target, `Provider` trait, `tests/helpers`) are all in place.
2. **HTTP-level mock for provider network paths**: wrap the underlying client call (rig) behind a trait or use `wiremock` to exercise the chat vs completions request-building paths offline — this is the only network seam still untested.
3. **Reporter injection pays for testing too**: a `&dyn Reporter` fake captures output, making the JSON envelope and quiet/verbose paths assertable (ties into §4).
4. Keep unit tests in-module (219 tests) and the integration layer separate, without disturbing either.

---

## 8. Recommendations for cipher (options + trade-offs; design left to the grilling ticket)

1. **Crate layout: keep single crate, keep lib + bin.** Workspace extraction (ripgrep/cargo style) is not justified for a solo-maintained CLI with one consumer; revisit only if the provider engine or rerun core becomes an external library.
2. **Fix the `state`↔`translate` cycle by moving the persisted type into `state`** (one-directional imports), or a neutral types module if that's cleaner — cheapest boundary fix available; a workspace to *enforce* the direction is overkill now.
3. **Decompose god modules by hidden abstraction, not line count**, with parameter objects/context structs first (cargo's `Config` precedent directly addresses the 17-arg functions), then submodule extraction along seams that already exist (`rerun/*`). Use `cargo-modules` to verify the module tree before and after.
4. **Replace global output statics with an injected reporter** (trait object or context object; ripgrep's sink-swap and cargo's context-object are the precedents). Keep stdout-data/stderr-progress discipline; JSON becomes one more reporter implementation, not global state.
5. **Keep typed errors at the CLI boundary and move the envelope type out of `translate/report.rs` into the error module** — consistent with ADR-0003 and the existing `error-handling.md` research; nothing else to change.
6. **State versioning: version field + load-time check + one-way migrate** (audit #4); `#[serde(default)]` for additive fields, `serde_ignored`-style warnings or `deny_unknown_fields` for forward compat. Light touch; the dedicated ticket owns details.
7. **Extend the existing `tests/` integration layer** — the `MockProvider` fixture harness and 28 integration tests already exist (the audit's "no CLI integration tests" is stale); add an end-to-end rerun flow test and an HTTP-level mock for the untested provider network paths (chat vs completions). Both seams (lib target, `Provider` trait) are in place.

---

## Primary Sources

| Source | URL |
|--------|-----|
| *A Philosophy of Software Design*, Ousterhout (2nd ed. 2021), ch. 2, 4, 5 | Book (Yaknyam Press) — ch. 4 "Modules Should Be Deep", ch. 5 "Information Hiding (and Leakage)" |
| Cargo book — Package Layout | <https://doc.rust-lang.org/cargo/guide/project-layout.html> |
| Cargo book — Workspaces | <https://doc.rust-lang.org/cargo/reference/workspaces.html> |
| Rust book — Test Organization (ch. 11) | <https://doc.rust-lang.org/book/ch11-03-test-organization.html> |
| Rust API Guidelines (checklist; C-GOOD-ERR in Interoperability) | <https://rust-lang.github.io/api-guidelines/checklist.html> |
| C-GOOD-ERR text (source of guidelines) | <https://github.com/rust-lang/api-guidelines/blob/master/src/interoperability.md> |
| ripgrep `Cargo.toml` (workspace members, bin at `crates/core/main.rs`, `autotests = false`) | <https://github.com/BurntSushi/ripgrep/blob/master/Cargo.toml> |
| ripgrep `grep-printer` crate (`Standard`/`Summary`/`JSON` + sinks) | <https://github.com/BurntSushi/ripgrep/tree/master/crates/printer/src> |
| cargo `Cargo.toml` (root package lib+bin, `crates/*`, `credential/*`) | <https://github.com/rust-lang/cargo/blob/master/Cargo.toml> |
| cargo `cargo-util-schemas` crate (schema types layer) | <https://github.com/rust-lang/cargo/tree/master/crates/cargo-util-schemas/src> |
| cargo bin structure (`src/bin/cargo/commands/*`) | <https://github.com/rust-lang/cargo/tree/master/src/bin/cargo> |
| fd `Cargo.toml` (single bin-only crate) + `src/` + `tests/` layout | <https://github.com/sharkdp/fd/blob/master/Cargo.toml>, <https://github.com/sharkdp/fd/tree/master/tests> |
| just `Cargo.toml` (lib+bin, snafu, workspace `crates/*`) | <https://github.com/casey/just/blob/master/Cargo.toml> |
| zellij `Cargo.toml` (root crate + plugin workspace members) | <https://github.com/zellij-org/zellij/blob/main/Cargo.toml> |
| cargo-modules (module tree visualizer) | <https://github.com/regexident/cargo-modules> |
| anyhow crate docs ("application" vs library intent) | <https://docs.rs/anyhow/latest/anyhow/> |
| thiserror crate docs | <https://docs.rs/thiserror/latest/thiserror/> |
| serde attribute reference (`default`, `deny_unknown_fields`, versioning) | <https://serde.rs/attributes.html> |
| `serde_ignored` (used by cargo for unknown-config warnings) | <https://github.com/dtolnay/serde-ignored> |
| wiremock (HTTP mocking for Rust) | <https://github.com/lawliet89/wiremock> |
| Existing cipher research: error handling | `docs/research/error-handling.md` |
| Existing cipher research: output crates | `docs/research/output-crates.md` |
