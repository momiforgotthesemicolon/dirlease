//! Unix session leases with a durable, single-owner cleanup worker.

mod lease;
mod service;

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
enum CliCommand {
    Create { ttl: u64, fresh: bool },
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

fn run() -> Result<(), String> {
    // Step #1: Dispatch informational commands without opening state.
    let command = parse(std::env::args_os().skip(1).collect())?;
    match command {
        CliCommand::Help => println!(
            "dirlease — temporary session directories\n\nUsage: dirlease [create] [--ttl 25h] [--new]\n       dirlease serve\n\nSet DIRLEASE_SESSION to a unique, inherited session identifier.\nTTL units: s, m, h, d. Reuse never extends expiry. See README.md for supervision."
        ),
        CliCommand::Version => println!("dirlease 0.0.1-alpha"),
        // Step #2: Keep native filesystem bytes intact on the client output.
        CliCommand::Create { ttl, fresh } => {
            let session = std::env::var_os("DIRLEASE_SESSION").filter(|v| !v.is_empty())
                .ok_or("DIRLEASE_SESSION is required; initialize once with: export DIRLEASE_SESSION=\"$(uuidgen)\"; inherit it in child processes")?;
            let path = service::request(session, ttl, fresh)?;
            let mut output = io::stdout().lock();
            output
                .write_all(&path)
                .and_then(|()| output.write_all(b"\n"))
                .map_err(|e| e.to_string())?;
        }
        CliCommand::Serve => service::serve()?,
    }
    Ok(())
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
