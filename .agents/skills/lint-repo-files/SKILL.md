---
name: lint-repo-files
description: Run package-scoped Bazel lint for Rust, build files, shell scripts, Markdown, and GitHub workflows.
---

# Lint Repo Files

Run `bazel run :lint` from the root for the full repository, or from `src/`
or `tools/` for that package and its descendants. From a nested directory without
a target, use the nearest owning package. Read `docs/development.md` for tool
installation and check coverage. Keep Bazel as the entry point.

Report the scope, command, outcome, and first actionable failure. If a sandbox
restriction blocks Bazel, use the environment's escalation mechanism and report
any remaining blocker. Do not silently skip a tool or accept version drift.
