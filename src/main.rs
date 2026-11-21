//! Unix session leases with a durable, single-owner cleanup worker.

mod lease;
mod service;
mod worktree;

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

unsafe extern "C" {
    fn getsid(pid: i32) -> i32;
}

#[derive(Debug, PartialEq)]
enum CliCommand {
    Create {
        ttl: u64,
        fresh: bool,
    },
    Worktree {
        session: OsString,
        repository: PathBuf,
        revision: OsString,
        branch: Option<OsString>,
        ttl: u64,
        fresh: bool,
    },
    Serve,
    Help,
    Version,
}

fn duration(value: &str) -> Result<u64, String> {
    // Step #1: Separate the final Unicode character without slicing a codepoint.
    let index = value
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index);
    let (digits, unit) = value.split_at(index);
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => {
            return Err(
                "TTL must be a positive integer followed by s, m, h, or d"
                    .into(),
            );
        }
    };
    // Step #2: Validate the quantity and checked unit conversion.
    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(multiplier))
        .filter(|n| *n > 0)
        .ok_or_else(|| "TTL is zero, invalid, or too large".into())
}

fn parse(arguments: Vec<OsString>) -> Result<CliCommand, String> {
    // Step #1: Recognize standalone information and worker commands.
    match arguments.as_slice() {
        [arg] if arg == "--help" || arg == "-h" => return Ok(CliCommand::Help),
        [arg] if arg == "--version" || arg == "-V" => {
            return Ok(CliCommand::Version);
        }
        [arg] if arg == "serve" => return Ok(CliCommand::Serve),
        _ => {}
    }
    if arguments.first().is_some_and(|arg| arg == "worktree") {
        return parse_worktree(&arguments[1..]);
    }
    // Step #2: Parse the default create operation and its options.
    let mut args = arguments.iter().peekable();
    if args.peek().is_some_and(|arg| *arg == "create") {
        args.next();
    }
    let (mut ttl, mut fresh) = (25 * 3600, false);
    while let Some(arg) = args.next() {
        if arg == "--new" {
            fresh = true;
        } else if arg == "--ttl" {
            ttl = duration(
                args.next()
                    .and_then(|v| v.to_str())
                    .ok_or("--ttl requires a duration such as 25h")?,
            )?;
        } else {
            return Err(format!("unknown argument {arg:?}; use --help"));
        }
    }
    Ok(CliCommand::Create { ttl, fresh })
}

