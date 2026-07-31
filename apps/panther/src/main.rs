// @file apps/panther/src/main.rs
// @description Entry point for the Panther application binary.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther application entry point.
//!
//! This binary runs the capability bootstrap at startup and prints one
//! diagnostics summary line derived from the returned reports. It holds no
//! browser shell, window, or event loop yet.

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

    Ok(())
}
