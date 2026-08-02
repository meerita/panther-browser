// @file apps/panther/src/main.rs
// @description Entry point for the Panther application binary.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther application entry point.
//!
//! This binary runs the capability bootstrap at startup, prints one diagnostics
//! summary line derived from the returned reports, and reports the effective
//! state of each foundational capability through the shell. It then opens the
//! application window and presents a frame through the selected graphics backend.

use anyhow::Result;
use panther_browser::{Availability, bootstrap};

fn main() -> Result<()> {
    let result = bootstrap()?;
    let reports = result.reports();

    let available = reports
        .iter()
        .filter(|report| report.availability() == Availability::Available)
        .count();

    println!(
        "capability bootstrap: {available} of {} capabilities available",
        reports.len()
    );

    panther_shell::report_startup_capabilities(&reports);

    panther_window::run_window()?;

    Ok(())
}
