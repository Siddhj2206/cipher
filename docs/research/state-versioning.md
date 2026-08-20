# State Schema Versioning & Migration Research

Date: 2026-08-21
Project: cipher
Ticket: wayfinder #96 (map #94)
Context: `RUN_METADATA_VERSION` / `GLOSSARY_STATE_VERSION` in `src/state/mod.rs:10-11` are written but never
validated on load; there is no migration machinery; forward compatibility relies on `serde(default)`;
one corrupt chapter state file fails the whole run with E007/exit 70; book config must stay portable and
secret-free (AGENTS.md). This document surveys how established tools version and migrate persisted JSON/TOML
state, and surfaces options with trade-offs. It deliberately does **not** design cipher's solution.

---

## 1. Current State (evidence)

- `RUN_METADATA_VERSION: u32 = 1` and `GLOSSARY_STATE_VERSION: u32 = 1` (src/state/mod.rs:10-11) are serialized
  into `RunMetadata.version` / `GlossaryState.version` and written on save, but **no code reads or checks the
  version on load** (`load_json` at src/state/mod.rs:340 just deserializes).
- `ChapterState` (src/state/mod.rs:57) has **no version field at all**.
- Forward compatibility is achieved ad hoc with `#[serde(default)]` on newer fields (`repair_profile`,
  `glossary_profile`, `translation_usage`, `source_text_hash`, ...).
- Corrupt JSON anywhere (including one chapter state file among many) becomes `Error::State`, exit 70, and
  aborts the whole run (src/error.rs:108; `load_all_chapter_states` at src/state/mod.rs:236 uses `?` on each
  file).
- Writes go through `io::atomic_write` (temp file + rename, src/io.rs:4) — atomic, but no fsync and no
  explicit permissions. API keys are stored inline in `~/.config/cipher/config.toml` (XDG config dir via
  `directories::ProjectDirs`, src/config/mod.rs:21-26) and written with the default umask (typically 0644).
- AGENTS.md constraints: state changes additive where possible, formats deterministic, book config portable
  and secret-free; add targeted tests when rerun/state behavior changes.

---

## 2. Schema Versioning Patterns

### 2.1 Top-level `version` field (flat)

The file carries `version = N` next to its data fields. Established, load-bearing examples:

