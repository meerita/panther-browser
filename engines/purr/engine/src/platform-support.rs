// @file engines/purr/engine/src/platform-support.rs
// @description Reports engine platform support for each declared capability.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::CapabilityId;

use crate::capability_declarations::{WEBGPU, engine_capabilities};

/// Reports whether the current platform supports a capability.
///
/// WebGPU is unsupported by default because no GPU backend exists in M0. This is
/// a platform-support result only; it is never a provider activation failure. A
/// later phase drives the supported WebGPU scenarios by overriding support
/// through the policy inputs, never through this probe.
///
/// The probe is fail closed: an identifier the engine does not declare is
/// reported unsupported, so a missing declaration never makes a capability
/// available.
pub fn platform_supports(capability: CapabilityId) -> bool {
    if !is_declared(capability) {
        return false;
    }
    capability != WEBGPU
}

fn is_declared(capability: CapabilityId) -> bool {
    engine_capabilities()
        .iter()
        .any(|definition| definition.id == capability)
}

#[cfg(test)]
mod tests {
    use super::platform_supports;
    use crate::capability_declarations::{
        AUTHOR_STYLES, SERVICE_WORKERS, USER_AGENT_STYLES, WEBGPU,
    };
    use capability_system::CapabilityId;

    #[test]
    fn webgpu_is_unsupported_and_the_others_are_supported() {
        assert!(!platform_supports(WEBGPU));
        assert!(platform_supports(USER_AGENT_STYLES));
        assert!(platform_supports(AUTHOR_STYLES));
        assert!(platform_supports(SERVICE_WORKERS));
    }

    #[test]
    fn an_undeclared_capability_is_unsupported() {
        let unknown = CapabilityId::new("purr.unknown");
        assert!(!platform_supports(unknown));
    }
}
