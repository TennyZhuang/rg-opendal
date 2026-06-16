#!/usr/bin/env bash
# rg-opendal parity harness: FF6 — -w/--word-regexp and -x/--line-regexp
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-$SCRIPT_DIR/../target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-ff6-$$"

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

# ── Test 1: -w matches whole words ────────────────────────────────
echo "--- Test 1: Word regexp ---"
rg -n -w needle "$tmp/local" 2>/dev/null \
  | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
  | LC_ALL=C sort > "$tmp/out/word-golden.sorted"

"$SCAFFOLD" -w needle "s3://$bucket/$prefix/" 2>/dev/null \
  | LC_ALL=C sort > "$tmp/out/word-actual.sorted"

if diff -u "$tmp/out/word-golden.sorted" "$tmp/out/word-actual.sorted" > "$tmp/out/word.diff" 2>&1; then
  echo "PASS: word-regexp/output-parity"
  passed=$((passed + 1))
else
  echo "FAIL: word-regexp/output-parity"
  cat "$tmp/out/word.diff"
  failed=$((failed + 1))
fi

# ── Test 2: -x matches full lines ─────────────────────────────────
echo "--- Test 2: Line regexp ---"
rg -n -x "needle in a text file" "$tmp/local" 2>/dev/null \
  | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
  | LC_ALL=C sort > "$tmp/out/line-golden.sorted"

"$SCAFFOLD" -x "needle in a text file" "s3://$bucket/$prefix/" 2>/dev/null \
  | LC_ALL=C sort > "$tmp/out/line-actual.sorted"

if diff -u "$tmp/out/line-golden.sorted" "$tmp/out/line-actual.sorted" > "$tmp/out/line.diff" 2>&1; then
  echo "PASS: line-regexp/output-parity"
  passed=$((passed + 1))
else
  echo "FAIL: line-regexp/output-parity"
  cat "$tmp/out/line.diff"
  failed=$((failed + 1))
fi

# ── Test 3: -x no-match (partial line shouldn't match) ─────────────
echo "--- Test 3: Line regexp no-match ---"
set +e
"$SCAFFOLD" -x needle "s3://$bucket/$prefix/" >/dev/null 2>&1
status=$?
set -e
if [ "$status" -eq 1 ]; then
  echo "PASS: line-regexp/no-match-exit (exit=1)"
  passed=$((passed + 1))
else
  echo "FAIL: line-regexp/no-match-exit (expected exit 1, got $status)"
  failed=$((failed + 1))
fi

# ── Test 4: -w with --streaming parity ────────────────────────────
echo "--- Test 4: Word regexp with streaming ---"
if "$SCAFFOLD" --help 2>&1 | grep -q '\--streaming'; then
  "$SCAFFOLD" -w needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/word-default.sorted"
  "$SCAFFOLD" --streaming -w needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/out/word-streaming.sorted"
  if diff -u "$tmp/out/word-default.sorted" "$tmp/out/word-streaming.sorted" >/dev/null 2>&1; then
    echo "PASS: word-regexp/streaming-parity"
    passed=$((passed + 1))
  else
    echo "FAIL: word-regexp/streaming-parity"
    failed=$((failed + 1))
  fi
else
  echo "SKIP: word-regexp/streaming-parity (no --streaming flag)"
fi

# ── Test 5: -w with glob filter ───────────────────────────────────
echo "--- Test 5: Word regexp with glob ---"
rg -n -w -g '*.rs' needle "$tmp/local" 2>/dev/null \
  | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
  | LC_ALL=C sort > "$tmp/out/word-glob-golden.sorted"

"$SCAFFOLD" -w -g '*.rs' needle "s3://$bucket/$prefix/" 2>/dev/null \
  | LC_ALL=C sort > "$tmp/out/word-glob-actual.sorted"

if diff -u "$tmp/out/word-glob-golden.sorted" "$tmp/out/word-glob-actual.sorted" > "$tmp/out/word-glob.diff" 2>&1; then
  echo "PASS: word-regexp/glob-parity"
  passed=$((passed + 1))
else
  echo "FAIL: word-regexp/glob-parity"
  cat "$tmp/out/word-glob.diff"
  failed=$((failed + 1))
fi

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "=== FF6 Results ==="
echo "Binary: $SCAFFOLD"
echo "Passed: $passed"
echo "Failed: $failed"
if [ "$failed" -eq 0 ]; then
  echo "Verdict: PASS_harness (FF6: word/line regexp)"
  echo "scope: word-parity/line-parity/no-match-exit/streaming-parity/glob-parity"
  exit 0
else
  echo "Verdict: FAIL_harness (FF6)"
  exit 1
fi
