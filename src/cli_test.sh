#!/usr/bin/env bash
# Keep all worker state and destructive operations inside the test fixture.
set -euo pipefail
binary="$PWD/$1"
export DIRLEASE_STATE_DIR="$TEST_TMPDIR/state"
export DIRLEASE_TEMP_ROOT="$TEST_TMPDIR/temp"
export DIRLEASE_SESSION="test-session"
worker=''
trap 'if [[ -n "$worker" ]]; then kill "$worker" 2>/dev/null || true; wait "$worker" 2>/dev/null || true; fi' EXIT
[[ "$(env -u DIRLEASE_SESSION "$binary" --version)" == "dirlease 0.0.3-alpha" ]]
[[ "$(DIRLEASE_SESSION='' "$binary" --version)" == "dirlease 0.0.3-alpha" ]]
"$binary" --help | grep -q 'temporary session directories'
"$binary" serve >"$TEST_TMPDIR/worker-output" 2>"$TEST_TMPDIR/worker-error" &
worker=$!
for _ in {1..100}; do
  [[ -S "$DIRLEASE_STATE_DIR/worker.sock" ]] && break
  sleep 0.02
done
automatic="$(env -u DIRLEASE_SESSION "$binary")"
empty="$(DIRLEASE_SESSION='' "$binary")"
[[ "$automatic" == "$DIRLEASE_TEMP_ROOT/"* && -d "$automatic" ]]
[[ "$empty" == "$DIRLEASE_TEMP_ROOT/"* && -d "$empty" ]]
[[ "$automatic" == "$empty" ]]
[[ "$(env -u DIRLEASE_SESSION "$binary")" == "$automatic" ]]
[[ "$(env -u DIRLEASE_SESSION "$binary" create --ttl 2d)" == "$automatic" ]]
# A new Unix session gets an independent default lease.
separate="$(env -u DIRLEASE_SESSION python3 -c 'import os, sys; os.setsid(); os.execv(sys.argv[1], sys.argv[1:])' "$binary")"
[[ "$separate" != "$automatic" && -d "$separate" ]]
automatic_fresh="$(env -u DIRLEASE_SESSION "$binary" --new)"
[[ "$automatic_fresh" != "$automatic" && -d "$automatic" ]]
[[ "$(DIRLEASE_SESSION='' "$binary")" == "$automatic_fresh" ]]
[[ "$DIRLEASE_SESSION" == "test-session" ]]
first="$("$binary")"
[[ "$first" == "$DIRLEASE_TEMP_ROOT/"* && -d "$first" ]]
[[ "$first" != "$automatic_fresh" ]]
[[ "$("$binary" create --ttl 2d 2>"$TEST_TMPDIR/error")" == "$first" ]]
[[ ! -s "$TEST_TMPDIR/error" ]]
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
[[ "$(env -u DIRLEASE_SESSION "$binary")" == "$automatic_fresh" ]]
for _ in {1..250}; do
  [[ ! -e "$fresh" ]] && break
  sleep 0.02
done
[[ ! -e "$fresh" && -d "$first" ]]
if "$binary" --ttl nonsense >"$TEST_TMPDIR/output" 2>"$TEST_TMPDIR/error"; then exit 1; fi
[[ ! -s "$TEST_TMPDIR/output" ]]

# The agent supplies a stable identity without changing its shell environment.
repository="$TEST_TMPDIR/repository with spaces"
git init -q --initial-branch=main "$repository"
printf 'tracked\n' >"$repository/tracked"
git -C "$repository" add tracked
git -C "$repository" -c user.name=Test -c user.email=test@example.invalid commit -qm initial
checkout="$("$binary" worktree --session rex-agent-one --repo "$repository" --branch agent/task --ttl 2d)"
[[ -f "$checkout/.git" && -f "$checkout/tracked" ]]
[[ "$("$binary" worktree --session rex-agent-one --repo "$repository" --branch agent/task --ttl 3d)" == "$checkout" ]]
[[ "$DIRLEASE_SESSION" == "test-session" ]]
pids=()
for index in {1..8}; do
  "$binary" worktree --session rex-agent-one --repo "$repository" --branch agent/task >"$TEST_TMPDIR/worktree-$index" &
  pids+=("$!")
done
for pid in "${pids[@]}"; do wait "$pid"; done
for index in {1..8}; do [[ "$(cat "$TEST_TMPDIR/worktree-$index")" == "$checkout" ]]; done
other="$("$binary" worktree --session rex-agent-two --repo "$repository")"
[[ "$other" != "$checkout" && -f "$other/tracked" ]]
if "$binary" worktree --repo "$repository" >"$TEST_TMPDIR/output" 2>"$TEST_TMPDIR/error"; then exit 1; fi
[[ ! -s "$TEST_TMPDIR/output" ]]
grep -q 'nonempty --session' "$TEST_TMPDIR/error"
if "$binary" worktree --session bad-ref --repo "$repository" --ref does-not-exist >"$TEST_TMPDIR/output" 2>"$TEST_TMPDIR/error"; then exit 1; fi
[[ ! -s "$TEST_TMPDIR/output" ]]
