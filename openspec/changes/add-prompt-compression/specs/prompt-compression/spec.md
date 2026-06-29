# Spec Delta — prompt-compression

## ADDED Requirements

### Requirement: Transparent prompt compression in the proxy path
The system SHALL, when prompt compression is enabled for a request, compress eligible
natural-language fields of the request body before forwarding it upstream, and SHALL
preserve the client-facing request and response contract unchanged (request shape,
response shape, status codes, and streaming semantics).

#### Scenario: Compression reduces forwarded prompt
- **WHEN** an enabled request contains a user message `"Please just make sure to run the tests"`
- **THEN** the body forwarded upstream contains the semantically-equivalent compressed text
  (e.g. `"ensure run the tests"`) instead of the original prose
- **AND** the response returned to the client is the upstream response unmodified.

#### Scenario: Disabled by default
- **WHEN** the system starts with no `compression` setting persisted
- **THEN** compression is disabled and the forwarded body is byte-identical to the original
  (apart from the pre-existing `stream_options` injection).

#### Scenario: Unparseable body is forwarded untouched
- **WHEN** the request body is not valid JSON (`body_json` is `None`)
- **THEN** the system forwards the raw bytes upstream without attempting compression.

### Requirement: Protocol-aware field targeting
The system SHALL compress only natural-language `system` and `user` text content for the
OpenAI, Anthropic, and Gemini protocols, and SHALL NOT modify any other field. It SHALL
never alter tool/function schemas, `assistant`/`tool`/`model` turns (unless assistant scope
is explicitly enabled), or any non-text content part (images, audio, files, `tool_use`,
`tool_result`, `functionCall`, `functionResponse`, `inlineData`, `fileData`).

#### Scenario: OpenAI user and system text compressed, tool schema untouched
- **WHEN** an OpenAI Chat Completions request has `messages` with a `system` and a `user`
  text message plus a populated `tools` array
- **THEN** the `system` and `user` text is compressed
- **AND** the `tools` array, `tool_choice`, and `response_format` are byte-identical.

#### Scenario: Multimodal parts preserved
- **WHEN** a `user` message `content` is an array containing a `{type:"text"}` part and an
  `{type:"image_url"}` part
- **THEN** only the `text` of the text part is compressed and the image part is unchanged.

#### Scenario: Anthropic system and user blocks targeted
- **WHEN** an Anthropic Messages request has a top-level `system` string and a `user`
  message with a `tool_result` block and a `{type:"text"}` block
- **THEN** the `system` string and the text block are compressed and the `tool_result`
  block is unchanged.

#### Scenario: Gemini text parts targeted by role
- **WHEN** a Gemini request has `systemInstruction.parts[].text`, a `user` content with a
  text part, and a `model` content with a text part
- **THEN** the system instruction text and the user text part are compressed and the
  `model` content is unchanged.

#### Scenario: Assistant turns skipped by default
- **WHEN** scope `assistant` is false and a request includes an `assistant`/`model` turn
- **THEN** that turn's content is forwarded unchanged.

### Requirement: Structure-preserving, code-safe compression
The compression transform SHALL only replace string leaf values; it SHALL NOT add, remove,
or reorder JSON keys or array elements. Within a targeted text field it SHALL NOT alter
content inside fenced code blocks, indented code lines, or inline backtick spans, and SHALL
NOT strip leading indentation or collapse alignment whitespace inside code.

#### Scenario: JSON shape preserved
- **WHEN** any request is compressed
- **THEN** the forwarded JSON has exactly the same keys, nesting, and array lengths as the
  original, differing only in the values of targeted text leaves.

