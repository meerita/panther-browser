// @file engines/purr/embedding/src/engine-capability-offer.rs
// @description Surfaces the engine capability offers upward to the product.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::{CapabilityDefinition, CapabilityId, CapabilityProvider};
use purr_engine::{
    SucceedingProvider, WEBGPU, WebGpuProvider, engine_capabilities, platform_supports,
};

/// One engine capability the embedder offers upward to the product.
///
/// The offer carries the engine declaration, its platform-support result, and a
/// provider, all shared vocabulary types. Activation failures surface through
/// the provider contract as the shared `ActivationFailure`, already translated
/// inside the engine, so no raw engine internal crosses the boundary.
pub struct EngineCapabilityOffer {
    pub definition: CapabilityDefinition,
    pub is_supported: bool,
    pub provider: Box<dyn CapabilityProvider>,
}

/// Collects every engine capability the product merges into its catalogue.
///
/// The offers keep the engine declaration order, so the product builds a
/// deterministic catalogue. Each call builds fresh providers, because a provider
/// owns runtime resources and must not be shared between managers.
pub fn engine_capability_offers() -> Vec<EngineCapabilityOffer> {
    engine_capabilities()
        .iter()
        .map(|definition| EngineCapabilityOffer {
            definition: definition.clone(),
            is_supported: platform_supports(definition.id),
            provider: provider_for(definition.id),
        })
        .collect()
}

fn provider_for(id: CapabilityId) -> Box<dyn CapabilityProvider> {
    // WebGPU uses the controllable backend provider so the product can drive the
    // supported activation-failure and quarantine scenarios; every other engine
    // capability uses the succeeding mock in M0.
    if id == WEBGPU {
        return Box::new(WebGpuProvider::new());
    }
    Box::new(SucceedingProvider)
}

#[cfg(test)]
mod tests {
    use super::engine_capability_offers;
    use purr_engine::{
        AUTHOR_STYLES, GPU_ACCELERATION, SERVICE_WORKERS, USER_AGENT_STYLES, WEBGPU,
    };

    #[test]
    fn the_upward_api_surfaces_the_five_engine_capabilities_with_support() {
        let offers = engine_capability_offers();

        assert_eq!(offers.len(), 5);
        for offer in &offers {
            assert_eq!(offer.definition.id.owner_namespace(), "purr");
        }

        let support = |id| {
            offers
                .iter()
                .find(|offer| offer.definition.id == id)
                .map(|offer| offer.is_supported)
        };

        assert_eq!(support(USER_AGENT_STYLES), Some(true));
        assert_eq!(support(AUTHOR_STYLES), Some(true));
        assert_eq!(support(SERVICE_WORKERS), Some(true));
        assert_eq!(support(GPU_ACCELERATION), Some(true));
        assert_eq!(support(WEBGPU), Some(false));
    }
}