fn parse_worktree(arguments: &[OsString]) -> Result<CliCommand, String> {
    // Step #1: Parse explicit agent identity and checkout configuration.
    let (mut session, mut repository, mut branch) = (None, None, None);
    let (mut revision, mut ttl, mut fresh) =
        (OsString::from("HEAD"), 25 * 3600, false);
    let mut args = arguments.iter();
    while let Some(arg) = args.next() {
        if arg == "--new" {
            fresh = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{arg:?} requires a value"))?;
        if arg == "--session" {
            session = Some(value.clone());
        } else if arg == "--repo" {
            repository = Some(PathBuf::from(value));
        } else if arg == "--ref" {
            revision = value.clone();
        } else if arg == "--branch" {
            branch = Some(value.clone());
        } else if arg == "--ttl" {
            ttl = duration(value.to_str().ok_or("TTL must be UTF-8")?)?;
        } else {
            return Err(format!(
                "unknown worktree argument {arg:?}; use --help"
            ));
        }
    }
    // Step #2: Require stable identity and an explicit source repository.
    let session = session
        .filter(|value| !value.is_empty())
        .ok_or("worktree requires a nonempty --session")?;
    let repository = repository
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or("worktree requires --repo")?;
    Ok(CliCommand::Worktree {
        session,
        repository,
        revision,
        branch,
        ttl,
        fresh,
    })
}

fn run() -> Result<(), String> {
    // Step #1: Dispatch informational commands without opening state.
    let command = parse(std::env::args_os().skip(1).collect())?;
    match command {
        CliCommand::Help => println!(
            "dirlease — temporary session directories\n\nUsage: dirlease [create] [--ttl 25h] [--new]\n       dirlease worktree --session ID --repo PATH [--ref HEAD] [--branch NAME] [--ttl 25h] [--new]\n       dirlease serve\n\nAn empty DIRLEASE_SESSION defaults to the current Unix session.\nExport DIRLEASE_SESSION to share a lease across Unix sessions.\nTTL units: s, m, h, d. Reuse never extends expiry. See README.md for supervision."
        ),
        CliCommand::Version => println!("dirlease 0.0.2-alpha"),
        // Step #2: Keep native filesystem bytes intact on the client output.
        CliCommand::Create { ttl, fresh } => {
            let path = service::request(session()?, ttl, fresh)?;
            let mut output = io::stdout().lock();
            output
                .write_all(&path)
                .and_then(|()| output.write_all(b"\n"))
                .map_err(|e| e.to_string())?;
        }
        CliCommand::Worktree {
            session,
            repository,
            revision,
            branch,
            ttl,
            fresh,
        } => {
            let spec = worktree::Spec::new(repository, revision, branch)
                .map_err(|error| error.to_string())?;
            let path = service::request_worktree(session, ttl, fresh, &spec)?;
            let mut output = io::stdout().lock();
            output
                .write_all(&path)
                .and_then(|()| output.write_all(b"\n"))
                .map_err(|error| error.to_string())?;
        }
        CliCommand::Serve => service::serve()?,
    }
    Ok(())
}

/// Resolve the inherited identifier or the current Unix session ID.
fn session() -> Result<OsString, String> {
    // Step #1: Preserve a nonempty inherited identifier.
    if let Some(session) =
        std::env::var_os("DIRLEASE_SESSION").filter(|value| !value.is_empty())
    {
        return Ok(session);
    }
    // Step #2: Derive a stable identifier shared by this Unix session's processes.
    // SAFETY: getsid(0) reads the caller's session ID without pointer arguments.
    let session_id = unsafe { getsid(0) };
    if session_id < 0 {
        return Err(format!(
            "read Unix session ID: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(OsString::from(format!("unix-session:{session_id}")))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dirlease: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worktree_requires_explicit_identity_and_repository() {
        // Step #1: Validate required values and duration failures before opening state.
        for arguments in [
            vec!["worktree"],
            vec!["worktree", "--session", "agent"],
            vec!["worktree", "--repo", "/repo"],
            vec!["worktree", "--session", "", "--repo", "/repo"],
            vec![
                "worktree",
                "--session",
                "agent",
                "--repo",
                "/repo",
                "--ttl",
                "0s",
            ],
        ] {
            assert!(
                parse(arguments.into_iter().map(OsString::from).collect())
                    .is_err()
            );
        }
        // Step #2: Keep caller-provided identity, ref, branch, and TTL intact.
        let args = [
            "worktree",
            "--session",
            "agent-id",
            "--repo",
            "/repo",
            "--ref",
            "main",
            "--branch",
            "agent/task",
            "--ttl",
            "2d",
            "--new",
        ];
        assert_eq!(
            parse(args.into_iter().map(OsString::from).collect()).unwrap(),
            CliCommand::Worktree {
                session: "agent-id".into(),
                repository: "/repo".into(),
                revision: "main".into(),
                branch: Some("agent/task".into()),
                ttl: 172800,
                fresh: true
            }
        );
    }

    #[test]
    fn command_defaults_and_duration_errors() {
        assert_eq!(
            parse(vec![]).unwrap(),
            CliCommand::Create {
                ttl: 90000,
                fresh: false
            }
        );
        for value in ["", "0s", "3", "-1h", "18446744073709551615d", "💣"] {
            assert!(duration(value).is_err());
        }
        assert_eq!(duration("2d").unwrap(), 172800);
    }
}
