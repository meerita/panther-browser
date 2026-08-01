// @file tools/localization-check/src/main.rs
// @description Entry point for the localization validation and no-prose checks.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::path::PathBuf;
use std::process::ExitCode;

use localization_check::{CheckError, run};

fn main() -> ExitCode {
    match execute() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("localization-check could not run: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> Result<ExitCode, CheckError> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let report = run(&root)?;

    if report.is_empty() {
        println!("localization-check: no findings");
        return Ok(ExitCode::SUCCESS);
    }

    print!("{report}");
    eprintln!("localization-check: {} finding(s)", report.len());
    Ok(ExitCode::FAILURE)
}
