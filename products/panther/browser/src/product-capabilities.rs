// @file products/panther/browser/src/product-capabilities.rs
// @description Declares the Panther product capabilities and their public accessor.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::{CapabilityDefinition, CapabilityId, Category, Maturity, Owner};

/// Developer mode. It gates the developer surface and is a dependency target for
/// the developer tooling. It is off until a user preference turns it on, so the
/// product supplies a default disable preference rather than leaving it available.
pub const DEVELOPER_MODE: CapabilityId = CapabilityId::new("panther.developer-mode");

/// Developer tools. Stable tooling that depends on developer mode. It exercises
/// lazy activation and deactivation with resource cleanup.
pub const DEVELOPER_TOOLS: CapabilityId = CapabilityId::new("panther.developer-tools");

/// Remote debugging. Experimental tooling that also depends on developer mode. It
/// is gated by maturity unless an experiment enables it, and enabling the
/// developer tools never enables it.
pub const REMOTE_DEBUGGING: CapabilityId = CapabilityId::new("panther.remote-debugging");

const NO_DEPENDENCIES: &[CapabilityId] = &[];
const ON_DEVELOPER_MODE: &[CapabilityId] = &[DEVELOPER_MODE];

static PRODUCT_CAPABILITIES: [CapabilityDefinition; 3] = [
    CapabilityDefinition {
        id: DEVELOPER_MODE,
        owner: Owner::Panther,
        category: Category::DeveloperTooling,
        maturity: Maturity::Stable,
        dependencies: NO_DEPENDENCIES,
        is_mandatory: false,
        is_built: true,
    },
    CapabilityDefinition {
        id: DEVELOPER_TOOLS,
        owner: Owner::Panther,
        category: Category::DeveloperTooling,
        maturity: Maturity::Stable,
        dependencies: ON_DEVELOPER_MODE,
        is_mandatory: false,
        is_built: true,
    },
    CapabilityDefinition {
        id: REMOTE_DEBUGGING,
        owner: Owner::Panther,
        category: Category::DeveloperTooling,
        maturity: Maturity::Experimental,
        dependencies: ON_DEVELOPER_MODE,
        is_mandatory: false,
        is_built: true,
    },
];

/// Returns the capabilities the Panther product declares.
///
/// The product merges these with the engine declarations to build its catalogue.
pub fn product_capabilities() -> &'static [CapabilityDefinition] {
    &PRODUCT_CAPABILITIES
}

#[cfg(test)]
mod tests {
    use super::{DEVELOPER_MODE, DEVELOPER_TOOLS, REMOTE_DEBUGGING, product_capabilities};
    use capability_system::{CapabilityDefinition, CapabilityId, Maturity, Owner};

    fn definition(id: CapabilityId) -> &'static CapabilityDefinition {
        product_capabilities()
            .iter()
            .find(|definition| definition.id == id)
            .expect("the product declares this capability")
    }

    #[test]
    fn every_definition_is_owned_by_panther_with_a_matching_namespace() {
        assert_eq!(product_capabilities().len(), 3);
        for definition in product_capabilities() {
            assert_eq!(definition.owner, Owner::Panther);
            assert_eq!(definition.id.owner_namespace(), "panther");
        }
    }

    #[test]
    fn developer_tools_and_remote_debugging_depend_on_developer_mode() {
        assert_eq!(definition(DEVELOPER_TOOLS).dependencies, &[DEVELOPER_MODE]);
        assert_eq!(definition(REMOTE_DEBUGGING).dependencies, &[DEVELOPER_MODE]);
    }

    #[test]
    fn developer_tools_is_stable_and_remote_debugging_is_experimental() {
        assert_eq!(definition(DEVELOPER_MODE).maturity, Maturity::Stable);
        assert_eq!(definition(DEVELOPER_TOOLS).maturity, Maturity::Stable);
        assert_eq!(
            definition(REMOTE_DEBUGGING).maturity,
            Maturity::Experimental
        );
    }
}
