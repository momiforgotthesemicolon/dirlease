#!/usr/bin/env bash
# Install tools into this checkout; versions also govern the quality targets.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
repo_root="$PWD"
mkdir -p .tools/bin
version() {
  python3 -c 'import json, sys; print(json.load(open("tools/tool_versions.json"))[sys.argv[1]])' "$1"
}
GOTOOLCHAIN="go$(version go)" GOBIN="$repo_root/.tools/bin" go install "github.com/bazelbuild/buildtools/buildifier@$(version buildifier)"
GOTOOLCHAIN="go$(version go)" GOBIN="$repo_root/.tools/bin" go install "github.com/rhysd/actionlint/cmd/actionlint@v$(version actionlint)"
python3 -m venv .venv
.venv/bin/python3 -m pip install "mdformat==$(version mdformat)" "mdformat-gfm==$(version mdformat-gfm)"
if ! command -v shellcheck >/dev/null || ! shellcheck --version | grep -q "version: $(version shellcheck)$"; then
  if [[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]]; then
    curl --fail --location "https://github.com/koalaman/shellcheck/releases/download/v$(version shellcheck)/shellcheck-v$(version shellcheck).linux.x86_64.tar.xz"       -o "$repo_root/.tools/shellcheck.tar.xz"
    tar -xJf .tools/shellcheck.tar.xz -C .tools
    cp ".tools/shellcheck-v$(version shellcheck)/shellcheck" .tools/bin/shellcheck
  else
    echo "Install ShellCheck $(version shellcheck) (for example, brew install shellcheck), then rerun." >&2
    exit 1
  fi
fi
