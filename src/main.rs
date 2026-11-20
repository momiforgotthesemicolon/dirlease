//! Command-line entry point. Directory leases and cleanup are not implemented yet.

use std::process::ExitCode;

/// Show scaffold information without creating or deleting filesystem entries.
fn main() -> ExitCode {
    // Step #1: Read native arguments without assuming paths are UTF-8.
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();

    // Step #2: Report scaffold status without performing filesystem operations.
    match arguments.as_slice() {
        [argument] if argument == "--help" || argument == "-h" => {
            println!(
                "dirlease — mkdir with scheduled cleanup\n\nUsage: dirlease [--help | --version]\n\nDirectory leases are not implemented yet."
            );
            ExitCode::SUCCESS
        }
        [argument] if argument == "--version" || argument == "-V" => {
            println!("dirlease 0.1.0");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "dirlease: directory leases are not implemented yet; use --help"
            );
            ExitCode::FAILURE
        }
    }
}
