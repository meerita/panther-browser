// @file apps/panther/src/main.rs
// @description Entry point for the Panther application binary.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther application entry point.
//!
//! This binary is the composition root. It runs the capability bootstrap at
//! startup, prints one diagnostics summary line derived from the returned reports,
//! and reports the effective state of each foundational capability through the
//! shell. It then builds the product core (the tab model), opens and attaches the
//! first tab, and injects the core into the window, which presents the active
//! tab's frame through the selected graphics backend.

use anyhow::Result;
use panther_browser::{Availability, TabModel, bootstrap, m2_demonstration_fixture};

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

    let mut tab_model = TabModel::new();
    let tab = tab_model.open_tab();
    tab_model.attach(tab, m2_demonstration_fixture())?;

    panther_window::run_window(tab_model)?;

    Ok(())
}
