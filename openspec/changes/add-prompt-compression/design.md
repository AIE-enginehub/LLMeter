# Design — Prompt Compression in the Proxy Path

## 1. Goal & constraints
Reduce input tokens forwarded upstream without changing the service. Hard constraints:
1. **API contract is sacred.** Request/response shapes, status codes, and streaming
   semantics seen by the client must be unchanged.
2. **Never corrupt content.** Compression must not alter the *meaning* of a prompt, and
   must never damage code, JSON, or structured payloads embedded in prose.
3. **Hot-path cheap.** The pass runs synchronously before forwarding; budget is low
   single-digit milliseconds (PRECC measures sub-ms for comparable inputs).
4. **Honest & reversible.** Savings are estimated and logged; users can opt out and verify.

## 2. Where it hooks in
`proxy_handler` (`src/proxy.rs`) currently builds `forwarded_body` from `body_json`,
injecting `stream_options` only for OpenAI streaming. We add one stage:

```
parse body_json
detect_protocol / extract_model        (unchanged)
credit check                            (unchanged)
find model_configs                      (unchanged)
┌─ NEW: if compression enabled & not opted out:
│     let (compressed_json, stats) = compress::compress_request(actual_protocol,
│                                       body_json.clone(), &cfg);
│     use compressed_json to build forwarded_body
│     carry `stats` into LogMeta
└─ else: forwarded_body from original body_json   (today's behavior)
inject stream_options on the *forwarded* body      (unchanged ordering)
forward upstream / stream / meter / deduct          (unchanged)
```

Key points:
- The compression decision is per-`model_config` attempt? No — compute **once** before the
  failover loop, keyed on `actual_protocol` of the first config. Re-running per attempt is
  wasteful and the protocol is stable for a given request. Compute the compressed body
  once; reuse across failover attempts (mirrors how `body_bytes` is reused today).
- The logging copy always comes from the **original**, uncompressed `body_json`. To bound
  storage, a successful request with a persisted charge keeps at most the first 64 KiB;
  errors, passthrough requests, and charge failures keep the full original body. The
  savings estimate and a `compressed=true` flag are logged alongside.

## 3. The code-safety problem (the crux)
PRECC's `compress.rs` was written for **CLAUDE.md / memory files** — markdown prose. Two of
its rules are unsafe for arbitrary prompts:

| PRECC rule | Effect | Hazard on prompts |
|---|---|---|
| `\n +` → `\n` | strip leading whitespace on every line | **destroys code indentation** (Python, YAML, diffs) |
| `  +` → ` ` | collapse runs of spaces | breaks aligned tables, ASCII art, code |

User prompts routinely embed code, stack traces, diffs, and tables. So the port keeps the
**lexical** rules (filler words + verbose-phrase rewrites — all `\b`-anchored, whitespace-
neutral, safe) and replaces the **whitespace** rules with a code-safe pass:

