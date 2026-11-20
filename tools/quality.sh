#!/usr/bin/env bash
# bazel run supplies the real checkout, including newly created files.
set -euo pipefail
repo_root="${BUILD_WORKSPACE_DIRECTORY:?Run this command through bazel run}"
cd "$repo_root"
exec python3 tools/quality.py "$@"
