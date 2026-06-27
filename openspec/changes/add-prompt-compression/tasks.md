# Tasks — Add prompt compression

Ordered, bottom-up. Do not start until the proposal is approved. Check boxes as completed.

## 1. Dependencies & module scaffold
- [x] Add `regex = "1"` to `Cargo.toml`.
- [x] Create `src/compress.rs`; declare `mod compress;` in `src/main.rs`.

## 2. Core compressor (ported, code-safe)
- [x] Port PRECC's filler/verbose-phrase `RULES` table verbatim from
      `~/precc_cc_priv/crates/precc-cc-core/src/compress.rs` (the `\b`-anchored lexical rules
      only — exclude the four whitespace rules).
- [x] Implement `compress_prose(&str) -> String`: apply lexical rules **outside** fenced
      code (```` ``` ````/`~~~`), indented-code lines (≥4 leading spaces or tab), and inline
      backtick spans; then a code-safe whitespace pass (trim trailing spaces, collapse 3+
      spaces outside code, collapse 3+ blank lines to 2; never strip leading indentation).
- [x] Port `estimate_tokens(&str) -> usize` (len/4) behind a seam for a future real tokenizer.
- [x] Unit tests: reuse PRECC's prose-compression assertions; ADD code-safety tests
      (fenced block untouched, indentation preserved, backtick span preserved, never-expand,
      JSON-leaf-only).

## 3. Config types & loading
- [x] Define `CompressionMode { Off, Prose }`, `CompressionConfig`
      (`enabled, mode, scope{system,user,assistant}, min_field_chars, min_savings_pct,
      max_body_bytes, emit_response_header`) with serde + a disabled `Default`.
- [x] `db::get_compression_config(pool) -> CompressionConfig` (mirror `get_credit_rates`,
      key `compression`).

## 4. Protocol-aware walker
- [x] In `src/protocol.rs` (or `compress.rs`), implement
      `compress_request(protocol, Value, &CompressionConfig) -> (Value, CompressionStats)`.
- [x] OpenAI Chat Completions: `messages[]` system/user, string + `text` parts; skip
      assistant/tool unless scope enables; never touch `tools`/`tool_choice`/`response_format`.
- [x] OpenAI Responses API: `instructions`, `input` string/array text items.
- [x] Anthropic: top-level `system` string/array, `user` message string/`text` blocks;
      skip `tool_result`/`image`/`document`/`tool_use` and assistant.
- [x] Gemini: `systemInstruction.parts[].text`, `contents[]` user text parts; skip `model`
      and non-text parts.
- [x] Enforce per-field guards (min size, min savings, never-expand, revert-on-invalid) and
      whole-body `max_body_bytes` bypass.
- [x] `CompressionStats { fields_compressed, chars_before, chars_after, est_tokens_saved }`.
- [x] Unit tests per protocol incl. the spec scenarios (multimodal preserved, schema
      untouched, role filtering, JSON shape identical).

## 5. Proxy integration
- [x] In `proxy_handler`, resolve effective enablement: per-request header
      `X-LLMeter-Compress` > per-model `compression_enabled` > global `enabled`.
- [x] Compute the compressed body **once** before the failover loop; reuse across attempts.
- [x] Build `forwarded_body` from the compressed body, preserving today's `stream_options`
      injection ordering; keep raw-bytes fallback when `body_json` is `None`.
- [x] Strip the `X-LLMeter-Compress` header from forwarded headers (extend `protocol::transform_headers`
      or filter at call site).
- [x] Thread `CompressionStats` + mode into `LogMeta`.

## 6. Persistence & migration
- [x] `migrations/005_prompt_compression.sql` (additive, idempotent):
      `request_logs` add `compressed`, `compression_mode`, `original_prompt_chars`,
      `forwarded_prompt_chars`, `est_tokens_saved`; `model_configs` add
      `compression_enabled BOOLEAN`; insert default `compression` global_settings row
      (`enabled=false`).
- [x] Register migration in `db::run_migrations`.
- [x] Extend both log-insert paths (`handle_normal_response`, `handle_streaming_response`)
      to write the new columns from `LogMeta`/stats.

## 7. Admin API & UI
- [x] `GET`/`PUT /api/settings/compression` in `src/admin.rs` (mirror credit_rates handlers).
- [x] Extend `/api/stats` with `total_est_tokens_saved` and compressed-request count.
- [x] Surface `compression_enabled` in model_config create/update endpoints.
- [x] `static/js/settings.js` + `index.html`: compression toggle + scope/threshold form.
- [x] `static/js/usage.js`: cumulative estimated savings tile.
- [x] `static/js/logs.js`: compression badge + estimate in Log Detail.
- [x] i18n strings (`static/js/i18n.js`) EN + ZH.

## 8. Verification
- [x] `cargo test` green (ported + new tests).
- [x] `cargo build --release` clean.
- [ ] Manual: same request with and without `X-LLMeter-Compress: off`; confirm forwarded
      body differs only in prose, response identical, log/savings correct, credit lower on
      the compressed run.
- [ ] Non-inferiority A/B (flip the opt-out header across a task set) confirms resolved-rate
      parity before recommending operators enable on real traffic.

## 9. Docs
- [x] README / README_zh: a "Prompt Compression" section (what it does, default-off,
      controls, transparency/opt-out recipe).
- [x] On ship: fold deltas into `openspec/specs/prompt-compression/spec.md` and archive
      this change.
