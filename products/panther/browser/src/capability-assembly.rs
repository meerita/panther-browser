// @file products/panther/browser/src/capability-assembly.rs
// @description Assembles the merged catalogue, owns the manager, and exposes bootstrap.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::{Catalogue, CatalogueBuildError, CatalogueBuilder, Manager};
use purr_embedding::{EngineCapabilityOffer, EnginePolicyHolder, engine_capability_offers};

use crate::product_capabilities::product_capabilities;
use crate::product_policy::{BootstrapConfig, build_policy_inputs};
use crate::product_providers::product_provider_for;

/// Reason capability assembly failed.
///
/// Assembly is fail closed: a malformed catalogue is rejected and no manager is
/// built. The message stays generic; the source keeps the validation detail for
/// diagnostics without exposing it to a user surface.
#[derive(Debug, thiserror::Error)]
pub enum AssemblyError {
    #[error("the capability catalogue is malformed")]
    MalformedCatalogue(#[from] CatalogueBuildError),
}

/// Product-owned result of a successful bootstrap.
///
/// The product owns the manager instance and the engine-policy holder. The
/// manager stays the single source of truth, so reports are derived from it on
/// request rather than cached.
pub struct BootstrapResult {
    pub manager: Manager,
    pub engine_policy: EnginePolicyHolder,
}

impl BootstrapResult {
    /// Returns a report for every capability in the catalogue, in catalogue
    /// order, from the current manager state.
    pub fn reports(&self) -> Vec<capability_system::CapabilityReport> {
        self.manager.catalogue_report()
    }
}

/// Assembles the capability system with the default policy.
///
/// This is the entry point the application calls. It merges the engine and
/// product declarations, resolves the default policy, constructs and populates
/// the manager, and pushes the effective engine snapshot to the embedding holder.
pub fn bootstrap() -> Result<BootstrapResult, AssemblyError> {
    bootstrap_with(&BootstrapConfig::new())
}

/// Assembles the capability system with a caller-supplied configuration.
///
/// The configuration only carries the platform-support override, because support
/// enters the manager at construction. Every other policy input stays changeable
/// on the returned manager.
pub fn bootstrap_with(config: &BootstrapConfig) -> Result<BootstrapResult, AssemblyError> {
    let offers = engine_capability_offers();

    let inputs = build_policy_inputs(&offers, config);
    let catalogue = assemble_catalogue(&offers)?;

    let mut manager = Manager::new(catalogue, inputs);
    register_product_providers(&mut manager);
    register_engine_providers(&mut manager, offers);

    let engine_policy = EnginePolicyHolder::new(manager.engine_policy_snapshot());

    Ok(BootstrapResult {
        manager,
        engine_policy,
    })
}

/// Merges the engine and product declarations and runs the validating builder.
///
/// The engine declarations come first, then the product declarations. A malformed
/// set is rejected, so assembly never yields a partial catalogue.
fn assemble_catalogue(offers: &[EngineCapabilityOffer]) -> Result<Catalogue, AssemblyError> {
    let mut builder = CatalogueBuilder::new();
    builder.extend(offers.iter().map(|offer| offer.definition.clone()));
    builder.extend(product_capabilities().iter().cloned());
    let catalogue = builder.build()?;
    Ok(catalogue)
}

fn register_product_providers(manager: &mut Manager) {
    for definition in product_capabilities() {
        manager.register_provider(definition.id, product_provider_for(definition.id));
    }
}

/// Registers the engine providers, moving each provider out of its offer.
///
/// A provider owns runtime resources, so the manager takes ownership; the offers
/// are consumed here.
fn register_engine_providers(manager: &mut Manager, offers: Vec<EngineCapabilityOffer>) {
    for offer in offers {
        manager.register_provider(offer.definition.id, offer.provider);
    }
}

#[cfg(test)]
mod tests {
    use super::{AssemblyError, bootstrap};
    use crate::product_capabilities::{DEVELOPER_MODE, DEVELOPER_TOOLS, REMOTE_DEBUGGING};
    use capability_system::{
        Availability, CapabilityDefinition, CapabilityId, CatalogueBuilder, Category, Maturity,
        Owner,
    };

    const ON_ABSENT: &[CapabilityId] = &[CapabilityId::new("panther.absent")];

    fn availability_of(
        reports: &[capability_system::CapabilityReport],
        id: CapabilityId,
    ) -> Availability {
        reports
            .iter()
            .find(|report| report.id() == id)
            .map(|report| report.availability())
            .expect("the report should exist")
    }

    #[test]
    fn bootstrap_builds_the_full_catalogue_and_reports_eight_capabilities() {
        let result = bootstrap().expect("the built-in catalogue should build");

        let reports = result.reports();
        assert_eq!(reports.len(), 8);

        let purr_entries = result.engine_policy.diagnostics().entries().len();
        assert_eq!(purr_entries, 5);
    }

    #[test]
    fn bootstrap_leaves_developer_capabilities_off_by_default() {
        let result = bootstrap().expect("the built-in catalogue should build");
        let reports = result.reports();

        assert_ne!(
            availability_of(&reports, DEVELOPER_MODE),
            Availability::Available
        );
        assert_ne!(
            availability_of(&reports, DEVELOPER_TOOLS),
            Availability::Available
        );
        assert_ne!(
            availability_of(&reports, REMOTE_DEBUGGING),
            Availability::Available
        );
    }

    #[test]
    fn assembly_rejects_a_malformed_catalogue() {
        let mut builder = CatalogueBuilder::new();
        builder.add(CapabilityDefinition {
            id: CapabilityId::new("panther.broken"),
            owner: Owner::Panther,
            category: Category::ProductFeature,
            maturity: Maturity::Stable,
            dependencies: ON_ABSENT,
            is_mandatory: false,
            is_built: true,
        });

        let error = AssemblyError::from(builder.build().expect_err("the fixture is malformed"));

        assert!(matches!(error, AssemblyError::MalformedCatalogue(_)));
    }
}