- Tokenize the text into fenced-code regions (```` ``` ```` / `~~~`) and prose regions.
  Apply *no* substitution inside fenced code.
- Within prose regions, also skip any line that looks like code/preformatting: lines with
  ≥4 leading spaces or a leading tab (markdown indented-code convention), and inline
  spans wrapped in single backticks.
- Whitespace cleanup is limited to: collapse runs of **3+** spaces to one *only outside
  code/backtick spans*, trim trailing spaces, collapse 3+ blank lines to 2. Never touch
  leading indentation.

The filler/phrase replacement table is ported **verbatim** from PRECC (it is the measured,
reviewed core) and lives behind the same `Rule { re, replacement }` + `LazyLock<Vec<Rule>>`
structure. This keeps the two implementations diffable and lets us re-sync if PRECC's table
evolves.

> Note: even the lexical rules can technically alter a string literal inside code that we
> failed to fence (e.g. a one-line `print("please just run")`). The fenced/indented/backtick
> guards cover the common cases; the conservative scope (system+user prose) plus per-field
> revert and default-off rollout bound the residual risk. The non-inferiority benchmark is
> the backstop that proves output quality is preserved before anyone enables it widely.

## 4. Protocol-aware field targeting
Only these fields are eligible (everything else is forwarded untouched):

**OpenAI Chat Completions** (`/v1/chat/completions`)
- `messages[]` where `role ∈ {system, user}`:
  - string `content` → compress.
  - array `content` → for each part with `type == "text"`, compress `part.text`; skip
    `image_url`, `input_audio`, `file`, etc.
- Skip `role ∈ {assistant, tool, function}` by default (assistant turns may carry text the
  user expects verbatim; tool turns are data). Scope is configurable; assistant is opt-in.
- Never touch `tools`, `functions`, `tool_choice`, `response_format`, `messages[].name`,
  `tool_call_id`, or any non-content field.

**OpenAI Responses API** (`/v1/responses`)
- `instructions` (string).
- `input`: string → compress; array → compress `text` of `input_text` parts on
  `role ∈ {system, user, developer}` items only.

**Anthropic Messages** (`/v1/messages`, `/anthropic/...`)
- top-level `system`: string → compress; array → compress `text` of `{type:"text"}` blocks.
- `messages[]` where `role == "user"`: string content → compress; array → compress `text`
  of `{type:"text"}` blocks; skip `tool_result`, `image`, `document`, `tool_use`.
- Skip `role == "assistant"` by default.

**Gemini** (`/v1beta/models/...:generateContent`)
- `systemInstruction.parts[].text`.
- `contents[]` where `role ∈ {user, ""}` (Gemini uses `user`/`model`): compress
  `parts[].text`; skip `inlineData`, `fileData`, `functionCall`, `functionResponse` parts.
- Skip `role == "model"`.

The walker is a single function `compress_request(protocol, value, cfg) -> (Value, Stats)`
that returns a new `serde_json::Value` plus per-request `Stats { fields_compressed,
chars_before, chars_after, est_tokens_saved }`.

## 5. Safety guards (ported philosophy from PRECC)
- **Min size floor**: skip any field shorter than `min_field_chars` (default 80) — mirrors
  PRECC's "never net-negative on tiny input".
- **Min savings**: if a field's estimated savings `< min_savings_pct` (default 5) of its
  tokens, keep the original field (avoid churn for negligible gain). Whole-request stats
  still reported.
- **Per-field revert / never-expand**: if a compressed field is empty when the original
  wasn't, or is *longer* than the original, or is not valid UTF-8 prose, keep the original.
- **Structural integrity**: the walker only ever replaces string leaves; it never adds,
  removes, or reorders keys/array elements, so the JSON shape is provably preserved.
- **Body size cap**: skip compression entirely above `max_body_bytes` (default 8 MiB) to
  bound worst-case latency; the request is forwarded uncompressed (still correct).
- **Destructive/structured passthrough**: if `body_json` failed to parse (already handled —
  `body_json` is `None`), there is nothing to compress; forward raw bytes as today.

## 6. Token estimation
Reuse PRECC's `estimate_tokens = len/4` heuristic for the **logged estimate** only, labeled
"est." The *authoritative* savings are implicit in the real upstream `usage.prompt_tokens`
that LLMeter already meters; we do not claim the estimate is exact. (A future enhancement
could swap in a real BPE tokenizer behind the same `estimate_tokens` seam.)

## 7. Control plane
`global_settings` row `key = 'compression'`, JSONB value:
```json
{
  "enabled": false,
  "mode": "prose",
  "scope": { "system": true, "user": true, "assistant": false },
  "min_field_chars": 80,
  "min_savings_pct": 5,
  "max_body_bytes": 8388608,
  "emit_response_header": true
}
```
- Loaded via `db::get_compression_config(pool)` (same shape as `get_credit_rates`), with a
  hardcoded default (disabled) when the row is absent.
- **Per-model override**: `model_configs.compression_enabled BOOLEAN NULL` — `NULL` inherits
  global, `true`/`false` overrides. Lets an operator disable compression for a model known
  to be format-sensitive.
- **Per-request opt-out**: header `X-LLMeter-Compress: off` (case-insensitive value
  `off`/`0`/`false`) forces passthrough for that request. The header is stripped from the
  forwarded request (it is LLMeter-internal). A matching `on` value can force-enable for
  testing even when globally enabled — but never overrides a `false` per-model setting.

Precedence: per-request `off` > per-model `false` > per-model `true` > global `enabled`.

## 8. Observability & transparency
- **request_logs** new columns (migration 005):
  - `compressed BOOLEAN NOT NULL DEFAULT false`
  - `compression_mode VARCHAR(20)`
  - `original_prompt_chars INT`, `forwarded_prompt_chars INT`
  - `est_tokens_saved INT`
- **Response header** (when `emit_response_header` and compression ran):
  `X-LLMeter-Compression: mode=prose; fields=3; est_saved=128`. Purely informational,
  additive, ignored by SDK clients.
- **Admin**: `/api/stats` gains `total_est_tokens_saved` and a compression-on request
  count; the Usage page shows cumulative estimated savings; the Log Detail view shows the
  per-request compression badge + estimate. Because the original body is logged, a reviewer
  can always see exactly what the user sent.
- **Opt-out verifiability**: documented recipe — send the same request twice, once with
  `X-LLMeter-Compress: off`, and diff. This is the user-facing "trust but verify" path.

## 9. Pluggable backend seam
`enum CompressionMode { Off, Prose }` today; designed to grow `Statistical` (wrapping
PRECC's opt-in `compression-prompt`, lossy ~50%) later. `compress_request` dispatches on
mode. Statistical mode would carry its own stronger guards and remains out of scope here.

## 10. Performance
- Regexes compiled once via `LazyLock` (as in PRECC). Pass is linear over targeted fields.
- Runs on the request task before the upstream call; adds latency strictly *before* the
  network round-trip that dominates. For pathological bodies the `max_body_bytes` cap and
  `min_field_chars` floor bound work. If profiling shows >~3 ms p99, move the pass to
  `tokio::task::spawn_blocking`; not expected to be necessary.
- Streaming responses are entirely unaffected — compression touches only the request.

## 11. Rollout
1. Ship disabled by default. Operators enable via Settings.
2. Provide a `--tasks N` run of the existing non-inferiority benchmark
   (`scripts/benchmark*.sh` exist in PRECC; LLMeter adds a thin A/B that flips the header)
   to confirm resolved-rate parity before enabling on real traffic.
3. Per-model override gives a fast kill-switch for any model that misbehaves.
