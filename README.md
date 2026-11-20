# dirlease

Create a directory now. Clean it up after a delay.

dirlease is a Rust command-line project designed for coding agents that create
temporary workspaces, Git worktrees, and build artifacts that should stick
around briefly for inspection before being cleaned up.

## Status

The Rust CLI and Bazel development workflow are scaffolded. `--help` and
`--version` work. Directory creation, leases, scheduling, and cleanup are not
implemented yet.

## Development

```sh
bazel build //:dirlease
bazel run //:dirlease -- --help
bazel test //...
bazel run //:format
bazel run //:format_check
bazel run //:lint
bazel run //:review-diff
```

See [development setup](docs/development.md), [Rust style](docs/STYLE.md), and
[contribution workflow](docs/contributing.md). Agent skills live in
`.agents/skills` and are shared with Codex and Claude.

## License

Licensed under the [MIT License](LICENSE).
