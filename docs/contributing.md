If you are using AI to contribute to this repository, you must follow the rules:

# Contribution workflow

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

## Agent work

Use `$work <task description>` to run a task in a dedicated
`../dirlease-<task-slug>` worktree. It runs an implementer, an independent
reviewer, and, when needed, a fixer in sequence, then reports the pull request
and validation results. No tracker account, issue identifier, assignment, or
status update is required. Ordinary edits do not trigger that workflow. Use
`$mom-review` for a review without implementation.
