#!/usr/bin/env bash
# Verify prompt compression is transparent: send the same request twice — once with
# X-LLMeter-Compress: off (passthrough) and once normally (compressed) — and compare.
#
# Confirms (1) the compressed run reports savings via the X-LLMeter-Compression header,
# and (2) the response is otherwise functionally identical. Requires a running LLMeter
# with compression enabled (Settings → Prompt Compression) and a valid API key.
#
# Usage:
#   BASE=http://localhost:5000 KEY=gc-xxxxxxxx MODEL=gpt-4o bash scripts/verify_compression.sh
set -euo pipefail

BASE="${BASE:-http://localhost:5000}"
KEY="${KEY:?set KEY to an LLMeter API key (gc-...)}"
MODEL="${MODEL:-gpt-4o}"

req_file="$(mktemp)"
trap 'rm -f "$req_file" off.body on.body off.head on.head' EXIT

# A prompt deliberately full of filler/verbose prose so compression has something to do.
cat > "$req_file" <<JSON
{
  "model": "$MODEL",
  "messages": [
    {"role": "system", "content": "Please just be concise. In order to help the user, you should basically answer directly."},
    {"role": "user", "content": "Kindly make sure to actually summarize, in terms of the main points, what a reverse proxy does, for example in the context of LLM APIs, and so on."}
  ]
}
JSON

echo "→ Run A: passthrough (X-LLMeter-Compress: off)"
curl -sS -D off.head -o off.body "$BASE/v1/chat/completions" \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -H "X-LLMeter-Compress: off" --data @"$req_file"

echo "→ Run B: compressed (default)"
curl -sS -D on.head -o on.body "$BASE/v1/chat/completions" \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  --data @"$req_file"

echo
echo "── X-LLMeter-Compression header ──"
echo "  A (off): $(grep -i '^x-llmeter-compression:' off.head || echo '(absent — expected, passthrough)')"
echo "  B (on):  $(grep -i '^x-llmeter-compression:' on.head  || echo '(absent — compression may be disabled or savings below threshold)')"

echo
echo "── HTTP status ──"
echo "  A: $(head -1 off.head | tr -d '\r')   B: $(head -1 on.head | tr -d '\r')"

# Both runs should succeed and return the same response shape (content will differ as LLM
# output is non-deterministic; we check structural keys, not exact text).
if command -v jq >/dev/null 2>&1; then
  echo
  echo "── Response shape (top-level keys) ──"
  a_keys="$(jq -rS 'keys|join(",")' off.body 2>/dev/null || echo 'parse-error')"
  b_keys="$(jq -rS 'keys|join(",")' on.body  2>/dev/null || echo 'parse-error')"
  echo "  A: $a_keys"
  echo "  B: $b_keys"
  if [ "$a_keys" = "$b_keys" ] && [ "$a_keys" != "parse-error" ]; then
    echo "  ✓ identical response shape"
  else
    echo "  ✗ response shapes differ — investigate"
  fi
fi

echo
echo "Done. The original (uncompressed) request body is what LLMeter logs for both runs;"
echo "inspect the Log Detail view to see 'Compressed' + est. tokens saved on run B."
