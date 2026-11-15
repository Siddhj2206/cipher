# The desktop app publishes the Reader

Reading on the phone happens in the Reader, a private website built with Astro and running on a Cloudflare Worker in the user's own Cloudflare account. Chapter text lives in Cloudflare storage (D1/R2). Publish uploads only changed data from the desktop app; it never builds the site. Pages are rendered on request and cached at the edge. This replaces the old pipeline (cipher CLI → hardcoded copy script → separate Astro repo → git commit → deploy). The Reader's code lives in this repo, and Publish is one action inside Cipher.

## Considered Options

- **Run the Rune app in the browser as the Reader**: rejected. A WebGPU canvas app is heavy on a phone and loses native text selection, reader mode, font scaling, and accessibility. Plain HTML reads better.
- **Static site built on the laptop and uploaded to Cloudflare Pages**: rejected. Every Publish would need Node or bun on the laptop and a full Astro rebuild of thousands of Chapter pages, and it leaves no backend for later phone features.
- **Keep a separate site repo fed by git commits**: rejected. That is the friction this rewrite exists to remove.

## Consequences

- The desktop app runs on a laptop that is often asleep or offline. The Reader must never depend on it being reachable. The phone talks only to Cloudflare, and anything from the phone (later: Suggestions, reading progress) is queued there until the laptop next opens Cipher.
- Translation and Publish are interrupted whenever the lid closes, so both must resume where they stopped, never restart or leave the Reader half-updated.
- The same Worker is where phone Suggestions and reading-progress sync will live later, so they need no new infrastructure.
- Changing the Reader's code is a separate, rare deploy of the Worker, distinct from Publishing content.
- The Reader is for the user alone, so it sits behind Cloudflare Access rather than being public.
- The desktop app holds a Cloudflare API token. Like LLM keys, it belongs to the user's machine and never lives in a Book.
