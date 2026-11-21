## THIS IS WORK IN PROGERESS

# dirlease

Coding agents create worktrees, build outputs, and temporary files that can
quickly fill a disk. dirlease gives each task a temporary directory or Git
worktree and schedules automatic cleanup after a fixed lifetime.

Runs on macOS and Linux. Worktrees require Git.

## Install

```sh
brew tap momiforgotthesemicolon/dirlease https://github.com/momiforgotthesemicolon/dirlease.git
brew install momiforgotthesemicolon/dirlease/dirlease
```

Later, use `brew update` and `brew upgrade dirlease`. If Homebrew asks for
trust, run `brew trust --formula momiforgotthesemicolon/dirlease/dirlease`.

## Create an agent worktree

```sh
dirlease worktree --session my-task --repo /path/to/repo --branch agent/my-task --ttl 2d
```

This prints the worktree's absolute path. Start your agent in that directory.
Repeat the same command to get the same path; use a different session and branch
for each task. No shell integration is needed.

Omit `--branch` for a detached checkout. Use `--ref main` to choose the starting
commit. An existing branch is reused at its current tip. `--new` requests a
fresh lease; a branch still in use cannot be checked out twice.

## Configure Rex

Add this action to `~/.config/rex/init.lua` (or your `REX_CONFIG` file). Ensure
`dirlease`, Git, and your agent executable are on the Rex server's `PATH`.

```lua
rex.action{
  name = "leased_agent",
  title = "Open agent worktree",
  args = { repo = "string", task = "string", agent = "string?" },
  run = function(_, args)
    -- Step #1: Create or retrieve this repository/task's leased worktree.
    local result, err = rex.exec({
      "dirlease", "worktree", "--session", args.repo .. ":" .. args.task,
      "--repo", args.repo, "--branch", "agent/" .. args.task, "--ttl", "2d",
    }, { timeout = 20 })
    if result == nil then error(err, 0) end
    if not result.ok then
      error(result.stderr ~= "" and result.stderr or "Checkout timed out; retry the same task", 0)
    end
    local path = result.stdout:gsub("\n$", "")
    -- Step #2: Start the agent in a new Rex tab at the returned path.
    local window, open_err = rex.session.new_window{
      layout = rex.layout.block{
        flavor = "com.superlogical.terminal.shell",
        options = { cwd = path, command = { args.agent or "codex" } },
      },
    }
    if window == nil then error(open_err, 0) end
    return window
  end,
}
```

Reload the configuration, then run this **inside Rex**:

```sh
rex config reload
rex do leased_agent repo=/absolute/path/to/repo task=fix-login
```

The default agent is `codex`; pass `agent=claude` to use Claude Code instead.
Repeat the same repository path and task name to reopen its worktree. Each call
opens another agent tab, so avoid running two agents on the same task at once.
The action uses
[Rex custom actions](https://www.superlogical.com/rex/docs/customize/actions.md).

## Temporary directories

```sh
dirlease                 # create or reuse this Unix session's directory
dirlease --ttl 2h        # set the lifetime when creating a directory
dirlease --new           # create a fresh directory
```

The default lifetime is 25 hours. Units are `s`, `m`, `h`, and `d`. An explicit
`DIRLEASE_SESSION` overrides the Unix session identity, for example:

```sh
DIRLEASE_SESSION=my-task dirlease --ttl 2d
```

## Cleanup

The worker starts automatically and removes expired directories and leased
worktrees, including their Git registrations. Branches remain.

**Expiry deletes dirty and untracked files, even while an agent is working.**
Repeating a command does not extend its deadline. To keep a worktree
temporarily:

```sh
git worktree lock /path/printed/by/dirlease
git worktree unlock /path/printed/by/dirlease
```

For cleanup across restarts, configure a
[background service](docs/runtime.md#supervision). After upgrading, restart an
older worker or its service before using new commands.

## More

- [Runtime, storage, and services](docs/runtime.md)
- [Development](docs/development.md) and [contributing](docs/contributing.md)
- [Releases and Homebrew](docs/releases.md)
- [MIT license](LICENSE)
