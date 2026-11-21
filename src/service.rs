//! Local socket transport and detached or supervised worker lifecycle.

use crate::lease::{self, Store};
use crate::worktree::Spec;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    fn setsid() -> i32;
    fn getuid() -> u32;
}

fn locations() -> io::Result<(PathBuf, PathBuf)> {
    // Step #1: Select persistent app state independently of the temporary data root.
    let state = if let Some(path) = std::env::var_os("DIRLEASE_STATE_DIR") {
        PathBuf::from(path)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(
            std::env::var_os("HOME")
                .ok_or_else(|| io::Error::other("HOME is required"))?,
        )
        .join("Library/Application Support/dirlease")
    } else if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(path).join("dirlease")
    } else {
        PathBuf::from(
            std::env::var_os("HOME")
                .ok_or_else(|| io::Error::other("HOME is required"))?,
        )
        .join(".local/state/dirlease")
    };
    let root = std::env::var_os("DIRLEASE_TEMP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("dirlease"));
    // Step #2: Reject relative locations and directories belonging to another account.
    for path in [&state, &root] {
        if !path.is_absolute() {
            return Err(io::Error::other(format!(
                "{} must be absolute",
                path.display()
            )));
        }
        lease::private_directory(path)?;
        // SAFETY: getuid has no pointer arguments or preconditions.
        if fs::symlink_metadata(path)?.uid() != unsafe { getuid() } {
            return Err(io::Error::other(
                "lease roots must belong to the current user",
            ));
        }
    }
    Ok((state, root))
}

fn frame(stream: &mut UnixStream, bytes: &[u8]) -> io::Result<()> {
    let length = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    if length > 65536 {
        return Err(io::Error::other("protocol frame too large"));
    }
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(bytes)
}