- **Cargo.lock** — starts with `version = 4` (formerly 3). Cargo writes the version on save; parsers dispatch
  on it. Old Cargo fails fast with a precise message on a newer version:
  `lock file version 4 was found, but this version of Cargo does not understand this lock file, perhaps Cargo
  needs to be updated?` (rust-lang/cargo#15306). Version 4 became default in 1.83 (rust-lang/cargo#14595);
  cargo-audit and cargo-outdated initially choked on v4 with `invalid Cargo.lock format version: 4` until
  updated (rustsec/rustsec#1296, kbknapp/cargo-outdated#405). The lockfile format version is distinct from the
  resolver version, and #12861 made the format version auto-selected by MSRV so old toolchains keep getting a
  lockfile they can parse (rust-lang/cargo#13503). Users can even hand-edit the top-level `version` down,
  which is only possible because the field is a first-class, validated part of the format.
- **rustup settings.toml** — `${RUSTUP_HOME}/settings.toml` carries a metadata `version = "12"` field (see
  rust-lang/rustup#4054 for a real file: `default_toolchain = "stable-x86_64-apple-darwin"`, `profile =
  "default"`, `version = "12"`). The docs deliberately state the schema is *not* a public interface
  (rust-lang.github.io/rustup/configuration.html). rustup distinguishes "silent, lossless conversion"
  (legacy multirust `version`/`default`/`overrides` files become a single settings.toml) from "explicit
  upgrade" (older metadata versions must run `rustup upgrade-data` because the migration wipes toolchains)
  (rust-lang/rustup#420).
- **Ecosystem crates** formalize the same shape: `version-migrate` supports both *wrapped*
  (`{"version":"1.0.0","data":{...}}`) and *flat* (`{"version":"..","field":..}`) formats
  (docs.rs/version-migrate); `rcman` uses an `_schema_version` field and a chain of `if version < N { ... }`
  steps (docs.rs/rcman); `magic_migrate` (Cloud Native Buildpacks cache keys) uses a chain of `TryFrom` impls
  between versioned structs (docs.rs/magic_migrate).

**Trade-offs:** flat keeps old files visually identical to today's cipher state; wrapped
(`{"version":N,"data":{...}}`) separates concerns but changes every file's shape once and breaks naive
consumers of the current format. Flat + top-level version is the most common, lowest-churn choice.

### 2.2 `serde(default)` forward compatibility

Serde's field-level `#[serde(default)]` / `#[serde(default = "path")]` fills missing fields during
deserialization (serde.rs/attr-default.html, serde.rs/field-attrs.html). Container-level `#[serde(default)]`
fills all missing fields from `Default` (serde.rs/container-attrs.html). Critically for JSON:

> "When this attribute is not present, by default unknown fields are ignored for self-describing formats
> like JSON." — serde.rs/container-attrs.html (`deny_unknown_fields`)

So a plain `#[derive(Deserialize)]` struct is *forward compatible by default*: a file written by a future
cipher is readable by today's cipher (unknown keys ignored). The gaps in this strategy:

- Missing fields with no default cause a hard error, even when the value is meaningless for the reader.
- `default` fills only *absent* keys; an explicit `null` still errors for non-`Option` fields.
- It cannot detect or explain *semantic* changes (renames, type changes, reinterpreted values). A rename
  silently drops data: the old key becomes an ignored unknown field and the new key gets its default.
- It cannot distinguish "file from a future version, probably fine" from "file from a future version that
  will corrupt behavior". Cargo chose a hard error for future lockfiles precisely so users are told to
  update their toolchain.

VS Code is the canonical "no version field, tolerate everything" example: `settings.json` has no version
marker, unknown settings are ignored, and that tolerance is treated as a feature (code.visualstudio.com/docs/
languages/json; `$schema` is a hint, not a version). Zed recently moved the other way, generating its schema
with `additionalProperties: false` and warning on unknown fields (zed-industries/zed#33678) — the trade-off
is live: tolerance helps forward-compat, strictness catches typos.

### 2.3 `#[serde(untagged)]` for legacy shapes

`#[serde(untagged)]` tries each variant in order and uses the first that parses (serde.rs/container-attrs.html).
Useful when a legacy shape lacks a version marker: define `enum StateFile { V1(LegacyShape), V2(CurrentShape) }`
and let shape sniffing pick the version. Caveats from serde docs: uninformative errors when nothing matches
(mitigate with `#[serde(expecting = "...")]`), and the approach can be costly in hot paths. rust-patterns.com
documents this as part of a versioned-migration workbook (chain `V1` to `V2` to `V3` structs with `default` +
`alias` + `untagged`).

### 2.4 Versioned directory/entity layout (Homebrew)

Homebrew versions *per artifact* via directory names: `Cellar/<formula>/<version>/` kegs, each keg holding an
`INSTALL_RECEIPT.json` whose `versions.version_scheme` distinguishes formula versioning schemes
(Homebrew/brew#11127). Installed-keg formula files are read in preference to the latest definition; if
unreadable, brew falls back to fetching the latest (`Formulary::from_installed` with a `factory` fallback,
Homebrew/brew#20603). `brew reinstall` moves the old keg aside to `#{path}.reinstall` as a backup
(Homebrew/brew#22505). This is the pattern to copy when many independent sub-documents each have their own
lifecycle — e.g. cipher's per-chapter state files.

---

## 3. Migration Strategies

### 3.1 Explicit version-to-version migration functions, linear chain

Every migration step is a pure function `Vn` to `Vn+1`; loading runs the chain until the current version,
never skipping steps (crdt_migrate: "Linear chain: Migrations run v1→v2→v3→…→current",
docs.rs/crdt-migrate). `magic_migrate` encodes the chain in types (`TryFrom` between versioned structs),
which makes incompleteness a compile error (docs.rs/magic_migrate). `version-migrate` registers typed paths
and guarantees the chain is complete at compile time. Because steps are pure data transforms, they are
trivially unit-testable.

### 3.2 Read-latest, migrate on load (lazy migration) vs eager migration

Two axes, often conflated:

- **When**: migrate *on first access* (lazy) vs *upfront at startup / on write* (eager).
- **Write-back**: after migrating in memory, persist the migrated file (so the next read is free) or not.

Practices observed:

- `rcman` runs migrations "lazily on first settings load", and if the value changed it is "immediately
  written back to disk" (docs.rs/rcman).
- `version-migrate` similarly: migrate on load; `update_and_save` persists atomically; a `SaveIfMissing`
  load behavior writes the initial (migrated) content (docs.rs/version-migrate).
- The jsonic.io migration guide calls this lazy migration on read with optional write-back, and recommends
  write-back so old-version documents are gradually eliminated; batch migration is only needed for large
  collections or breaking changes that must land before deploy (jsonic.io/guides/json-migrations).
- rustup is the prime *eager, conditional* example: lossless conversions happen silently as soon as rustup
  runs; destructive upgrades are gated behind an explicit `rustup upgrade-data` command (rust-lang/rustup#420).
- Cargo rewrites a v3 lockfile as v4 on the next lockfile write after 1.83 — migration on write, silently,
  but it never *downgrades*.

**Trade-offs:** lazy migration on access + write-back keeps startup cheap, self-heals files as they are
touched, and is the pattern the ecosystem crates converged on. Eager whole-directory migration on first run
is simpler to reason about but rewrites many files up front (and needs a scan step). Migrating on write only
never fixes files the user isn't writing. Migrations are one-way — don't support downgrades (rcman best
practice; cargo deliberately refuses newer lockfiles instead of downgrading).

### 3.3 Migration testing

Established practice (jsonic.io/guides/json-migrations; rustup#420 added explicit tests for the legacy-file
conversion; magic_migrate docs):

- **Fixture per version**: commit a sample JSON file for every historical version as living documentation.
- **Forward test**: load v1 fixture, run the chain, assert exact current-version output.
- **Round-trip**: for tools that support `down()`, `down(up(doc)) == doc`. For one-way app state, a
  down-migration is usually not worth it — rustup and cargo do not downgrade.
- **Idempotency**: migrating an already-migrated file is a no-op.
- Keep migrations forever — users can upgrade from any version (rcman best practice; rustup still ships the
  metadata-upgrade path for its "12"-era ancestors).

---
## 4. Corrupt-File Handling: What Established Tools Do

Three distinct responses, chosen by what the file is worth:

### 4.1 Fail fast, manual repair — *precious, user-authored config*

- **git**: `fatal: bad config file line 1 in .git/config` — every git command refuses to run; there is no
  auto-repair (recovery is manual: fix or delete the file, then re-add remotes; mandeepsingh.hashnode.dev
  guide; Stack Overflow threads). Git's own mailing list discussion about lenient config parsing was
  rejected — git deliberately dies on syntax errors (public-inbox.org/git).
- **rustup**: unreadable settings.toml produces `error: could not read settings file: ...` and it stops
  (rust-lang/rustup#2254).

Rationale: silently ignoring a broken config would make the tool behave differently from what the user
wrote, which is worse than an error. Error messages must name the file and the problem so a human can fix it.

### 4.2 Quarantine (rename aside), then regenerate — *derived or cached state*

- **fish-shell** renames a stale settings file to `settings.toml.bak` before regenerating defaults — the
  pattern rustup maintainers discussed adopting for `rustup-init` (rust-lang/rustup#4744, comment by
  FranciscoTGouveia, a fish-shell maintainer).
- **cargo registry cache**: extraction is guarded by a `.cargo-ok` marker; if the marker is not valid JSON,
  cargo clears the cache entry and re-extracts ("we clear the cache and re-extract it",
  rust-lang/cargo#3661). On git-index corruption ("Object not found") cargo deletes the index and retries
  once (rust-lang/cargo#8735). Earlier versions just errored with a manual fix, `rm -rf $CARGO_HOME/registry`
  (rust-lang/cargo#2403) — the modern direction is automatic quarantine + rebuild.
- **git index**: `error: bad index file sha1 signature / fatal: index file corrupt`; the documented fix is
  to delete `.git/index` — the index is *derived* state (recoverable from HEAD + worktree), so deletion and
  regeneration is safe (lazacode.org; public-inbox.org/git submodule thread).
- **Homebrew**: when an installed keg formula file is unreadable, `Formulary` falls back to the latest
  published formula (Homebrew/brew#20603) — degrade gracefully rather than abort.

### 4.3 The dividing line

| File kind | Examples | Response |
|---|---|---|
| User-authored config | git config, rustup settings.toml | Fail fast, name the file, manual repair |
| Derived / regenerable state | cargo registry cache, git index | Quarantine or rebuild automatically |
| Sub-document of derived state | one chapter state among many | Skip/isolate the bad one, continue |

cipher's chapter state is derived data (recoverable by rerunning from source text); failing the whole run
over one corrupt chapter file (E007/exit 70) is the behavior the ecosystem moved away from. Its run
metadata and glossary state are also derived, but they encode completed work — which argues for quarantine
(rename to `.corrupt`/`.bak`) rather than silent deletion, so forensics survive and the user can recover
manually if the corruption is meaningful.

---

## 5. Backward/Forward Compatibility Conventions

- **Additive changes only; defaults for new fields.** This is the universal convention (serde `default`;
  protobuf: "Adding new fields is safe" — protobuf.dev/programming-guides/proto3).
- **Never reuse a name for a different meaning.** Protobuf's rule "don't re-use a tag number" maps to JSON
  as "don't re-use a field name" — and ProtoJSON is explicit that renaming fields/enum values in JSON-based
  formats is *not* safe the way binary renames are (protobuf.dev/best-practices/dos-donts,
  protobuf.dev/programming-guides/json). For JSON state, field names are the wire contract.
- **Reserve deleted names.** When a field is removed, keep the name reserved so a future version doesn't
  silently reintroduce it with a different meaning (protobuf `reserved` directive).
- **`serde(alias)` for graceful renames**: accept the old name on deserialize while serializing the new
  name (rust-patterns.com/22-serialization-project3-migration.html).
- **Don't change the default value of an existing field**: clients and servers straddling the change see
  different values for the same unset field (protobuf best practices) — the same skew applies to cipher
  reading old state files with new code.
- **Rust API guidelines (C-SERDE)**: types that are data structures should implement `Serialize` /
  `Deserialize` (rust-lang.github.io/api-guidelines/interoperability.html). cipher already follows this.
- **cargo-semver-checks** is the Rust-ecosystem tool for catching *API* semver violations (removed items,
  signature changes); it does not apply to JSON files directly, but its posture — automated checks against
  a documented reference (the Cargo SemVer reference, doc.rust-lang.org/cargo/reference/semver.html) — is
  the model for keeping schema promises honest (github.com/obi1kenobi/cargo-semver-checks).

---

## 6. Atomic Writes, Permissions, XDG

### 6.1 Atomic write (temp + rename)

cipher already implements the standard pattern in `io::atomic_write` (src/io.rs:4): write temp file in the
same directory, then `rename()` over the target. Same-dir rename is atomic on POSIX and avoids partial
reads. Established practice adds:

- **fsync the temp file before rename** (crash-durability; secret-write docs: "write to temp file, fsync,
  then rename over original ... never a truncated/empty file"; docs.rs/secret-write).
- **fsync the parent directory after rename** (best-effort; secret-write).
- **Open the temp file with the final mode, not chmod-after**: set permissions at creation (Secursive
  security guidance: "Set permissions before you create sensitive files, not after") so there is never a
  world-readable window.
- git uses the same idea via `.lock` files for refs/config (the `.git/index.lock` error message is the
  well-known face of this pattern); cargo uses temp + rename for lockfiles and cache writes.

### 6.2 File permissions for secrets

Convention (fs-safe.io/secret-file.html; crates.io/crates/secret-write; XDG spec requires 0700 for
`$XDG_RUNTIME_DIR`): secret files 0600, secret dirs 0700, with refusal to write into a wider-permission
directory. **cipher finding:** API keys are stored inline in the global config.toml
(src/config/mod.rs:12-18, `ApiKey.value`) and written by `atomic_write` with no mode control — the file
lands at the process umask (typically 0644, world-readable). This contradicts the 0600 convention and is
worth flagging to the design ticket regardless of state versioning.

### 6.3 XDG base directory spec

`$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, `$XDG_STATE_HOME` partition user config/data/state
(specifications.freedesktop.org/basedir/latest). cipher's global config already lands in
`~/.config/cipher` via `directories::ProjectDirs` (src/config/mod.rs:21-26), which is XDG-conformant.
cipher's *state* is deliberately book-local (`.cipher/`), so XDG state dirs do not apply to it — the book
directory is the portable unit (AGENTS.md: book config must stay portable). Versioning must keep state
self-contained in the book directory and secret-free.

---

## 7. When Versioning Matters vs `serde(default)` Suffices

Decision framework synthesized from the surveyed sources:

**`serde(default)` alone suffices when all of these hold:**
1. Every future change is *additive* (new optional fields with defaults) — serde's unknown-field tolerance
   and defaults handle it (serde.rs).
2. No field will ever be renamed, retyped, or semantically reinterpreted (ProtoJSON: renames are unsafe in
   JSON; protobuf: don't change types or defaults).
3. Old binaries never need to *refuse* newer files — misbehavior risk on unknown fields is acceptable.
4. The project is pre-1.0 or low-commitment about its on-disk format.

**A version field earns its keep when:**
1. A breaking change is anticipated (rename, restructure, type change, meaning change) and needs a
   migration transform.
2. Old binaries should fail fast with an actionable message on newer files (cargo lockfile: "this version
   of Cargo does not understand this lock file, perhaps Cargo needs to be updated?").
3. Migration logic needs a home (version dispatch) and needs to be tested per transition.
4. Determinism/explainability matter: "state was migrated from v1 to v2" is a state change the user can
   see (AGENTS.md: preserve explainability, deterministic formats).

**An unwritten-but-validated version field is a liability** (cipher's current state): the constants exist
and are serialized but nothing checks them, so the first breaking change silently slips through with no
dispatch, no error, and no migration — the version field provides false confidence. Either validate +
dispatch on it, or remove it and rely on `serde(default)` consciously.

**Per-file versioning vs file-level:** with independent sub-documents (cipher's per-chapter state files),
each file should carry its own version (rustup settings.toml as a whole; Homebrew per-keg receipts), so a
future change to chapter-state schema doesn't force a whole-book migration. `#[serde(untagged)]` handles
pre-version legacy shapes.

---

## 8. Recommendations for cipher (options, not a design)

1. **Validate the version on load (cheap, do first).** Add a load-time check: `version > CURRENT` fails
   with an actionable E007 message ("state file from a newer cipher — update cipher"), `version < CURRENT`
   either migrates or, when no migration is needed, deserializes and rewrites at the current version.
   This is the Cargo.lock pattern (fail fast on future, accept past). Removes the false-confidence
   liability while staying additive.
2. **Give ChapterState a version field.** It is the only unversioned state type; without one, per-file
   migration and per-file fail-fast are impossible. This is additive (defaults to 1 on old files).
3. **Corrupt-file policy: isolate, don't abort (recommendation).** Chapter state is per-file derived data;
   quarantine a corrupt file (rename `foo.json` to `foo.json.corrupt`, optionally with a warning) and
   treat that chapter as pending, mirroring cargo's `.cargo-ok` re-extract and fish's `.bak` patterns —
   while keeping fail-fast for the *config* files (global config.toml, book cipher.toml), matching git and
   rustup. Alternative (git-style): fail fast with a precise message naming the file and the documented
   fix. Both are defensible; the grilling ticket should pick based on how much the corrupt file is worth
   to the user.
4. **Migration machinery: linear chain of pure transforms, run on load, write back.** Migrations are
   `fn(serde_json::Value) -> Result<Value>` steps v1→v2→…→current, applied lazily on load, persisted via
   the existing `save_*`/`atomic_write` path on the next write (cipher already rewrites state after runs).
   Eager whole-book migration on first run is the alternative if startup cost or "all files at current
   version" matters more. Never downgrade.
5. **Test every migration with fixtures.** One inline or fixture JSON per historical version; forward,
   idempotency, and (where applicable) round-trip tests. AGENTS.md already requires targeted tests for
   state behavior changes.
6. **Keep formats deterministic and book-config portable.** Version bumps and migration rewrites must be
   deterministic (stable ordering; `serde_json::to_string_pretty` as today); nothing in `.cipher/` or
   `cipher.toml` gains secrets.
7. **Fix secret permissions in the same effort.** Global config.toml holds API keys; write it 0600 (mode
   set at temp-file creation, not after rename) per the secret-file convention. State files carry no
   secrets and can stay at default perms. Also consider fsync in `atomic_write` for crash durability.

**Bottom line:** cipher's current position — version fields written but unvalidated, no migration
machinery, serde(default) as the only compat mechanism, whole-run abort on one corrupt chapter file — is
the combination the surveyed tools explicitly moved away from. The cheapest safe posture is: validate
version on load (fail fast on future versions), add a version to ChapterState, isolate corrupt chapter
files, and add a tested linear migration chain only when the first breaking change actually arrives.
Adding a wrapper envelope or per-version typed structs (magic_migrate style) is overkill for a single-user
CLI today.

---

## Primary Sources

| Source | URL |
|--------|-----|
| serde container attributes (deny_unknown_fields, default, untagged, expecting) | https://serde.rs/container-attrs.html |
| serde field attributes (default, alias, rename) | https://serde.rs/field-attrs.html |
| serde default-value docs | https://serde.rs/attr-default.html |
| serde JSON conventions (unknown fields ignored) | https://serde.rs/json.html |
| rust-patterns serialization migration workbook | https://www.rust-patterns.com/22-serialization-project3-migration.html |
| Rust API guidelines, C-SERDE | https://rust-lang.github.io/api-guidelines/interoperability.html |
| Cargo SemVer reference | https://doc.rust-lang.org/cargo/reference/semver.html |
| cargo-semver-checks | https://github.com/obi1kenobi/cargo-semver-checks |
| Cargo lockfile v4 default (PR #14595) | https://github.com/rust-lang/cargo/pull/14595 |
| Cargo "lock file version 4 ... needs to be updated?" (#15306) | https://github.com/rust-lang/cargo/issues/15306 |
| Cargo lockfile version hand-editing (#13503) | https://github.com/rust-lang/cargo/issues/13503 |
| Cargo registry corruption, manual fix (#2403) | https://github.com/rust-lang/cargo/issues/2403 |
| Cargo `.cargo-ok` re-extract on corruption (#3661) | https://github.com/rust-lang/cargo/issues/3661 |
| Cargo index reinitialize on corruption (PR #8735) | https://github.com/rust-lang/cargo/pull/8735 |
| rustup settings.toml location & non-public schema | https://rust-lang.github.io/rustup/configuration.html |
| rustup metadata version, silent vs explicit upgrade (PR #420) | https://github.com/rust-lang/rustup/pull/420 |
| rustup settings.toml real file with `version = "12"` (#4054) | https://github.com/rust-lang/rustup/issues/4054 |
| rustup unreadable settings → fail fast (#2254) | https://github.com/rust-lang/rustup/issues/2254 |
| rustup-init quarantine-vs-regenerate discussion (#4744) | https://github.com/rust-lang/rustup/issues/4744 |
| git bad config: fail fast, manual repair | https://mandeepsingh.hashnode.dev/how-to-fix-a-corrupted-git-config-file-fatal-bad-config-line-1-error |
| git index corrupt: delete and regenerate | https://lazacode.org/2503/git-status-shows-bad-signature-0x00000000-index-file-corrupt |
| git lenient-config discussion (rejected) | https://public-inbox.org/git/A5CDBB91-E889-4849-953A-2C1DB4A04513@gmail.com/T/ |
| VS Code JSON editing docs (settings.json, $schema) | https://code.visualstudio.com/docs/languages/json |
| Zed warn on unknown settings fields (PR #33678) | https://github.com/zed-industries/zed/pull/33678 |
| Homebrew keg version_scheme receipts (#11127) | https://github.com/Homebrew/brew/issues/11127 |
| Homebrew keg-formula fallback (PR #20603) | https://github.com/Homebrew/brew/pull/20603 |
| Homebrew `.reinstall` backup kegs (PR #22505) | https://github.com/Homebrew/brew/pull/22505 |
| protobuf best practices (reserve tags, don't reuse, defaults) | https://protobuf.dev/best-practices/dos-donts/ |
| ProtoJSON (renames unsafe in JSON) | https://protobuf.dev/programming-guides/json/ |
| proto3 language guide (adding/removing fields, reserved) | https://protobuf.dev/programming-guides/proto3/ |
| version-migrate crate (wrapped/flat formats, lazy migration) | https://docs.rs/version-migrate |
| magic_migrate crate (TryFrom chains) | https://docs.rs/magic_migrate |
| crdt_migrate crate (linear migration chain) | https://docs.rs/crdt-migrate |
| rcman crate (lazy migration + write-back, testing) | https://docs.rs/rcman |
| JSON migration guide (lazy read, fixtures, round-trip) | https://jsonic.io/guides/json-migrations |
| XDG Base Directory Specification | https://specifications.freedesktop.org/basedir/latest/ |
| fs-safe secret-file conventions (0600/0700) | https://fs-safe.io/secret-file.html |
| secret-write crate (atomic 0600 writes, fsync) | https://crates.io/crates/secret-write |
| Secursive: set perms at creation, not after | https://blog.secursive.com/posts/security-file-manipulation-bash-scripts/ |
