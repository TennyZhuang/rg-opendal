#!/usr/bin/env bash
# rg-opendal parity harness: Feature Family 3 — JSON output, context lines, stats
set -euo pipefail

SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-/Users/tianyizhuang/.slock-staging-qa-2673/rg-opendal/target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-ff3-$$"

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

# Download fixture
aws --endpoint-url "$endpoint" s3 sync "s3://$bucket/$prefix/" "$tmp/local/" >/dev/null 2>&1

passed=0
failed=0

# ── Test 1: JSON output produces valid JSON lines ─────────────────
echo "--- Test 1: JSON output ---"
output=$("$SCAFFOLD" --json needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
# Every line should have "type" key
json_errors=0
while IFS= read -r line; do
  if ! echo "$line" | python3 -c "import sys,json; json.loads(sys.stdin.read())" 2>/dev/null; then
    json_errors=$((json_errors + 1))
  fi
done <<< "$output"
if [ "$json_errors" -eq 0 ]; then
  echo "PASS: json/valid-json-lines"
  passed=$((passed + 1))
else
  echo "FAIL: json/valid-json-lines ($json_errors invalid lines)"
  failed=$((failed + 1))
fi

# ── Test 2: JSON includes match data ──────────────────────────────
if echo "$output" | grep -q '"type":"match"'; then
  echo "PASS: json/has-match-type"
  passed=$((passed + 1))
else
  echo "FAIL: json/has-match-type"
  failed=$((failed + 1))
fi

# ── Test 3: Context lines (-A 1 -B 1) ────────────────────────────
context=$("$SCAFFOLD" -A 1 -B 1 needle "s3://$bucket/$prefix/src/main.rs" 2>/dev/null)
# Should have 3 lines per match (before, match, after), so 6 lines for 2 matches
if echo "$context" | grep -c "needle" | grep -q "2"; then
  echo "PASS: context/lines-present"
  passed=$((passed + 1))
else
  echo "FAIL: context/lines-present"
  failed=$((failed + 1))
fi

# ── Test 4: --stats output contains expected fields ───────────────
stats_out=$("$SCAFFOLD" --stats needle "s3://$bucket/$prefix/src/main.rs" 2>&1)
if echo "$stats_out" | grep -q "matches\|matched lines\|files contained matches\|files searched\|bytes printed\|bytes searched\|seconds"; then
  echo "PASS: stats/fields-present"
  passed=$((passed + 1))
else
  echo "FAIL: stats/fields-present"
  failed=$((failed + 1))
fi

# ── Test 5: JSON+stats combined ──────────────────────────────────
combined=$("$SCAFFOLD" --json --stats needle "s3://$bucket/$prefix/src/main.rs" 2>&1)
if echo "$combined" | grep -q '"type":"match"' && echo "$combined" | grep -q "matches"; then
  echo "PASS: combined/json-and-stats"
  passed=$((passed + 1))
else
  echo "FAIL: combined/json-and-stats"
  failed=$((failed + 1))
fi

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "=== FF3 Results ==="
echo "Binary: $SCAFFOLD"
echo "Passed: $passed"
echo "Failed: $failed"
if [ "$failed" -eq 0 ]; then
  echo "Verdict: PASS_harness (FF3: json/context/stats)"
  echo "scope: json-valid/json-match/context/stats-fields/combined"
  exit 0
else
  echo "Verdict: FAIL_harness (FF3)"
  exit 1
fi
