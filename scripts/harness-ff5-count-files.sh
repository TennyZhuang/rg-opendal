#!/usr/bin/env bash
# rg-opendal parity harness: FF5 — -c/--count and -l/--files-with-matches
# Verifies count and files-with-matches output matches native rg
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-$SCRIPT_DIR/../target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-ff5-$$"

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

# ── Test 1: -c produces path:COUNT output ─────────────────────────
echo "--- Test 1: Count mode ---"
rg -c needle "$tmp/local" 2>/dev/null \
  | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
  | LC_ALL=C sort > "$tmp/out/count-golden.sorted"

"$SCAFFOLD" -c needle "s3://$bucket/$prefix/" 2>/dev/null \
  | LC_ALL=C sort > "$tmp/out/count-actual.sorted"

if diff -u "$tmp/out/count-golden.sorted" "$tmp/out/count-actual.sorted" > "$tmp/out/count.diff" 2>&1; then
  echo "PASS: count/output-parity"
  passed=$((passed + 1))
else
  echo "FAIL: count/output-parity"
  cat "$tmp/out/count.diff"
  failed=$((failed + 1))
fi

# ── Test 2: -l produces path-only output ──────────────────────────
echo "--- Test 2: Files-with-matches mode ---"
rg -l needle "$tmp/local" 2>/dev/null \
  | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
  | LC_ALL=C sort > "$tmp/out/files-golden.sorted"

"$SCAFFOLD" -l needle "s3://$bucket/$prefix/" 2>/dev/null \
  | LC_ALL=C sort > "$tmp/out/files-actual.sorted"

if diff -u "$tmp/out/files-golden.sorted" "$tmp/out/files-actual.sorted" > "$tmp/out/files.diff" 2>&1; then
  echo "PASS: files-with-matches/output-parity"
  passed=$((passed + 1))
else
  echo "FAIL: files-with-matches/output-parity"
  cat "$tmp/out/files.diff"
  failed=$((failed + 1))
fi

# ── Test 3: -c no-match exit code ─────────────────────────────────
echo "--- Test 3: Count no-match ---"
set +e
"$SCAFFOLD" -c nonexistent_pattern_xyz "s3://$bucket/$prefix/" >/dev/null 2>&1
status=$?
set -e
if [ "$status" -eq 1 ]; then
  echo "PASS: count/no-match-exit (exit=1)"
  passed=$((passed + 1))
else
  echo "FAIL: count/no-match-exit (expected exit 1, got $status)"
  failed=$((failed + 1))
fi

# ── Test 4: -c with glob filter ───────────────────────────────────
echo "--- Test 4: Count with glob ---"
rg -c -g '*.rs' needle "$tmp/local" 2>/dev/null \
  | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
  | LC_ALL=C sort > "$tmp/out/count-glob-golden.sorted"

"$SCAFFOLD" -c -g '*.rs' needle "s3://$bucket/$prefix/" 2>/dev/null \
  | LC_ALL=C sort > "$tmp/out/count-glob-actual.sorted"

if diff -u "$tmp/out/count-glob-golden.sorted" "$tmp/out/count-glob-actual.sorted" > "$tmp/out/count-glob.diff" 2>&1; then
  echo "PASS: count/glob-parity"
  passed=$((passed + 1))
else
  echo "FAIL: count/glob-parity"
  cat "$tmp/out/count-glob.diff"
  failed=$((failed + 1))
fi

# ── Test 5: --streaming with -c ───────────────────────────────────
echo "--- Test 5: Count with streaming ---"
if "$SCAFFOLD" --help 2>&1 | grep -q '\--streaming'; then
  "$SCAFFOLD" -c needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/count-default.sorted"
  "$SCAFFOLD" --streaming -c needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/count-streaming.sorted"
  if diff -u "$tmp/out/count-default.sorted" "$tmp/out/count-streaming.sorted" >/dev/null 2>&1; then
    echo "PASS: count/streaming-parity"
    passed=$((passed + 1))
  else
    echo "FAIL: count/streaming-parity"
    failed=$((failed + 1))
  fi
else
  echo "SKIP: count/streaming-parity (no --streaming flag)"
fi

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "=== FF5 Results ==="
echo "Binary: $SCAFFOLD"
echo "Passed: $passed"
echo "Failed: $failed"
if [ "$failed" -eq 0 ]; then
  echo "Verdict: PASS_harness (FF5: count/files-with-matches)"
  echo "scope: count-parity/files-parity/no-match-exit/glob-parity/streaming-parity"
  exit 0
else
  echo "Verdict: FAIL_harness (FF5)"
  exit 1
fi
