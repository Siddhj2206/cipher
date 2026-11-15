# Architecture

How the code is laid out. It uses the vocabulary of deep modules:
- **module:** anything with an interface and an implementation;
- **interface:** everything a caller must know to use it, not just its signatures;
- **seam:** a place where behaviour can be swapped without editing that place;
- **adapter:** a concrete thing that fills a seam.

The aim is a few deep modules, each hiding a lot of behaviour behind a small interface, tested through that interface. Domain terms follow `CONTEXT.md`. Decisions behind this are ADRs 0002, 0004, 0005 and 0006.

## Workspace

```
Cargo.toml              workspace; edition 2024; toolchain pinned in rust-toolchain.toml
crates/
  cipher-text/          pure text logic, no I/O
  cipher-book/          the Book: SQLite storage, domain types, Revisions, Jobs
  cipher-llm/           the Model seam and its adapters, Endpoint credentials
  cipher-engine/        the fixed pipelines (ADR 0005) and the Job runner
  cipher-app/           the Rune desktop app (binary `cipher`)
  cipher-dev/           developer CLI: import a folder, run a Job, print usage (not shipped)
reader/                 later: the Astro Reader on Cloudflare (ADR 0003)
prototypes/editor-spike throwaway UI prototype; the visual reference, not a code base
```

Dependencies point one way: `app → engine → {book, llm, text}`. `book`, `llm` and `text` don't depend on each other: the Book stores text that callers have already split.
- Only `cipher-app` depends on Rune (ADR 0002).
- Only `cipher-llm` does network I/O.
- Only `cipher-book` touches SQLite.

## Modules

### cipher-text: pure text logic

Everything Cipher can do to text without a model or a database.

**Interface (groups of free functions over `&str` and plain structs):**
- **Blocks:** split Chapter markdown into Blocks; render a Block's inline markdown to text plus styled spans.
- **Headings:** read a Chapter heading (`227화`, `第12話`, `# Chapter 3: Title`) into number and title, take it off the front of a Chapter, detect the number of pasted text.
- **Markers:** write numbered Blocks as `[n]` lines; parse a model response leniently into Pairs (`[n]`, `[n-m]`, and stray formats such as `12.` or `【12】`), plus the `NEW_TERMS:` and `NOTES:` trailer. Report missing and duplicate numbers.
- **Matching:** a recall matcher (selecting Terms for a prompt) and a precision matcher (checks: longest match first, with Hangul boundary rules for short source forms).
- **Checks:** coverage, leftover source script, Forbidden Renderings present, missing names at Chapter level, length ratio on long Blocks. These return findings, never fixes.
- **Renames:** an exact replace with word boundaries, case, possessives, "a"/"an" agreement and partial names. It returns the new text and the spans changed.
- **Drift:** per Term, per Chapter, from counts.
- **Source resolution:** given Pairs that contain a Selection, find the source n-gram frequent in them and rare elsewhere.
- **Diff:** a word-level diff of two strings, for Review display.

**Tests:** table tests through these functions, with synthetic fixtures checked in. `Test-Books/` is gitignored, so tests never depend on it.

### cipher-book: the Book

One Book is one SQLite file (ADR 0004). This module owns the schema, every query, and the rule that every change is a Revision. How Revisions and undo work, and the invariants `apply` enforces, are in `storage.md`.

**Interface** (the crate docs in `crates/cipher-book/src/lib.rs` and the `Change` docs are authoritative):

