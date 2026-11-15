# Desktop-first rewrite

Cipher is rebuilt from scratch as a desktop app for reading and translating books, replacing the CLI. Fixing a translation is mostly a reading activity: you notice something, point at it, and say what's wrong. A CLI can only offer rerun-and-edit-files for that. Nothing from the CLI (code, file formats, conventions, or earlier ADRs) is carried over; anything reused must be re-decided for the GUI.

## Scope

- Books only. Markdown Chapters are the only format. EPUB and other formats may come later as import/export, never as the internal model.
- One Target Language per Book for now. The model should not rule out several Target Languages later.
- Bring-your-own-key, OpenAI-compatible Endpoints only. (Amended by ADR 0006: Sign in with ChatGPT is also supported.)

## Considered Options

- **Keep the CLI and add a GUI on top**: rejected. The CLI's model (whole-chapter reruns driven by tracked hashes) does not fit fixing individual Blocks while reading, and keeping both would tie the new design to the old one.
- **General documents and professional CAT features (translation memory, layout-preserving formats)**: deferred. Books are the use case the author actually has. Layout preservation is where every existing tool struggles, and it would dominate early work.
