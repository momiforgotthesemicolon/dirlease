//! Git worktree creation and targeted registration cleanup for leased checkouts.

use crate::lease;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spec {
    pub(crate) repository: PathBuf,
    pub(crate) common: PathBuf,
    pub(crate) revision: OsString,
    pub(crate) branch: Option<OsString>,
}

/// Run Git against an explicit repository, excluding inherited repository overrides.
fn git(repository: &Path) -> Command {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.as_bytes().starts_with(b"GIT_") {
            command.env_remove(key);
        }
    }
    command.arg("-C").arg(repository).args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "worktree.useRelativePaths=false",
    ]);
    command
}

fn checked(
    output: Output,
    operation: &str,
    repository: &Path,
) -> io::Result<Vec<u8>> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git {operation} in {}: {}",
            repository.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

impl Spec {
    /// Resolve native repository paths; reject empty refs and branch option injection.
    pub(crate) fn new(
        repository: PathBuf,
        revision: OsString,
        branch: Option<OsString>,
    ) -> io::Result<Self> {
        // Step #1: Resolve the repository and common administrative directory.
        let repository = fs::canonicalize(&repository).map_err(|error| {
            io::Error::other(format!(
                "repository {}: {error}",
                repository.display()
            ))
        })?;
        let output = git(&repository)
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .output()?;
        let bytes = checked(output, "rev-parse", &repository)?;
        let common = fs::canonicalize(PathBuf::from(OsString::from_vec(
            bytes.strip_suffix(b"\n").unwrap_or(&bytes).to_vec(),
        )))?;
        // Step #2: Keep options out of revision and branch arguments.
        if revision.is_empty() || revision.as_bytes().starts_with(b"-") {
            return Err(io::Error::other(
                "worktree ref must be nonempty and must not start with '-'",
            ));
        }
        if let Some(name) = &branch {
            let mut reference = OsString::from("refs/heads/");
            reference.push(name);
            if name.is_empty() || name.as_bytes().starts_with(b"-") {
                return Err(io::Error::other("invalid worktree branch"));
            }
            checked(
                git(&repository)
                    .arg("check-ref-format")
                    .arg(reference)
                    .output()?,
                "check-ref-format",
                &repository,
            )?;
        }
        Ok(Self {
            repository,
            common,
            revision,
            branch,
        })
    }

    /// Create a detached checkout or a new branch without executing repository hooks.
    pub(crate) fn create(&self, directory: &Path) -> io::Result<()> {
        // Step #1: Resolve a commit before changing the repository's worktree registrations.
        let mut reference = self.revision.clone();
        reference.push("^{commit}");
        let output = git(&self.repository)
            .args(["rev-parse", "--verify", "--end-of-options"])
            .arg(reference)
            .output()?;
        let commit = checked(output, "rev-parse", &self.repository)?;
        let commit = OsString::from_vec(
            commit.strip_suffix(b"\n").unwrap_or(&commit).to_vec(),
        );
        // Step #2: Create only inside the already persisted lease directory.
        let mut command = git(&self.repository);
        command.args(["worktree", "add"]);
        if let Some(branch) = &self.branch {
            let mut reference = OsString::from("refs/heads/");
            reference.push(branch);
            let status = git(&self.repository)
                .args(["show-ref", "--verify", "--quiet"])
                .arg(reference)
                .status()?;
            if status.success() {
                checked(
                    command
                        .arg("--")
                        .arg(directory.join("checkout"))
                        .arg(branch)
                        .output()?,
                    "worktree add",
                    &self.repository,
                )?;
                return Ok(());
            }
            if status.code() != Some(1) {
                return Err(io::Error::other(
                    "cannot inspect existing worktree branch",
                ));
            }
            command.arg("-b").arg(branch);
        } else {
            command.arg("--detach");
        }
        checked(
            command
                .arg("--")
                .arg(directory.join("checkout"))
                .arg(commit)
                .output()?,
            "worktree add",
            &self.repository,
        )?;
        Ok(())
    }

    /// Find only registrations whose backlink names this lease's checkout.
    pub(crate) fn registrations(
        &self,
        directory: &Path,
    ) -> io::Result<Vec<(String, u64, u64)>> {
        // Step #1: Require the recorded common directory to remain available for retry.
        let common = fs::symlink_metadata(&self.common).map_err(|error| {
            io::Error::other(format!(
                "read Git common directory {}: {error}",
                self.common.display()
            ))
        })?;
        if !common.is_dir() {
            return Err(io::Error::other(format!(
                "invalid Git common directory {}",
                self.common.display()
            )));
        }
        let root = self.common.join("worktrees");
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(vec![]);
            }
            Err(error) => return Err(error),
        };
        let parent = directory
            .parent()
            .ok_or_else(|| io::Error::other("lease has no parent"))?;
        let name = directory
            .file_name()
            .ok_or_else(|| io::Error::other("lease has no name"))?;
        let mut expected = fs::canonicalize(parent)?
            .join(name)
            .join("checkout/.git")
            .into_os_string()
            .into_vec();
        expected.push(b'\n');
        // Step #2: Preserve other registrations, moved checkouts, and locked worktrees.
        let mut registrations = vec![];
        for entry in entries {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_dir() {
                continue;
            }
            let backlink = match fs::read(entry.path().join("gitdir")) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    continue;
                }
                Err(error) => return Err(error),
            };
            if backlink != expected {
                continue;
            }
            match fs::symlink_metadata(entry.path().join("locked")) {
                Ok(_) => {
                    return Err(io::Error::other(format!(
                        "worktree {} is locked; unlock it to allow expiry",
                        directory.display()
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let name = entry.file_name().into_string().map_err(|_| {
                io::Error::other("non-UTF-8 worktree registration name")
            })?;
            registrations.push((name, metadata.dev(), metadata.ino()));
        }
        Ok(registrations)
    }

    /// Remove captured registrations through descriptor-validated directory cleanup.
    pub(crate) fn remove_registrations(
        &self,
        registrations: Vec<(String, u64, u64)>,
    ) -> io::Result<()> {
        // Step #1: Remove only captured, identity-checked registration directories.
        if registrations.is_empty() {
            return Ok(());
        }
        for (name, device, inode) in registrations {
            lease::remove_registered_directory(
                &self.common.join("worktrees"),
                &name,
                device,
                inode,
            )?;
        }
        // Step #2: Persist retirement in the owning Git administrative directory.
        File::open(self.common.join("worktrees"))?.sync_all()
    }
}
