// @file foundation/capability-system/src/capability-manager.rs
// @description Owns capability lifecycle, precedence, and the provider registry.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::HashMap;

use crate::availability::Availability;
use crate::capability_definition::CapabilityDefinition;
use crate::capability_id::CapabilityId;
use crate::capability_provider::CapabilityProvider;
use crate::capability_report::CapabilityReport;
use crate::capability_request_error::CapabilityRequestError;
use crate::catalogue::Catalogue;
use crate::effective_state::EffectiveState;
use crate::engine_policy_snapshot::{EngineCapabilityState, EnginePolicySnapshot};
use crate::lifecycle::{FailureCategory, Lifecycle};
use crate::owner::Owner;
use crate::policy_inputs::{PolicyInputs, UserPreference};
use crate::resolver::{Resolution, resolve};

/// Default number of activation failures that quarantines a capability.
///
/// A single failure below this count stays `Failed`; only reaching the count
/// quarantines the capability. The threshold is program set and can be changed
/// with [`Manager::set_failure_threshold`].
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 3;

/// Single-threaded owner of capability lifecycle and precedence.
///
/// The manager holds an immutable catalogue, the current policy inputs, the
/// resolved effective state, the runtime lifecycle of each capability, the
/// provider registry, and the per-capability failure counter. Transitions take
/// `&mut self` and re-resolve when policy changes; queries take `&self`, read
/// the cached resolution, and never mutate. The type uses no locks and no
/// interior mutability; a single writer serializes every transition.
pub struct Manager {
    catalogue: Catalogue,
    inputs: PolicyInputs,
    resolution: Resolution,
    lifecycles: HashMap<CapabilityId, Lifecycle>,
    providers: HashMap<CapabilityId, Box<dyn CapabilityProvider>>,
    failure_counts: HashMap<CapabilityId, u32>,
    failure_threshold: u32,
}

impl Manager {
    pub fn new(catalogue: Catalogue, inputs: PolicyInputs) -> Self {
        let resolution = resolve(&catalogue, &inputs);
        Self {
            catalogue,
            inputs,
            resolution,
            lifecycles: HashMap::new(),
            providers: HashMap::new(),
            failure_counts: HashMap::new(),
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
        }
    }

    pub fn set_failure_threshold(&mut self, threshold: u32) {
        self.failure_threshold = threshold.max(1);
    }

    pub fn register_provider(&mut self, id: CapabilityId, provider: Box<dyn CapabilityProvider>) {
        self.providers.insert(id, provider);
    }

    /// Requests activation of a capability on demand.
    ///
    /// The capability must resolve to `Available`; a request for any other state
    /// returns `NotAvailable` and never invokes the provider, so a quarantined
    /// capability is not activated. An activation whose provider fails is a
    /// runtime outcome, not a request error: the request returns `Ok` and the
    /// resulting lifecycle reports the failure.
    pub fn request_activation(&mut self, id: CapabilityId) -> Result<(), CapabilityRequestError> {
        if !self.catalogue.contains(id) {
            return Err(CapabilityRequestError::UnknownCapability(id));
        }
        if !self.is_available(id) {
            return Err(CapabilityRequestError::NotAvailable(id));
        }

        match self.lifecycle_of(id) {
            Lifecycle::Active => Ok(()),
            Lifecycle::Dormant
            | Lifecycle::Starting
            | Lifecycle::Deactivating
            | Lifecycle::Failed(_) => {
                self.activate_now(id);
                Ok(())
            }
        }
    }

    /// Deactivates an active capability and releases its provider.
    ///
    /// This is a runtime lifecycle transition only; it does not change policy.
    /// A capability that is not active is left unchanged.
    pub fn deactivate(&mut self, id: CapabilityId) -> Result<(), CapabilityRequestError> {
        if !self.catalogue.contains(id) {
            return Err(CapabilityRequestError::UnknownCapability(id));
        }

        match self.lifecycle_of(id) {
            Lifecycle::Active => {
                self.lifecycles.insert(id, Lifecycle::Deactivating);
                if let Some(provider) = self.providers.get_mut(&id) {
                    provider.deactivate();
                }
                self.lifecycles.insert(id, Lifecycle::Dormant);
                Ok(())
            }
            Lifecycle::Dormant
            | Lifecycle::Starting
            | Lifecycle::Deactivating
            | Lifecycle::Failed(_) => Ok(()),
        }
    }

