//! Durable lease records and identity-checked cleanup under private roots.

use crate::worktree::Spec;
use rustix::fs::{self as descriptor, AtFlags, Dir, Mode, OFlags};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{
    DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Read persisted wall-clock seconds, failing for a pre-epoch clock.
pub(crate) fn now() -> io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(io::Error::other)
}

/// Generate a process-local unique, session-independent allocation name.
pub(crate) fn unique() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Create a private managed directory or reject unsafe existing permissions.
pub(crate) fn private_directory(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other(format!(
            "{} must be a private directory (mode 700)",
            path.display()
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Pending,
    Ready,
    Retired,
}

#[derive(Debug)]
struct Lease {
    id: String,
    session: Vec<u8>,
    expires: u64,
    device: u64,
    inode: u64,
    worktree: Option<Spec>,
    phase: Phase,
}

/// Single-worker durable lease snapshot; callers must hold the worker lock.
pub(crate) struct Store {
    state: PathBuf,
    root: PathBuf,
    leases: Vec<Lease>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> io::Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(io::Error::other("invalid hex"));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16).map_err(io::Error::other)
        })
        .collect()
}

fn decode_worktree(fields: &[&str]) -> io::Result<Option<Spec>> {
    // Step #1: Accept directory-only records without hidden worktree metadata.
    if fields.len() == 5 {
        return Ok(None);
    }
    if fields[5] == "-" {
        if fields[6..9] != ["-", "-", "-"] || fields[9] == "0" {
            return Err(io::Error::other("invalid directory lease metadata"));
        }
        return Ok(None);
    }
    // Step #2: Require native absolute paths and nonempty checkout arguments.
    let repository = PathBuf::from(OsString::from_vec(unhex(fields[5])?));
    let common = PathBuf::from(OsString::from_vec(unhex(fields[6])?));
    let revision = OsString::from_vec(unhex(fields[7])?);
    let branch = if fields[8] == "-" {
        None
    } else {
        Some(OsString::from_vec(unhex(fields[8])?))
    };
    let invalid_argument = |value: &std::ffi::OsStr| {
        value.is_empty()
            || value.as_bytes().contains(&0)
            || value.as_bytes().starts_with(b"-")
    };
    if !repository.is_absolute()
        || !common.is_absolute()
        || repository.as_os_str().as_bytes().contains(&0)
        || common.as_os_str().as_bytes().contains(&0)
        || invalid_argument(&revision)
        || branch.as_deref().is_some_and(invalid_argument)
    {
        return Err(io::Error::other("invalid worktree lease metadata"));
    }
    Ok(Some(Spec {
        repository,
        common,
        revision,
        branch,
    }))
}

impl Store {
    /// Load state for a fixed native temporary root, rejecting malformed storage.
    pub(crate) fn load(state: PathBuf, root: PathBuf) -> io::Result<Self> {
        // Step #1: Validate managed roots before decoding durable records.
        private_directory(&state)?;
        private_directory(&root)?;
        let mut store = Self {
            state,
            root,
            leases: vec![],
        };
        let bytes = match fs::read(store.state.join("leases")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(store),
            Err(e) => return Err(e),
        };
        // Step #2: Reject malformed records rather than guessing deletion targets.
        let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
        let mut lines = text.lines();
        let version = lines.next();
        if !matches!(version, Some("dirlease-v1" | "dirlease-v2"))
            || lines.next()
                != Some(hex(store.root.as_os_str().as_bytes()).as_str())
        {
            return Err(io::Error::other(
                "invalid lease state version or changed temporary root",
            ));
        }
        for line in lines {
            let fields: Vec<_> = line.split(' ').collect();
            if fields.len()
                != if version == Some("dirlease-v1") {
                    5
                } else {
                    10
                }
                || fields[0].is_empty()
                || !fields[0].bytes().all(|b| b.is_ascii_digit() || b == b'-')
            {
                return Err(io::Error::other("invalid lease record"));
            }
            let number =
                |i: usize| fields[i].parse::<u64>().map_err(io::Error::other);
            store.leases.push(Lease {
                id: fields[0].into(),
                session: unhex(fields[1])?,
                expires: number(2)?,
                device: number(3)?,
                inode: number(4)?,
                worktree: decode_worktree(&fields)?,
                phase: if fields.len() == 5 {
                    Phase::Ready
                } else {
                    match fields[9] {
                        "0" => Phase::Pending,
                        "1" => Phase::Ready,
                        "2" => Phase::Retired,
                        _ => {
                            return Err(io::Error::other(
                                "invalid lease phase",
                            ));
                        }
                    }
                },
            });
        }
        Ok(store)
    }