fn read_frame(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut size = [0; 4];
    stream.read_exact(&mut size)?;
    let length = u32::from_be_bytes(size) as usize;
    if length > 65536 {
        return Err(io::Error::other("protocol frame too large"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn timeouts(stream: &UnixStream) -> io::Result<()> {
    // Accepted sockets inherit the nonblocking listener mode on macOS.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))
}

fn launch() -> io::Result<()> {
    // Step #1: Disconnect every inherited standard stream.
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Step #2: Detach the worker session before executing the foreground entry point.
    // SAFETY: setsid is async-signal-safe and touches no Rust state after fork.
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    command.spawn()?;
    Ok(())
}

/// Request a persisted lease, starting a detached worker if necessary.
pub(crate) fn request(
    session: OsString,
    ttl: u64,
    fresh: bool,
) -> Result<Vec<u8>, String> {
    let mut bytes = vec![u8::from(fresh)];
    bytes.extend_from_slice(&ttl.to_be_bytes());
    bytes.extend_from_slice(session.as_bytes());
    exchange(bytes, false)
}

/// Request a serialized, durable Git worktree lease using native argument bytes.
pub(crate) fn request_worktree(
    session: OsString,
    ttl: u64,
    fresh: bool,
    spec: &Spec,
) -> Result<Vec<u8>, String> {
    // Step #1: Encode checkout configuration without losing native path bytes.
    let mut bytes = vec![2 + u8::from(fresh)];
    bytes.extend_from_slice(&ttl.to_be_bytes());
    for value in [
        session.as_os_str(),
        spec.repository.as_os_str(),
        spec.revision.as_os_str(),
        spec.branch.as_deref().unwrap_or_default(),
    ] {
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
    }
    // Step #2: Allow the worker to complete a serialized Git checkout.
    exchange(bytes, true)
}

fn exchange(bytes: Vec<u8>, worktree: bool) -> Result<Vec<u8>, String> {
    // Step #1: Bound startup retries, including a worker racing another client.
    let (state, _) = locations().map_err(|e| e.to_string())?;
    std::env::set_current_dir(&state).map_err(|e| e.to_string())?;
    let socket = PathBuf::from("worker.sock");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut launched = false;
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "cannot connect to {}: {error}; run dirlease serve to diagnose",
                        socket.display()
                    ));
                }
                if !launched {
                    launch().map_err(|e| e.to_string())?;
                    launched = true;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    };
    // Step #2: Send native session bytes and decode a tagged native-path response.
    timeouts(&stream).map_err(|e| e.to_string())?;
    if worktree {
        stream
            .set_read_timeout(Some(Duration::from_secs(300)))
            .map_err(|error| error.to_string())?;
    }
    frame(&mut stream, &bytes).map_err(|e| e.to_string())?;
    let response = read_frame(&mut stream).map_err(|error| {
        if worktree {
            format!("worktree response: {error}; after an upgrade, restart the cleanup worker; otherwise retry with the same --session")
        } else {
            error.to_string()
        }
    })?;
    match response.split_first() {
        Some((0, path)) => Ok(path.to_vec()),
        Some((1, error)) => Err(String::from_utf8_lossy(error).into_owned()),
        _ => Err("malformed worker response".into()),
    }
}

fn handle(store: &mut Store, stream: &mut UnixStream) -> io::Result<()> {
    // Step #1: Validate the complete bounded request before touching lease state.
    timeouts(stream)?;
    let bytes = read_frame(stream)?;
    if bytes.len() < 10 || bytes[0] > 3 {
        return Err(io::Error::other("invalid create request"));
    }
    let ttl =
        u64::from_be_bytes(bytes[1..9].try_into().map_err(io::Error::other)?);
    // Step #2: Encode errors separately from native absolute paths.
    let result = if bytes[0] < 2 {
        store.create(bytes[9..].to_vec(), ttl, bytes[0] == 1, lease::now()?)
    } else {
        create_worktree(store, &bytes[9..], ttl, bytes[0] == 3)
    };
    let response = match result {
        Ok(path) => {
            let mut bytes = vec![0];
            bytes.extend_from_slice(path.as_os_str().as_bytes());
            bytes
        }
        Err(error) => {
            let mut bytes = vec![1];
            bytes.extend_from_slice(error.to_string().as_bytes());
            bytes
        }
    };
    frame(stream, &response)
}

fn create_worktree(
    store: &mut Store,
    bytes: &[u8],
    ttl: u64,
    fresh: bool,
) -> io::Result<PathBuf> {
    // Step #1: Decode exactly four NUL-terminated, native-byte arguments.
    let fields: Vec<_> = bytes.split(|byte| *byte == 0).collect();
    if fields.len() != 5
        || !fields[4].is_empty()
        || fields[..3].iter().any(|field| field.is_empty())
    {
        return Err(io::Error::other("invalid worktree request"));
    }
    let native = |bytes: &[u8]| OsString::from_vec(bytes.to_vec());
    let repository = PathBuf::from(native(fields[1]));
    if !repository.is_absolute() {
        return Err(io::Error::other("worktree repository must be absolute"));
    }
    // Step #2: Resolve the repository in the worker before recording ownership.
    let spec = Spec::new(
        repository,
        native(fields[2]),
        if fields[3].is_empty() {
            None
        } else {
            Some(native(fields[3]))
        },
    )?;
    store.worktree(fields[0].to_vec(), ttl, fresh, lease::now()?, spec)
}

fn lock(state: &std::path::Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(state.join("worker.lock"))?;
    // SAFETY: the descriptor stays owned by file; flock only changes its advisory lock.
    if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

/// Own state and serve requests indefinitely, sweeping overdue leases on startup.
pub(crate) fn serve() -> Result<(), String> {
    // Step #1: Own the persistent lock before recovering state or replacing a stale socket.
    let (state, root) = locations().map_err(|e| e.to_string())?;
    let _lock = lock(&state)
        .map_err(|e| format!("worker lock {}: {e}", state.display()))?;
    let mut store = Store::load(state.clone(), root)
        .map_err(|e| format!("load {}: {e}", state.display()))?;
    std::env::set_current_dir(&state).map_err(|e| e.to_string())?;
    let socket = PathBuf::from("worker.sock");
    match fs::remove_file(&socket) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let listener = UnixListener::bind(&socket)
        .map_err(|e| format!("bind {}: {e}", socket.display()))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    fs::write(state.join("worker.pid"), std::process::id().to_string())
        .map_err(|e| e.to_string())?;
    // Step #2: Sweep overdue records immediately and retry failures once each second.
    let mut next_cleanup = Instant::now();
    loop {
        if Instant::now() >= next_cleanup {
            if let Err(error) =
                store.cleanup(lease::now().map_err(|e| e.to_string())?)
            {
                eprintln!("dirlease cleanup: {error}");
            }
            next_cleanup = Instant::now() + Duration::from_secs(1);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(error) = handle(&mut store, &mut stream) {
                    eprintln!("dirlease request: {error}");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_delayed_frames_from_nonblocking_streams() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        timeouts(&server).unwrap();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            frame(&mut client, b"request").unwrap();
        });
        let response = read_frame(&mut server);
        sender.join().unwrap();
        assert_eq!(response.unwrap(), b"request");
    }

    #[test]
    fn rejects_large_protocol_frames() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client.write_all(&65537_u32.to_be_bytes()).unwrap();
        assert!(read_frame(&mut server).is_err());
    }
}
