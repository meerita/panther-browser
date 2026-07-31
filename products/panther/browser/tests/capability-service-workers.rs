// @file products/panther/browser/tests/capability-service-workers.rs
// @description Integration test for the default-disabled service workers capability.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Drives the assembled system through service workers, which product policy
//! disables by default in M0. It proves the `Disabled` availability is distinct
//! from `Unsupported` and `NotBuilt`, and that a disabled capability does not
//! reach its provider on an activation request.

use capability_system::{Availability, CapabilityRequestError, DecidingAuthority, Reason};
use panther_browser::bootstrap;
use purr_embedding::SERVICE_WORKERS;

#[test]
fn service_workers_are_disabled_by_default() {
    let result = bootstrap().expect("the built-in catalogue should build");

    let report = result
        .manager
        .report(SERVICE_WORKERS)
        .expect("service workers are in the catalogue");
    assert_eq!(report.availability(), Availability::Disabled);
    assert_eq!(report.reason(), Reason::UserDisabled);
    assert_eq!(report.authority(), DecidingAuthority::UserPreference);
    assert_eq!(report.lifecycle(), None);
}

#[test]
fn a_disabled_service_workers_request_does_not_reach_the_provider() {
    let mut result = bootstrap().expect("the built-in catalogue should build");

    assert_eq!(
        result.manager.request_activation(SERVICE_WORKERS),
        Err(CapabilityRequestError::NotAvailable(SERVICE_WORKERS))
    );
    assert_eq!(result.manager.lifecycle(SERVICE_WORKERS), None);
}