    fn persist(&self) -> io::Result<()> {
        // Step #1: Flush a complete replacement before publishing its name.
        let temporary = self.state.join(format!("state-{}", unique()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        writeln!(
            file,
            "dirlease-v2\n{}",
            hex(self.root.as_os_str().as_bytes())
        )?;
        for lease in &self.leases {
            let fields = match &lease.worktree {
                Some(spec) => format!(
                    "{} {} {} {}",
                    hex(spec.repository.as_os_str().as_bytes()),
                    hex(spec.common.as_os_str().as_bytes()),
                    hex(spec.revision.as_bytes()),
                    spec.branch.as_ref().map_or_else(
                        || "-".into(),
                        |name| hex(name.as_bytes())
                    )
                ),
                None => "- - - -".into(),
            };
            writeln!(
                file,
                "{} {} {} {} {} {} {}",
                lease.id,
                hex(&lease.session),
                lease.expires,
                lease.device,
                lease.inode,
                fields,
                match lease.phase {
                    Phase::Pending => 0,
                    Phase::Ready => 1,
                    Phase::Retired => 2,
                }
            )?;
        }
        file.sync_all()?;
        // Step #2: Commit the snapshot and its directory entry before acknowledgement.
        fs::rename(&temporary, self.state.join("leases"))?;
        File::open(&self.state)?.sync_all()
    }

    /// Reuse or durably allocate a lease at the supplied wall-clock second.
    pub(crate) fn create(
        &mut self,
        session: Vec<u8>,
        ttl: u64,
        fresh: bool,
        now: u64,
    ) -> io::Result<PathBuf> {
        self.allocate(session, ttl, fresh, now, None)
    }

    /// Create or reuse a Git checkout with a fixed deadline and persisted ownership.
    pub(crate) fn worktree(
        &mut self,
        session: Vec<u8>,
        ttl: u64,
        fresh: bool,
        now: u64,
        spec: Spec,
    ) -> io::Result<PathBuf> {
        self.allocate(session, ttl, fresh, now, Some(spec))
            .map(|path| path.join("checkout"))
    }

    fn allocate(
        &mut self,
        session: Vec<u8>,
        ttl: u64,
        fresh: bool,
        now: u64,
        worktree: Option<Spec>,
    ) -> io::Result<PathBuf> {
        // Step #1: Validate the deadline and reuse only the latest original directory.
        let expires = now
            .checked_add(ttl)
            .filter(|_| ttl > 0)
            .ok_or_else(|| io::Error::other("expiry overflow or zero TTL"))?;
        if !fresh
            && let Some(lease) = self
                .leases
                .iter()
                .rev()
                .find(|lease| lease.session == session)
        {
            let path = self.root.join(&lease.id);
            if lease.expires > now
                && lease.phase == Phase::Ready
                && matches_identity(&path, lease)?
            {
                if lease.worktree != worktree {
                    return Err(io::Error::other(
                        "session already has a different lease configuration; use another --session or --new",
                    ));
                }
                if lease.worktree.is_some()
                    && (!fs::symlink_metadata(path.join("checkout"))
                        .is_ok_and(|metadata| metadata.is_dir())
                        || !fs::symlink_metadata(path.join("checkout/.git"))
                            .is_ok_and(|metadata| metadata.is_file()))
                {
                    return Err(io::Error::other(
                        "leased checkout is missing or replaced; use --new",
                    ));
                }
                return Ok(path);
            }
        }
        // Step #2: Allocate a session-independent name and durably record its identity.
        let id = unique();
        let path = self.root.join(&id);
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        File::open(&path)?.sync_all()?;
        File::open(&self.root)?.sync_all()?;
        self.leases.push(Lease {
            id,
            session,
            expires,
            device: metadata.dev(),
            inode: metadata.ino(),
            phase: if worktree.is_none() {
                Phase::Ready
            } else {
                Phase::Pending
            },
            worktree,
        });
        if let Err(error) = self.persist() {
            self.leases.pop();
            let _ = fs::remove_dir(&path);
            return Err(error);
        }
        // Step #3: Persist intent before Git runs; acknowledge only a complete checkout.
        let index = self.leases.len() - 1;
        if let Some(spec) = &self.leases[index].worktree {
            if let Err(error) = spec.create(&path) {
                self.leases[index].expires = now;
                self.persist()?;
                return Err(error);
            }
            self.leases[index].phase = Phase::Ready;
            if let Err(error) = self.persist() {
                self.leases[index].phase = Phase::Pending;
                return Err(error);
            }
        }
        Ok(path)
    }

    /// Remove overdue original directories; persist retirement and retry failures.
    pub(crate) fn cleanup(&mut self, now: u64) -> io::Result<()> {
        // Step #1: Retain failed deletions and the latest session identity; retire older entries.
        if self.leases.iter().all(|lease| {
            lease.phase == Phase::Retired
                || (lease.expires > now && lease.phase == Phase::Ready)
        }) {
            return Ok(());
        }
        let original_count = self.leases.len();
        let mut sessions = HashSet::new();
        let current: HashSet<_> = self
            .leases
            .iter()
            .rev()
            .filter(|lease| sessions.insert(lease.session.clone()))
            .map(|lease| lease.id.clone())
            .collect();
        let mut retained = Vec::new();
        let mut failure = None;
        let mut changed = false;
        for mut lease in self.leases.drain(..) {
            if lease.phase == Phase::Retired {
                if current.contains(&lease.id) {
                    retained.push(lease);
                }
                continue;
            }
            if lease.expires > now && lease.phase == Phase::Ready {
                retained.push(lease);
                continue;
            }
            if let Err(error) = cleanup_lease(&self.root, &lease) {
                failure = Some(error);
                retained.push(lease);
            } else if current.contains(&lease.id) {
                // Keep the session's latest identity as a tombstone, never revive an older lease.
                lease.phase = Phase::Retired;
                changed = true;
                retained.push(lease);
            }
        }
        // Step #2: Persist retirement even when another entry needs a later retry.
        self.leases = retained;
        File::open(&self.root)?.sync_all()?;
        if changed || self.leases.len() != original_count {
            self.persist()?;
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn cleanup_lease(root: &Path, lease: &Lease) -> io::Result<()> {
    if let Some(spec) = &lease.worktree {
        let registrations = spec.registrations(&root.join(&lease.id))?;
        remove_lease(root, lease)?;
        spec.remove_registrations(registrations)
    } else {
        remove_lease(root, lease)
    }
}

/// Remove one recorded Git administrative directory without following replaced paths.
pub(crate) fn remove_registered_directory(
    root: &Path,
    id: &str,
    device: u64,
    inode: u64,
) -> io::Result<()> {
    // Step #1: Bind the registration to its captured filesystem identity.
    let registration = Lease {
        id: id.into(),
        session: vec![],
        expires: 0,
        device,
        inode,
        worktree: None,
        phase: Phase::Ready,
    };
    // Step #2: Quarantine and remove it through retained directory descriptors.
    remove_lease(root, &registration)
}

fn matches_identity(path: &Path, lease: &Lease) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.is_dir()
            && meta.dev() == lease.device
            && meta.ino() == lease.inode),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn open_directory(
    parent: impl AsFd,
    name: impl rustix::path::Arg,
) -> rustix::io::Result<File> {
    Ok(File::from(descriptor::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?))
}

/// Remove children relative to retained descriptors; never follow a symlink.
fn remove_contents(directory: &File) -> io::Result<()> {
    // Step #1: Enumerate the opened directory, ignoring traversal aliases.
    for entry in Dir::read_from(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == c"." || name == c".." {
            continue;
        }
        // Step #2: Recurse only through no-follow directory descriptors.
        match open_directory(directory, name) {
            Ok(child) => {
                remove_contents(&child)?;
                descriptor::unlinkat(directory, name, AtFlags::REMOVEDIR)?;
            }
            Err(rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
                descriptor::unlinkat(directory, name, AtFlags::empty())?;
            }
            Err(rustix::io::Errno::NOENT) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn remove_lease(root: &Path, lease: &Lease) -> io::Result<()> {
    remove_lease_with_hook(root, lease, || {}).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cleanup {}: {error}", root.join(&lease.id).display()),
        )
    })
}

/// The hook runs after descriptor identity validation, before any recursive deletion.
fn remove_lease_with_hook(
    root: &Path,
    lease: &Lease,
    after_validation: impl FnOnce(),
) -> io::Result<()> {
    // Step #1: Anchor quarantine operations to the opened root, including restart recovery.
    let root_directory = descriptor::open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let quarantine = format!(".cleanup-{}", lease.id);
    match descriptor::statat(
        &root_directory,
        &quarantine,
        AtFlags::SYMLINK_NOFOLLOW,
    ) {
        Ok(_) => {}
        Err(rustix::io::Errno::NOENT) => {
            match descriptor::renameat(
                &root_directory,
                &lease.id,
                &root_directory,
                &quarantine,
            ) {
                Ok(()) => {}
                Err(rustix::io::Errno::NOENT) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    }
    // Step #2: Validate the descriptor that will be traversed, never its mutable pathname.
    let directory = open_directory(&root_directory, quarantine.as_str());
    let original = match &directory {
        Ok(directory) => {
            let metadata = directory.metadata()?;
            metadata.dev() == lease.device && metadata.ino() == lease.inode
        }
        Err(_) => false,
    };
    if !original {
        if matches!(
            descriptor::statat(
                &root_directory,
                &lease.id,
                AtFlags::SYMLINK_NOFOLLOW
            ),
            Err(rustix::io::Errno::NOENT)
        ) {
            descriptor::renameat(
                &root_directory,
                &quarantine,
                &root_directory,
                &lease.id,
            )?;
        }
        return Err(io::Error::other(format!(
            "refusing replaced lease {}",
            root.join(&lease.id).display()
        )));
    }
    // Step #3: Retain the validated descriptor across concurrent renames and recursive deletion.
    after_validation();
    remove_contents(&directory?)?;
    descriptor::unlinkat(&root_directory, &quarantine, AtFlags::REMOVEDIR)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("dirlease-test-{}", unique()));
            private_directory(&path).unwrap();
            Self(path)
        }
        fn repository(&self) -> Spec {
            // Step #1: Initialize an isolated committed repository with native paths.
            let repository = self.0.join("repository with spaces-λ");
            fs::create_dir(&repository).unwrap();
            git_test(&repository, &["init", "-q", "--initial-branch=main"]);
            fs::write(repository.join("tracked"), b"original").unwrap();
            git_test(&repository, &["add", "tracked"]);
            // Step #2: Supply fixture identity independently of developer Git config.
            git_test(
                &repository,
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "-qm",
                    "initial",
                ],
            );
            Spec::new(repository, "HEAD".into(), None).unwrap()
        }
        fn store(&self) -> Store {
            Store::load(self.0.join("state"), self.0.join("temp")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git_test(repository: &Path, arguments: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn worktree_reuses_original_deadline_and_removes_only_its_registration() {
        // Step #1: Create an agent branch and an unrelated stale registration.
        let fixture = Fixture::new();
        let mut spec = fixture.repository();
        git_test(
            &spec.repository,
            &["config", "worktree.useRelativePaths", "true"],
        );
        spec.branch = Some("agent/task".into());
        let mut store = fixture.store();
        let checkout = store
            .worktree(b"rex-agent".to_vec(), 10, false, 100, spec.clone())
            .unwrap();
        fs::write(checkout.join("dirty"), b"uncommitted").unwrap();
        let mut detached = spec.clone();
        detached.branch = None;
        let unrelated = fixture.0.join("unrelated");
        fs::create_dir(&unrelated).unwrap();
        detached.create(&unrelated).unwrap();
        fs::remove_dir_all(unrelated.join("checkout")).unwrap();
        let count =
            fs::read_dir(spec.common.join("worktrees")).unwrap().count();
        assert_eq!(count, 2);
        // Step #2: Reuse across restart without extending the deadline or changing config.
        drop(store);
        let mut store = fixture.store();
        assert_eq!(
            store
                .worktree(b"rex-agent".to_vec(), 1000, false, 105, spec.clone())
                .unwrap(),
            checkout
        );
        assert!(store.create(b"rex-agent".to_vec(), 10, false, 105).is_err());
        assert!(
            store
                .worktree(b"rex-agent".to_vec(), 10, false, 105, detached)
                .is_err()
        );
        store.cleanup(109).unwrap();
        assert!(checkout.exists());
        // Step #3: Expiry removes dirty files and exactly one registration, retaining the branch.
        store.cleanup(110).unwrap();
        assert!(!checkout.exists());
        assert_eq!(
            fs::read_dir(spec.common.join("worktrees")).unwrap().count(),
            1
        );
        git_test(
            &spec.repository,
            &["show-ref", "--verify", "refs/heads/agent/task"],
        );
        let recreated = store
            .worktree(b"rex-agent".to_vec(), 10, false, 111, spec.clone())
            .unwrap();
        assert_ne!(recreated, checkout);
        store.cleanup(121).unwrap();
        fs::rename(&spec.common, spec.common.with_extension("unavailable"))
            .unwrap();
        store.cleanup(122).unwrap();
    }

    #[test]
    fn worktree_cleanup_retries_locked_or_unavailable_repositories() {
        // Step #1: Lock an expired worktree and ensure no files are removed.
        let fixture = Fixture::new();
        let spec = fixture.repository();
        let mut store = fixture.store();
        let checkout = store
            .worktree(b"agent".to_vec(), 1, false, 100, spec.clone())
            .unwrap();
        git_test(
            &spec.repository,
            &["worktree", "lock", checkout.to_str().unwrap()],
        );
        assert!(store.cleanup(101).is_err());
        assert!(checkout.join("tracked").exists());
        git_test(
            &spec.repository,
            &["worktree", "unlock", checkout.to_str().unwrap()],
        );
        // Step #2: Missing repository metadata delays cleanup until it returns.
        let moved = fixture.0.join("saved-git");
        fs::rename(&spec.common, &moved).unwrap();
        assert!(store.cleanup(102).is_err());
        assert!(checkout.exists());
        fs::rename(&moved, &spec.common).unwrap();
        store.cleanup(103).unwrap();
        assert!(!checkout.exists());
        assert_eq!(
            fs::read_dir(spec.common.join("worktrees")).unwrap().count(),
            0
        );
    }

    #[test]
    fn worktree_recovery_cleans_pending_creation_and_interrupted_deletion() {
        // Step #1: Recover intent persisted before Git's creation acknowledgement.
        let fixture = Fixture::new();
        let spec = fixture.repository();
        let mut store = fixture.store();
        let checkout = store
            .worktree(b"pending".to_vec(), 1000, false, 100, spec.clone())
            .unwrap();
        store.leases[0].phase = Phase::Pending;
        store.persist().unwrap();
        drop(store);
        let mut store = fixture.store();
        store.cleanup(101).unwrap();
        assert!(!checkout.exists());
        // Step #2: Resume after files disappear but before registration removal.
        let checkout = store
            .worktree(b"interrupted".to_vec(), 1, false, 102, spec.clone())
            .unwrap();
        remove_lease(&store.root, store.leases.last().unwrap()).unwrap();
        drop(store);
        let mut store = fixture.store();
        store.cleanup(103).unwrap();
        assert!(!checkout.exists());
        assert_eq!(
            fs::read_dir(spec.common.join("worktrees")).unwrap().count(),
            0
        );
    }

    #[test]
    fn worktree_cleanup_preserves_replaced_lease_and_external_data() {
        // Step #1: Replace a leased root with a symlink to unrelated data.
        let fixture = Fixture::new();
        let spec = fixture.repository();
        let mut store = fixture.store();
        let checkout = store
            .worktree(b"agent".to_vec(), 1, false, 100, spec.clone())
            .unwrap();
        let root = checkout.parent().unwrap();
        let original = fixture.0.join("saved-lease");
        fs::rename(root, &original).unwrap();
        let external = fixture.0.join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("keep"), b"safe").unwrap();
        symlink(&external, root).unwrap();
        // Step #2: Refuse cleanup, preserve Git's registration, and retry the restored original.
        assert!(store.cleanup(101).is_err());
        assert_eq!(fs::read(external.join("keep")).unwrap(), b"safe");
        assert_eq!(
            fs::read_dir(spec.common.join("worktrees")).unwrap().count(),
            1
        );
        fs::remove_file(root).unwrap();
        fs::rename(&original, root).unwrap();
        store.cleanup(102).unwrap();
        assert!(!checkout.exists());
    }

    #[test]
    fn imports_v1_directory_leases_and_rejects_malformed_worktree_records() {
        // Step #1: Rewrite a fixture record in the original directory-only format.
        let fixture = Fixture::new();
        let mut store = fixture.store();
        let path = store.create(b"existing".to_vec(), 10, false, 100).unwrap();
        let lease = &store.leases[0];
        let header = format!(
            "dirlease-v1\n{}\n",
            hex(store.root.as_os_str().as_bytes())
        );
        let record = format!(
            "{} {} {} {} {}",
            lease.id,
            hex(&lease.session),
            lease.expires,
            lease.device,
            lease.inode
        );
        fs::write(store.state.join("leases"), format!("{header}{record}\n"))
            .unwrap();
        drop(store);
        let mut store = fixture.store();
        assert_eq!(
            store.create(b"existing".to_vec(), 100, false, 101).unwrap(),
            path
        );
        // Step #2: Upgrade on persistence and fail closed on invalid v2 metadata.
        store.create(b"new".to_vec(), 10, false, 101).unwrap();
        let state = fs::read_to_string(store.state.join("leases")).unwrap();
        assert!(state.starts_with("dirlease-v2\n"));
        for metadata in [
            "- 00 - - 1",
            "- - - - 0",
            "00 2f 48454144 - 1",
            "2f 2f 48454144 - 9",
        ] {
            fs::write(
                store.state.join("leases"),
                format!(
                    "{}{} {}\n",
                    header.replace("v1", "v2"),
                    record,
                    metadata
                ),
            )
            .unwrap();
            assert!(
                Store::load(store.state.clone(), store.root.clone()).is_err()
            );
        }
    }

    #[test]
    fn interrupted_metadata_quarantine_is_recovered_without_broad_pruning() {
        // Step #1: Interrupt after a registration was moved into quarantine.
        let fixture = Fixture::new();
        let spec = fixture.repository();
        let mut store = fixture.store();
        let checkout = store
            .worktree(b"agent".to_vec(), 1, false, 100, spec.clone())
            .unwrap();
        let registrations =
            spec.registrations(checkout.parent().unwrap()).unwrap();
        let (name, _, _) = &registrations[0];
        let root = spec.common.join("worktrees");
        fs::rename(root.join(name), root.join(format!(".cleanup-{name}")))
            .unwrap();
        // Step #2: Restart and clean the captured registration and leased files.
        drop(store);
        let mut store = fixture.store();
        store.cleanup(101).unwrap();
        assert!(!checkout.exists());
        assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    }

    #[test]
    fn sessions_reuse_isolate_refresh_and_restart_without_extension() {
        // Step #1: Exercise session reuse, explicit freshness, and durable reload.
        let fixture = Fixture::new();
        let mut store = fixture.store();
        let first = store.create(b"one".to_vec(), 10, false, 100).unwrap();
        assert_eq!(
            first,
            store.create(b"one".to_vec(), 500, false, 101).unwrap()
        );
        let other = store.create(b"two".to_vec(), 10, false, 101).unwrap();
        assert_ne!(first, other);
        drop(store);
        let mut store = fixture.store();
        assert_eq!(
            first,
            store.create(b"one".to_vec(), 500, false, 102).unwrap()
        );
        // Step #2: The initial deadline survives reuse and restart.
        store.cleanup(110).unwrap();
        assert!(!first.exists());
        assert!(other.exists());
        let fresh = store.create(b"two".to_vec(), 5, true, 111).unwrap();
        assert_ne!(other, fresh);
        fs::remove_dir(&fresh).unwrap();
        assert_ne!(
            fresh,
            store.create(b"two".to_vec(), 5, false, 112).unwrap()
        );
    }

    #[test]
    fn cleanup_preserves_symlinks_replacements_and_external_targets() {
        // Step #1: Put a symlink inside a lease and replace a second lease directory.
        let fixture = Fixture::new();
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"safe").unwrap();
        let mut store = fixture.store();
        let first = store.create(b"one".to_vec(), 1, false, 100).unwrap();
        symlink(&outside, first.join("link")).unwrap();
        let replaced = store.create(b"two".to_vec(), 1, false, 100).unwrap();
        fs::rename(&replaced, fixture.0.join("original")).unwrap();
        symlink(&outside, &replaced).unwrap();
        // Step #2: Sweep removes only the original first directory.
        assert!(store.cleanup(101).is_err());
        assert!(!first.exists());
        assert!(
            fs::symlink_metadata(&replaced)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"safe");
        fs::remove_file(&replaced).unwrap();
        fs::create_dir(&replaced).unwrap();
        assert!(store.cleanup(102).is_err());
        assert!(replaced.is_dir());
    }

    #[test]
    fn cleanup_replacement_after_validation_preserves_substituted_contents() {
        // Step #1: Allocate an expired lease containing nested data and an external symlink.
        let fixture = Fixture::new();
        let mut store = fixture.store();
        let path = store.create(b"one".to_vec(), 1, false, 100).unwrap();
        fs::create_dir(path.join("nested")).unwrap();
        fs::write(path.join("nested/remove"), b"expired").unwrap();
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"external").unwrap();
        symlink(&outside, path.join("link")).unwrap();
        let quarantine =
            store.root.join(format!(".cleanup-{}", store.leases[0].id));
        let moved_original = fixture.0.join("moved-original");
        // Step #2: Replace quarantine after its opened identity passes validation.
        let result =
            remove_lease_with_hook(&store.root, &store.leases[0], || {
                fs::rename(&quarantine, &moved_original).unwrap();
                fs::create_dir(&quarantine).unwrap();
                fs::write(quarantine.join("keep"), b"replacement").unwrap();
            });
        assert!(result.is_err());
        assert_eq!(fs::read(quarantine.join("keep")).unwrap(), b"replacement");
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"external");
        assert_eq!(fs::read_dir(&moved_original).unwrap().count(), 0);
    }

    #[test]
    fn latest_expired_lease_never_revives_an_older_session_lease() {
        let fixture = Fixture::new();
        let mut store = fixture.store();
        let old = store.create(b"one".to_vec(), 1000, false, 100).unwrap();
        let latest = store.create(b"one".to_vec(), 1, true, 101).unwrap();
        store.cleanup(102).unwrap();
        drop(store);
        let mut store = fixture.store();
        let next = store.create(b"one".to_vec(), 10, false, 103).unwrap();
        assert_ne!(next, old);
        assert_ne!(next, latest);
    }

    #[test]
    fn restart_resumes_quarantine_and_native_paths() {
        // Step #1: Persist a lease beneath a native root and interrupt quarantine.
        let fixture = Fixture::new();
        #[cfg(target_os = "linux")]
        let root = {
            use std::os::unix::ffi::OsStringExt;
            fixture
                .0
                .join(std::ffi::OsString::from_vec(b"native-\xff".to_vec()))
        };
        #[cfg(not(target_os = "linux"))]
        let root = fixture.0.join("native-λ");
        let state = fixture.0.join("state");
        let mut store = Store::load(state.clone(), root.clone()).unwrap();
        let path = store
            .create(b"session-\xff".to_vec(), 1, false, 100)
            .unwrap();
        let quarantine = root.join(format!(
            ".cleanup-{}",
            path.file_name().unwrap().to_str().unwrap()
        ));
        fs::rename(&path, &quarantine).unwrap();
        drop(store);
        // Step #2: Recovery uses the recorded inode and native root bytes.
        let mut store = Store::load(state, root).unwrap();
        store.cleanup(101).unwrap();
        assert!(!quarantine.exists());
    }

    #[test]
    fn rejects_corrupt_storage_and_overflow_and_preserves_native_paths() {
        let fixture = Fixture::new();
        let mut store = fixture.store();
        assert!(store.create(b"one".to_vec(), u64::MAX, false, 1).is_err());
        fs::write(fixture.0.join("state/leases"), b"corrupt").unwrap();
        assert!(
            Store::load(fixture.0.join("state"), fixture.0.join("temp"))
                .is_err()
        );
        assert_eq!(unhex(&hex(b"\xff\x00")).unwrap(), b"\xff\x00");
    }
}
