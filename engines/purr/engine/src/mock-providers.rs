// @file engines/purr/engine/src/mock-providers.rs
// @description Mock capability providers for the Purr engine capabilities.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::{ActivationFailure, CapabilityProvider, FailureCategory};

/// Provider whose activation always succeeds.
///
/// It stands in for the styling and service-worker capabilities in M0, where no
/// real subsystem exists yet.
pub struct SucceedingProvider;

impl CapabilityProvider for SucceedingProvider {
    fn activate(&mut self) -> Result<(), ActivationFailure> {
        Ok(())
    }

    fn deactivate(&mut self) {}
}

/// Internal failure of the mock WebGPU backend.
///
/// This is engine-internal detail. It is translated into an [`ActivationFailure`]
/// before it can cross the capability boundary, so no engine internal reaches the
/// product.
enum WebGpuBackendError {
    NoBackend,
}

fn translate(error: WebGpuBackendError) -> ActivationFailure {
    match error {
        WebGpuBackendError::NoBackend => ActivationFailure::new(
            FailureCategory::ActivationError,
            "WebGPU backend is not available",
        ),
    }
}

/// Provider for WebGPU whose activation outcome is controllable.
///
/// Its failure is independent of platform support, so it can drive the supported
/// activation-failure and quarantine scenarios when support is overridden to
/// true. By default the backend is not ready, so activation fails with a
/// translated failure.
#[derive(Default)]
pub struct WebGpuProvider {
    backend_ready: bool,
}

impl WebGpuProvider {
    pub const fn new() -> Self {
        Self {
            backend_ready: false,
        }
    }

    pub const fn with_backend_ready(backend_ready: bool) -> Self {
        Self { backend_ready }
    }
}

impl CapabilityProvider for WebGpuProvider {
    fn activate(&mut self) -> Result<(), ActivationFailure> {
        if self.backend_ready {
            return Ok(());
        }
        Err(translate(WebGpuBackendError::NoBackend))
    }

    fn deactivate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::{SucceedingProvider, WebGpuProvider};
    use capability_system::{CapabilityProvider, FailureCategory};

    #[test]
    fn the_succeeding_provider_activates() {
        let mut provider = SucceedingProvider;
        assert!(provider.activate().is_ok());
    }

    #[test]
    fn the_webgpu_provider_returns_a_translated_activation_failure() {
        let mut provider = WebGpuProvider::new();

        let failure = provider
            .activate()
            .expect_err("the default WebGPU backend is not ready");

        assert_eq!(failure.category(), FailureCategory::ActivationError);
        assert!(!failure.message().is_empty());
    }

    #[test]
    fn the_webgpu_provider_can_be_driven_to_succeed() {
        let mut provider = WebGpuProvider::with_backend_ready(true);
        assert!(provider.activate().is_ok());
    }
}
