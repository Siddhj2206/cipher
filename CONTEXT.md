# Cipher

A CLI tool for translating chapter-based books with LLMs.

## Language

**Book**:
A project directory containing raw chapters (`raw/`), translations (`tl/`), a glossary (`glossary.json`), a style guide (`style.md`), state history (`.cipher/`), and a book-level configuration file.
_Avoid_: Project, workspace

**Chapter**:
A single markdown file in the `raw/` directory, identified by its filename. A chapter follows this lifecycle: raw markdown → glossary injection → translate → validate → repair (if needed) → write to `tl/` as translated markdown. The output path is derived from the input filename.
_Avoid_: Section, segment, doc

**Translation**:
The output markdown file in `tl/` produced from a Chapter.
_Avoid_: Target, output (when ambiguous with other output files)

**Glossary**:
A collection of term pairs (source → translation) that guides consistent translation. Glossary terms are normally extracted from completed translations and presented for user approval; manual editing in `glossary.json` is an override.
_Avoid_: Dictionary, vocabulary

**Glossary Injection**:
The process of prepending relevant glossary terms as a preamble (e.g., `## Glossary\nexample → ejemplo`) before sending a chapter for translation. The raw markdown is not modified.
_Avoid_: Term replacement, substitution

**Injection Mode**:
Controls which glossary terms are injected. `Smart` (the default) selects only terms that appear in the chapter's raw text. `Full` injects every term regardless of appearance. Smart is the canonical mode.
_Avoid_: (none)

**Profile**:
A named selection of a provider, model, and API key. Profiles are stored globally (not per-book) and can be referenced by a book config or overridden at the CLI. A profile has no LLM parameters — temperature, top_p, etc. are not part of a profile.
_Avoid_: Config, preset, template (when referring to profiles)

**Provider**:
An LLM backend (e.g., Gemini, OpenAI) that performs translations and glossary extractions. Each provider has a distinct API, auth mechanism, and model list.

**State**:
A persistent record of what has been translated, including per-chapter source hashes, glossary baselines, run metadata, and output file tracking. Stored in `.cipher/state.json` per book.
_Avoid_: History, log, cache

**Rerun**:
Re-translating chapters whose tracked inputs changed since their last translation. Three modes: `source` (chapter raw markdown changed), `glossary` (terms appearing in the chapter changed), and `all` (both conditions).
_Avoid_: Retranslate, redo, refresh

**Rerun Decision**:
A per-chapter determination made during planning: skip, re-translate, or re-extract glossary terms. Based on comparing current inputs against the tracked baseline in State.

**Style Guide**:
A markdown file (`style.md`) in a book that defines tone, register, naming conventions, and formatting preferences for translations. It is sent as context during every chapter translation and repair.
_Avoid_: Style file, tone doc
