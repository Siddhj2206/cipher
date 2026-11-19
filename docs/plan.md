# Implementation plan

The single place for **status**: what is done, what is next, and what is open.

How to use it:
- **Resuming:** start each session here. Pick up the first unchecked task of the current milestone and read the docs it names.
- **Where things live:** design lives in `docs/design/`, decisions in `docs/adr/`, words in `CONTEXT.md`. When work changes one of those, edit it in place; don't restate it here.
- **Before ending a session or handing off:**
  - tick finished tasks;
  - move **Current** forward;
  - add anything learned that the next session needs under **Notes**.

## Current

- Reader Chapter titles: saved titles now appear in the opener and Contents, including queued drafts after reopening; untitled Chapters retain their existing labels.

**Milestone 7c (a polished desktop app) in progress** (2026-11-16). Overall scope is through milestone 8; milestone 9 is on hold and milestone 10 (web Reader) is deferred, both until the user asks.
- Done: milestones 0–6 and 8. The real-Endpoint checks ran on 2026-11-16 through the user's ChatGPT sign-in (Free plan, gpt-6-luna): see each milestone. The API-key routes (Chat Completions, Responses with a key) have still never met a real server.
- Milestone 7: done except Test and Save against a real Endpoint from the form.
- Milestone 7b: done; three Shelf actions unchecked on a real desktop.
- Milestone 7c: packaging and CI are written but have never run on GitHub; the first push runs them (Windows and macOS have only been type-checked as far as C dependencies allow). Next is the rest of its list below, then a pass on a real desktop (theme following, window restore, drag and drop, `packaging/linux/install.sh`).
- Open decision: inertial scrolling needs a Rune change (see Open questions); updating Rune to `8fc71fb` did not add it.
- Book notes: editable Forms of Address and Story Notes now have a separate page beside Style, grouped saves and queued compaction. See `desktop-ux.md` › Book notes.

## Milestones

Each milestone ends on a checkable state. Don't start the next milestone until it holds.

### 0. Workspace

- [x] Workspace `Cargo.toml` with the crates from `docs/design/architecture.md`.
- [x] Root `rust-toolchain.toml` (1.100.0 since 2026-11-16, clippy, rustfmt).
- [x] The Rune git pin in `cipher-app`.
- [x] Machine-specific link flags documented as in `architecture.md` › Building.
- [x] `cipher-app` opens an empty window.

**Done when** `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` pass, which makes the pre-commit hook pass.

### 1. Text logic (`cipher-text`)

- [x] Block splitting and inline markdown rendering to text plus spans.
- [x] The marker protocol: write `[n]` lines; parse leniently, including merges, stray formats and the trailer; report gaps and duplicates.
- [x] Recall and precision matchers, including the Hangul boundary rule.
- [x] Checks: coverage, leftover script, Forbidden Renderings, missing names, length ratio.
- [x] Rename with boundaries, case, possessives, "a"/"an" and partial names.
- [x] Drift counts, local source resolution and the word-level diff.

**Done when** each item has table tests with checked-in synthetic fixtures, all green. Read `translation-pipeline.md` first.

### 2. The Book (`cipher-book`)

- [x] The schema from `storage.md` and migrations via `user_version`.
- [x] `create`, `open` and the read methods.
- [x] `apply` for every `Change`, recording Revisions.
- [x] `undo`, with conflict reporting.
- [x] The Job queue: enqueue, claim, save.
- [x] `cipher-dev import` for a `raw/` + `tl/` folder. Existing translations are aligned locally (since 2026-11-16; before, Pairs by index).

**Done when** tests cover apply and undo for every Change, and importing `Test-Books/S-Class-Transmigrator` yields 94 Chapters and 190 Terms (34 of its 224 glossary entries have no source form and are skipped).

### 3. Models (`cipher-llm`)

- [x] The `Model` trait and `Request`/`Completion`/`LlmError`.
- [x] The `ChatCompletions` adapter with SSE streaming and usage.
- [x] The `Fake` adapter (scripted, cut-off, malformed).
- [x] `Secrets` with `Keychain` and `InMemory` adapters.
- [x] Endpoint config: base URL, models, reasoning effort.

**Done when** the adapters have tests against a local stub server, and one manual call to a real Endpoint has been logged in Notes with its token usage.

