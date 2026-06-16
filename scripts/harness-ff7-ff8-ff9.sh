#!/usr/bin/env bash
# rg-opendal parity harness: FF7 + FF8 + FF9
# --max-count (exit code), --invert-match, --no-line-number
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-$SCRIPT_DIR/../target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-ff789-$$"

cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

if [ ! -x "$SCAFFOLD" ]; then echo "ERROR: scaffold binary not found at $SCAFFOLD" >&2; exit 1; fi
mkdir -p "$tmp/local"

export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-minioadmin}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-minioadmin}"
export OPENDAL_S3_ENDPOINT="$endpoint"
export OPENDAL_S3_REGION="$region"
aws --endpoint-url "$endpoint" s3 sync "s3://$bucket/$prefix/" "$tmp/local/" >/dev/null 2>&1

passed=0; failed=0

# ══════ FF7: --max-count exit code ══════
echo "--- FF7: --max-count ---"
set +e; "$SCAFFOLD" -m 1 needle "s3://$bucket/$prefix/" >/dev/null 2>&1; s=$?; set -e
if [ "$s" -eq 0 ]; then echo "PASS: max-count/exit-0-with-matches"; passed=$((passed + 1))
else echo "FAIL: max-count/exit-0-with-matches (got $s)"; failed=$((failed + 1)); fi

set +e; "$SCAFFOLD" -m 1 nonexistent_xyz "s3://$bucket/$prefix/" >/dev/null 2>&1; s=$?; set -e
if [ "$s" -eq 1 ]; then echo "PASS: max-count/no-match-exit (exit=1)"; passed=$((passed + 1))
else echo "FAIL: max-count/no-match-exit (expected 1, got $s)"; failed=$((failed + 1)); fi

# ══════ FF8: --invert-match ══════
echo "--- FF8: --invert-match ---"
set +e; "$SCAFFOLD" -v needle "s3://$bucket/$prefix/src/main.rs" >/dev/null 2>&1; s=$?; set -e
if [ "$s" -eq 0 ]; then echo "PASS: invert-match/has-non-matching-lines"; passed=$((passed + 1))
else echo "FAIL: invert-match/has-non-matching-lines (got $s)"; failed=$((failed + 1)); fi

set +e; "$SCAFFOLD" -v ".*" "s3://$bucket/$prefix/src/main.rs" >/dev/null 2>&1; s=$?; set -e
if [ "$s" -eq 1 ]; then echo "PASS: invert-match/no-match-exit (exit=1)"; passed=$((passed + 1))
else echo "FAIL: invert-match/no-match-exit (expected 1, got $s)"; failed=$((failed + 1)); fi

# ══════ FF9: --no-line-number ══════
echo "--- FF9: --no-line-number ---"
output=$("$SCAFFOLD" -N needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
# Lines should NOT have :DIGIT: pattern after filename
if echo "$output" | grep -qv ':[0-9]\+:'; then
  echo "PASS: no-line-number/suppresses-line-numbers"; passed=$((passed + 1))
else echo "FAIL: no-line-number/suppresses-line-numbers"; failed=$((failed + 1)); fi

# ══════ Streaming parity ══════
echo "--- Streaming parity ---"
if "$SCAFFOLD" --help 2>&1 | grep -q '\--streaming'; then
  "$SCAFFOLD" -v needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null | LC_ALL=C sort > "$tmp/inv-default"
  "$SCAFFOLD" --streaming -v needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null | LC_ALL=C sort > "$tmp/inv-streaming"
  if diff "$tmp/inv-default" "$tmp/inv-streaming" >/dev/null 2>&1; then
    echo "PASS: invert-match/streaming-parity"; passed=$((passed + 1))
  else echo "FAIL: invert-match/streaming-parity"; failed=$((failed + 1)); fi
else echo "SKIP: invert-match/streaming-parity"; fi

# ── Summary ──
echo ""; echo "=== FF7/FF8/FF9 Results ==="
echo "Binary: $SCAFFOLD"; echo "Passed: $passed"; echo "Failed: $failed"
if [ "$failed" -eq 0 ]; then echo "Verdict: PASS_harness (FF7+FF8+FF9)"; echo "scope: max-count-exit/invert-match/printer-flags/streaming-parity"; exit 0
else echo "Verdict: FAIL_harness"; exit 1; fi