```rust
let mut book = Book::create(path, BookMeta { title, source_lang, target_lang })?;
let book = Book::open(path)?;               // migrates to the current schema

// Reads return plain owned values.
book.meta() / chapters() / chapter(id) / glossary() / story_notes()
book.suggestions(SuggestionFilter { chapter, open_only }) // with Proposed Edits, Stale derived
book.revisions(limit)                       // history, newest first

// Every write goes through one door and becomes one Revision.
book.apply(Change) -> Result<Applied { revision, created: Vec<i64> }>
book.undo(RevisionId, UndoMode) -> Result<UndoOutcome> // Whole | SkipConflicts → Undone { skipped } | AlreadyUndone | Conflict

// The Job queue lives in the Book, so it survives sleep. Jobs sit outside Revisions.
book.enqueue(JobSpec) -> JobId
book.claim_next_job() -> Option<Job>        // child Jobs first, then oldest; paused ones after resume_after
book.save_job(&Job)                         // state, partial output, usage, error
book.job(id) / jobs(active_only) / requeue_interrupted() / retry_job(id)
```

`Change` is an enum covering every kind of edit:
- add Chapter;
- set a Chapter's Translation (Pairs), flag a Pair;
- edit a Block (a Hand Edit, or a pipeline Repair);
- add a Suggestion with its Proposed Edits, add more edits, set its state;
- decide Proposed Edits;
- upsert, rename, re-status or delete a Term; add or remove a Forbidden Rendering;
- edit Style Rules, Forms of Address or Story Notes, and compact Story Notes;
- `Group`: several Changes as one Revision (a translated Chapter with its Pending Terms and Story Note).

`apply` validates the Change, writes it in one transaction and records what is needed to undo it. Callers never write SQL and never see a transaction.

**Seam:** none at the external interface. SQLite runs in tests too (in-memory or a temp file), so there is no storage trait. One adapter would be a hypothetical seam.

**Concurrency:** the app and the engine each open their own `Book` on the same file (WAL mode). The engine writes; the app reads and applies the user's own Changes. SQLite's lock serialises writes.

**Tests:** create a Book in a temp dir, apply Changes, assert on reads and undo outcomes.

### cipher-llm: the Model seam

**Interface:**

```rust
pub trait Model: Send {
    /// Streams text to `on_text` as it arrives and returns the whole response.
    fn complete(&self, req: &Request, on_text: &mut dyn FnMut(&str)) -> Result<Completion, LlmError>;
}
pub struct Request { pub instructions: String, pub input: String, pub max_output: u32, pub reasoning: Effort }
pub struct Completion { pub text: String, pub usage: Usage, pub finished: bool }
pub enum LlmError { RateLimited { retry_after }, UsageLimit { resets_at }, Auth, Network(String), Server(String), Rejected(String) }

/// Both of an Endpoint's models. ChatGPT Endpoints share one session between them,
/// because refresh tokens rotate. A missing API key sends no Authorization header (local
/// servers); a ChatGPT Endpoint that never signed in is `Auth`.
pub fn connect(endpoint: &Endpoint, secrets: Arc<dyn Secrets>) -> Result<Connection { translate, small }, LlmError>;

/// Sign in with ChatGPT the Codex way (ADR 0006): start (loopback listener + authorize URL),
/// open the URL in the browser, finish (code exchange + ID-token checks) → Credentials
/// (account, email, plan), saved to Secrets. `ChatGptSession::models()` lists the plan's models.
SignIn::start(Issuer::openai(), previous_email)?.finish(timeout)
```

**Adapters at the Model seam (a real seam, with four adapters):**
- `ChatCompletions`: POST `{base}/chat/completions`, streaming SSE.
- `Responses`: POST `{base}/responses`, streaming SSE.
- `ChatGpt`: Responses on the Codex backend with OAuth tokens and the `chatgpt-account-id` header (`ChatGptSession`: refreshes 2 minutes before expiry, retries once on 401, writes rotated tokens back to Secrets; unusable refresh tokens → `Auth`, network failures keep the Credentials).
- `Fake`: scripted responses, including cut-off streams and malformed markers, for engine tests.

**Endpoint config:** `Endpoints { endpoints, default }` in `$XDG_CONFIG_HOME/cipher/endpoints.json` (`Endpoints::default_path/load/save`, atomic write). It holds no secret.

**Second seam, `Secrets`:** `Keychain` (the OS keychain via `keyring` 4.2, whose Linux backend is pure Rust, so no libdbus) and `InMemory` for tests.

