# Rune as the GUI toolkit

The desktop app uses `anomalyco/rune`, a Rust GPU-rendered toolkit, for its UI. It has the text features a translation reader needs: Parley-based shaping and bidi, script font fallback, IME, rich text editing, virtualized long content, streaming markdown, plus split panes, popovers, context menus, and native file dialogs.

## Consequences

- Rune is pre-release (`0.0.1`, created 2026-10-22, no tags), has one dominant maintainer, and does not accept pull requests. We pin a git revision and expect to carry local patches in a fork.
- Book, Glossary, and LLM logic live in crates with no Rune dependency, so the toolkit can be swapped without touching the domain.
- Known gaps: no PDF rendering or webview (irrelevant while Chapters are markdown), an open Wayland IME redraw bug (rune#225), and a kit styled after OpenCode that will need restyling.
