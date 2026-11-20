"""Package-scoped developer commands, shared by the root and child packages."""

load("@rules_shell//shell:sh_binary.bzl", "sh_binary")

def repo_commands(scope):
    """Expose quality and review commands for a repository-relative directory.

    Args:
        scope: Directory to include, or "." for the whole repository.
    """
    for command in ["format", "format_check", "lint", "review-diff"]:
        sh_binary(
            name = command,
            srcs = ["//tools:quality.sh"],
            args = [command, scope],
            data = ["//tools:quality_files"],
            visibility = ["//visibility:public"],
        )
