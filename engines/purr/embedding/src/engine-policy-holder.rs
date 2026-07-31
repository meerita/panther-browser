// @file engines/purr/embedding/src/engine-policy-holder.rs
// @description Stores the downward engine-policy snapshot for engine diagnostics.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::EnginePolicySnapshot;

/// Engine-side holder of the downward effective engine-policy snapshot.
///
/// The embedder receives the resolved snapshot from the product and stores it
/// here. Engine diagnostics read it back unchanged. The holder keeps the shared
/// snapshot type only, so no product or engine internal crosses the boundary. A
/// future out-of-process split serializes the same snapshot without reshaping
/// the holder.
pub struct EnginePolicyHolder {
    snapshot: EnginePolicySnapshot,
}

impl EnginePolicyHolder {
    pub fn new(snapshot: EnginePolicySnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the stored snapshot for read-only engine diagnostics.
    pub fn diagnostics(&self) -> &EnginePolicySnapshot {
        &self.snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::EnginePolicyHolder;
    use capability_system::{
        CapabilityDefinition, CapabilityId, CatalogueBuilder, Category, EnginePolicySnapshot,
        Manager, Maturity, Owner, PolicyInputs,
    };

    const ENGINE_CAPABILITY: CapabilityId = CapabilityId::new("purr.user-agent-styles");
    const NO_DEPENDENCIES: &[CapabilityId] = &[];

    fn engine_snapshot() -> EnginePolicySnapshot {
        let mut builder = CatalogueBuilder::new();
        builder.add(CapabilityDefinition {
            id: ENGINE_CAPABILITY,
            owner: Owner::Purr,
            category: Category::EngineService,
            maturity: Maturity::Stable,
            dependencies: NO_DEPENDENCIES,
            is_mandatory: true,
            is_built: true,
        });
        let catalogue = builder.build().expect("the fixture catalogue builds");

        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(ENGINE_CAPABILITY);

        Manager::new(catalogue, inputs).engine_policy_snapshot()
    }

    #[test]
    fn the_holder_returns_the_stored_snapshot_unchanged() {
        let snapshot = engine_snapshot();
        assert!(!snapshot.entries().is_empty());

        let holder = EnginePolicyHolder::new(snapshot.clone());

        assert_eq!(holder.diagnostics(), &snapshot);
    }
}
