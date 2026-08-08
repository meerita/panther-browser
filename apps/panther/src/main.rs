// @file apps/panther/src/main.rs
// @description Entry point for the Panther application binary.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther application entry point.
//!
//! This binary is the composition root. It runs the capability bootstrap at
//! startup, prints one diagnostics summary line derived from the returned reports,
//! and reports the effective state of each foundational capability through the
//! shell. It then builds the product core (the tab model), opens and attaches the
//! first tab, constructs the chrome text producer at the system locale, and
//! injects both into the window, which presents the active tab's frame and the
//! chrome labels through the selected graphics backend.
//!
//! The composition root owns the locale policy: it builds the producer from the
//! baked catalogues detected against the operating-system preference, then hands
//! the producer to the window. The window realizes and presents the chrome but
//! owns no locale policy.

use anyhow::Result;
use panther_browser::{Availability, TabModel, bootstrap, m2_demonstration_fixture};
use panther_chrome_text::ChromeText;
use panther_localization::{
    ActiveLocaleState, BakedResourceProvider, LocaleRequest, LocaleResolver, MessageCatalog,
    ResourceProvider,
};
use panther_shell::ScaleFactor;

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

    let resolver = LocaleResolver::with_system_detection(BakedResourceProvider.available_locales());
    let state = ActiveLocaleState::new(resolver, LocaleRequest::new());
    // The composition root has no window yet, so it builds the producer at the
    // identity scale. The window reads the real display scale at init and drives
    // the producer to it before the first paint.
    let chrome_text = ChromeText::new(state, MessageCatalog::load(), ScaleFactor::ONE)?;

    panther_window::run_window(tab_model, chrome_text)?;

    Ok(())
}