Status (2026-11-16): done through the ChatGPT route: Chapter 236 of the test Book, 4,619 tokens in and 2,910 out, ~50 s, gpt-6-luna on a Free plan. The Chat Completions adapter has still only met the stub server.

### 4. Translating (`cipher-engine`)

- [x] The worker loop and Events.
- [x] The translate pipeline: prepare → translate → check → repair → learn.
- [x] Resume from partial output.
- [x] Usage-limit pause.
- [x] Story Notes compaction.
- [x] `cipher-dev translate`.

**Done when**:
- Fake-model tests cover a clean run, a cut-off stream resumed, a missing range re-requested, and a Forbidden Rendering fixed locally;
- one real Chapter has been translated end to end, with tokens in and out noted.

Status (2026-11-16): done. In the app, Chapter 236 (143 paragraphs): 4,619 in, 2,910 out. With `cipher-dev translate` and reasoning effort low, Chapter 238 (169 paragraphs): 4,856 in, 3,072 out, 58 s.

### 5. Reading app (`cipher-app`)

- [x] Port the prototype's look to the real Book: contents, page, context sidebar, focus layout, scrubber.
- [x] Dark mode, Cast & terms with real Drift, the palette, interlinear and source view.

**Done when** headless snapshots of the same scenes as the prototype render from a real `.cipher` file. Read `desktop-ux.md` first.

### 6. Fixing while reading

- [x] Selection chip and fix box with Reach, the Rename toggle and Scope.
- [x] `engine::suggest`, with local renames and model batches.
- [x] Grouped and inline Review; Undo of a Revision.
- [x] Stale edits; From now on writes the Term or Style Rule plus its Forbidden Rendering.

**Done when** the prototype's scripted flow (select → type → send → review → accept → undo) passes on a real Book, with real Proposed Edits from a real model for a non-rename Suggestion.

Status: the scripted flow passes (`sh crates/cipher-app/scripts/check-fix.sh <book>`, on a temp copy). Real model edits: done 2026-11-16 through a Drift fix (a non-rename Suggestion; the same batch path): "Ai" in chapter 228 gave 2 correct edits and 2 correct skips once Pairs were aligned. Still open: a stale edit's "regenerate" button (the engine has no regenerate call yet).

### 7. Adding and queueing

- [x] The paste sheet.
- [x] Queue progress in the header.
- [x] Failed-Job retry.
- [x] Flagged Pairs.
- [x] Endpoint settings with an API key and a test button.

Status: built and checked headless (scenes `intake`, `queued`, `translating`, `failed`, `flagged`, `settings`, `settings-chatgpt`, dark variants; `sh crates/cipher-app/scripts/check-queue.sh <book>`). The worker translates live in the app (2026-11-16, `CIPHER_LIVE`), streaming paragraphs onto the page. Per-Chapter usage and model show in the header's tooltip. Still open: Test and Save pressed against a real Endpoint.

### 7b. The Shelf (everything in the GUI)

Added by the user (2026-11-15): no step a user takes may need the CLI.
- [x] `cipher` starts on the Shelf: Books in `~/Documents/Cipher` and opened from elsewhere, most recently opened first.
- [x] New book, Import folder (`cipher_engine::import`, on a thread), Open file, Rename, Move to Trash / Remove from shelf, Show folder.
- [x] Empty Books open and take pasted Chapters; Ctrl+O and the contents sidebar return to the Shelf.
- [x] Realistic, restrained Shelf design (`desktop-ux.md` › Shelf), light and dark.

Checked: `sh crates/cipher-app/scripts/check-shelf.sh Test-Books/S-Class-Transmigrator` (new book → paste → back → import, headless on a temporary Shelf). Unchecked on a real desktop: the XDG portal file chooser, Show folder, and the trash.

### 7c. A polished desktop app

