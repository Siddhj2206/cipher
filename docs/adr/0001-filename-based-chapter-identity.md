# Filename-based chapter identity

Chapters are identified by their filename within `raw/`, not by a UUID or stable identifier. The output path in `tl/` is derived from the same filename. Renaming a `raw/chapter-01.md` to `raw/chapter-02.md` creates a new chapter — the old state is orphaned.

This keeps the identity model trivially inspectable (you can see at a glance which files map to which state entries) and avoids the complexity of UUID generation, cross-reference tables, and filename-to-ID resolution. The trade-off is that renames lose their history — a deliberate choice for a tool where chapter filenames are stable in practice (books don't typically rename chapters).
