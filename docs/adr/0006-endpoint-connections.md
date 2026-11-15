# Endpoint connections: API keys and Sign in with ChatGPT

Amends ADR 0001's "bring-your-own-key only".

An Endpoint connects in one of two ways:

1. **An API key for an OpenAI-compatible base URL.** This covers OpenRouter, DeepSeek, local servers, and OpenCode Zen and Go. OpenCode Go is a $10/month plan used with a key at `https://opencode.ai/zen/go/v1`, and no OAuth is involved.
2. **Sign in with ChatGPT, the way Codex does** (amended 2026-11-15).
   - **Flow:** OAuth with PKCE and a loopback redirect, under Codex's public client id. There is no client secret.
   - **Requests:** go to the Codex backend's Responses endpoint, and spend the plan's Codex limits.
   - **Eligibility:** every plan Codex works on, Free included.
   - **Why not OpenAI's Sign in with ChatGPT program:** the first route (`dynamic_agent_client`, `chatgpt.tokens.use.direct`) failed its first real sign-in with `invalid_grant`, and it serves paid plans only. The user asked for Free plans with Codex limits.
   - **Trade-off:** Cipher presents itself as Codex (`originator: codex_cli_rs`), as OpenCode's Codex plugin does. OpenAI could restrict this client to Codex at any time. If it does, the API-key route is the fallback.

Cipher speaks two wire protocols, Chat Completions and Responses. Responses is needed for ChatGPT plans and for models some providers serve only that way.

Credentials, meaning API keys and OAuth refresh tokens, live in the OS keychain, never in a Book or a config file.

## Considered Options

- **Claude Pro or Max subscription sign-in:** rejected. Anthropic's terms (February 2026) forbid using subscription OAuth tokens outside Claude Code and Claude.ai, and they block it server-side. Anthropic models remain usable with an API key through Anthropic's OpenAI SDK compatibility endpoint or a gateway.
- **Chat Completions only:** rejected. It would exclude ChatGPT plans and Responses-only models.

## Consequences

- A ChatGPT plan's Codex limits are shared with Codex itself and any other app using them. Cipher must treat "usage limit reached" as a normal pause that resumes later, not a failure.
- Reasoning effort can be set on the ChatGPT-plan route (checked 2026-11-16 with `low`); the backend does not report reasoning tokens.
- Implementation facts (`crates/cipher-llm/src/chatgpt.rs`), from `openai/codex` rust-v0.169.0 (`codex-rs/login`, `codex-rs/codex-api`) and `numman-ali/opencode-openai-codex-auth`:
  - client `app_EMoamEEZ73f0CkXaXp7hrann`; authorize `https://auth.openai.com/oauth/authorize`, token `…/oauth/token`, JWKS `…/.well-known/jwks.json` (RS256);
  - scope `openid profile email offline_access`, plus `id_token_add_organizations=true`, `codex_cli_simplified_flow=true`, `originator=codex_cli_rs`, and `login_hint` on a later sign-in;
  - redirect `http://127.0.0.1:{1455|1457}/auth/callback`. Only these two ports are registered, so if both are taken (a Codex sign-in waiting, say) the sign-in fails with a message;
  - exchange and refresh are form posts with `client_id` and no `resource`;
  - the account is the ID token's `https://api.openai.com/auth` › `chatgpt_account_id`, and the plan is `chatgpt_plan_type`;
  - requests go to `https://chatgpt.com/backend-api/codex/responses` with `chatgpt-account-id`, `OpenAI-Beta: responses=experimental` and `originator`, plus `store: false`, `stream: true` and no `max_output_tokens`;
  - the plan's models come from `…/codex/models?client_version=0.169.0` (`visibility: list`, by `priority`) and fill the form after sign-in;
  - a 429 `usage_limit_reached` is a usage limit, with `resets_at` in unix seconds; `usage_not_included` means the plan lacks Codex.
- Checked with a real sign-in (2026-11-16, Free plan, gpt-6-luna): the backend accepts Cipher's own `instructions` (OpenCode's plugin sends Codex's prompt instead) and `reasoning.effort`, and the model list (gpt-6-luna, gpt-5.6-terra, gpt-5.6-luna) fills the form.
- Sources, checked 2026-11-11:
  - github.com/openai/codex (rust-v0.169.0, checked 2026-11-15)
  - github.com/numman-ali/opencode-openai-codex-auth (checked 2026-11-15)
  - developers.openai.com/siwc (the route this replaced)
  - opencode.ai/docs/en/go
  - The Register, 2026-02-20, on Anthropic's terms
