# cipher

A desktop app for reading and translating books with LLMs. It is being rebuilt from scratch: start with `docs/plan.md`, `CONTEXT.md` and `docs/design/`.

In Cast, open a Term and choose **Merge duplicate…** to preview which source forms and renderings to keep. The merge can be undone in History and leaves chapter text unchanged.

Settings adapts to small windows and interface zoom: sections become wrapping tabs, while the heading, close button and Endpoint test/save feedback stay visible. Run `sh crates/cipher-app/scripts/check-all.sh` after building `cipher-app` and `cipher-dev` to check the desktop against its bundled fixture, including Settings at 640×480 and 160% zoom.