    /// Sets the user preference for a capability and re-resolves.
    ///
    /// A disable request on a mandatory capability is rejected rather than
    /// silently ignored, so a security invariant is never presented as an
    /// optional switch.
    pub fn set_preference(
        &mut self,
        id: CapabilityId,
        preference: UserPreference,
    ) -> Result<(), CapabilityRequestError> {
        let Some(definition) = self.catalogue.get(id) else {
            return Err(CapabilityRequestError::UnknownCapability(id));
        };
        let is_mandatory = definition.is_mandatory;
        if preference == UserPreference::Disable && is_mandatory {
            return Err(CapabilityRequestError::MandatoryCapabilityNotDisableable(
                id,
            ));
        }

        self.inputs.set_preference(id, preference);
        self.reresolve();
        Ok(())
    }

    pub fn set_safe_mode(&mut self, value: bool) {
        self.inputs.set_safe_mode(value);
        self.reresolve();
    }

    pub fn enable_experiment(&mut self, id: CapabilityId) {
        self.inputs.enable_experiment(id);
        self.reresolve();
    }

    /// Clears a quarantine and resets the failure counter so the capability can
    /// be retried.
    pub fn clear_quarantine(&mut self, id: CapabilityId) -> Result<(), CapabilityRequestError> {
        if !self.catalogue.contains(id) {
            return Err(CapabilityRequestError::UnknownCapability(id));
        }
        self.inputs.clear_quarantine(id);
        self.failure_counts.insert(id, 0);
        self.lifecycles.insert(id, Lifecycle::Dormant);
        self.reresolve();
        Ok(())
    }

    /// Returns the current effective state, overlaying the runtime lifecycle on
    /// an available capability. Unknown identifiers have no state.
    pub fn effective_state(&self, id: CapabilityId) -> Option<EffectiveState> {
        let base = self.resolution.state(id)?;
        match base {
            EffectiveState::Available {
                reason, authority, ..
            } => Some(EffectiveState::Available {
                reason,
                authority,
                lifecycle: self.lifecycle_of(id),
            }),
            EffectiveState::Unavailable { .. } => Some(base),
        }
    }

    /// Returns the runtime lifecycle only when the capability is available; an
    /// unavailable capability carries no lifecycle.
    pub fn lifecycle(&self, id: CapabilityId) -> Option<Lifecycle> {
        match self.effective_state(id)? {
            EffectiveState::Available { lifecycle, .. } => Some(lifecycle),
            EffectiveState::Unavailable { .. } => None,
        }
    }

    /// Builds a read-only diagnostic report for one capability, or `None` when
    /// the catalogue does not hold the identifier.
    pub fn report(&self, id: CapabilityId) -> Option<CapabilityReport> {
        let definition = self.catalogue.get(id)?;
        let state = self.effective_state(id)?;
        Some(CapabilityReport::new(
            definition.id,
            definition.owner,
            definition.category,
            definition.maturity,
            self.inputs.preference(id),
            state,
            self.unmet_dependencies(definition),
        ))
    }

    /// Builds a report for every capability in the catalogue, in catalogue
    /// insertion order.
    pub fn catalogue_report(&self) -> Vec<CapabilityReport> {
        self.catalogue
            .definitions()
            .iter()
            .filter_map(|definition| self.report(definition.id))
            .collect()
    }

    /// Builds the downward effective engine-policy snapshot from the resolved
    /// state of every `purr.*` capability, in catalogue insertion order.
    pub fn engine_policy_snapshot(&self) -> EnginePolicySnapshot {
        let entries = self
            .catalogue
            .definitions()
            .iter()
            .filter(|definition| definition.owner == Owner::Purr)
            .filter_map(|definition| {
                let state = self.resolution.state(definition.id)?;
                Some(EngineCapabilityState::new(
                    definition.id,
                    state.availability(),
                    state.reason(),
                    state.authority(),
                ))
            })
            .collect();
        EnginePolicySnapshot::new(entries)
    }

