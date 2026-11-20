//! Durable lease records and identity-checked cleanup under private roots.

use rustix::fs::{self as descriptor, AtFlags, Dir, Mode, OFlags};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
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

#[derive(Debug)]
struct Lease {
    id: String,
    session: Vec<u8>,
    expires: u64,
    device: u64,
    inode: u64,
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
        if lines.next() != Some("dirlease-v1")
            || lines.next()
                != Some(hex(store.root.as_os_str().as_bytes()).as_str())
        {
            return Err(io::Error::other(
                "invalid lease state version or changed temporary root",
            ));
        }
        for line in lines {
            let fields: Vec<_> = line.split(' ').collect();
            if fields.len() != 5
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
            "dirlease-v1\n{}",
            hex(self.root.as_os_str().as_bytes())
        )?;
        for lease in &self.leases {
            writeln!(
                file,
                "{} {} {} {} {}",
                lease.id,
                hex(&lease.session),
                lease.expires,
                lease.device,
                lease.inode
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
            if lease.expires > now && matches_identity(&path, lease)? {
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
        });
        if let Err(error) = self.persist() {
            self.leases.pop();
            let _ = fs::remove_dir(&path);
            return Err(error);
        }
        Ok(path)
    }

    /// Remove overdue original directories; persist retirement and retry failures.
    pub(crate) fn cleanup(&mut self, now: u64) -> io::Result<()> {
        // Step #1: Retain failed deletions and the latest session identity; retire older entries.
        if self.leases.iter().all(|lease| lease.expires > now) {
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
        for lease in self.leases.drain(..) {
            if lease.expires > now {
                retained.push(lease);
                continue;
            }
            if let Err(error) = remove_lease(&self.root, &lease) {
                failure = Some(error);
                retained.push(lease);
            } else if current.contains(&lease.id) {
                // Keep the session's latest identity as a tombstone, never revive an older lease.
                retained.push(lease);
            }
        }
        // Step #2: Persist retirement even when another entry needs a later retry.
        self.leases = retained;
        File::open(&self.root)?.sync_all()?;
        if self.leases.len() != original_count {
            self.persist()?;
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
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
        fn store(&self) -> Store {
            Store::load(self.0.join("state"), self.0.join("temp")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
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
