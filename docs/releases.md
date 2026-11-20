# Releases and Homebrew

The repository is also a Homebrew tap. After this change reaches `main`:

```sh
brew tap momiforgotthesemicolon/dirlease https://github.com/momiforgotthesemicolon/dirlease.git
brew install momiforgotthesemicolon/dirlease/dirlease
brew test momiforgotthesemicolon/dirlease/dirlease
```

The initial formula installs version 0.1.0 from the existing public commit
`35d16bdcdbbfb84c5e8f137ef7ba308ed73f7e1a`, verified with its actual archive
SHA-256. It does not depend on a release that has not been published. Homebrew
compiles from source using its Rust dependency and `cargo install --locked`;
this is the distribution exception to the Bazel development entry point. There
are no Homebrew bottles. The formula test only reads help and version.

Clients start the cleanup worker automatically. An optional foreground service
can instead be managed explicitly:

```sh
brew services start momiforgotthesemicolon/dirlease/dirlease
brew services stop momiforgotthesemicolon/dirlease/dirlease
```

Installation does not start this service. Read [supervision](../README.md)
before switching supervisors; use the same account and state/root environment as
the clients. Do not run two supervisors for the same store.

## Prepare and exercise a release

Use Python 3.10 or newer. Change the version together in `Cargo.toml`, the
`dirlease` entry in `Cargo.lock`, the `--version` output in `src/main.rs`, the
expectation in `src/cli_test.sh`, and the module version in `MODULE.bazel`. Use
`major.minor.patch` versions with an optional prerelease suffix, such as
`0.0.1-alpha`. Commit the complete change first. Bazel remains the build and
test entry point:

```sh
bazel test //:test
bazel run //:format_check
bazel run //:lint
bazel run //:review-diff
bazel build --config=release //:dirlease
bazel run --config=release //tools:release -- --tag v0.0.1-alpha --binary "$PWD/bazel-bin/src/dirlease" --output /tmp/dirlease-release
(cd /tmp/dirlease-release && shasum -a 256 -c SHA256SUMS)
```

Choose a fresh output directory each time. Packaging requires a clean committed
checkout, matching versions and executable output, and refuses an existing
output directory. It needs no credentials or network. Archives have normalized
ownership and timestamps; identical committed files and binary inputs produce
identical archives. This does not promise bit-identical Rust compilation across
hosts or toolchains. Source archives include committed regular files (repository
agent symlinks are omitted). `SOURCE_COMMIT` records the exact source revision.

PRs and manual workflow dispatches build and test packages with read-only
permissions and upload temporary workflow artifacts. They cannot publish a
release. Native builds cover `ubuntu-24.04` Linux x86_64 and `macos-15` macOS
arm64; no other prebuilt architectures are promised. Linux binaries inherit the
runner's libc requirements; use Homebrew/source builds on other systems. macOS
archives are unsigned and unnotarized.

## Publish intentionally after merge

Only a maintainer's deliberate `v*` tag push publishes. From an up-to-date
`main`, inspect the version and confirm the tag/release do not already exist:

```sh
git fetch origin --tags
git switch main
git pull --ff-only
version=0.0.1-alpha
git tag "v$version"
git push origin "v$version"
```

Use the actual new version. Never force or move a published tag. The workflow
requires the tagged commit to be an ancestor of `origin/main`, checks versions,
builds both native packages and creates a new GitHub Release with checksums,
source, binaries, provenance revision, and `dirlease.rb`. Tags with a prerelease
suffix create a GitHub prerelease and do not replace the latest stable release.
Only the final tag job gets `contents: write`; release creation refuses an
existing release and does not replace assets. A failed partial publication
requires manual inspection, not force-uploading assets. Protect release tags
using repository rulesets as appropriate for maintainer access.

Download the release and verify `SHA256SUMS` before extracting a binary. Archive
names identify their actual host OS and architecture; install the extracted
`dirlease` executable into your own `PATH`.

After publication, download `dirlease.rb` from that release, verify it against
`SHA256SUMS`, and propose it as `Formula/dirlease.rb` in a separate PR. Its URL
and SHA pin the published source asset. Test the formula before merging:

```sh
brew install --build-from-source momiforgotthesemicolon/dirlease/dirlease
brew test momiforgotthesemicolon/dirlease/dirlease
```

Use a disposable Homebrew installation for pre-merge formula testing. A local
formula checkout can be tapped with an explicit filesystem URL as described in
[Homebrew's tap guide](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap).
Formula conventions follow the
[cookbook](https://docs.brew.sh/Formula-Cookbook). No dedicated external tap,
credentials, or Homebrew/core submission is required.