    fn is_available(&self, id: CapabilityId) -> bool {
        matches!(
            self.resolution.state(id),
            Some(state) if state.availability() == Availability::Available
        )
    }

    fn lifecycle_of(&self, id: CapabilityId) -> Lifecycle {
        self.lifecycles
            .get(&id)
            .copied()
            .unwrap_or(Lifecycle::Dormant)
    }

    /// Collects the dependencies of a definition whose resolved availability is
    /// not available. A dependency absent from the catalogue counts as unmet, so
    /// the report stays fail closed.
    fn unmet_dependencies(&self, definition: &CapabilityDefinition) -> Vec<CapabilityId> {
        definition
            .dependencies
            .iter()
            .copied()
            .filter(|&dependency| !self.is_available(dependency))
            .collect()
    }

    fn activate_now(&mut self, id: CapabilityId) {
        self.lifecycles.insert(id, Lifecycle::Starting);
        let outcome = match self.providers.get_mut(&id) {
            Some(provider) => provider.activate(),
            None => Ok(()),
        };
        match outcome {
            Ok(()) => {
                self.failure_counts.insert(id, 0);
                self.lifecycles.insert(id, Lifecycle::Active);
            }
            Err(failure) => self.record_failure(id, failure.category()),
        }
    }

    fn record_failure(&mut self, id: CapabilityId, category: FailureCategory) {
        let threshold = self.failure_threshold;
        let count = {
            let entry = self.failure_counts.entry(id).or_insert(0);
            *entry += 1;
            *entry
        };

        if count >= threshold {
            self.inputs.quarantine(id);
            self.lifecycles
                .insert(id, Lifecycle::Failed(FailureCategory::Quarantined));
            self.reresolve();
        } else {
            self.lifecycles.insert(id, Lifecycle::Failed(category));
        }
    }

    fn reresolve(&mut self) {
        self.resolution = resolve(&self.catalogue, &self.inputs);
    }
}

#[cfg(test)]
mod tests {
    use super::Manager;
    use crate::activation_failure::ActivationFailure;
    use crate::availability::Availability;
    use crate::capability_definition::CapabilityDefinition;
    use crate::capability_id::CapabilityId;
    use crate::capability_provider::CapabilityProvider;
    use crate::capability_request_error::CapabilityRequestError;
    use crate::catalogue::Catalogue;
    use crate::catalogue_builder::CatalogueBuilder;
    use crate::category::Category;
    use crate::deciding_authority::DecidingAuthority;
    use crate::engine_policy_snapshot::EngineCapabilityState;
    use crate::lifecycle::{FailureCategory, Lifecycle};
    use crate::maturity::Maturity;
    use crate::owner::Owner;
    use crate::policy_inputs::{PolicyInputs, UserPreference};
    use crate::reason::Reason;

    const OPTIONAL: CapabilityId = CapabilityId::new("purr.author-styles");
    const MANDATORY: CapabilityId = CapabilityId::new("purr.user-agent-styles");
    const PRODUCT: CapabilityId = CapabilityId::new("panther.reading-mode");
    const NO_DEPENDENCIES: &[CapabilityId] = &[];

    struct SucceedingProvider;

    impl CapabilityProvider for SucceedingProvider {
        fn activate(&mut self) -> Result<(), ActivationFailure> {
            Ok(())
        }

        fn deactivate(&mut self) {}
    }

    struct FailingProvider;

    impl CapabilityProvider for FailingProvider {
        fn activate(&mut self) -> Result<(), ActivationFailure> {
            Err(ActivationFailure::new(
                FailureCategory::ActivationError,
                "mock activation failure",
            ))
        }

        fn deactivate(&mut self) {}
    }

    struct PanicOnActivateProvider;

    impl CapabilityProvider for PanicOnActivateProvider {
        fn activate(&mut self) -> Result<(), ActivationFailure> {
            panic!("provider must not activate while quarantined");
        }

        fn deactivate(&mut self) {}
    }

    /// Provider that rejects a second activation without an intervening
    /// deactivation. A successful re-activation therefore proves the manager
    /// released the provider on deactivation.
    struct AcquireReleaseProvider {
        acquired: bool,
    }