#### Scenario: Fenced code in a prompt is untouched
- **WHEN** a user message contains a fenced ```` ```python ```` block whose body uses
  4-space indentation and the word "just" inside a string literal
- **THEN** the fenced block (including its indentation and literal text) is forwarded
  byte-identical, while filler words in the surrounding prose are still removed.

#### Scenario: Indentation never stripped
- **WHEN** a targeted field contains lines beginning with leading spaces or tabs
- **THEN** the leading whitespace of those lines is preserved in the forwarded body.

### Requirement: Per-field safety guards
The system SHALL skip compressing a field whose content is shorter than the configured
minimum size, SHALL keep the original field when the compressed result yields less than the
configured minimum savings percentage, and SHALL revert to the original field whenever the
compressed result is empty (when the original was not), longer than the original, or
otherwise invalid. The system SHALL skip compression entirely for bodies larger than the
configured maximum size and forward them uncompressed.

#### Scenario: Tiny field skipped
- **WHEN** a targeted field is shorter than `min_field_chars`
- **THEN** the field is forwarded unchanged.

#### Scenario: Negligible savings reverted
- **WHEN** compressing a field would save fewer than `min_savings_pct` percent of its tokens
- **THEN** the original field text is forwarded.

#### Scenario: Never expands a field
- **WHEN** a compression rule would make a field longer than the original
- **THEN** the original field is forwarded.

#### Scenario: Oversized body bypassed
- **WHEN** the request body exceeds `max_body_bytes`
- **THEN** no compression is attempted and the original body is forwarded.

### Requirement: Compression controls and precedence
The system SHALL provide three control levels — a global setting, an optional per-model
override, and a per-request opt-out header — and SHALL resolve them with the precedence:
per-request `off` > per-model `false` > per-model `true` > global `enabled`. The opt-out
header SHALL be stripped from the request before forwarding upstream.

#### Scenario: Per-request opt-out
- **WHEN** a request carries `X-LLMeter-Compress: off` while compression is globally enabled
- **THEN** the request is forwarded uncompressed
- **AND** the `X-LLMeter-Compress` header is not present in the request sent upstream.

#### Scenario: Per-model kill switch
- **WHEN** the matched `model_config` has `compression_enabled = false` while global is enabled
- **THEN** requests routed through that config are forwarded uncompressed.

#### Scenario: Per-model inherits global
- **WHEN** the matched `model_config` has `compression_enabled = NULL`
- **THEN** the global `enabled` value decides whether compression runs.

### Requirement: Billing consistency
Prompt compression SHALL NOT change credit-deduction logic. Credit SHALL continue to be
deducted from upstream-reported token usage, which reflects the compressed prompt.

#### Scenario: User billed on compressed usage
- **WHEN** compression reduces a prompt and the upstream reports the smaller
  `prompt_tokens`
- **THEN** the credit deducted is computed from that smaller `prompt_tokens` using the
  existing rate logic, with no compression-specific adjustment.

### Requirement: Observability and transparency
The system SHALL record, per request, whether compression ran, the mode used, the original
and forwarded prompt character counts, and the estimated tokens saved; SHALL continue to
log the original (uncompressed) request body for audit; MAY emit an additive
`X-LLMeter-Compression` response header describing the result; and SHALL expose aggregate
estimated savings through the admin stats API and UI.

#### Scenario: Per-request log records savings
- **WHEN** a request is compressed
- **THEN** its `request_logs` row has `compressed = true`, a `compression_mode`, populated
  `original_prompt_chars`/`forwarded_prompt_chars`, and a non-negative `est_tokens_saved`
- **AND** `request_body` still contains the original uncompressed body.

#### Scenario: Informational response header
- **WHEN** compression runs and `emit_response_header` is true
- **THEN** the response includes an `X-LLMeter-Compression` header summarizing mode, fields
  compressed, and estimated tokens saved, and the header is safely ignorable by clients.

#### Scenario: Aggregate savings visible
- **WHEN** an admin opens the Usage view
- **THEN** the cumulative estimated tokens saved by compression is displayed.

### Requirement: Compression configuration API
The system SHALL expose admin endpoints to read and update the global compression
configuration, persisted in `global_settings` under key `compression`, defaulting to
disabled when absent.

#### Scenario: Read default config
- **WHEN** an admin GETs the compression settings and none is persisted
- **THEN** the response reports `enabled = false` with the documented default thresholds
  and scope.

#### Scenario: Update config
- **WHEN** an admin PUTs a compression config with `enabled = true`
- **THEN** the setting is persisted and subsequent matching requests are compressed
  according to it.

### Requirement: Bounded hot-path latency
The compression pass SHALL run synchronously before the upstream request, compile its
patterns once for the process lifetime, and SHALL NOT affect response streaming.

#### Scenario: Patterns compiled once
- **WHEN** many requests are compressed over the process lifetime
- **THEN** the regex pattern set is compiled a single time (lazily) and reused.

#### Scenario: Streaming unaffected
- **WHEN** a streaming request is compressed
- **THEN** only the request body is altered and the SSE response stream is proxied exactly
  as it is today.
