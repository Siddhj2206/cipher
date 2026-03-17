# cipher

A CLI tool for translating book chapters using LLMs. Feed it raw chapters in any language, point it at an LLM provider, and it produces translated markdown files with glossary management built in.

cipher is designed for serialized web novels but works for any long-form text that benefits from consistent terminology across chapters.

## How It Works

cipher organizes translations as **book projects** -- directories containing raw chapters, translated output, a glossary, and a style guide. Each chapter is sent to an LLM with the glossary and style guide as context, producing a translated markdown file and optionally discovering new glossary terms along the way.

The translation flow for each chapter:

1. Load the raw chapter markdown
2. Select relevant glossary terms (smart mode matches terms by original-language characters; full mode sends everything)
3. Send the chapter + glossary + style guide to the LLM
4. Validate the response (check heading format, balanced code fences, no JSON leakage)
5. If validation fails, send a repair request with the errors
6. Write the translated file and merge any new glossary terms

New glossary terms discovered by the LLM during translation are appended to the glossary file after each chapter, so subsequent chapters benefit from the accumulated terminology.

## Installation

```bash
cargo install --git https://www.github.com/siddhj2206/cipher.git
```

## Quick Start

### 1. Set up a profile

cipher uses **profiles** to manage LLM provider configurations. Create one interactively:

```bash
cipher profile new
```

This walks you through selecting a provider (OpenAI or any OpenAI-compatible API), entering your API key, and choosing a model.

### 2. Create a book project

**From an EPUB file:**

```bash
cipher import my-novel.epub
```

This extracts chapters from the EPUB, converts them to markdown, and sets up the project directory.

**From scratch:**

```bash
cipher init my-book
```

Then place your raw chapter files (markdown) in the `my-book/raw/` directory.

### 3. Translate

```bash
cipher translate my-book
```

Translated chapters appear in the `tl/` directory. The glossary is updated as new terms are discovered.

### 4. Check status

```bash
cipher status my-book
```

Shows which chapters have been translated, failed, or are still pending.

## Book Project Structure

```
my-book/
  config.json        # Book configuration (profile, directories, glossary mode)
  glossary.json      # Glossary terms (accumulated across chapters)
  style.md           # Style guide injected into every translation prompt
  raw/               # Raw source chapters (markdown)
    001.md
    002.md
    ...
  tl/                # Translated output
    001.md
    002.md
    ...
  .cipher/           # Internal state (run history, backups)
```

## Commands

### `cipher translate [book_dir]`

Translate a book. Defaults to the current directory.

```bash
cipher translate                          # translate book in current dir
cipher translate my-book                  # translate book in my-book/
cipher translate my-book --overwrite      # re-translate all chapters (backs up existing)
cipher translate my-book --fail-fast      # stop on first error
cipher translate my-book --profile fast   # use a specific profile
```

Already-translated chapters are skipped unless `--overwrite` is passed. Failed chapters can be retried by running the command again.

### `cipher import <epub_path>`

Import an EPUB file into a new book project. (Still in testing. Don't expect much yet.)

```bash
cipher import novel.epub            # creates novel/ directory
cipher import novel.epub --force    # re-import (prompts before deleting existing chapters)
```

### `cipher init <book_dir>`

Initialize an empty book project.

```bash
cipher init my-book
cipher init my-book --profile myprofile
cipher init my-book --from other-book            # copy glossary from another book
cipher init my-book --import-glossary terms.json  # import glossary from file
```

### `cipher status <book_dir>`

Show translation progress -- profile info, chapter counts, and any failed chapters with error details.

### `cipher doctor [book_dir]`

Run diagnostics. With a book directory, checks the project layout and profile configuration. Without one, checks the global configuration.

```bash
cipher doctor my-book    # check book setup
cipher doctor            # check global config
```

### `cipher glossary <subcommand> <book_dir>`

Manage the glossary.

```bash
cipher glossary list my-book                     # list all terms
cipher glossary import my-book new-terms.json    # merge terms from file
cipher glossary export my-book backup.json       # export to file
```

### `cipher profile <subcommand>`

Manage LLM profiles.

```bash
cipher profile new                    # create a new profile (interactive)
cipher profile list                   # list all profiles
cipher profile show myprofile         # show profile details
cipher profile set-default myprofile  # set the default profile
cipher profile test myprofile         # validate profile configuration
```

## Configuration

### Global Config

Stored at `~/.config/cipher/cipher/config.json`. Managed through `cipher profile` commands. Contains:

- **Providers** -- named LLM provider configs (OpenAI or OpenAI-compatible with a base URL)
- **API keys** -- stored per provider, with optional labels for managing multiple keys (Note: keys are stored as plain text as of now. Suggestions would be appreciated.)
- **Profiles** -- named combinations of provider + model
- **Default profile** -- used when no profile is specified

### Book Config

Each book has a `config.json`:

```json
{
  "profile": "",
  "raw_dir": "raw",
  "out_dir": "tl",
  "glossary_path": "glossary.json",
  "style_path": "style.md",
  "glossary_injection": "smart"
}
```

**Profile resolution order:** `--profile` CLI flag > book `config.json` profile > global default profile.

### Glossary

A JSON array of terms:

```json
[
  {
    "term": "Starship",
    "og_term": "星空舰",
    "definition": "The main character's vessel"
  },
  {
    "term": "River Map",
    "og_term": "山河图",
    "definition": "An ancient artifact containing a sealed dimension",
    "notes": "Sometimes referred to as 'The Map' in casual dialogue"
  }
]
```

- `term` -- the English translation
- `og_term` -- the original-language term (used for smart matching)
- `definition` -- context for the LLM
- `notes` -- optional additional guidance

**Glossary injection modes:**

- **`smart`** (default) -- matches `og_term` values against the chapter text using n-gram fuzzy matching. Only relevant terms are sent to the LLM. Falls back to the full glossary if fewer than 5 terms match.
- **`full`** -- sends the entire glossary with every chapter.

New terms discovered by the LLM during translation are appended to the end of the glossary file.

### Style Guide

The `style.md` file is injected into every translation prompt. Use it to specify tone, character voice, formatting preferences, or anything else the LLM should follow consistently across chapters.

## Providers

cipher works with any OpenAI-compatible API. During profile creation, you can choose:

- **OpenAI** -- uses the standard OpenAI API
- **OpenAI-compatible** -- any provider with a compatible API (Gemini, local LLMs, etc.) by specifying a base URL
