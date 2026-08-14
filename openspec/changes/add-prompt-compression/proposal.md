# Change: Add predictive prompt compression to the proxy path

## Status
Proposed — not yet implemented. (No code changes land until this proposal is approved.)

## Why
LLMeter forwards user requests to external LLM providers using the operator's paid
provider keys. Every input token in the request body costs money upstream and burns the
organization's prepaid credit. A large fraction of those tokens is **prose**: filler words
("please", "just", "basically"), verbose phrasings ("in order to", "due to the fact
that"), and redundant whitespace — text that does not change what the model is asked to do.

PRECC (`~/precc_cc_priv`) already ships a measured, deterministic compressor for exactly
this prose (`crates/precc-cc-core/src/compress.rs`), reporting real token savings with
sub-millisecond latency. Porting its predictive feature into LLMeter's proxy lets us send
fewer tokens upstream **while delivering the same service** — the model receives a terser
but semantically equivalent prompt, the operator pays less, and because credit is deducted
from upstream-reported usage, the user's credit consumption drops in lockstep with no
billing logic change.

The integration must be **transparent**:
- *Seamless* — the client-facing API contract (request shape, response shape, streaming
  behavior) is unchanged.
- *Observable & honest* — every compressed request is logged with its estimated savings,
  aggregate savings are visible in the admin dashboard, and any user can opt out per
  request and verify byte-for-byte that behavior is otherwise identical.

## What Changes
- **New proxy stage**: between body parse and `forwarded_body` construction in
  `proxy_handler`, a protocol-aware compression pass rewrites only natural-language text
  fields and forwards the shrunk body upstream. The logging copy always comes from the
  original body; successful charged requests retain at most its first 64 KiB, while error,
  passthrough, and charge-failure logs retain the full body.
- **New module `src/compress.rs`**: a *code-safe* port of PRECC's `compress.rs`. The filler/
  phrase lexical rules are reused verbatim; the whitespace rules are replaced with a
  code-safe variant (fenced-code-aware, never strips leading indentation) because user
  prompts — unlike CLAUDE.md — routinely contain code. Plus a protocol-aware walker that
  targets the right JSON fields per protocol.
- **Field targeting** (`src/protocol.rs`): compress only `system`/`user` text content for
  OpenAI, Anthropic, and Gemini; never touch tool schemas, `assistant`/`tool` messages,
  images, audio, `tool_use`/`tool_result` blocks, or any non-`text` content part.
- **Safety guards**: per-field try-and-revert (invalid/oversized output → keep original),
  a minimum-savings threshold, a minimum-size floor (skip tiny content), and a hard skip
  for any field the walker cannot prove is plain prose.
- **Control plane**: a new `compression` key in `global_settings` (enabled flag, mode,
  thresholds, scope), an optional per-`model_config` override column, and a per-request
  opt-out header (`X-LLMeter-Compress: off`). Admin REST + UI to read/write the setting.
- **Observability**: new `request_logs` columns recording whether compression ran, the
  mode, and estimated tokens saved; an optional `X-LLMeter-Compression` response header;
  aggregate savings in `/api/stats` and the Usage page.
- **Data model**: additive migration `005_prompt_compression.sql` (new columns + default
  global setting).
- **Pluggable backend seam**: a `CompressionMode` enum so the deterministic `prose` mode
  can later sit alongside an opt-in lossy statistical backend
  (PRECC's `compression-prompt`) without re-architecting.

## Impact
- **Affected capability**: `prompt-compression` (new).
- **Affected code**: `src/proxy.rs` (new stage), `src/protocol.rs` (field walker),
  new `src/compress.rs`, `src/db.rs` (`get_compression_config`), `src/admin.rs`
  (settings + stats endpoints), `src/main.rs` (module decl), `Cargo.toml` (`regex` dep),
  `migrations/005_prompt_compression.sql`, `static/js/{settings,usage}.js`,
  `static/index.html`, i18n strings.
- **Backward compatibility**: fully backward compatible. Shipped **disabled by default**
  (`compression.enabled = false`); enabling is an explicit operator choice. With it off,
  the proxy path is byte-identical to today.
- **Billing**: no change to credit math. Deduction continues to use upstream-reported
  usage, which now reflects the smaller prompt.
- **Risk**: prompt mutation could in principle change model output. Mitigated by
  restricting to filler/phrase prose, code-safety, conservative scope (system+user only),
  per-field revert, opt-out, default-off rollout, and the non-inferiority benchmark in
  `tasks.md`.
