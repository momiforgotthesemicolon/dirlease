# dirlease

Create a temporary session directory now; remove it after a fixed lifetime. Unix
only (macOS and Linux), using rustix for safe filesystem descriptor access.

```sh
workspace="$(dirlease)"                 # automatically creates a session
cd "$workspace"
```

An unset or empty `DIRLEASE_SESSION` is initialized automatically for each
invocation. A child process cannot export variables back into its parent shell,
so separate commands get separate sessions unless you export a shared
identifier:

```sh
# Initialize once in the parent shell or agent launcher; children inherit it.
export DIRLEASE_SESSION="$(uuidgen)"
workspace="$(dirlease)"                 # default TTL: 25 hours
cd "$workspace"
dirlease create --ttl 2d               # reuses the same path and original expiry
dirlease --new --ttl 30m               # changes this session's current lease
```

A nonempty `DIRLEASE_SESSION` is preserved. Choose a different identifier for
independent sessions; the identifier never becomes a filename. No arguments
means `create`. TTL accepts positive integer seconds (`s`), minutes (`m`), hours
(`h`), or days (`d`). Reuse does not renew the expiry, even with a different
TTL. An expired, removed, or replaced current directory causes a fresh
allocation. `--new` preserves the old lease until its own expiry. Successful
create commands write only an absolute native path and newline to stdout; errors
go to stderr. Paths and identifiers need not be UTF-8.

**Expiry can delete files used by active consumers.** Returning a path cannot
track activity. Choose a sufficient TTL. An activity-aware Bazel run wrapper is
deferred. Git worktree directories are deleted as filesystem trees; run
`git worktree prune` in the owning repository afterward to clear its metadata.

## Worker and durability

The client automatically starts a detached `dirlease serve` process with all
standard streams closed to the caller. Command substitutions finish promptly. A
private Unix socket serializes requests; a filesystem lock prevents duplicate
workers. Startup retries take at most five seconds, and each socket read/write
has a three-second timeout. `dirlease serve` runs in the foreground for
diagnosis or supervision. Automatic startup diagnostics are suppressed; run the
foreground command if startup fails.

State lives outside temporary storage: `~/Library/Application Support/dirlease`
on macOS, and `$XDG_STATE_HOME/dirlease` or `~/.local/state/dirlease` on Linux.
Directories live under the OS temporary directory's `dirlease/<unique ID>`.
`DIRLEASE_STATE_DIR` and `DIRLEASE_TEMP_ROOT` override these roots with absolute
paths. Roots must be private (mode 700), owned by the current user, and stable;
changing the temporary root with existing records is rejected. Use a per-user
override if another account owns the default temporary root. Parent directories
and account-owned state must be trusted; this is not a security boundary against
other processes running as the same user.

The worker flushes an atomic state snapshot and its directory entry before
returning a path. It sweeps overdue leases on restart, then every second, and
retains failures for retry. Expiry uses persisted Unix wall-clock seconds; clock
changes affect deadlines. Sleeps use monotonic time. Cleanup quarantines the
entry, opens it without following symlinks, and verifies the descriptor against
the recorded device/inode. Recursive removal stays relative to retained
directory descriptors, so replacing quarantine cannot redirect traversal. The
final nonrecursive unlink can remove an empty replacement directory; nonempty
replacements are preserved and reported for retry. Interrupted quarantine
operations are resumed. Missing paths are retired. State corruption fails
closed. A crash between directory allocation and its first state commit can
leave an unacknowledged orphan; it is preserved rather than guessing ownership.
Interrupted snapshot writes can leave harmless `state-*` files. Do not edit
state manually.

`worker.pid` is advisory diagnostics (it may be stale); `leases` contains the
versioned snapshot with hex-encoded native session/root bytes and expiry/device/
inode fields. Lease IDs are independent of session identifiers. Back up state
with its configured temporary root if needed; removing state loses cleanup
knowledge. The OS may remove temporary directories earlier than their TTL.

## Supervision

Auto-start recovers on the next client call after a worker crash. Install a
supervisor for unattended restart and cleanup after login. Install the binary at
a stable absolute path first (`bazel build --config=release //:dirlease`). Never
point a service at a Bazel sandbox or a transient runfiles path. Both examples
use the default roots; configure matching environment overrides for service and
clients if needed. Before first enabling a service, stop an existing
auto-started worker after verifying `worker.pid` against its running process.
The supervisor and auto-started worker share the same lock.

On Linux, save this as `~/.config/systemd/user/dirlease.service`, replacing the
binary path, then run `systemctl --user daemon-reload` and
`systemctl --user enable --now dirlease.service`:

```ini
[Unit]
Description=Directory lease cleanup

[Service]
ExecStart=/absolute/path/to/dirlease serve
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
```

User services normally run while logged in. For logout/boot persistence enable
lingering with `loginctl enable-linger "$USER"` if local policy permits it.
Check diagnostics with `journalctl --user -u dirlease`.

On macOS, save this as `~/Library/LaunchAgents/local.dirlease.plist`, replacing
the binary path (XML-escape special characters), then run
`launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/local.dirlease.plist"`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>local.dirlease</string>
  <key>ProgramArguments</key><array>
    <string>/absolute/path/to/dirlease</string><string>serve</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>2</integer>
</dict></plist>
```

This LaunchAgent resumes at login and restarts on failure; it does not run while
logged out. Inspect it with `launchctl print "gui/$(id -u)/local.dirlease"`. No
service is installed automatically.

## Development

```sh
bazel build //:dirlease
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

## Installation and releases

See [Homebrew installation and maintainer releases](docs/releases.md) for the
source tap, optional service, checksummed archives, and release dry runs.
