# One SQLite database per Book

Each Book is stored as a single SQLite file, not a folder of markdown files. Cipher needs per-Block history, atomic multi-Block changes (an Everywhere Review touches dozens of Blocks at once), and a translation queue that survives the laptop sleeping mid-Chapter. Plain files would mean building a fragile transaction and journal layer by hand.

## Consequences

- Text cannot be hand-edited in an outside editor, so Cipher itself must offer editing of both source and translated text. Hand edits go through the same history as every other change and can be undone.
- Markdown export stays available so that a Book's text is never locked in.
