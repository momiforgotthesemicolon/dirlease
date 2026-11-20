# Development

Bazel is the build and validation entry point. Cargo metadata supports editors
and local Rust tooling; keep its package version, edition, and dependencies
aligned with Bazel. No external Rust crates are required by the scaffold.

Install Bazelisk, Python 3.10 or newer, Go (with toolchain downloads enabled),
and ShellCheck 0.11.0. Run `tools/install-dev-tools.sh` to install pinned
Buildifier, actionlint, mdformat, and the GFM plugin into `.tools/` and
`.venv/`. On Linux x86_64 the installer also supplies ShellCheck if needed. Go
tools are built using the pinned Go 1.26.0 toolchain, downloaded automatically
by Go. Bazel downloads Rust 1.92.0, rustfmt, and Clippy; rustup is optional for
Bazel and uses `rust-toolchain.toml` for Cargo/editor commands. macOS also needs
the Xcode command-line tools.

```sh
bazel build //:dirlease
bazel run //:dirlease -- --help
bazel test //...
bazel run //:format
bazel run //:format_check
bazel run //:lint
bazel run //:review-diff
bazel build --config=release //:dirlease
```

`format` fixes Rust, Bazel/Starlark, and Markdown. `format_check` checks the
same files without rewriting them. `lint` adds Clippy (warnings are errors),
Buildifier lint, ShellCheck, and actionlint. Files are enumerated through Git,
including new files but excluding ignored artifacts and files removed from the
working tree. Skill Markdown is excluded from mdformat to preserve YAML
frontmatter, matching Reeve. Validate edited skills with the skill-creator
validator. Tool versions live in `tools/tool_versions.json`; the installer and
checks share those pins. CI runs the same commands on GitHub-hosted runners
without secrets.

The root targets cover the repository. From `src/`, `bazel run :lint`,
`bazel run :format`, `bazel run :format_check`, `bazel run :review-diff`, and
`bazel test :test` cover that package. `tools/` has the same commands. A parent
scope includes its descendants. Define `repo_commands` from `build_defs.bzl` in
new packages and add their tests to the nearest parent `test` suite.

`review-diff` prints branch changes against local `main` when on a feature
branch, working changes against `HEAD`, untracked file contents, and focused
validation suggestions. Update local `main` before reviewing a branch against a
newer base.

`MODULE.bazel.lock` and `Cargo.lock` are committed. `user.bazelrc` is an ignored
optional override for checkout-specific Bazel settings. Repository checks run on
the host; Rust compilation, formatting, and lint use Bazel's pinned toolchain.

The workflow is adapted from Reeve. C++/Go/frontend toolchains, compile-command
refresh, self-hosted runner infrastructure, signing, deployment, and browser
configuration are omitted because they do not apply to this Rust CLI.
