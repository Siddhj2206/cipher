Fonts bundled into the `cipher` binary, so the book text looks the same on every platform:

- Inter (Regular, Medium, SemiBold): the interface. Copied from Rune's assets. `OFL-Inter.txt`.
- Noto Serif, upright and italic, variable weight: book text. `OFL-NotoSerif.txt`.

Korean, Japanese and Chinese text uses the system's CJK fonts (Noto Serif CJK on Linux,
AppleMyungjo / Apple SD Gothic Neo on macOS, Batang / Malgun Gothic on Windows); the families
are listed in `src/theme.rs`.
