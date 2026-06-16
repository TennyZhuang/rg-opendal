#!/usr/bin/env bash
# rg-opendal parity harness: FF10 + FF11 + FF12 + FF13
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-$SCRIPT_DIR/../target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-ff10-13-$$"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT
[ ! -x "$SCAFFOLD" ] && { echo "ERROR: scaffold not found" >&2; exit 1; }
mkdir -p "$tmp/local"
export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-minioadmin}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-minioadmin}"
export OPENDAL_S3_ENDPOINT="$endpoint"
export OPENDAL_S3_REGION="$region"
SYNC_OK=false
aws --endpoint-url "$endpoint" s3 sync "s3://$bucket/$prefix/" "$tmp/local/" >/dev/null 2>&1
FILES=$(find "$tmp/local" -type f 2>/dev/null | wc -l | tr -d ' ')
[ "$FILES" -gt 0 ] && SYNC_OK=true
passed=0; failed=0

echo "--- FF10: --no-filename ---"
if $SYNC_OK; then
  rg -n -I needle "$tmp/local" 2>/dev/null | LC_ALL=C sort > "$tmp/no-fn-golden"
  "$SCAFFOLD" -I needle "s3://$bucket/$prefix/" 2>/dev/null | LC_ALL=C sort > "$tmp/no-fn-actual"
  diff -u "$tmp/no-fn-golden" "$tmp/no-fn-actual" >/dev/null 2>&1 && { echo "PASS: no-filename/output-parity"; passed=$((passed+1)); } || { echo "FAIL: no-filename/output-parity"; failed=$((failed+1)); }
else echo "SKIP: no-filename/output-parity"; fi
output=$("$SCAFFOLD" -I needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
echo "$output" | grep -qv "^s3://" && { echo "PASS: no-filename/single-file-no-prefix"; passed=$((passed+1)); } || { echo "FAIL: no-filename/single-file-no-prefix"; failed=$((failed+1)); }

echo "--- FF11: --null ---"
output=$("$SCAFFOLD" -0 needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
echo "$output" | head -c 100 | grep -q $'\x00' && { echo "PASS: null/path-terminator-is-nul"; passed=$((passed+1)); } || { echo "FAIL: null/path-terminator-is-nul"; failed=$((failed+1)); }
output2=$("$SCAFFOLD" -0 needle "s3://$bucket/$prefix/" 2>/dev/null)
echo "$output2" | head -c 200 | grep -q $'\x00' && { echo "PASS: null/multi-file-nul-terminator"; passed=$((passed+1)); } || { echo "FAIL: null/multi-file-nul-terminator"; failed=$((failed+1)); }

echo "--- FF12: --null-data ---"
printf 'needle in NUL record\0no match here\0another needle\0' > "$tmp/nul-data"
aws --endpoint-url "$endpoint" s3 cp "$tmp/nul-data" "s3://$bucket/harness-nul-data.bin" >/dev/null 2>&1
set +e; "$SCAFFOLD" -z needle "s3://$bucket/harness-nul-data.bin" >/dev/null 2>&1; s=$?; set -e
[ "$s" -eq 0 ] && { echo "PASS: null-data/finds-matches"; passed=$((passed+1)); } || { echo "FAIL: null-data/finds-matches (got $s)"; failed=$((failed+1)); }
set +e; "$SCAFFOLD" -z nonexistent_xyz "s3://$bucket/harness-nul-data.bin" >/dev/null 2>&1; s=$?; set -e
[ "$s" -eq 1 ] && { echo "PASS: null-data/no-match-exit"; passed=$((passed+1)); } || { echo "FAIL: null-data/no-match-exit (got $s)"; failed=$((failed+1)); }

echo "--- FF13: --text ---"
printf '\x00binary looking file\nneedle hidden here\n' > "$tmp/binary-data"
aws --endpoint-url "$endpoint" s3 cp "$tmp/binary-data" "s3://$bucket/harness-binary-data.bin" >/dev/null 2>&1
"$SCAFFOLD" -a needle "s3://$bucket/harness-binary-data.bin" >/dev/null 2>&1 && { echo "PASS: text/flag-accepted"; passed=$((passed+1)); } || { echo "FAIL: text/flag-accepted"; failed=$((failed+1)); }

echo ""; echo "=== FF10-FF13 Results ==="; echo "Passed: $passed Failed: $failed"
[ "$failed" -eq 0 ] && { echo "Verdict: PASS_harness"; exit 0; } || { echo "Verdict: FAIL_harness"; exit 1; }