Asked by the user (2026-11-15): "a proper desktop app … how it auto syncs to device dark/light theme … proper settings, usability, design", with T3 Code as the example; and all Endpoint and model setup in Settings, a Book only choosing its model. Design: `desktop-ux.md` › The page, Settings, Endpoints and models, Desktop.
- [x] Continuous reading across Chapters (no runaway: the strip settles after scrolling stops).
- [x] Preferences (`prefs.json`): appearance following the system live, reading type, interface zoom, sidebars, window size, reopen last Book; per-Book reading position in `shelf.json`.
- [x] Settings window (Ctrl+,) with General, Reading, Translation, Shortcuts, About; instant apply with per-row reset.
- [x] Several Endpoints with extra models; a Book's Model Choice from the header's model menu; the worker uses it.
- [x] Keys: shortcuts sheet (Ctrl+/, F1), Space paging, Alt+arrows, text size, F11, Ctrl+Q, Ctrl+W; one table in `keys.rs`.
- [x] "Aa" type popover; Settings gear; headers drag the window.
- [x] Shelf: search, sort, Continue line, right-click menu, Show in Files.
- [x] Right-click a Chapter; drop a Book or a folder on the window.
- [x] App id, single instance with `file://` hand-off, `packaging/linux/` (desktop entry, MIME, icons, AppStream, install script).
- [x] Packaging and CI (asked 2026-11-15): AppImage, tar.gz, .deb, .rpm, Windows installer and portable zip, macOS universal .dmg; CI on every push; tag-driven draft releases (`docs/releasing.md`); new icon. Cross-platform paths, trash and bundled fonts. Linux packages built and run locally; Windows and macOS builds not yet run (they first run in CI).
- [x] Onboarding and adding (asked 2026-11-15: "proper onboarding and a much more integrated new chapter flow, not a random popup with a text box"): first-run Welcome, new Book typed onto its cover on the Shelf, new chapter as a page in the Book (`desktop-ux.md` › Welcome, Shelf, Adding chapters). The paste sheet and the new-book sheet are gone.
- [ ] Find in Book (Ctrl+F).
- [x] A Style page: the Book's Style Rules as one text, sent with every Chapter (asked 2026-11-16: "a base prompt for a book"); import takes `style.md` whole.
- [x] Drift redone (asked 2026-11-16, "fine with redoing our whole suggestions and drift workflow"; method simulated on Genius Wizard first): evidence of what the Book calls a Term, the user marks same-name wordings and keeps one (an exact Rename), the model only on request; answers checked locally; Review steps only through real changes and shows the paragraph in full, now and proposed (`translation-pipeline.md` › What the Book calls a Term).
- [x] Piece Q: Drift Keep stages persistent Glossary, Story Note and prose proposals, with explicit independent Review decisions, conflict checks and Revision-based Undo. Cast says "No drift flagged" rather than claiming uniformity. From now on Suggestions outside Drift Keep are unchanged.
- [ ] Term editing (accept or dismiss pending Terms, notes, forms, merging duplicate Terms such as 포혈공 / 포혈공(怖血公)) and a Book notes page (Story Notes, Forms of Address).
- [x] Chapter actions: Translate again, and Translate all N from here (2026-11-16).
- [ ] Delete a Chapter.
- [x] A signed-in plan's models as choices in the form (2026-11-16).
- [ ] "Fetch models" from an API-key Endpoint's `/models`.
- [x] Several chapters at once: numbered multi-chapter paste or repeated file picks, with ordered preview, conflict notes and per-chapter Include/Skip.
- [x] Sidebars that give way on narrow windows (2026-11-16).
- [x] Resizable, remembered sidebar widths.
- [x] A History panel: persistent Revisions with dry-run previews, chapter conflicts and Undo, including re-translations and re-pairs (Ctrl+H).
  - Metadata Review accepts also preview and undo after reopening; later Term edits are skipped, and restored proposals follow the normal pending/stale rules.
- [ ] Notifications when a long translation finishes while the window is unfocused.

Checked headless: `sh crates/cipher-app/scripts/check-desktop.sh <book>` (Ctrl+, → text size → Esc → Alt+→ → Ctrl+/ → Aa → Ctrl+-), plus the earlier checks; scenes `settings-*`, `shortcuts`, `type-popover`, `chapter-menu`, `model-menu`, `reading-sans`. Not checked on a real desktop yet: live theme following, window size restore, single-instance hand-off, drag and drop, install.sh.

Drift (2026-11-15, redone 2026-11-16): Cast rows open to show a Term's drift and how the Book names it; "Fix in N chapters" (`fix_drift`) is gone, replaced by Keep a wording (`use_wording`) and Ask the model (`ask_model`). Checked headless on a copy of Genius Wizard: Magic Scribe's evidence, marking, Keep (998 exact replacements), and the Review's now/proposed view; the Style page saves. Not yet run: Ask the model against a real Endpoint since the rewrite.

