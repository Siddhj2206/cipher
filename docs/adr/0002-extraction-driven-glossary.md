# Extraction-driven glossary

The primary source of glossary terms is extraction from completed translations — not manual authoring. After a chapter is translated, the tool extracts candidate term pairs from the output, presents them for user approval, and appends them to `glossary.json`. Manual editing of `glossary.json` is an override path.

This means the glossary grows organically alongside the translation rather than requiring up-front term collection. The trade-off is that extracted terms need review and may include noise; the alternative (manually defining every term before translating) puts friction at the start of a book when the terminology may not yet be settled.
