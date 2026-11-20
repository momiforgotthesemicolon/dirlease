---
name: mom-review
description: Review a dirlease PR, branch, or working diff in the maintainer's direct style, emphasizing maintainability, Rust ownership and errors, and safe delayed filesystem cleanup. Use for a mom review, review like me, or a requested pre-review.
---

# Mom Review

Prioritize maintainability: the next person must be able to read, change, and own
this code safely. Read `AGENTS.md`, `docs/STYLE.md`, and every changed file in
context. Review the named PR, branch, or working diff; default to the current
branch against main. Use the GitHub connector or `gh pr diff` for PRs and
`bazel run :review-diff` for local changes.

Check:

1. Correctness: ownership, races, partial failure, retry behavior, error context,
   numeric overflow, and process exit status.
2. Readability: descriptive names, logical separators, simple control flow, and
   `Step #` comments for functions longer than 20 lines. Prefer simplifying code
   over explaining avoidable complexity.
3. Interfaces: Rustdoc explains purpose, inputs, results, failure, side effects,
   and relevant lifetime assumptions. Avoid wrappers and traits without a concrete
   use; idiomatic `std` types are welcome.
4. Rust: borrow instead of cloning by habit, distinguish `Result` from `Option`,
   avoid panics on environmental failures, and explain any necessary `unsafe`.
   Expose domain operations instead of dense iterator or generic machinery.
5. Directory leases: handle non-UTF-8 paths, symlinks, path replacement, accidental
   deletion outside managed roots, expired and far-future deadlines, clock changes,
   restart recovery, and Git worktree metadata. Require evidence for cleanup safety
   rather than assuming canonicalization alone prevents races.
6. Tests and process: exercise changed behavior and relevant failure boundaries,
   isolate destructive tests in temporary roots, use controllable clocks, validate
   through the scoped Bazel targets, and report unrelated work separately from the reviewed change.

Write short, direct findings, usually as a question pointing at the fix. One
finding per anchor; group repeated instances. Rank correctness first,
maintainability second, design and performance third. Do not invent defects or
require abstraction for its own sake.

```text
src/lease.rs:42
  what happens if this path is replaced before the cleanup runs?
  -> validate the managed directory identity at deletion time and test replacement.
```

Each finding includes a file and line, the maintainer-style comment, and a concrete
fix after `->`. End with a brief verdict for each of the six checks, including any
validation gaps. If no findings are supported, say so.

Return findings in chat by default. Only when asked to post, create a pending
GitHub review with inline comments; leave submission to the maintainer. If a
pending review already exists, report it and return findings without submitting
or replacing it. When asked to fix findings, address valid findings and rerun the
relevant checks before reporting.
