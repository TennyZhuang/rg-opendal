#!/usr/bin/env bash
# rg-opendal parity harness: Feature Family 2 — Glob + Type filtering
# Uses minio as single source of truth — downloads fixture to local for golden
set -euo pipefail

SCAFFOLD="${RG_OPENDAL_SCAFFOLD:-/Users/tianyizhuang/.slock-staging-qa-2673/rg-opendal/target/release/rg-opendal}"
bucket="${RG_OPENDAL_TEST_BUCKET:-rg-test}"
prefix="${RG_OPENDAL_TEST_PREFIX:-harness-ff2}"
endpoint="${OPENDAL_S3_ENDPOINT:-http://127.0.0.1:9000}"
region="${OPENDAL_S3_REGION:-us-east-1}"
tmp="${TMPDIR:-/tmp}/rg-opendal-harness-ff2-$$"

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

# Download fixture from minio to local mirror (single source of truth)
echo "Syncing fixture from minio..."
aws --endpoint-url "$endpoint" s3 sync "s3://$bucket/$prefix/" "$tmp/local/" >/dev/null 2>&1
FILE_COUNT=$(find "$tmp/local" -type f | wc -l | tr -d ' ')
echo "Fixture: $FILE_COUNT files synced"

# Verify scaffold works
set +e
"$SCAFFOLD" needle "s3://$bucket/$prefix/" >/dev/null 2>&1
check_status=$?
set -e
if [ "$check_status" -ne 0 ] && [ "$check_status" -ne 1 ]; then
  echo "ERROR: scaffold failed (exit=$check_status)" >&2
  exit 1
fi

passed=0
failed=0

# Helper: run golden (native rg on local mirror) vs actual (scaffold on minio)
check_test() {
  local name="$1"
  shift
  local golden_args=() actual_args=()
  local target="golden"
  for arg in "$@"; do
    if [ "$arg" = "--" ]; then
      target="actual"
      continue
    fi
    if [ "$target" = "golden" ]; then
      golden_args+=("$arg")
    else
      actual_args+=("$arg")
    fi
  done

  # Golden: native rg on local mirror, then rewrite paths to s3:// prefix  
  # The last element of golden_args is the local path
  rg -n "${golden_args[@]}" 2>/dev/null \
    | sed "s|^$tmp/local/|s3://$bucket/$prefix/|" \
    | LC_ALL=C sort > "$tmp/out/${name}.golden"

  # Actual: scaffold
  "$SCAFFOLD" "${actual_args[@]}" 2>/dev/null \
    | LC_ALL=C sort > "$tmp/out/${name}.actual"

  if diff -u "$tmp/out/${name}.golden" "$tmp/out/${name}.actual" > "$tmp/out/${name}.diff" 2>&1; then
    echo "PASS: $name"
    passed=$((passed + 1))
  else
    echo "FAIL: $name"
    echo "--- diff ---"
    cat "$tmp/out/${name}.diff"
    failed=$((failed + 1))
  fi
}

check_exit() {
  local name="$1" expected="$2"
  shift 2
  set +e
  "$SCAFFOLD" "$@" > "$tmp/out/${name}.out" 2> "$tmp/out/${name}.err"
  local status=$?
  set -e
  if [ "$status" -eq "$expected" ]; then
    echo "PASS: $name (exit=$expected)"
    passed=$((passed + 1))
  else
    echo "FAIL: $name (expected exit $expected, got $status)"
    failed=$((failed + 1))
  fi
}

# ── Test 1: No-filter baseline ────────────────────────────────────
echo "--- Test 1: No filter ---"
check_test "no-filter" \
  needle "$tmp/local" \
  -- \
  needle "s3://$bucket/$prefix/"

# ── Test 2: Glob — *.rs only ──────────────────────────────────────
echo "--- Test 2: Glob *.rs ---"
check_test "glob-rs" \
  -g '*.rs' needle "$tmp/local" \
  -- \
  -g '*.rs' needle "s3://$bucket/$prefix/"

# ── Test 3: Glob — exclude IGNORE files ───────────────────────────
echo "--- Test 3: Glob exclude IGNORE ---"
check_test "glob-exclude-ignore" \
  -g '!*IGNORE*' needle "$tmp/local" \
  -- \
  -g '!*IGNORE*' needle "s3://$bucket/$prefix/"

# ── Test 4: Glob — no-match (filters everything) ──────────────────
echo "--- Test 4: Glob no-match ---"
check_exit "glob-no-match" 1 \
  -g '*.xyz' needle "s3://$bucket/$prefix/"

# ── Test 5: Output format check ───────────────────────────────────
echo "--- Test 5: Output format ---"
output=$("$SCAFFOLD" needle "s3://$bucket/$prefix/" 2>/dev/null)
if echo "$output" | grep -qv "^s3://"; then
  echo "FAIL: output-format (missing s3:// prefix)"
  failed=$((failed + 1))
else
  echo "PASS: output-format"
  passed=$((passed + 1))
fi

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "=== FF2 Results ==="
echo "Binary: $SCAFFOLD"
echo "Passed: $passed"
echo "Failed: $failed"
if [ "$failed" -eq 0 ]; then
  echo "Verdict: PASS_harness (FF2: glob/type filtering)"
  echo "scope: no-filter/glob-rs/glob-exclude/glob-no-match/output-format"
  exit 0
else
  echo "Verdict: FAIL_harness (FF2)"
  exit 1
fi