**I/O style:** blocking HTTP on the engine's worker thread (`ureq` 2.12, rustls), with no async runtime. There is one request at a time, and Rune's UI thread never waits on it.

**Out of scope:** prompts. They belong to the engine.

### cipher-engine: pipelines and the Job runner

The fixed pipelines of ADR 0005, run as Jobs from the Book's queue. Full design: `translation-pipeline.md`.

**Interface:**

```rust
/// The worker. Runs on its own thread until `stop`, claiming Jobs and emitting Events.
pub fn run(book_path: &Path, models: Models, events: Sender<Event>, stop: StopFlag);

/// What `run` loops: claims and runs one Job, returns false when nothing is runnable.
/// Tests and `cipher-dev` drive this directly.
Worker::new(book, models)?.step(&mut |event| ...) -> Result<bool>

/// `translate` model, optional cheaper `small` model (Repair, compaction), reasoning effort.
pub struct Models { translate: Box<dyn Model>, small: Option<Box<dyn Model>>, reasoning: Effort }

/// Local, for the fix box: what a Suggestion would reach (Blocks, Chapters, exact matches,
/// source expression, Term). Here is ~0.5 ms; Everywhere reads every Chapter (~75 ms on the
/// 94-Chapter test Book), so call it per Selection or Scope change, off the UI thread.
pub fn reach(book: &Book, selection: &Selection, scope: Scope) -> Result<Reach>;

/// Records a Suggestion (`SuggestionRequest { selection, said, scope, rename }`).
/// A Rename's exact replacements become Proposed Edits at once; the rest become Jobs.
pub fn suggest(book: &mut Book, request: SuggestionRequest) -> Result<SuggestionId>;

/// Queues Chapters for translation.
pub fn translate(book: &mut Book, chapters: &[ChapterId]) -> Result<()>;

/// Makes a new Book at `out` from a folder of `raw/<n>.md` (+ optional `tl/`, `glossary.json`,
/// `style.md`), as the Shelf's "Import folder…" does.
pub fn import(folder: &Path, out: &Path) -> Result<ImportReport>;
```

`Event` covers: Job started, Job progress (Blocks done of total), Job finished (with usage) or failed, Job paused (usage limit or transient error, with the resume time), and Book changed (a Revision id).

**Internal seams, tested through the interface with the `Fake` model:**
- prompt building;
- each pipeline step (prepare, translate, check, repair, learn);
- Suggestion batching.

### cipher-app: the desktop app

The Rune app described in `desktop-ux.md`. Everything a user does happens here; nothing needs the CLI. `cipher` starts on the Shelf; a `.cipher` argument (what a file manager passes) opens that Book instead.
- **Shelf:** `shelf.rs` lists `~/Documents/Cipher` (from `XDG_DOCUMENTS_DIR`) plus Books opened from elsewhere, remembered with their open times in `shelf.json` beside `endpoints.json`; removing a Book moves it to the freedesktop trash. `home.rs` holds the Shelf screen's state and actions.
- **One state:** Rune types handlers by the app's state, so the Shelf lives inside `State`; while it shows, `lib` holds a blank in-memory Book (`Library::blank`).
- **Reads:** it holds a `Book` for reads.
- **Writes:** the user's own edits are applied directly with `book.apply`.
- **Background work:** it starts `cipher_engine::run` on a thread and drains `Event`s in Rune's poll, re-reading what changed.
- **View state:** sidebars, theme, scroll and selection are app state, not Book state.
- **Tests:** headless Rune scenes and scripts (`--snap`, `--scene`, `--script`, `--tree`), as the prototype does.

### cipher-dev: developer CLI

