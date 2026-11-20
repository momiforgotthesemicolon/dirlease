# Instructions

Start each message with "Mr. President, "

When making changes, don't worry about backwards compatibility, but make sure
that all references are updated.

Follow repository coding style specified in the docs very strictly. Do not ever
restore formatting changes.

## Code Clarity and Documentation

- Make changes easy to understand and follow. Use intuitive, descriptive names.
- Every function definition longer than 20 lines must include concise comments
  labeled `Step #1`, `Step #2`, and so on, explaining its logical stages in
  order.
- Code documentation describes the code's context and behavior, never why a
  change was made. Include brief usage instructions or examples when helpful.
- Keep documentation and agent responses concise. Do not use many words to
  explain simple things or repeat what the code already makes clear.

## Work Tracking

Work starts from the user's task description and acceptance criteria. No issue
tracker or external workspace is required. Keep changes scoped to the task and
report deferred work in the handoff; include an existing issue link when
supplied.

## Commits

A commit message carries exactly three things, in this order, and nothing else:

```text
<short imperative subject line>

Prior state:
<what the code did / looked like before this commit>

This commit introduces:
<what this commit changes, in terms of behavior and structure>

Generated with AI assistance.
```

Wrap the body at 72 columns and keep each section to at most four sentences.
Never append a `Co-Authored-By` trailer, a `Claude-Session` trailer, a session
link, or any other footer.

A pull request body carries those three sections, then a `Context` section
before the AI-usage disclosure — and nothing else. It is Markdown that GitHub
renders, so write it as Markdown and never hard-wrap it: GitHub turns every
newline into a line break, and a 72-column PR body renders as a ragged narrow
column. One paragraph is one long line.

```text
### Prior state

<...>

### This commit introduces

<...>

### Context

**Why.** <the issue and the problem or limitation behind it>

**Fit.** <modules, build targets, runtime paths, and data contracts touched>

**Decisions**

- **<choice taken>** — <alternative rejected, and the reason>

**Validation**

- `<exact command>` — <result>

**Risks**

- <known risk, deliberate omission, or follow-up>

Generated with AI assistance.
```

The first two sections make the same claims as the commit message, reflowed and
still at most four sentences each. `Context` carries the reasoning, but as a
fixed block a reviewer reads on one screen: exactly those five labels, in that
order, none added, dropped, or renamed — write a `- None.` bullet instead. `Why`
and `Fit` are one sentence each; `Decisions`, `Validation`, and `Risks` hold at
most three one-line bullets apiece, and only the ones a reviewer could not
reconstruct from the diff. No further sub-headings, no tables, no images, no
pasted command output, and the whole body stays under 350 words. No "Generated
with Claude Code" footer, no emoji.

## Documentation Map / Agent Instructions

Use this file as a routing table, not as full project documentation. Open the
smallest linked document that matches the current task.

- `docs/README.md`: documentation entry point.

- `docs/development.md`: Bazel, tool versions, package scope, and validation.

- `docs/STYLE.md`: Rust coding and filesystem conventions.

- `docs/contributing.md`: commit and PR formats and issue workflow.

## Target Doc Pattern

For target-specific guidance, derive the target from the current path and prefer
this lookup order:

1. `{target}/docs/README.md`
1. `{target}/docs/STYLE.md`
1. `{target}/docs/BAZEL_TARGETS.md`
1. `{target}/README.md`

Available targets:

- `src`: Rust CLI sources and CLI tests.
- `tools`: repository quality tools and their tests.

If a target-specific document does not exist yet, use the closest parent
document and keep new documentation small and colocated with the target it
describes. For both current targets, the root `docs/` directory supplies the
shared guidance.

## Targets

This project is a Rust CLI built with Bazel. Its shorthand targets are
intentionally package-sensitive.

- Main available targets: `test`, `format`, `format_check`, `lint`, and
  `review-diff`.
- Running a shorthand from a package covers only that package and its
  descendants.
- Running the same shorthand from a parent package aggregates the matching child
  targets at that level and below.
- Higher-level Bazel targets have wider scope; lower-level targets have narrower
  scope.
- Run `bazel test :test` for tests and `bazel run :<command>` for quality
  commands. Root targets cover the repository; `src/` and `tools/` provide
  narrower scopes.

Run `bazel run :review-diff` before reporting a completed change or committing.
Run focused tests plus format checks and lint for the touched scope. The Unix
CLI implements session leases, durable state, and background cleanup.

## Repository Skills

Canonical skills live in `.agents/skills`. `.codex/skills` and `.claude/skills`
link to that directory so both agents use the same instructions.

- `format-repo-files`, `lint-repo-files`, and `review-diff`: Bazel quality
  commands.
- `mom-review`: maintainer-style review adapted for Rust and directory leases.
- `work`: an explicitly requested task through implement, review, and fix agents
  in sequence, on a dedicated `dirlease` worktree.
