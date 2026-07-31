// @file products/panther/browser/tests/capability-webgpu.rs
// @description Integration tests for the three separate WebGPU runtime scenarios.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Drives the assembled system through WebGPU as three strictly separate
//! scenarios that never share one runtime condition: an unsupported platform,
//! a supported activation failure, and a supported repeated failure that reaches
//! runtime quarantine. Each scenario uses distinct support and policy inputs.

use capability_system::{
    ActivationFailure, Availability, CapabilityProvider, CapabilityRequestError, DecidingAuthority,
    FailureCategory, Lifecycle, Reason,
};
use panther_browser::{BootstrapConfig, bootstrap, bootstrap_with};
use purr_embedding::WEBGPU;

/// Provider that panics if activation is invoked. Registering it proves the
/// manager does not reach the provider while the capability is not available.
struct PanicOnActivateProvider;

impl CapabilityProvider for PanicOnActivateProvider {
    fn activate(&mut self) -> Result<(), ActivationFailure> {
        panic!("the provider must not activate while the capability is not available");
    }

    fn deactivate(&mut self) {}
}

/// Provider whose activation always succeeds. It proves a cleared quarantine
/// allows a fresh activation to reach `Active`.
struct SucceedingProvider;

impl CapabilityProvider for SucceedingProvider {
    fn activate(&mut self) -> Result<(), ActivationFailure> {
        Ok(())
    }

    fn deactivate(&mut self) {}
}

fn supported_webgpu_config() -> BootstrapConfig {
    let mut config = BootstrapConfig::new();
    config.set_platform_support(WEBGPU, true);
    config
}

#[test]
fn an_unsupported_platform_leaves_webgpu_unsupported_without_a_provider_call() {
    let mut result = bootstrap().expect("the built-in catalogue should build");
    result
        .manager
        .register_provider(WEBGPU, Box::new(PanicOnActivateProvider));

    let report = result
        .manager
        .report(WEBGPU)
        .expect("webgpu is in the catalogue");
    assert_eq!(report.availability(), Availability::Unsupported);
    assert_eq!(report.reason(), Reason::PlatformUnsupported);
    assert_eq!(report.authority(), DecidingAuthority::PlatformSupport);
    assert_eq!(report.lifecycle(), None);

    assert_eq!(
        result.manager.request_activation(WEBGPU),
        Err(CapabilityRequestError::NotAvailable(WEBGPU))
    );
}

#[test]
fn a_supported_activation_failure_leaves_webgpu_failed_without_quarantine() {
    let mut result =
        bootstrap_with(&supported_webgpu_config()).expect("the built-in catalogue should build");
    result.manager.enable_experiment(WEBGPU);

    let available = result
        .manager
        .report(WEBGPU)
        .expect("webgpu is in the catalogue");
    assert_eq!(available.availability(), Availability::Available);
    assert_eq!(available.lifecycle(), Some(Lifecycle::Dormant));

    result
        .manager
        .request_activation(WEBGPU)
        .expect("a valid request is accepted even when activation fails");

    let failed = result
        .manager
        .report(WEBGPU)
        .expect("webgpu is in the catalogue");
    assert_eq!(failed.availability(), Availability::Available);
    assert_eq!(
        failed.lifecycle(),
        Some(Lifecycle::Failed(FailureCategory::ActivationError))
    );
    assert_eq!(
        failed.runtime_failure(),
        Some(FailureCategory::ActivationError)
    );
}

#[test]
fn repeated_failures_quarantine_webgpu_and_clearing_allows_a_retry() {
    let mut result =
        bootstrap_with(&supported_webgpu_config()).expect("the built-in catalogue should build");
    result.manager.enable_experiment(WEBGPU);
    result.manager.set_failure_threshold(2);

    result
        .manager
        .request_activation(WEBGPU)
        .expect("the first request is accepted");
    assert_eq!(
        result.manager.lifecycle(WEBGPU),
        Some(Lifecycle::Failed(FailureCategory::ActivationError))
    );

    result
        .manager
        .request_activation(WEBGPU)
        .expect("the second request is accepted");

    let quarantined = result
        .manager
        .report(WEBGPU)
        .expect("webgpu is in the catalogue");
    assert_eq!(quarantined.availability(), Availability::Prohibited);
    assert_eq!(quarantined.reason(), Reason::QuarantinedAfterFailure);
    assert_eq!(quarantined.authority(), DecidingAuthority::RuntimeHealth);
    assert_eq!(quarantined.lifecycle(), None);

    result
        .manager
        .register_provider(WEBGPU, Box::new(PanicOnActivateProvider));
    assert_eq!(
        result.manager.request_activation(WEBGPU),
        Err(CapabilityRequestError::NotAvailable(WEBGPU))
    );

    result
        .manager
        .clear_quarantine(WEBGPU)
        .expect("clearing a known capability succeeds");
    result
        .manager
        .register_provider(WEBGPU, Box::new(SucceedingProvider));

    result
        .manager
        .request_activation(WEBGPU)
        .expect("the retry is accepted after the quarantine is cleared");
    assert_eq!(result.manager.lifecycle(WEBGPU), Some(Lifecycle::Active));
}