    impl CapabilityProvider for AcquireReleaseProvider {
        fn activate(&mut self) -> Result<(), ActivationFailure> {
            if self.acquired {
                return Err(ActivationFailure::new(
                    FailureCategory::ActivationError,
                    "already acquired",
                ));
            }
            self.acquired = true;
            Ok(())
        }

        fn deactivate(&mut self) {
            self.acquired = false;
        }
    }

    fn definition(id: CapabilityId, is_mandatory: bool) -> CapabilityDefinition {
        CapabilityDefinition {
            id,
            owner: Owner::Purr,
            category: Category::EngineService,
            maturity: Maturity::Stable,
            dependencies: NO_DEPENDENCIES,
            is_mandatory,
            is_built: true,
        }
    }

    fn catalogue() -> Catalogue {
        let mut builder = CatalogueBuilder::new();
        builder.add(definition(OPTIONAL, false));
        builder.add(definition(MANDATORY, true));
        builder.build().expect("the fixture catalogue should build")
    }

    fn supported_inputs() -> PolicyInputs {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(OPTIONAL).mark_supported(MANDATORY);
        inputs
    }

    fn manager() -> Manager {
        Manager::new(catalogue(), supported_inputs())
    }

    fn product_definition(id: CapabilityId) -> CapabilityDefinition {
        CapabilityDefinition {
            id,
            owner: Owner::Panther,
            category: Category::ProductFeature,
            maturity: Maturity::Stable,
            dependencies: NO_DEPENDENCIES,
            is_mandatory: false,
            is_built: true,
        }
    }

    /// Builds a manager whose catalogue mixes `purr.*` and `panther.*`
    /// capabilities, so the engine snapshot filter can be observed.
    fn mixed_manager() -> Manager {
        let mut builder = CatalogueBuilder::new();
        builder
            .add(definition(OPTIONAL, false))
            .add(definition(MANDATORY, true))
            .add(product_definition(PRODUCT));
        let catalogue = builder.build().expect("the fixture catalogue should build");

        let mut inputs = PolicyInputs::new();
        inputs
            .mark_supported(OPTIONAL)
            .mark_supported(MANDATORY)
            .mark_supported(PRODUCT);
        Manager::new(catalogue, inputs)
    }

    #[test]
    fn lazy_activation_moves_dormant_to_active() {
        let mut manager = manager();
        manager.register_provider(OPTIONAL, Box::new(SucceedingProvider));

        assert_eq!(manager.lifecycle(OPTIONAL), Some(Lifecycle::Dormant));

        manager
            .request_activation(OPTIONAL)
            .expect("activation should be accepted");

        assert_eq!(manager.lifecycle(OPTIONAL), Some(Lifecycle::Active));
    }

    #[test]
    fn a_single_failure_below_the_threshold_reaches_failed_without_quarantine() {
        let mut manager = manager();
        manager.set_failure_threshold(2);
        manager.register_provider(OPTIONAL, Box::new(FailingProvider));

        manager
            .request_activation(OPTIONAL)
            .expect("a valid request is accepted even when activation fails");

        assert_eq!(
            manager.lifecycle(OPTIONAL),
            Some(Lifecycle::Failed(FailureCategory::ActivationError))
        );
        let state = manager.effective_state(OPTIONAL).expect("state exists");
        assert_eq!(state.availability(), Availability::Available);
    }

    #[test]
    fn failures_reaching_the_threshold_quarantine_and_clearing_allows_a_retry() {
        let mut manager = manager();
        manager.set_failure_threshold(2);
        manager.register_provider(OPTIONAL, Box::new(FailingProvider));

        manager
            .request_activation(OPTIONAL)
            .expect("first request is accepted");
        assert_eq!(
            manager.lifecycle(OPTIONAL),
            Some(Lifecycle::Failed(FailureCategory::ActivationError))
        );

        manager
            .request_activation(OPTIONAL)
            .expect("second request is accepted");
        let state = manager.effective_state(OPTIONAL).expect("state exists");
        assert_eq!(state.availability(), Availability::Prohibited);
        assert_eq!(manager.lifecycle(OPTIONAL), None);

        manager.register_provider(OPTIONAL, Box::new(PanicOnActivateProvider));
        assert_eq!(
            manager.request_activation(OPTIONAL),
            Err(CapabilityRequestError::NotAvailable(OPTIONAL))
        );

        manager
            .clear_quarantine(OPTIONAL)
            .expect("clearing a known capability succeeds");
        manager.register_provider(OPTIONAL, Box::new(SucceedingProvider));

        manager
            .request_activation(OPTIONAL)
            .expect("the retry is accepted");
        assert_eq!(manager.lifecycle(OPTIONAL), Some(Lifecycle::Active));
    }