Big Books (2026-11-15): a 1,285-Chapter Book froze the app for 49 s on open. Now it opens in 0.2 s. The fixes were schema V2's indexes; `Library::open_quick`, which counts only the open Chapter, plus a background `Library::scan` (2.8 s on 8 threads, Aho-Corasick prefilter) for the rest; and a virtualized contents list with a thinned scrubber. Time it with `CIPHER_BIG_BOOK=<book> cargo test --release -p cipher-app big_book -- --ignored --nocapture`.

### 8. Sign in with ChatGPT

- [x] The `Responses` adapter.
- [x] OAuth: PKCE, loopback, refresh.
- [x] The ChatGPT adapter.
- [x] The sign-in button in Endpoint settings.
- [x] Switched to Codex's sign-in (2026-11-15): the first real sign-in through Sign in with ChatGPT's dynamic registration failed with `invalid_grant`, and that route needs a paid plan. The user asked for Free plans with Codex limits. Details in ADR 0006.
- [x] Real sign-in and real calls (2026-11-16): the user signed in from Settings (Free plan); translations, a Drift fix and the model list all worked, with Cipher's own instructions and with reasoning effort low (ADR 0006).

### 9. Hand editing

On hold (user, 2026-11-11): work stops after milestone 8. Don't start this until asked.


- [ ] Edit a Block in place.
- [ ] Explicit split and merge commands that adjust Pairs.
- [ ] Source edits mark translated Blocks Outdated.
- [ ] Retranslating keeps Hand Edits.

### 10. Reader and Publish (ADR 0003)

Deferred by the user (2026-11-11): desktop only for now. To be designed in detail before starting.

## Open questions

- **Inertial scrolling (required, `desktop-ux.md` › Feel):** needs a Rune change on Linux. Patch the pinned Rune (a fork or `[patch]`), or upstream it? Decide before milestone 7 ends.

