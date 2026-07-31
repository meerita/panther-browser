// @file products/panther/browser/tests/capability-developer-tooling.rs
// @description Integration tests for the developer tooling capability scenarios.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Drives the assembled system through the developer tooling capabilities:
//! the developer-mode gate, the developer-tools dependency with lazy activation
//! and cleanup, and the maturity-gated, dependent, never auto-enabled
//! remote-debugging capability.

use capability_system::{Availability, DecidingAuthority, Lifecycle, Reason, UserPreference};
use panther_browser::{DEVELOPER_MODE, DEVELOPER_TOOLS, REMOTE_DEBUGGING, bootstrap};

#[test]
fn developer_mode_is_gated_until_a_user_preference_enables_it() {
    let mut result = bootstrap().expect("the built-in catalogue should build");

    let gated = result
        .manager
        .report(DEVELOPER_MODE)
        .expect("developer mode is in the catalogue");
    assert_eq!(gated.availability(), Availability::Disabled);
    assert_eq!(gated.reason(), Reason::UserDisabled);
    assert_eq!(gated.authority(), DecidingAuthority::UserPreference);

    result
        .manager
        .set_preference(DEVELOPER_MODE, UserPreference::Enable)
        .expect("enabling developer mode is accepted");

    let enabled = result
        .manager
        .report(DEVELOPER_MODE)
        .expect("developer mode is in the catalogue");
    assert_eq!(enabled.availability(), Availability::Available);
    assert_eq!(enabled.reason(), Reason::UserEnabled);
    assert_eq!(enabled.authority(), DecidingAuthority::UserPreference);
    assert_eq!(enabled.lifecycle(), Some(Lifecycle::Dormant));
}

#[test]
fn developer_tools_depend_on_developer_mode() {
    let result = bootstrap().expect("the built-in catalogue should build");

    let report = result
        .manager
        .report(DEVELOPER_TOOLS)
        .expect("developer tools are in the catalogue");
    assert_eq!(report.availability(), Availability::Disabled);
    assert_eq!(report.reason(), Reason::DependencyUnmet);
    assert_eq!(report.authority(), DecidingAuthority::DependencyCheck);
    assert_eq!(report.unmet_dependencies(), &[DEVELOPER_MODE]);
}

#[test]
fn developer_tools_activate_lazily_and_release_on_deactivation() {
    let mut result = bootstrap().expect("the built-in catalogue should build");
    result
        .manager
        .set_preference(DEVELOPER_MODE, UserPreference::Enable)
        .expect("enabling developer mode is accepted");

    assert_eq!(
        result.manager.lifecycle(DEVELOPER_TOOLS),
        Some(Lifecycle::Dormant)
    );

    result
        .manager
        .request_activation(DEVELOPER_TOOLS)
        .expect("activation is accepted once the dependency is met");
    assert_eq!(
        result.manager.lifecycle(DEVELOPER_TOOLS),
        Some(Lifecycle::Active)
    );

    result
        .manager
        .deactivate(DEVELOPER_TOOLS)
        .expect("deactivation is accepted");
    assert_eq!(
        result.manager.lifecycle(DEVELOPER_TOOLS),
        Some(Lifecycle::Dormant)
    );

    result
        .manager
        .request_activation(DEVELOPER_TOOLS)
        .expect("a released provider accepts a second activation");
    assert_eq!(
        result.manager.lifecycle(DEVELOPER_TOOLS),
        Some(Lifecycle::Active)
    );
}

#[test]
fn remote_debugging_stays_maturity_gated_and_is_not_auto_enabled_by_developer_tools() {
    let mut result = bootstrap().expect("the built-in catalogue should build");
    result
        .manager
        .set_preference(DEVELOPER_MODE, UserPreference::Enable)
        .expect("enabling developer mode is accepted");

    let gated = result
        .manager
        .report(REMOTE_DEBUGGING)
        .expect("remote debugging is in the catalogue");
    assert_eq!(gated.availability(), Availability::Disabled);
    assert_eq!(gated.reason(), Reason::ExperimentGated);
    assert_eq!(gated.authority(), DecidingAuthority::MaturityGating);

    result
        .manager
        .request_activation(DEVELOPER_TOOLS)
        .expect("developer tools activate");

    let after_tools = result
        .manager
        .report(REMOTE_DEBUGGING)
        .expect("remote debugging is in the catalogue");
    assert_eq!(after_tools.availability(), Availability::Disabled);
    assert_eq!(after_tools.reason(), Reason::ExperimentGated);
    assert_eq!(result.manager.lifecycle(REMOTE_DEBUGGING), None);

    result.manager.enable_experiment(REMOTE_DEBUGGING);
    let enabled = result
        .manager
        .report(REMOTE_DEBUGGING)
        .expect("remote debugging is in the catalogue");
    assert_eq!(enabled.availability(), Availability::Available);
    assert_eq!(enabled.lifecycle(), Some(Lifecycle::Dormant));
}

#[test]
fn remote_debugging_depends_on_developer_mode_even_with_an_experiment() {
    let mut result = bootstrap().expect("the built-in catalogue should build");
    result.manager.enable_experiment(REMOTE_DEBUGGING);

    let report = result
        .manager
        .report(REMOTE_DEBUGGING)
        .expect("remote debugging is in the catalogue");
    assert_eq!(report.availability(), Availability::Disabled);
    assert_eq!(report.reason(), Reason::DependencyUnmet);
    assert_eq!(report.authority(), DecidingAuthority::DependencyCheck);
    assert_eq!(report.unmet_dependencies(), &[DEVELOPER_MODE]);
}
