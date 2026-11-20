# Rust style

Use Rust 2024 and the pinned stable toolchain. `rustfmt` owns layout; Clippy
warnings fail lint. Prefer plain standard-library types over wrappers that add
no meaning. Name domain operations rather than exposing long iterator chains.

Use `Result` for failure, `Option` for absence, and enums for distinct states.
Return actionable errors with the path and operation that failed. Avoid `unwrap`
and `expect` in production paths that can fail because of input or the
environment. Use `Path` and `PathBuf`; paths need not be UTF-8. Borrow values
unless ownership needs to move. Add dependencies only for a concrete need.

Document public interfaces with Rustdoc: purpose, inputs, results, failures, and
side effects. Explain ordering, lifetime, clock, or filesystem assumptions where
correctness depends on them. Functions longer than 20 lines carry concise
`Step #1`, `Step #2`, and subsequent comments at logical stages.

Prefer safe Rust. Any necessary `unsafe` block must explain the invariant that
makes it safe. Keep mutable state with its owner and minimize shared state.

For directory leases, distinguish persisted expiry times from monotonic waits.
Check overflow when parsing durations or computing deadlines. Cleanup must deal
explicitly with symlinks, replaced paths, restarts, partial failures, and Git
worktree metadata. Test destructive behavior only in isolated temporary roots.
Use a controllable clock for expiry tests instead of waiting on real delays.