    #[test]
    fn deactivation_returns_the_capability_to_dormant() {
        let mut manager = manager();
        manager.register_provider(
            OPTIONAL,
            Box::new(AcquireReleaseProvider { acquired: false }),
        );

        manager
            .request_activation(OPTIONAL)
            .expect("activation should be accepted");
        assert_eq!(manager.lifecycle(OPTIONAL), Some(Lifecycle::Active));

        manager
            .deactivate(OPTIONAL)
            .expect("deactivation should be accepted");
        assert_eq!(manager.lifecycle(OPTIONAL), Some(Lifecycle::Dormant));

        manager
            .request_activation(OPTIONAL)
            .expect("a released provider accepts a second activation");
        assert_eq!(manager.lifecycle(OPTIONAL), Some(Lifecycle::Active));
    }

    #[test]
    fn a_disable_request_on_a_mandatory_capability_is_rejected() {
        let mut manager = manager();

        assert_eq!(
            manager.set_preference(MANDATORY, UserPreference::Disable),
            Err(CapabilityRequestError::MandatoryCapabilityNotDisableable(
                MANDATORY
            ))
        );

        let state = manager.effective_state(MANDATORY).expect("state exists");
        assert_eq!(state.availability(), Availability::Available);
    }

    #[test]
    fn activation_of_a_non_available_capability_is_rejected() {
        let mut manager = manager();
        manager
            .set_preference(OPTIONAL, UserPreference::Disable)
            .expect("disabling an optional capability succeeds");

        manager.register_provider(OPTIONAL, Box::new(PanicOnActivateProvider));

        assert_eq!(
            manager.request_activation(OPTIONAL),
            Err(CapabilityRequestError::NotAvailable(OPTIONAL))
        );
    }

    #[test]
    fn a_report_reflects_resolved_state_and_lifecycle() {
        let mut manager = manager();
        manager.register_provider(OPTIONAL, Box::new(SucceedingProvider));
        manager
            .request_activation(OPTIONAL)
            .expect("activation should be accepted");

        let report = manager.report(OPTIONAL).expect("a report should exist");

        assert_eq!(report.id(), OPTIONAL);
        assert_eq!(report.owner(), Owner::Purr);
        assert_eq!(report.category(), Category::EngineService);
        assert_eq!(report.availability(), Availability::Available);
        assert_eq!(report.reason(), Reason::DefaultAvailable);
        assert_eq!(report.authority(), DecidingAuthority::UserPreference);
        assert_eq!(report.lifecycle(), Some(Lifecycle::Active));
        assert!(report.unmet_dependencies().is_empty());
        assert!(!report.message().is_empty());
    }

    #[test]
    fn the_catalogue_report_lists_every_capability_once() {
        let manager = manager();

        let reports = manager.catalogue_report();

        assert_eq!(reports.len(), 2);
        let ids: Vec<CapabilityId> = reports.iter().map(|report| report.id()).collect();
        assert!(ids.contains(&OPTIONAL));
        assert!(ids.contains(&MANDATORY));
    }

    #[test]
    fn the_engine_snapshot_contains_only_purr_entries() {
        let manager = mixed_manager();

        let snapshot = manager.engine_policy_snapshot();

        assert_eq!(snapshot.entries().len(), 2);
        for entry in snapshot.entries() {
            assert_eq!(entry.id().owner_namespace(), "purr");
            assert_eq!(entry.availability(), Availability::Available);
        }
        assert!(snapshot.get(PRODUCT).is_none());
        assert_eq!(
            snapshot
                .get(OPTIONAL)
                .map(EngineCapabilityState::availability),
            Some(Availability::Available)
        );
    }
}
