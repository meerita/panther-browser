// @file products/panther/browser/tests/capability-styles.rs
// @description Integration tests for the mandatory and optional styling capabilities.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Drives the assembled system through the styling capabilities: mandatory
//! user-agent styles that reject a lower-authority disable request, and optional
//! author styles that are enabled by default and user-disableable without
//! removing user-agent styles.

use capability_system::{
    Availability, CapabilityRequestError, DecidingAuthority, Reason, UserPreference,
};
use panther_browser::bootstrap;
use purr_embedding::{AUTHOR_STYLES, USER_AGENT_STYLES};

#[test]
fn user_agent_styles_are_mandatory_and_reject_a_disable_request() {
    let mut result = bootstrap().expect("the built-in catalogue should build");

    let before = result
        .manager
        .report(USER_AGENT_STYLES)
        .expect("user-agent styles are in the catalogue");
    assert_eq!(before.availability(), Availability::Available);
    assert_eq!(before.reason(), Reason::MandatorySecurity);
    assert_eq!(before.authority(), DecidingAuthority::MandatorySecurity);

    assert_eq!(
        result
            .manager
            .set_preference(USER_AGENT_STYLES, UserPreference::Disable),
        Err(CapabilityRequestError::MandatoryCapabilityNotDisableable(
            USER_AGENT_STYLES
        ))
    );

    let after = result
        .manager
        .report(USER_AGENT_STYLES)
        .expect("user-agent styles are in the catalogue");
    assert_eq!(after.availability(), Availability::Available);
    assert_eq!(after.reason(), Reason::MandatorySecurity);
    assert_eq!(after.authority(), DecidingAuthority::MandatorySecurity);
}

#[test]
fn author_styles_are_enabled_by_default() {
    let result = bootstrap().expect("the built-in catalogue should build");

    let report = result
        .manager
        .report(AUTHOR_STYLES)
        .expect("author styles are in the catalogue");
    assert_eq!(report.availability(), Availability::Available);
    assert_eq!(report.reason(), Reason::DefaultAvailable);
    assert_eq!(report.authority(), DecidingAuthority::UserPreference);
}

#[test]
fn disabling_author_styles_leaves_user_agent_styles_available() {
    let mut result = bootstrap().expect("the built-in catalogue should build");

    result
        .manager
        .set_preference(AUTHOR_STYLES, UserPreference::Disable)
        .expect("disabling an optional capability is accepted");

    let author = result
        .manager
        .report(AUTHOR_STYLES)
        .expect("author styles are in the catalogue");
    assert_eq!(author.availability(), Availability::Disabled);
    assert_eq!(author.reason(), Reason::UserDisabled);
    assert_eq!(author.authority(), DecidingAuthority::UserPreference);

    let user_agent = result
        .manager
        .report(USER_AGENT_STYLES)
        .expect("user-agent styles are in the catalogue");
    assert_eq!(user_agent.availability(), Availability::Available);
    assert_eq!(user_agent.reason(), Reason::MandatorySecurity);
}
