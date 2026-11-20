---
name: review-diff
description: Show branch and working changes, including new files, with focused validation suggestions through Bazel.
---

# Review Diff

Run `bazel run :review-diff` from the root for the full repository, or from `src/`
or `tools/` for that package and its descendants. From a nested directory without
a target, use the nearest owning package. Read `docs/development.md` for tool
installation and check coverage. Keep Bazel as the entry point.

Report the scope, command, outcome, and first actionable failure. If a sandbox
restriction blocks Bazel, use the environment's escalation mechanism and report
any remaining blocker. Do not silently skip a tool or accept version drift.

Run this target before committing or reporting a completed change. Its output
includes committed changes against local main, working changes, untracked file
contents, and scoped validation suggestions. Run relevant suggested checks and
report any skipped checks with their reason.
