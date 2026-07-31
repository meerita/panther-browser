// @file products/panther/browser/src/product-policy.rs
// @description Builds the product policy inputs and the platform-support override seam.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::HashMap;

use capability_system::{CapabilityId, PolicyInputs, UserPreference};
use purr_embedding::{EngineCapabilityOffer, SERVICE_WORKERS};

use crate::product_capabilities::{DEVELOPER_MODE, product_capabilities};

/// Per-capability platform-support overrides for assembly.
///
/// Platform support enters the policy inputs before the manager is built, so it
/// cannot be changed on the manager afterwards. The override lets a caller drive
/// the supported versus unsupported scenarios (for example WebGPU) by forcing the
/// support decision for one capability. The other policy inputs (safe mode,
/// experiments, preferences, and quarantine) stay changeable on the manager after
/// assembly.
#[derive(Debug, Default)]
pub struct BootstrapConfig {
    support_overrides: HashMap<CapabilityId, bool>,
}

impl BootstrapConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forces the platform-support decision for one capability, overriding the
    /// engine probe or the product default.
    pub fn set_platform_support(&mut self, id: CapabilityId, supported: bool) -> &mut Self {
        self.support_overrides.insert(id, supported);
        self
    }

    fn platform_support(&self, id: CapabilityId, default: bool) -> bool {
        self.support_overrides.get(&id).copied().unwrap_or(default)
    }
}

/// Builds the policy inputs the resolver reads for one assembly.
///
/// Product capabilities run in process, so their platform support defaults to
/// true. Engine capabilities take their support from the engine probe carried by
/// each offer, so an unsupported engine capability (WebGPU in M0) stays
/// unsupported. Any override in the config wins over both. Developer mode and
/// service workers are off until a user preference turns them on, so each carries
/// a default disable preference; a later enable request lifts it. Service workers
/// are engine owned but disabled by product policy in M0, so the product sets the
/// default here rather than the engine.
pub fn build_policy_inputs(
    offers: &[EngineCapabilityOffer],
    config: &BootstrapConfig,
) -> PolicyInputs {
    let mut inputs = PolicyInputs::new();

    for definition in product_capabilities() {
        if config.platform_support(definition.id, true) {
            inputs.mark_supported(definition.id);
        }
    }

    for offer in offers {
        if config.platform_support(offer.definition.id, offer.is_supported) {
            inputs.mark_supported(offer.definition.id);
        }
    }

    inputs.set_preference(DEVELOPER_MODE, UserPreference::Disable);
    inputs.set_preference(SERVICE_WORKERS, UserPreference::Disable);

    inputs
}

#[cfg(test)]
mod tests {
    use super::{BootstrapConfig, build_policy_inputs};
    use crate::product_capabilities::DEVELOPER_MODE;
    use capability_system::{CapabilityId, UserPreference};
    use purr_embedding::{SERVICE_WORKERS, engine_capability_offers};

    /// Finds the engine capability the probe reports unsupported. In M0 this is
    /// WebGPU. The test finds it through the boundary rather than naming the
    /// engine identifier, so the product does not reach the engine core.
    fn unsupported_engine_capability() -> CapabilityId {
        engine_capability_offers()
            .into_iter()
            .find(|offer| !offer.is_supported)
            .expect("the engine reports at least one unsupported capability in M0")
            .definition
            .id
    }

    #[test]
    fn product_support_defaults_true_and_the_engine_probe_leaves_webgpu_unsupported() {
        let offers = engine_capability_offers();
        let inputs = build_policy_inputs(&offers, &BootstrapConfig::new());

        assert!(inputs.is_supported(DEVELOPER_MODE));
        assert!(!inputs.is_supported(unsupported_engine_capability()));
    }

    #[test]
    fn an_override_drives_engine_support() {
        let offers = engine_capability_offers();
        let webgpu = unsupported_engine_capability();

        let mut config = BootstrapConfig::new();
        config.set_platform_support(webgpu, true);
        let inputs = build_policy_inputs(&offers, &config);

        assert!(inputs.is_supported(webgpu));
    }

    #[test]
    fn developer_mode_is_off_by_default() {
        let offers = engine_capability_offers();
        let inputs = build_policy_inputs(&offers, &BootstrapConfig::new());

        assert_eq!(
            inputs.preference(DEVELOPER_MODE),
            Some(UserPreference::Disable)
        );
    }

    #[test]
    fn service_workers_are_off_by_default() {
        let offers = engine_capability_offers();
        let inputs = build_policy_inputs(&offers, &BootstrapConfig::new());

        assert_eq!(
            inputs.preference(SERVICE_WORKERS),
            Some(UserPreference::Disable)
        );
    }
}
