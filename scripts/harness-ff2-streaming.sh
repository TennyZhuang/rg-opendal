#!/usr/bin/env bash
# rg-opendal parity harness: FF2 glob/type with --streaming pass
# Extends the FF2 harness to also verify --streaming flag parity.
# Reuses the existing FF2 fixture and harness pattern.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-$SCRIPT_DIR/../target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-streaming-$$"

cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

if [ ! -x "$SCAFFOLD" ]; then
  echo "ERROR: scaffold binary not found at $SCAFFOLD" >&2
  exit 1
fi

# Verify --streaming flag exists
if ! "$SCAFFOLD" --help 2>&1 | grep -q '\--streaming'; then
  echo "SKIP: --streaming flag not available in this build"
  exit 0
fi

mkdir -p "$tmp/local" "$tmp/out"

export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-minioadmin}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-minioadmin}"
export OPENDAL_S3_ENDPOINT="$endpoint"
export OPENDAL_S3_REGION="$region"

# Download fixture
aws --endpoint-url "$endpoint" s3 sync "s3://$bucket/$prefix/" "$tmp/local/" >/dev/null 2>&1

passed=0
failed=0

# ── Test 1: --streaming matches default output ────────────────────
echo "--- Test 1: Streaming vs default output parity ---"
"$SCAFFOLD" needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/default.sorted"
"$SCAFFOLD" --streaming needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/streaming.sorted"

if diff -u "$tmp/out/default.sorted" "$tmp/out/streaming.sorted" > "$tmp/out/parity.diff" 2>&1; then
  echo "PASS: streaming/output-parity"
  passed=$((passed + 1))
else
  echo "FAIL: streaming/output-parity"
  cat "$tmp/out/parity.diff"
  failed=$((failed + 1))
fi

# ── Test 2: --streaming with glob filter ───────────────────────────
echo "--- Test 2: Streaming with glob ---"
"$SCAFFOLD" -g '*.rs' needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/glob-default.sorted"
"$SCAFFOLD" --streaming -g '*.rs' needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/glob-streaming.sorted"

if diff -u "$tmp/out/glob-default.sorted" "$tmp/out/glob-streaming.sorted" > "$tmp/out/glob-parity.diff" 2>&1; then
  echo "PASS: streaming/glob-parity"
  passed=$((passed + 1))
else
  echo "FAIL: streaming/glob-parity"
  cat "$tmp/out/glob-parity.diff"
  failed=$((failed + 1))
fi

# ── Test 3: --streaming exit code on no-match ─────────────────────
echo "--- Test 3: Streaming no-match exit ---"
set +e
"$SCAFFOLD" --streaming -g '*.xyz' needle "s3://$bucket/$prefix/" >/dev/null 2>&1
status=$?
set -e
if [ "$status" -eq 1 ]; then
  echo "PASS: streaming/no-match-exit (exit=1)"
  passed=$((passed + 1))
else
  echo "FAIL: streaming/no-match-exit (expected exit 1, got $status)"
  failed=$((failed + 1))
fi

# ── Test 4: --streaming with --json ───────────────────────────────
echo "--- Test 4: Streaming with JSON ---"
output=$("$SCAFFOLD" --streaming --json needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
json_errors=0
while IFS= read -r line; do
  if ! echo "$line" | python3 -c "import sys,json; json.loads(sys.stdin.read())" 2>/dev/null; then
    json_errors=$((json_errors + 1))
  fi
done <<< "$output"
if [ "$json_errors" -eq 0 ]; then
  echo "PASS: streaming/json-valid"
  passed=$((passed + 1))
else
  echo "FAIL: streaming/json-valid ($json_errors invalid lines)"
  failed=$((failed + 1))
fi

# ── Test 5: --streaming with --stats ──────────────────────────────
echo "--- Test 5: Streaming with stats ---"
stats_out=$("$SCAFFOLD" --streaming --stats needle "s3://$bucket/$prefix/src/main.rs" 2>&1)
if echo "$stats_out" | grep -q "matches"; then
  echo "PASS: streaming/stats"
  passed=$((passed + 1))
else
  echo "FAIL: streaming/stats"
  failed=$((failed + 1))
fi

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "=== Streaming Harness Results ==="
echo "Binary: $SCAFFOLD"
echo "Passed: $passed"
echo "Failed: $failed"
if [ "$failed" -eq 0 ]; then
  echo "Verdict: PASS_harness (--streaming parity)"
  echo "scope: output-parity/glob-parity/no-match-exit/json-valid/stats"
  exit 0
else
  echo "Verdict: FAIL_harness (--streaming)"
  exit 1
fi
