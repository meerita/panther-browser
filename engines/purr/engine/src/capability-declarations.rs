// @file engines/purr/engine/src/capability-declarations.rs
// @description Declares the Purr engine capabilities and their public accessor.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::{CapabilityDefinition, CapabilityId, Category, Maturity, Owner};

/// Mandatory user-agent styles. The engine baseline stylesheet must always
/// apply, so a lower-authority disable request is rejected during resolution.
pub const USER_AGENT_STYLES: CapabilityId = CapabilityId::new("purr.user-agent-styles");

/// Optional author styles. Page-supplied CSS is enabled by default and the user
/// can disable it. Disabling it does not remove user-agent styles or the CSS
/// engine; it only stops applying page-supplied rules.
pub const AUTHOR_STYLES: CapabilityId = CapabilityId::new("purr.author-styles");

/// Optional experimental WebGPU access. Platform support and provider activation
/// are independent runtime conditions.
pub const WEBGPU: CapabilityId = CapabilityId::new("purr.webgpu");

/// Optional service workers. Disabled by default for M0 through product policy,
/// so its provider is normally not invoked.
pub const SERVICE_WORKERS: CapabilityId = CapabilityId::new("purr.service-workers");

/// Optional hardware graphics acceleration. This toggles the acceleration path,
/// not rendering itself. When it resolves to disabled or the hardware is
/// unavailable, the software backend still renders. It is distinct from
/// `purr.webgpu`, which exposes the WebGPU web platform surface.
pub const GPU_ACCELERATION: CapabilityId = CapabilityId::new("purr.gpu-acceleration");

const NO_DEPENDENCIES: &[CapabilityId] = &[];

static ENGINE_CAPABILITIES: [CapabilityDefinition; 5] = [
    CapabilityDefinition {
        id: USER_AGENT_STYLES,
        owner: Owner::Purr,
        category: Category::EngineService,
        maturity: Maturity::Stable,
        dependencies: NO_DEPENDENCIES,
        is_mandatory: true,
        is_built: true,
    },
    CapabilityDefinition {
        id: AUTHOR_STYLES,
        owner: Owner::Purr,
        category: Category::WebPlatform,
        maturity: Maturity::Stable,
        dependencies: NO_DEPENDENCIES,
        is_mandatory: false,
        is_built: true,
    },
    CapabilityDefinition {
        id: WEBGPU,
        owner: Owner::Purr,
        category: Category::WebPlatform,
        maturity: Maturity::Experimental,
        dependencies: NO_DEPENDENCIES,
        is_mandatory: false,
        is_built: true,
    },
    CapabilityDefinition {
        id: SERVICE_WORKERS,
        owner: Owner::Purr,
        category: Category::WebPlatform,
        maturity: Maturity::Stable,
        dependencies: NO_DEPENDENCIES,
        is_mandatory: false,
        is_built: true,
    },
    CapabilityDefinition {
        id: GPU_ACCELERATION,
        owner: Owner::Purr,
        category: Category::EngineService,
        maturity: Maturity::Stable,
        dependencies: NO_DEPENDENCIES,
        is_mandatory: false,
        is_built: true,
    },
];

/// Returns the capabilities the Purr engine declares.
///
/// This is the upward declaration entry point the product merges into its
/// catalogue. The engine declares definitions only; it holds no product policy.
pub fn engine_capabilities() -> &'static [CapabilityDefinition] {
    &ENGINE_CAPABILITIES
}

#[cfg(test)]
mod tests {
    use super::{
        AUTHOR_STYLES, GPU_ACCELERATION, SERVICE_WORKERS, USER_AGENT_STYLES, WEBGPU,
        engine_capabilities,
    };
    use capability_system::{CapabilityDefinition, CapabilityId, Category, Maturity, Owner};

    fn definition(id: CapabilityId) -> &'static CapabilityDefinition {
        engine_capabilities()
            .iter()
            .find(|definition| definition.id == id)
            .expect("the engine declares this capability")
    }

    #[test]
    fn every_definition_is_owned_by_purr_with_a_matching_namespace() {
        assert_eq!(engine_capabilities().len(), 5);
        for definition in engine_capabilities() {
            assert_eq!(definition.owner, Owner::Purr);
            assert_eq!(definition.id.owner_namespace(), "purr");
        }
    }

    #[test]
    fn user_agent_styles_is_mandatory_and_author_styles_is_optional_default_enabled() {
        let user_agent = definition(USER_AGENT_STYLES);
        assert!(user_agent.is_mandatory);

        let author = definition(AUTHOR_STYLES);
        assert!(!author.is_mandatory);
        assert_eq!(author.maturity, Maturity::Stable);
    }

    #[test]
    fn webgpu_is_experimental_and_service_workers_is_stable() {
        assert_eq!(definition(WEBGPU).maturity, Maturity::Experimental);
        assert!(!definition(WEBGPU).is_mandatory);

        assert_eq!(definition(SERVICE_WORKERS).maturity, Maturity::Stable);
        assert!(!definition(SERVICE_WORKERS).is_mandatory);
    }

    #[test]
    fn gpu_acceleration_is_a_stable_optional_engine_service_owned_by_purr() {
        let gpu_acceleration = definition(GPU_ACCELERATION);
        assert_eq!(gpu_acceleration.owner, Owner::Purr);
        assert_eq!(gpu_acceleration.category, Category::EngineService);
        assert_eq!(gpu_acceleration.maturity, Maturity::Stable);
        assert!(!gpu_acceleration.is_mandatory);
    }
}
