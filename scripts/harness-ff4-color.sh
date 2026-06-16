#!/usr/bin/env bash
# rg-opendal parity harness: FF4 — --color flag
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-$SCRIPT_DIR/../target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-ff4-$$"

cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

if [ ! -x "$SCAFFOLD" ]; then
  echo "ERROR: scaffold binary not found at $SCAFFOLD" >&2
  exit 1
fi

mkdir -p "$tmp/local" "$tmp/out"

export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-minioadmin}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-minioadmin}"
export OPENDAL_S3_ENDPOINT="$endpoint"
export OPENDAL_S3_REGION="$region"

aws --endpoint-url "$endpoint" s3 sync "s3://$bucket/$prefix/" "$tmp/local/" >/dev/null 2>&1

passed=0
failed=0

# ── Test 1: --color=always emits ANSI escape sequences ────────────
echo "--- Test 1: Color always ---"
output=$("$SCAFFOLD" --color=always needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
if echo "$output" | grep -q $'\033\['; then
  echo "PASS: color/always-emits-ansi"
  passed=$((passed + 1))
else
  echo "FAIL: color/always-emits-ansi (no ANSI escapes found)"
  failed=$((failed + 1))
fi

# ── Test 2: --color=never does NOT emit ANSI escapes ──────────────
echo "--- Test 2: Color never ---"
output=$("$SCAFFOLD" --color=never needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
if echo "$output" | grep -qv $'\033\['; then
  echo "PASS: color/never-no-ansi"
  passed=$((passed + 1))
else
  echo "FAIL: color/never-no-ansi (ANSI escapes found)"
  failed=$((failed + 1))
fi

# ── Test 3: --color=auto when piped (not a terminal) = no color ───
echo "--- Test 3: Color auto (piped) ---"
output=$("$SCAFFOLD" --color=auto needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
if echo "$output" | grep -qv $'\033\['; then
  echo "PASS: color/auto-piped-no-ansi"
  passed=$((passed + 1))
else
  echo "FAIL: color/auto-piped-no-ansi (ANSI escapes found when piped)"
  failed=$((failed + 1))
fi

# ── Test 4: --color output matches native rg with --color=never ───
echo "--- Test 4: Color never parity with native rg ---"
rg -n --color=never needle "$tmp/local" 2>/dev/null \
  | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
  | LC_ALL=C sort > "$tmp/out/color-golden.sorted"

"$SCAFFOLD" --color=never needle "s3://$bucket/$prefix/" 2>/dev/null \
  | LC_ALL=C sort > "$tmp/out/color-actual.sorted"

if diff -u "$tmp/out/color-golden.sorted" "$tmp/out/color-actual.sorted" > "$tmp/out/color.diff" 2>&1; then
  echo "PASS: color/output-parity-with-native-rg"
  passed=$((passed + 1))
else
  echo "FAIL: color/output-parity-with-native-rg"
  cat "$tmp/out/color.diff"
  failed=$((failed + 1))
fi

# ── Test 5: --color with --json should still work ─────────────────
echo "--- Test 5: Color with JSON ---"
output=$("$SCAFFOLD" --color=always --json needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
json_errors=0
while IFS= read -r line; do
  if ! echo "$line" | python3 -c "import sys,json; json.loads(sys.stdin.read())" 2>/dev/null; then
    json_errors=$((json_errors + 1))
  fi
done <<< "$output"
if [ "$json_errors" -eq 0 ]; then
  echo "PASS: color/json-still-valid"
  passed=$((passed + 1))
else
  echo "FAIL: color/json-still-valid ($json_errors invalid lines)"
  failed=$((failed + 1))
fi

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "=== FF4 Results ==="
echo "Binary: $SCAFFOLD"
echo "Passed: $passed"
echo "Failed: $failed"
if [ "$failed" -eq 0 ]; then
  echo "Verdict: PASS_harness (FF4: --color)"
  echo "scope: always-ansi/never-no-ansi/auto-piped/output-parity/json-compat"
  exit 0
else
  echo "Verdict: FAIL_harness (FF4)"
  exit 1
fi
