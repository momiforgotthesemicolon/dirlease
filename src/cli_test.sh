#!/usr/bin/env bash
# Exercise the compiled entry point without touching any user directories.
set -euo pipefail
binary="$PWD/$1"
[[ "$("$binary" --version)" == "dirlease 0.1.0" ]]
"$binary" --help | grep -q 'not implemented yet'
if "$binary" >"$TEST_TMPDIR/output" 2>&1; then
  echo 'The scaffold must fail when asked to perform an unimplemented operation.' >&2
  exit 1
fi
if "$binary" create "$TEST_TMPDIR/lease" >"$TEST_TMPDIR/output" 2>&1; then
  echo 'An unimplemented command must not report success.' >&2
  exit 1
fi
[[ ! -e "$TEST_TMPDIR/lease" ]]