For developers only: users never need it. The UI does not depend on it.
- `cipher-dev import <folder> <book.cipher>`: `cipher_engine::import`, as the Shelf's Import folder.
- `cipher-dev translate <book> <chapter>`: the Endpoint comes from `CIPHER_*` variables (`cipher-dev` prints them); with `CIPHER_PROTOCOL=chatgpt`, `CIPHER_ENDPOINT_ID` reuses the app's sign-in for that Endpoint.
- `cipher-dev jobs <book>`: Jobs with tokens and the model that did them.
- `cipher-dev suggestions <book>`: every Suggestion with its Proposed Edits, before/after or the skip reason.
- `cipher-dev realign <book>`: re-pair Chapters an old import paired by position, timed.

Driving the real app headless (`--script`, `--snap`, `--tree`, `--out` from Rune's tooling) always works on a scratch copy of the Book. Two variables help a session check real behaviour:
- `CIPHER_LIVE=1` reads the real `endpoints.json` and keychain and runs the worker, so a headless run translates with the user's Endpoint (use `--realtime`). Nothing is written to `endpoints.json`; a ChatGPT session may refresh its tokens in the keychain, as the app does.
- `CIPHER_SCRATCH=DIR` keeps the scratch copy in DIR (printed at start) to inspect afterwards with `cipher-dev`.

## Building

- **Toolchain:** Rust 1.100.0, edition 2024, pinned in `rust-toolchain.toml` for CI and rustup. No `rust-version`: nothing is published, and the pin already decides the compiler. mise sets `RUSTUP_TOOLCHAIN`, which overrides the file, so keep mise's Rust at the pinned version or CI and local builds differ.
- **Rune:** pinned by git revision `8fc71fbf21db41920409bf6fa20ffb41ddb2f99d` from `https://github.com/anomalyco/rune`, with the `tooling` feature (release builds need it for `--snap`, `--scene`, `--script` and `--tree`). Rune vendors patched `parley_engine`, `winit` and `wgpu-hal` and asks apps to patch them too, so the root `Cargo.toml` has a `[patch.crates-io]` pointing at the same revision; bump all four revs together. (The macOS and Windows accesskit patches are left out: Linux only.)
- **Linker:** clang with mold, from the root `.cargo/config.toml`.
- **This machine (Fedora Atomic) has no `libxkbcommon-devel`.** Linking anything that depends on Rune needs Homebrew's copy:

  ```toml
  [target.x86_64-unknown-linux-gnu]
  rustflags = ["-L", "/home/linuxbrew/.linuxbrew/lib"]
  [env]
  PKG_CONFIG_PATH = "/home/linuxbrew/.linuxbrew/lib/pkgconfig:/home/linuxbrew/.linuxbrew/share/pkgconfig"
  ```

  The prototype carries this in `prototypes/editor-spike/.cargo/config.toml`. On this machine it is in `~/.cargo/config.toml` (outside the repo, because it is machine-specific); cargo merges it with the repo's `.cargo/config.toml`.
- **Pre-commit hook:** `.githooks/pre-commit` (`core.hooksPath`) runs `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test`. 
- **Snapshot tooling:** Rune apps take `--snap out.png --scene NAME --scale 1`, `--script file.json` and `--tree out.json`. In script events, keys are `{"type":"key","name":"j","cmd":true}`, and a triple-click needs about 0.6 seconds before the next key so the selection is in place.

## Packaging and platforms

How the app is packaged, released and kept working on each platform. The release process itself is `docs/releasing.md`.

**One identity everywhere:** `app.cipher.Cipher` is the Wayland app_id and X11 WM_CLASS, the desktop entry and icon name on Linux, the bundle id on macOS; Windows uses an Inno Setup AppId GUID (in `packaging/windows/cipher.iss`, never change it, or upgrades install side by side).

**Per platform** (in code, `cfg(target_os)`):
- Config (`endpoints.json`, `prefs.json`, `shelf.json`): Linux `$XDG_CONFIG_HOME/cipher`; elsewhere `dirs::config_dir()/cipher` (`%APPDATA%\cipher`, `~/Library/Application Support/cipher`).
- Books folder: Linux follows `XDG_DOCUMENTS_DIR`; elsewhere `dirs::document_dir()/Cipher`.
- Move to Trash: Linux writes the freedesktop trash itself (testable against a temp folder); elsewhere the `trash` crate (Recycle Bin, Finder trash).
- Credentials: `keyring`'s defaults (Secret Service, Windows Credential Manager, macOS Keychain).
- Fonts: Inter and Noto Serif are compiled in (`crates/cipher-app/assets/fonts/`, OFL); CJK falls back to the system's fonts, named per platform in `theme.rs`.
- Windows: `build.rs` embeds the icon and version info (`winresource`); release builds use the GUI subsystem (no console).

**Icon:** `packaging/icons/make_icon.py` draws `cipher.svg` (1024 grid with macOS's 100 px margin) and `cipher-small.svg` (16 to 48 px); `build.py` renders `out/` (hicolor PNGs, `cipher.ico`, `cipher.icns`), which is checked in so CI never draws. Needs `rsvg-convert` and ImageMagick.

**Packages** (all written to `target/dist/`, named `Cipher-<version>-<platform>.<ext>`):

| Platform | Built by | Files |
|---|---|---|
| Linux x86_64 (on Ubuntu 22.04, so glibc 2.35+) | `packaging/linux/package.sh`, `cargo deb`, `cargo generate-rpm` (metadata in `crates/cipher-app/Cargo.toml`) | AppImage, tar.gz (with `install.sh`), .deb, .rpm |
| Windows x64 | `packaging/windows/package.ps1` (Inno Setup 6) | `-setup.exe` (per-user by default, `.cipher` association), `-portable.zip` |
| macOS universal (12+) | `packaging/macos/package.sh` (both targets, `lipo`, `Info.plist` with the `.cipher` type) | `.dmg` with Cipher.app |

The Linux binary links only libc, libm and libgcc; Vulkan or GL, Wayland or X11 and xkbcommon are loaded at run time, so the AppImage bundles nothing else.

**Not yet:** code signing on Windows (SmartScreen warns) and, until the secrets are set, on macOS (ad hoc only; Gatekeeper asks once). Opening a `.cipher` file from Finder or Explorer while Cipher already runs only brings the window forward on those platforms (Rune forwards links, not paths, there); a first launch opens the file.

## CI

`.github/workflows/ci.yml`, on pushes to `master`, `preview`, `next` and on pull requests (docs-only changes skip it; a newer push cancels the run):
- Linux: `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, and the headless GUI checks (`crates/cipher-app/scripts/check-all.sh` on the synthetic book in `crates/cipher-app/tests/fixtures/the-quiet-tower/`, rendered with Mesa's software Vulkan).
- Windows and macOS: build and test, on pull requests and pushes to `preview` and `master` only.
- `Swatinem/rust-cache` keeps one Linux cache, saved only from `preview` and `next`, so feature branches read it without churning it.

`.github/workflows/release.yml` builds the packages; see `docs/releasing.md`. Dependabot updates the actions monthly.

## Linux desktop integration

`packaging/linux/` holds the freedesktop files for the app ID `app.cipher.Cipher` (also the Wayland app_id):

- `app.cipher.Cipher.desktop`: `Exec=cipher %U`, so a `file://…/x.cipher` URI opens a Book; `SingleMainWindow=true` and `StartupNotify=true` because later launches are forwarded to the running instance.
- `app.cipher.Cipher.mime.xml`: declares `application/x-cipher-book` (`*.cipher`, a subclass of `application/vnd.sqlite3`).
- `app.cipher.Cipher.metainfo.xml`: AppStream metadata (`project_license` is `LicenseRef-proprietary` until the repo has a license).
- `icons/app.cipher.Cipher.svg` and `icons/app.cipher.Cipher-symbolic.svg`: the scalable and symbolic hicolor icons.
- `install.sh [--uninstall]`: per-user install into `~/.local` (no sudo); builds `target/release/cipher` first if missing.
