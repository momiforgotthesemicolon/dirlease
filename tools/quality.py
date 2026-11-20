"""Scoped repository checks and review output used by Bazel entry points."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
PINS = json.loads((ROOT / "tools/tool_versions.json").read_text())


def run(arguments, **kwargs):
    """Run a tool in the checkout and propagate failures to the caller."""
    return subprocess.run(arguments, cwd=ROOT, check=True, **kwargs)


def output(arguments):
    """Read a command's output without splitting filenames at whitespace."""
    return run(arguments, stdout=subprocess.PIPE).stdout.decode()


def repo_files(scope):
    """Include indexed and new files; exclude ignored and removed paths."""
    names = output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"])
    return sorted({
        name for name in names.split("\0") if name and (ROOT / name).is_file()
        and (scope == "." or name.startswith(scope + "/"))
    })


def require_tool(name):
    """Resolve a pinned tool, failing with installation guidance on mismatch."""
    # Step #1: Prefer repository-local installations over host copies.
    candidates = [ROOT / ".tools/bin" / name, ROOT / ".venv/bin" / name]
    executable = next((str(path) for path in candidates if path.is_file()), None)
    executable = executable or shutil.which(name)
    if not executable:
        raise RuntimeError(f"Missing {name}; see docs/development.md for installation.")

    # Step #2: Compare the version reported by the tool with the shared pin.
    if name == "buildifier":
        version = output(["go", "version", "-m", executable])
        matches = PINS[name] in version.split()
    elif name == "mdformat":
        version = output([executable, "--version"])
        matches = version.split()[:2] == ["mdformat", PINS[name]]
        # Resolve the CLI's interpreter, so a different host Python cannot mask a plugin mismatch.
        shebang = Path(executable).resolve().read_text().splitlines()[0]
        python = shebang.removeprefix("#!").strip()
        if not Path(python).is_file():
            raise RuntimeError("Install mdformat with tools/install-dev-tools.sh.")
        installed = output([
            python, "-c",
            "import importlib.metadata; print(importlib.metadata.version('mdformat-gfm'))",
        ])
        matches = matches and installed.strip() == PINS["mdformat-gfm"]
    else:
        flag = "-version" if name == "actionlint" else "--version"
        version = output([executable, flag])
        matches = PINS[name] in [word.removeprefix("v") for word in version.split()]
    if not matches:
        raise RuntimeError(f"{name} must match {PINS[name]}; see docs/development.md.")
    return executable


def format_files(files, check):
    """Apply or check the same Bazel and Markdown formatting rules."""
    bazel = [name for name in files if name.endswith((".bazel", ".bzl"))]
    # Skill frontmatter must remain intact; mdformat does not parse YAML headers.
    markdown = [
        name for name in files
        if name.endswith(".md") and not name.startswith(".agents/")
    ]
    if bazel:
        flags = ["-mode=check"] if check else ["-lint=fix"]
        run([require_tool("buildifier"), *flags, *bazel])
    if markdown:
        flags = ["--check"] if check else []
        run([require_tool("mdformat"), *flags, "--wrap", "80", *markdown])


def lint_files(files):
    """Lint repository build files, shell scripts, workflows, and Markdown."""
    format_files(files, check=True)
    bazel = [name for name in files if name.endswith((".bazel", ".bzl"))]
    shell = [name for name in files if name.endswith(".sh")]
    workflows = [
        name for name in files
        if name.startswith(".github/workflows/") and name.endswith((".yml", ".yaml"))
    ]
    if bazel:
        run([require_tool("buildifier"), "-mode=check", "-lint=warn", *bazel])
    if shell:
        run([require_tool("shellcheck"), *shell])
    if workflows:
        run([require_tool("actionlint"), *workflows])


def review(scope):
    """Print committed, staged, unstaged, and untracked changes for review."""
    # Step #1: Show status and branch changes relative to the local main branch.
    run(["git", "status", "--short", "--", scope])
    branch = output(["git", "branch", "--show-current"]).strip()
    if branch != "main":
        print("\nCommitted changes against main", flush=True)
        run(["git", "--no-pager", "diff", "--stat", "main...HEAD", "--", scope])
        run(["git", "--no-pager", "diff", "main...HEAD", "--", scope])

    # Step #2: Include working changes and new files, not only indexed hunks.
    print("\nWorking changes", flush=True)
    run(["git", "--no-pager", "diff", "--stat", "HEAD", "--", scope])
    run(["git", "--no-pager", "diff", "HEAD", "--", scope])
    names = output(["git", "ls-files", "-z", "--others", "--exclude-standard", "--", scope])
    for name in names.split("\0"):
        if name and (ROOT / name).is_symlink():
            target = (ROOT / name).readlink()
            print(
                f"diff --git a/{name} b/{name}\nnew file mode 120000\n"
                f"--- /dev/null\n+++ b/{name}\n@@ -0,0 +1 @@\n+{target}",
                flush=True,
            )
        elif name:
            result = subprocess.run(["git", "--no-pager", "diff", "--no-index", "--", "/dev/null", name], cwd=ROOT)
            if result.returncode not in (0, 1):
                raise RuntimeError(f"Could not show the new file {name}")

    # Step #3: Suggest validation at the same scope as the displayed diff.
    prefix = "//" if scope == "." else f"//{scope}/"
    label = "//" if scope == "." else f"//{scope}"
    print(f"\nSuggested validation:\nbazel test {prefix}...\nbazel run {label}:format_check\nbazel run {label}:lint")


def main():
    """Select repository checks and native Rust toolchain actions by scope."""
    # Step #1: Parse only repository-relative scopes so tools cannot escape it.
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["format", "format_check", "lint", "review-diff"])
    parser.add_argument("scope", nargs="?", default=".")
    args = parser.parse_args()
    scope = Path(args.scope).as_posix()
    if scope.startswith("/") or ".." in Path(scope).parts:
        parser.error("scope must be a repository-relative directory")
    files = repo_files(scope)
    if args.command == "review-diff":
        review(scope)
        return

    # Step #2: Run repository checks using the shared file set and version pins.
    if args.command == "lint":
        lint_files(files)
    else:
        format_files(files, check=args.command == "format_check")

    # Step #3: Use Bazel's pinned Rust tools for scopes containing Rust sources.
    if any(name.endswith(".rs") for name in files):
        pattern = "//..." if scope == "." else f"//{scope}/..."
        if args.command == "lint":
            run([
                "bazel", "build",
                "--aspects=@rules_rust//rust:defs.bzl%rust_clippy_aspect",
                "--output_groups=clippy_checks", pattern,
            ])
        else:
            flags = ["--check"] if args.command == "format_check" else []
            run(["bazel", "run", "@rules_rust//:rustfmt", "--", pattern, "--", *flags])


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
