# Cipher

A desktop app for reading and translating books with LLMs. You add a book's chapters, Cipher translates them while keeping names and terms consistent, and you fix the translation while you read.

## Language

### The book

**Book**:
A single translation project: one work, one Source Language, one Target Language, with its Chapters, Glossary, and Style Rules.
_Avoid_: Project, workspace, document

**Chapter**:
One markdown file of the Book's source text, in reading order.
_Avoid_: Section, file, doc

**Block**:
The smallest unit of text in a Chapter or Translation: a paragraph, heading, quote, or list item. Hand Edits and Proposed Edits always belong to a Block.
_Avoid_: Segment, paragraph (when a heading or list item is also meant), chunk

**Pair**:
A run of consecutive source Blocks matched to the run of consecutive translated Blocks that renders them. Usually one Block on each side. Pairs come from the numbering in the translation step itself, never from matching paragraph counts afterwards.
_Avoid_: Alignment, mapping, segment

**Source Language** / **Target Language**:
The language the Book is written in, and the language it is translated into. A Book has exactly one of each for now.
_Avoid_: From/to, input/output language

**Translation**:
The Target Language text of a Chapter, made of translated Blocks grouped into Pairs with the source Blocks they render.
_Avoid_: Output, target text

### Consistency

**Glossary**:
The Book's collection of Terms. Translation consults it so that the same Term is rendered the same way across every Chapter.
_Avoid_: Dictionary, vocabulary, termbase

**Term**:
A Glossary entry: a source expression (a name, place, title, or recurring phrase) and its approved rendering in the Target Language. It may also have:
- a kind;
- a gender, for characters;
- a short note;
- Forbidden Renderings.

A Term covers every grammatical form of the expression, not just one spelling.
_Avoid_: Entry, word, keyword

**Forbidden Rendering**:
A rendering of a Term that must not appear in the Translation. Renaming a Term moves its old rendering here; rejecting a Proposed Edit's wording can add one. Checks look for Forbidden Renderings rather than for missing renderings, because the source often repeats a name where the Target Language uses a pronoun.
_Avoid_: Blacklist, banned word, old name

**Pending Term**:
A Term that the translation step proposed and the user has not yet accepted or dismissed. It is used provisionally in later Chapters, so a new name stays consistent before the user decides.
_Avoid_: Suggestion, candidate, extracted term

**Form of Address**:
How one character addresses another (형, 선배님, -san, 师兄). Kept in a small table beside the Style Rules, not as Terms, because it depends on who is speaking to whom.
_Avoid_: Honorific term, title

**Style Rule**:
A standing instruction about how the Book should read ("Mara speaks formally", "keep honorifics"). It guides every later translation.
_Avoid_: Style guide, prompt, preference

**Story Notes**:
A short running summary of what has happened so far in the Book, carried forward so that translation of later Chapters has the context of earlier ones. Renames apply to Story Notes too.
_Avoid_: Summary, memory, context

**Drift**:
A Term rendered inconsistently in a Chapter. Either a Forbidden Rendering still appears, or the source names the Term at least three times and the Translation never renders it. The second rule skips Terms whose source is one character (명 also counts people) and Terms that **may go unnamed**: names the Translation often replaces with a pronoun, as the user marks them. Cipher finds Drift locally, without an LLM.
_Avoid_: Inconsistency, error, mismatch

### Fixing while reading

**Selection**:
A span of translated text the user has highlighted to act on.

**Suggestion**:
What the user says should change about a Selection, in their own words. It is the start of every fix.
_Avoid_: Prompt, request, comment, instruction

**Rename**:
A Suggestion the user marks as a plain change of rendering (old words → new words). Cipher applies it by exact replacement, with no LLM call, and sends only the Blocks that rendered the expression some other way to the model.
_Avoid_: Find and replace, substitution

**Scope**:
How far a Suggestion reaches. One of:
- **Here**: only the Selection's Block.
- **Everywhere**: every Block already translated where the same source expression appears, or where the selected text appears.
- **From now on**: Everywhere, and also updates the Term or Style Rule for future translation.

_Avoid_: Range, mode

**Proposed Edit**:
A single change Cipher wants to make to one translated Block in response to a Suggestion. It is not applied until the user accepts it. A Proposed Edit whose Block changed after it was proposed is **stale** and is never applied.
_Avoid_: Patch, diff, fix, change

**Review**:
The step where the user accepts, rejects, or adjusts Proposed Edits before any of them change the Translation. A Rename's exact replacements are reviewed as one group; model-written edits are reviewed one by one. Drift Keep also stages Glossary and Story Note items: these require their own explicit acceptance, independently of prose, including when there are no prose replacements. Staged items survive restarting the app; a changed Term or Story Note is flagged stale rather than overwritten.
_Avoid_: Approval, confirmation

**Hand Edit**:
A change to source or translated text that the user types themselves, rather than one Cipher proposes.
_Avoid_: Manual edit, override

**Outdated**:
A translated Block whose source Block has changed since it was translated.
_Avoid_: Stale (that word is for Proposed Edits), dirty, out of sync

### History and work

**Revision**:
One undoable unit of change to a Book: a translation, an applied Review, or a Hand Edit. Undoing a Revision reverts all of its changes together.
_Avoid_: Commit, version, changeset

**Job**:
One resumable unit of LLM work: translating a Chapter, a batch of a Suggestion, a Repair. It is stored in the Book, so it survives the laptop sleeping or closing mid-call.
_Avoid_: Task, request, run

### Reading elsewhere

**Reader**:
The private, phone-first website where the user reads their Books' Translations away from the desktop. It is part of Cipher, not a separate site.
_Avoid_: Site, web app, frontend

**Publish**:
Bringing the Reader up to date with the current Translations. It happens from the desktop app and needs no other tools.
_Avoid_: Sync, deploy, export

### Setup

**Shelf**:
Every Book Cipher knows: the Books in its folder (`~/Documents/Cipher`) and any opened from elsewhere. It is the home screen.
_Avoid_: Library (the app's code uses `Library` for the open Book's view data), home, dashboard

**Endpoint**:
The LLM service the user brings. It is either an OpenAI-compatible base URL with an API key, or a ChatGPT plan through Sign in with ChatGPT. Each Endpoint names:
- a default translate model, and more models a Book may choose;
- optionally a cheaper small model;
- the reasoning effort.

Endpoints belong to the user, not the Book, and their credentials live in the OS keychain. A Book only keeps its **Model Choice**: which Endpoint and model it translates with, else the default.
_Avoid_: Provider, profile, backend
