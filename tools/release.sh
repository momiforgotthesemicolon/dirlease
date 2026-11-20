#!/usr/bin/env bash
set -euo pipefail
cd "${BUILD_WORKSPACE_DIRECTORY:?run with bazel run //tools:release}"
exec python3 tools/release.py "$@"
