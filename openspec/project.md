# Project Context — LLMeter

## Purpose
LLMeter is a multi-protocol LLM API gateway. It accepts OpenAI-, Anthropic-, and
Gemini-shaped requests from end users (authenticated by an LLMeter-issued API key),
forwards them to the configured upstream provider using the operator's *real* provider
keys, meters token usage, and deducts prepaid credit from the requesting organization.

It is a **transparent reverse proxy**: the client-facing request/response contract is
identical to the upstream provider's. The proxy core (`src/proxy.rs`) forwards the body
nearly unchanged — the only existing mutation is injecting `stream_options.include_usage`
for OpenAI streaming so usage can be metered.

## This change set
We are integrating the **predictive prompt-compression** capability from PRECC
(`~/precc_cc_priv`, crate `precc-cc-core`, module `compress.rs`) into the proxy path so
that fewer input tokens are piped to the external LLM, while delivering the same service.
Because credit is deducted from upstream-reported usage, shrinking the prompt reduces both
the operator's upstream bill and the user's credit consumption automatically. The work
must be **transparent** in both senses: seamless (API contract unchanged) and observable
(savings are logged, surfaced, and opt-out-able).

## Tech Stack
- Rust 2021, `axum` 0.8 + `tokio`, `reqwest` for upstream calls.
- PostgreSQL via `sqlx`; idempotent SQL migrations in `migrations/` run at startup
  (`db::run_migrations`).
- Embedded static admin SPA (`static/`, vanilla JS) served via `rust-embed`.
- `regex` is NOT yet a dependency of LLMeter; the ported compressor introduces it
  (PRECC uses `regex` + `std::sync::LazyLock`).

## Key Code Paths
- `src/proxy.rs::proxy_handler` — request entry; parses body, detects protocol, extracts
  model, builds `forwarded_body`, forwards upstream, meters usage, deducts credit.
- `src/protocol.rs` — `Protocol` enum, `detect_protocol`, `extract_model`, header/usage
  helpers. The natural home for protocol-aware field walking.
- `src/db.rs` — `ModelConfig`, `CreditRates`, `global_settings` access
  (`get_credit_rates`). New `get_compression_config` will follow the same pattern.
- `src/admin.rs` — admin REST API + router; `/api/settings/credit_rates`,
  `/api/logs`, `/api/stats`. New compression settings + stats hang here.
- `migrations/00X_*.sql` — additive, `IF NOT EXISTS`-guarded.
- `static/js/{settings,usage,logs}.js` — admin UI surfaces.

## Conventions
- Migrations are additive and idempotent (`ADD COLUMN IF NOT EXISTS`, `CREATE TABLE IF NOT EXISTS`).
- Comments and tracing in this repo are bilingual (Chinese + English); match the file.
- Logging/credit work happens in `tokio::spawn` off the response hot path — keep it there.
- Never break the upstream API contract. Any new client-facing surface (headers) must be
  additive and ignorable by strict SDK clients.

## Source of Ported Logic
- `~/precc_cc_priv/crates/precc-cc-core/src/compress.rs` — deterministic prose compressor
  (filler-word removal, verbose-phrase rewrites, whitespace cleanup; ~MIT-0, ported from
  token-saver by RubenAQuispe). Structure-preserving for markdown; **must be adapted to be
  code-safe** for arbitrary user prompts (see the change's `design.md`).
- `~/precc_cc_priv/crates/precc-cc-core/src/compression_prompt.rs` — adapter for the
  optional lossy statistical `compression-prompt` backend (future pluggable mode).