- ~~**First-time import of existing translations**~~ settled 2026-11-16: aligned locally (`translation-pipeline.md` › Importing existing translations).
- **Prompt wording and the instruction prefix:** settle these during milestone 4 by measuring on 3–5 real Chapters with a cheap model and a strong one.
- ~~**Rune virtualisation**~~ settled: Chapter 271 (~180 Blocks) scrolls at p50 10.1 ms / p95 12.4 ms per frame in a debug build, rendering every Block. A release first frame is 67–89 ms.
- **Wayland IME redraw bug (rune#225):** check that Korean input works in the fix box.

## Notes
- 2026-11-19: Linux CI and release build jobs install clang and mold before Cargo, matching `.cargo/config.toml`, plus `libxkbcommon-dev` for Rune; the stock runner lacked mold and the xkbcommon linker library.
- 2026-11-16: The user's real Genius Wizard Book holds damage from the old position-paired drift fixes (accepted 2026-11-15): chapter 246 has a duplicated paragraph and lost "Contrary to Evelyn's concerns…" (blocks 10416/10417, Suggestion #2), and that Suggestion also turned "Blaiver Tower" into "Blaiver Magic Tower" in 9 accepted edits. Only these were checked. Undo needs a History panel or a by-hand restore from the tl/ files; the user hasn't been asked yet. Its open Suggestions #3–#5 are old drift fixes with 170 queued batches; dismissing them is the user's call.
- 2026-11-16: Open on Drift: source words longer than a Term (흑마법사 is 61% of 흑마법's Pairs), Terms that aren't names (세 번째 = "The Third"), sibling Terms ("Magic Tower" family) and duplicate Terms. Glossary editing, not Drift, should handle them.

- 2026-11-16: Drove the app headless against the user's ChatGPT Endpoint (`CIPHER_LIVE`, see `architecture.md` › cipher-dev) on scratch copies and fixed what showed up. Biggest: imported Books were paired by position and drifted apart within a few paragraphs, so Interlinear, Drift and Suggestions were wrong and a Drift fix proposed overwriting unrelated sentences; now aligned locally, old Books re-paired once. Also: `SetTranslation` failed with a foreign-key error on any Chapter with a Suggestion (re-translate, re-pair); drift Suggestions showed the model's instruction as the reader's words; no feedback after Fix or Translate again; toasts never went away; narrow windows clipped the page; Terms sidebar empty before translating; Cast had no search. Measured live: a Chapter of 143 paragraphs took ~50 s on gpt-6-luna (Free plan) for 4,619 tokens in and 2,910 out. Still open from that pass: no History panel, so a re-translation or a re-pair can only be undone from code; settings sheet keeps its fixed height; Copy translation needs a Rune clipboard call.

- Rune's `El::modal` takes Escape itself and calls its dismiss before `on_key` sees it; a closing sheet stays in `--tree` output while it animates out (allow ~1 s in scripts).
- `on_paste` works only on the view's root element.
- Rune ships `rune-kit` (menus, selects, switches, dialogs, toasts, titlebar) at the same rev; Cipher draws its own controls so far to keep its look, but the kit is there if a control gets hard.

- 2026-11-15: The user wants the whole app GUI-first ("everything must be gui") and a realistic but tasteful design app-wide. The Shelf is the first piece in that style; the reading screens are next only once the user has seen it. Headless runs that wait on a thread (import) need `--realtime`. `Color::mix` mixes in linear light: use Oklab (`toward` in `view/shelf.rs`) for small tints.
- 2026-11-14: Milestone 7 in the app: `library/queue.rs` (paste → `Intake`, add and queue, Translate, Retry), `settings.rs` (the Endpoint form, Test, Save, sign-in on threads), `view/sheets.rs`, and `fix/work.rs` now carries `Progress`/`Paused` and restarts the worker on Save (the new worker joins the old one first, so they never share a Job). Ctrl+V uses Rune's root `on_paste`, so it never steals a paste from a text field. Headless runs never touch the keychain or `endpoints.json`.
- 2026-11-14: Rune updated `a7f276c` → `8fc71fb` (720 commits). No source changes needed; tests, clippy, the scenes and `check-fix.sh` all pass, and the reading scene looks the same (release first frame ~100 ms warm). It needed a `[patch.crates-io]` for Rune's vendored crates and the `tooling` feature (see `architecture.md` › Building). Wheel lines on Linux now scroll 3× further (120 pt); check how that feels. Useful additions: `scroll_key_to` reaches unmounted virtual rows, `rune-kit` has a `ToastStack`, primary-selection paste. No sign that rune#225 (Wayland IME redraw) is fixed.
- 2026-11-11: `engine::reach` on the test Book: "Magic Tower Lord" in chapter 227 reaches 16 Blocks in 7 Chapters (10 exact, 6 where the source has 마탑주 but the translation lacks the name); "Holy Scripture" reaches 275 (144 exact, 131 for the model). Most of those 131 are likely Pairs where English used a pronoun, so the model will mostly SKIP them: about 11 batch calls. If that's too costly, narrow model candidates for Term Renames to Pairs whose translation contains a Forbidden Rendering or a near variant.
- 2026-11-11: Milestone 6 in the app: `fix/` (fix box, Review, worker in `fix/work.rs`), `view/{fix,notes,review,diff}.rs`, `library/edits.rs`. The worker starts only when `Endpoints` has a current Endpoint; without one, Renames still work and model Suggestions show "Waiting for an Endpoint". Headless runs (scenes, scripts) copy the Book to a temp dir first. Deviations: one-by-one Review writes a Revision per action (Undo reverses them in order; Accept all is one Revision); Undo on a note works only for Reviews done in the current session (the Book doesn't map Suggestions to Revisions); skipped edits come last with Dismiss. Unchecked: Korean input in the fix box on Wayland (rune#225), and whether Rune drops `State` on exit (the worker's StopFlag is set in Drop).
- 2026-11-11: The build cache can delete `target/` binaries between builds; rebuild before running `check-fix.sh` (it now fails loudly if the binary is missing).
- 2026-11-11: `cipher <book.cipher>` opens the reading app; `CIPHER_CHAPTER=N` opens at a Chapter. Scenes: reading, focus, interlinear, source, cast, palette, term, dark, dark-cast, dark-focus. Imported Pairs (flag `imported`) are shown quietly; other flags get an amber gutter dot. Not yet built in the app: red scrubber ticks, the Term card's Rename everywhere, Add chapters (they come with milestones 6 and 7).
- 2026-11-11: Milestone 7 backend ready ahead of its UI: `Book::retry_job`, `cipher_llm::Endpoints` (config file), `cipher_llm::connect` returning both models. Milestone 8 code done; needs a real sign-in.
- 2026-11-11: Undo has two modes (`UndoMode::Whole`, `SkipConflicts`), see `storage.md`.
- 2026-11-11: Design agreed; prototype complete with light/dark, sidebars, grouped review. Measured token sizes are in `translation-pipeline.md`.
