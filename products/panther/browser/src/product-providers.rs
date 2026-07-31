// @file products/panther/browser/src/product-providers.rs
// @description Mock capability providers for the Panther product capabilities.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::{ActivationFailure, CapabilityId, CapabilityProvider, FailureCategory};

use crate::product_capabilities::DEVELOPER_TOOLS;

/// Provider whose activation always succeeds and whose release does nothing.
///
/// It stands in for a product capability that owns no runtime resource in M0.
pub struct SucceedingProvider;

impl CapabilityProvider for SucceedingProvider {
    fn activate(&mut self) -> Result<(), ActivationFailure> {
        Ok(())
    }

    fn deactivate(&mut self) {}
}

/// Provider that acquires a resource on activation and releases it on
/// deactivation.
///
/// It rejects a second activation until a deactivation releases the resource, so
/// a successful re-activation proves the manager released the provider. This lets
/// the developer tools exercise deactivation with cleanup.
#[derive(Default)]
pub struct AcquireReleaseProvider {
    acquired: bool,
}

impl CapabilityProvider for AcquireReleaseProvider {
    fn activate(&mut self) -> Result<(), ActivationFailure> {
        if self.acquired {
            return Err(ActivationFailure::new(
                FailureCategory::ActivationError,
                "the developer tools resource is already acquired",
            ));
        }
        self.acquired = true;
        Ok(())
    }

    fn deactivate(&mut self) {
        self.acquired = false;
    }
}

/// Builds a fresh provider for one product capability.
///
/// Each manager owns its own providers, so this builds a new instance on every
/// call. The developer tools use the acquire and release provider to exercise
/// cleanup; the other product capabilities use the succeeding provider.
pub fn product_provider_for(id: CapabilityId) -> Box<dyn CapabilityProvider> {
    if id == DEVELOPER_TOOLS {
        return Box::new(AcquireReleaseProvider::default());
    }
    Box::new(SucceedingProvider)
}

#[cfg(test)]
mod tests {
    use super::{AcquireReleaseProvider, SucceedingProvider, product_provider_for};
    use crate::product_capabilities::{DEVELOPER_MODE, DEVELOPER_TOOLS};
    use capability_system::CapabilityProvider;

    #[test]
    fn the_succeeding_provider_activates() {
        let mut provider = SucceedingProvider;
        assert!(provider.activate().is_ok());
    }

    #[test]
    fn the_acquire_release_provider_rejects_a_second_activation_until_release() {
        let mut provider = AcquireReleaseProvider::default();

        assert!(provider.activate().is_ok());
        assert!(provider.activate().is_err());

        provider.deactivate();
        assert!(provider.activate().is_ok());
    }

    #[test]
    fn the_developer_tools_use_the_acquire_release_provider() {
        let mut tools = product_provider_for(DEVELOPER_TOOLS);
        assert!(tools.activate().is_ok());
        assert!(tools.activate().is_err());

        let mut other = product_provider_for(DEVELOPER_MODE);
        assert!(other.activate().is_ok());
        assert!(other.activate().is_ok());
    }
}
