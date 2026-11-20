---
name: work
description: Implement a requested task in a dedicated git worktree through sequential implementer, independent mom-review reviewer, and fixer agents, producing a validated pull request against main without requiring an issue tracker.
---

# Work

Use this skill when the user invokes `$work <task description>`, for example
`$work add duration parsing for directory leases`. Use the supplied description,
context, and acceptance criteria; no issue identifier or tracker access is required.

The orchestrator only coordinates. It never edits code itself and never runs
agents in parallel: each subagent starts after the previous one returns, and
each starts fresh with only the context the orchestrator hands it.

## Workflow

1. Establish the task from the user's request.
   - Capture the task summary, relevant context, and acceptance criteria.
   - Ask only for missing information that blocks implementation.
   - Derive a short, lowercase, hyphenated task slug and a `work/<task-slug>` branch.
2. Create a worktree for the task from the latest `main`:

   ```sh
   git fetch origin
   git worktree add ../dirlease-<TASK_SLUG> -b work/<TASK_SLUG> origin/main
   ```

   Every subagent works only inside this worktree path. Inspect existing branches
   and worktrees before reusing them; reuse only a matching task worktree and
   preserve any existing changes. An unrelated name collision needs a distinct slug.
3. Spawn the implementer subagent with the
   [Implementer Prompt](#implementer-prompt). Wait for it to return the PR URL.
   Do not continue without an open PR.
4. Spawn an independent reviewer subagent with the
   [Reviewer Prompt](#reviewer-prompt). It runs the `mom-review` skill on the
   PR, edits nothing, and returns its findings.
5. Spawn an independent fixer subagent with the [Fixer Prompt](#fixer-prompt),
   passing the reviewer's findings verbatim. Skip this step only when the
   reviewer reported no findings.
6. Report the PR URL, the review findings, what the fixer changed or rejected,
   validation, deferred work, and remaining risks. Do not merge the PR or remove
   the task worktree unless the user asks.

## Commit Message Format

A commit message carries exactly three things, in this order, and nothing else.
No issue trackers, no file lists, no rationale essays, no test output, no
emoji, no bullet padding.

```text
<short imperative subject line>

Prior state:
<what the code did / looked like before this commit>

This commit introduces:
<what this commit changes, in terms of behavior and structure>

Generated with AI assistance.
```

Rules:

- Wrap the body at 72 columns. This applies to the commit message only, never
  to the PR body.
- Each section is at most four sentences. A commit message that needs more is
  a commit that should have been split.
- `Prior state` describes the world before the change, not the problem's
  history and not the issue text.
- `This commit introduces` describes the change itself, not the review process
  or future work.
- Keep the AI-usage disclosure verbatim as the last line of the message.
- Do not add any other section, trailer, or footer. No `Co-Authored-By`, no
  `Claude-Session`, no session links.

## PR Body Format

The PR body carries the commit message's three sections, then a `Context`
section before the AI-usage disclosure. Nothing else.

It is Markdown that GitHub renders, so it is written as Markdown — headings
and lists, not a wall of plain text. Two consequences matter:

- GitHub turns every newline into a line break. Do not hard-wrap the PR body
  at 72 columns; hard wrapping renders as a ragged narrow column. One
  paragraph is one long line, and the browser wraps it.
- Section labels are `###` headings, so `Prior state:` becomes
  `### Prior state`.

A reviewer must be able to read the whole body on one screen and know what
changed, why, and what to distrust. It is a one-pager, not a design document.

```text
### Prior state

<what the code did / looked like before this change>

### This commit introduces

<what this change does, in terms of behavior and structure>

### Context

**Why.** <the requested task and the problem or limitation it addresses>

**Fit.** <modules, build targets, runtime paths, and data contracts touched>

**Decisions**

- **<choice taken>** — <alternative rejected, and the reason>

**Validation**

- `<exact command>` — <result>

**Risks**

- <known risk, deliberate omission, or follow-up work>

Generated with AI assistance.
```

`Prior state` and `This commit introduces` make the same claims as the commit
message, reflowed as unwrapped paragraphs and at most four sentences each. On
a branch with several commits they describe the branch as a whole rather than
any single commit.

`Context` is the part a reviewer reads to understand the change without
opening the diff. It is a fixed, scannable block, not an essay.

Rules for `Context`:

- Use exactly these five labels, in this order: `Why`, `Fit`, `Decisions`,
  `Validation`, `Risks`. Never add, drop, rename, or reorder a label; write a
  single `- None.` bullet when a label has nothing to report.
- `Why` and `Fit` are one sentence each, two at the very most.
- `Decisions`, `Validation`, and `Risks` are lists of at most three bullets,
  one rendered line each. Lead each `Decisions` bullet with the choice in
  bold. Keep only what a reviewer could not reconstruct from the diff; drop
  the routine entries rather than shortening every entry.
- Quote commands inline as `` `command` `` and give the outcome in a few
  words. Pasted command output, transcripts, and multi-line code blocks do not
  belong here.
- No sub-headings beyond the five labels, no tables, no images.
- The whole body stays under 350 words. Detail that does not fit belongs in
  the code, a comment, a doc, or a follow-up reported in the handoff.

Rules:

- No `Co-Authored-By`, no `Claude-Session`, no session links, no
  "Generated with Claude Code" footer, no emoji.
- Keep the AI-usage disclosure verbatim as the last line of the body.

## Implementer Prompt

```text
Implement the requested task in the worktree <WORKTREE_PATH> on branch
<BRANCH>. Work only inside that worktree.

Task:
<summary, description, acceptance criteria, relevant context>

Follow AGENTS.md and the smallest target docs for the files you touch. Keep
the change scoped to the task and update every affected reference. Run the
focused tests for the touched targets.

When the change is done and validated:
- commit it using the commit message format below;
- push the branch with upstream set;
- open a pull request against main with `gh pr create`, titled with a concise
  task summary, with the PR body format below.

<Commit Message Format and PR Body Format sections from this skill>

Return the PR URL, changed files, validation run and results, and any risks.
```

## Reviewer Prompt

```text
Run the mom-review skill on pull request <PR_URL> for the requested task.
The branch is checked out at <WORKTREE_PATH>.

Task summary and acceptance criteria: <summary and criteria>

Do not edit files, commit, or post the review. Return the findings in the
mom-review output format, ranked by severity, each with a file and line anchor
and the concrete fix.
```

The reviewer gets no implementer summary beyond the task and the PR, so it
judges the diff on its own.

## Fixer Prompt

```text
Address the review findings on pull request <PR_URL> for the requested task,
in the worktree <WORKTREE_PATH> on branch <BRANCH>. Work only inside that worktree.

Findings:
<reviewer findings, verbatim>

Fix every finding that holds. Reject a finding only when it conflicts with
the repository conventions or is factually wrong, and say why. Rerun the
focused tests for the touched targets, then commit using the commit message
format below and push to the same branch. If the fixes change what the PR
describes, update the PR body with `gh pr edit`.

<Commit Message Format and PR Body Format sections from this skill>

Return each finding with its outcome (fixed or rejected, and why), the
commits pushed, and validation run and results.
```
