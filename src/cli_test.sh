#!/usr/bin/env bash
# Keep all worker state and destructive operations inside the test fixture.
set -euo pipefail
binary="$PWD/$1"
export DIRLEASE_STATE_DIR="$TEST_TMPDIR/state"
export DIRLEASE_TEMP_ROOT="$TEST_TMPDIR/temp"
export DIRLEASE_SESSION="test-session"
worker=''
trap 'if [[ -n "$worker" ]]; then kill "$worker" 2>/dev/null || true; wait "$worker" 2>/dev/null || true; fi' EXIT
[[ "$("$binary" --version)" == "dirlease 0.1.0" ]]
"$binary" --help | grep -q 'temporary session directories'
if env -u DIRLEASE_SESSION "$binary" >"$TEST_TMPDIR/output" 2>"$TEST_TMPDIR/error"; then exit 1; fi
[[ ! -s "$TEST_TMPDIR/output" ]]
grep -q 'uuidgen' "$TEST_TMPDIR/error"
"$binary" serve >"$TEST_TMPDIR/worker-output" 2>"$TEST_TMPDIR/worker-error" &
worker=$!
for _ in {1..100}; do
  [[ -S "$DIRLEASE_STATE_DIR/worker.sock" ]] && break
  sleep 0.02
done
first="$("$binary")"
[[ "$first" == "$DIRLEASE_TEMP_ROOT/"* && -d "$first" ]]
[[ "$("$binary" create --ttl 2d)" == "$first" ]]
pids=()
for index in {1..8}; do
  "$binary" >"$TEST_TMPDIR/result-$index" &
  pids+=("$!")
done
for pid in "${pids[@]}"; do wait "$pid"; done
for index in {1..8}; do [[ "$(cat "$TEST_TMPDIR/result-$index")" == "$first" ]]; done
fresh="$("$binary" --new --ttl 2s)"
[[ "$fresh" != "$first" ]]
kill "$worker"
wait "$worker" || true
worker=''
# Auto-start detaches all standard streams, so command substitution finishes.
pids=()
for index in {1..8}; do
  "$binary" >"$TEST_TMPDIR/restarted-$index" &
  pids+=("$!")
done
for pid in "${pids[@]}"; do wait "$pid"; done
for index in {1..8}; do [[ "$(cat "$TEST_TMPDIR/restarted-$index")" == "$fresh" ]]; done
# Find the replacement worker through its lock owner for fixture teardown.
worker="$(cat "$DIRLEASE_STATE_DIR/worker.pid")"
for _ in {1..250}; do
  [[ ! -e "$fresh" ]] && break
  sleep 0.02
done
[[ ! -e "$fresh" && -d "$first" ]]
if "$binary" --ttl nonsense >"$TEST_TMPDIR/output" 2>"$TEST_TMPDIR/error"; then exit 1; fi
[[ ! -s "$TEST_TMPDIR/output" ]]
