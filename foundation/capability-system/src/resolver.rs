// @file foundation/capability-system/src/resolver.rs
// @description Ordered resolver that turns definitions and policy inputs into effective states.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::HashMap;

use crate::availability::{Availability, UnavailableStatus};
use crate::capability_definition::CapabilityDefinition;
use crate::capability_id::CapabilityId;
use crate::catalogue::Catalogue;
use crate::deciding_authority::DecidingAuthority;
use crate::effective_state::EffectiveState;
use crate::lifecycle::Lifecycle;
use crate::maturity::Maturity;
use crate::policy_inputs::{PolicyInputs, UserPreference};
use crate::reason::Reason;

/// Resolved effective state for every capability in a catalogue.
///
/// A resolution is produced once per policy snapshot. Every identifier the
/// catalogue holds has exactly one effective state. An identifier absent from
/// the catalogue has no state, which keeps an unknown identifier fail closed:
/// it is never reported as available.
#[derive(Debug)]
pub struct Resolution {
    states: HashMap<CapabilityId, EffectiveState>,
}

impl Resolution {
    pub fn state(&self, id: CapabilityId) -> Option<EffectiveState> {
        self.states.get(&id).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (CapabilityId, EffectiveState)> + '_ {
        self.states.iter().map(|(&id, &state)| (id, state))
    }
}

/// Resolves every capability in the catalogue against the policy inputs.
///
/// Dependencies resolve before dependents, so a dependent can read the resolved
/// state of each dependency. The catalogue builder already rejected dependency
/// cycles, so the recursive resolution always terminates. Each capability
/// resolves once; the per-capability layer pass is O(layers) and allocation
/// free.
pub fn resolve(catalogue: &Catalogue, inputs: &PolicyInputs) -> Resolution {
    let mut states = HashMap::with_capacity(catalogue.definitions().len());
    for definition in catalogue.definitions() {
        resolve_into(definition.id, catalogue, inputs, &mut states);
    }
    Resolution { states }
}

/// Resolves one identifier and its transitive dependencies into the map.
///
/// A dependency that the catalogue does not hold counts as not available, which
/// keeps the dependent fail closed.
fn resolve_into(
    id: CapabilityId,
    catalogue: &Catalogue,
    inputs: &PolicyInputs,
    states: &mut HashMap<CapabilityId, EffectiveState>,
) {
    if states.contains_key(&id) {
        return;
    }

    let Some(definition) = catalogue.get(id) else {
        return;
    };

    let mut all_dependencies_available = true;
    for &dependency in definition.dependencies {
        resolve_into(dependency, catalogue, inputs, states);
        let available = match states.get(&dependency) {
            Some(state) => state.availability() == Availability::Available,
            None => false,
        };
        if !available {
            all_dependencies_available = false;
        }
    }

    let base = resolve_layers(definition, inputs);
    let state = apply_dependency_check(definition, base, all_dependencies_available);
    states.insert(id, state);
}

/// Applies the ordered seven-layer pipeline to one capability.
///
/// The first layer that decides a state wins. Build availability and platform
/// support are hard floors: no later layer can lift them. A mandatory capability
/// locks to available before any discretionary layer can disable it. The
/// discretionary layers below the lock can only narrow toward not available, so
/// the security invariant holds.
fn resolve_layers(definition: &CapabilityDefinition, inputs: &PolicyInputs) -> EffectiveState {
    if !definition.is_built {
        return unavailable(
            UnavailableStatus::NotBuilt,
            Reason::NotCompiledIn,
            DecidingAuthority::BuildAvailability,
        );
    }

    if !inputs.is_supported(definition.id) {
        return unavailable(
            UnavailableStatus::Unsupported,
            Reason::PlatformUnsupported,
            DecidingAuthority::PlatformSupport,
        );
    }

    if definition.is_mandatory {
        return EffectiveState::Available {
            reason: Reason::MandatorySecurity,
            authority: DecidingAuthority::MandatorySecurity,
            lifecycle: Lifecycle::Dormant,
        };
    }

    // Deferred insertion point: enterprise or managed policy (research layer 4).

    if inputs.safe_mode() {
        return unavailable(
            UnavailableStatus::Disabled,
            Reason::SafeMode,
            DecidingAuthority::SafeMode,
        );
    }

    // Deferred insertion point: product channel policy (research layer 6). The
    // experiment part of that research layer is the maturity gating below.

    if definition.maturity == Maturity::Experimental && !inputs.is_experiment_enabled(definition.id)
    {
        return unavailable(
            UnavailableStatus::Disabled,
            Reason::ExperimentGated,
            DecidingAuthority::MaturityGating,
        );
    }

    let preference = inputs.preference(definition.id);
    match preference {
        Some(UserPreference::Disable) => {
            return unavailable(
                UnavailableStatus::Disabled,
                Reason::UserDisabled,
                DecidingAuthority::UserPreference,
            );
        }
        Some(UserPreference::Enable) | None => {}
    }

    // Deferred insertion points: site and origin setting (research layer 8),
    // document Permissions Policy (research layer 9), and per-site user
    // permission (research layer 10).

    if inputs.is_quarantined(definition.id) {
        return unavailable(
            UnavailableStatus::Prohibited,
            Reason::QuarantinedAfterFailure,
            DecidingAuthority::RuntimeHealth,
        );
    }

    let reason = match preference {
        Some(UserPreference::Enable) => Reason::UserEnabled,
        Some(UserPreference::Disable) | None => Reason::DefaultAvailable,
    };
    EffectiveState::Available {
        reason,
        authority: DecidingAuthority::UserPreference,
        lifecycle: Lifecycle::Dormant,
    }
}

/// Overrides an available result when a dependency is not available.
///
/// The dependency check is a lower authority than the mandatory-security lock,
/// so it never lowers a mandatory capability. It only overrides an available
/// result of a non-mandatory capability; a result that is already not available
/// keeps its own deciding layer.
fn apply_dependency_check(
    definition: &CapabilityDefinition,
    base: EffectiveState,
    all_dependencies_available: bool,
) -> EffectiveState {
    if definition.is_mandatory {
        return base;
    }

    match base {
        EffectiveState::Available { .. } => {
            if all_dependencies_available {
                base
            } else {
                unavailable(
                    UnavailableStatus::Disabled,
                    Reason::DependencyUnmet,
                    DecidingAuthority::DependencyCheck,
                )
            }
        }
        EffectiveState::Unavailable { .. } => base,
    }
}

const fn unavailable(
    status: UnavailableStatus,
    reason: Reason,
    authority: DecidingAuthority,
) -> EffectiveState {
    EffectiveState::Unavailable {
        status,
        reason,
        authority,
    }
}

#[cfg(test)]
mod tests {
    use super::{Resolution, resolve};
    use crate::availability::Availability;
    use crate::capability_definition::CapabilityDefinition;
    use crate::capability_id::CapabilityId;
    use crate::catalogue::Catalogue;
    use crate::catalogue_builder::CatalogueBuilder;
    use crate::category::Category;
    use crate::deciding_authority::DecidingAuthority;
    use crate::maturity::Maturity;
    use crate::owner::Owner;
    use crate::policy_inputs::{PolicyInputs, UserPreference};
    use crate::reason::Reason;

    const CAPABILITY: CapabilityId = CapabilityId::new("purr.author-styles");
    const MANDATORY: CapabilityId = CapabilityId::new("purr.user-agent-styles");
    const DEPENDENCY: CapabilityId = CapabilityId::new("purr.style-engine");
    const DEPENDENT: CapabilityId = CapabilityId::new("purr.author-styles");

    const NO_DEPENDENCIES: &[CapabilityId] = &[];
    const ON_DEPENDENCY: &[CapabilityId] = &[DEPENDENCY];

    struct Fixture {
        id: CapabilityId,
        maturity: Maturity,
        dependencies: &'static [CapabilityId],
        is_mandatory: bool,
        is_built: bool,
    }

    impl Fixture {
        fn new(id: CapabilityId) -> Self {
            Self {
                id,
                maturity: Maturity::Stable,
                dependencies: NO_DEPENDENCIES,
                is_mandatory: false,
                is_built: true,
            }
        }

        fn mandatory(mut self) -> Self {
            self.is_mandatory = true;
            self
        }

        fn experimental(mut self) -> Self {
            self.maturity = Maturity::Experimental;
            self
        }

        fn not_built(mut self) -> Self {
            self.is_built = false;
            self
        }

        fn depending_on(mut self, dependencies: &'static [CapabilityId]) -> Self {
            self.dependencies = dependencies;
            self
        }

        fn definition(&self) -> CapabilityDefinition {
            CapabilityDefinition {
                id: self.id,
                owner: Owner::Purr,
                category: Category::EngineService,
                maturity: self.maturity,
                dependencies: self.dependencies,
                is_mandatory: self.is_mandatory,
                is_built: self.is_built,
            }
        }
    }

    fn catalogue_of(fixtures: &[Fixture]) -> Catalogue {
        let mut builder = CatalogueBuilder::new();
        for fixture in fixtures {
            builder.add(fixture.definition());
        }
        builder.build().expect("the fixture catalogue should build")
    }

    fn resolve_single(fixture: Fixture, inputs: &PolicyInputs) -> Resolution {
        let catalogue = catalogue_of(&[fixture]);
        resolve(&catalogue, inputs)
    }

    #[test]
    fn built_and_supported_default_capability_is_available() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(CAPABILITY);

        let resolution = resolve_single(Fixture::new(CAPABILITY), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Available);
        assert_eq!(state.reason(), Reason::DefaultAvailable);
        assert_eq!(state.authority(), DecidingAuthority::UserPreference);
    }

    #[test]
    fn not_built_capability_resolves_to_not_built() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(CAPABILITY);

        let resolution = resolve_single(Fixture::new(CAPABILITY).not_built(), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::NotBuilt);
        assert_eq!(state.authority(), DecidingAuthority::BuildAvailability);
    }

    #[test]
    fn unsupported_capability_resolves_to_unsupported() {
        let inputs = PolicyInputs::new();

        let resolution = resolve_single(Fixture::new(CAPABILITY), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Unsupported);
        assert_eq!(state.authority(), DecidingAuthority::PlatformSupport);
    }

    #[test]
    fn mandatory_capability_stays_available_when_the_user_disables_it() {
        let mut inputs = PolicyInputs::new();
        inputs
            .mark_supported(MANDATORY)
            .set_preference(MANDATORY, UserPreference::Disable)
            .set_safe_mode(true);

        let resolution = resolve_single(Fixture::new(MANDATORY).mandatory(), &inputs);
        let state = resolution.state(MANDATORY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Available);
        assert_eq!(state.reason(), Reason::MandatorySecurity);
        assert_eq!(state.authority(), DecidingAuthority::MandatorySecurity);
    }

    #[test]
    fn experimental_capability_is_disabled_without_an_experiment() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(CAPABILITY);

        let resolution = resolve_single(Fixture::new(CAPABILITY).experimental(), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Disabled);
        assert_eq!(state.reason(), Reason::ExperimentGated);
        assert_eq!(state.authority(), DecidingAuthority::MaturityGating);
    }

    #[test]
    fn experimental_capability_is_available_with_an_experiment() {
        let mut inputs = PolicyInputs::new();
        inputs
            .mark_supported(CAPABILITY)
            .enable_experiment(CAPABILITY);

        let resolution = resolve_single(Fixture::new(CAPABILITY).experimental(), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Available);
    }

    #[test]
    fn safe_mode_disables_a_non_mandatory_capability() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(CAPABILITY).set_safe_mode(true);

        let resolution = resolve_single(Fixture::new(CAPABILITY), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Disabled);
        assert_eq!(state.reason(), Reason::SafeMode);
        assert_eq!(state.authority(), DecidingAuthority::SafeMode);
    }

    #[test]
    fn quarantined_capability_is_prohibited() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(CAPABILITY).quarantine(CAPABILITY);

        let resolution = resolve_single(Fixture::new(CAPABILITY), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Prohibited);
        assert_eq!(state.reason(), Reason::QuarantinedAfterFailure);
        assert_eq!(state.authority(), DecidingAuthority::RuntimeHealth);
    }

    #[test]
    fn user_disabled_capability_is_disabled() {
        let mut inputs = PolicyInputs::new();
        inputs
            .mark_supported(CAPABILITY)
            .set_preference(CAPABILITY, UserPreference::Disable);

        let resolution = resolve_single(Fixture::new(CAPABILITY), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_eq!(state.availability(), Availability::Disabled);
        assert_eq!(state.reason(), Reason::UserDisabled);
        assert_eq!(state.authority(), DecidingAuthority::UserPreference);
    }

    #[test]
    fn dependent_with_an_unavailable_dependency_reports_dependency_unmet() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(DEPENDENT);

        let catalogue = catalogue_of(&[
            Fixture::new(DEPENDENCY),
            Fixture::new(DEPENDENT).depending_on(ON_DEPENDENCY),
        ]);
        let resolution = resolve(&catalogue, &inputs);

        let dependency = resolution.state(DEPENDENCY).expect("state should exist");
        assert_eq!(dependency.availability(), Availability::Unsupported);

        let dependent = resolution.state(DEPENDENT).expect("state should exist");
        assert_eq!(dependent.availability(), Availability::Disabled);
        assert_eq!(dependent.reason(), Reason::DependencyUnmet);
        assert_eq!(dependent.authority(), DecidingAuthority::DependencyCheck);
    }

    #[test]
    fn dependent_with_an_available_dependency_is_available() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(DEPENDENCY).mark_supported(DEPENDENT);

        let catalogue = catalogue_of(&[
            Fixture::new(DEPENDENCY),
            Fixture::new(DEPENDENT).depending_on(ON_DEPENDENCY),
        ]);
        let resolution = resolve(&catalogue, &inputs);

        let dependent = resolution.state(DEPENDENT).expect("state should exist");
        assert_eq!(dependent.availability(), Availability::Available);
    }

    #[test]
    fn unknown_identifier_has_no_state() {
        let mut inputs = PolicyInputs::new();
        inputs.mark_supported(CAPABILITY);

        let resolution = resolve_single(Fixture::new(CAPABILITY), &inputs);

        assert!(resolution.state(MANDATORY).is_none());
    }

    #[test]
    fn unresolved_support_never_yields_available() {
        let inputs = PolicyInputs::new();

        let resolution = resolve_single(Fixture::new(CAPABILITY), &inputs);
        let state = resolution.state(CAPABILITY).expect("state should exist");

        assert_ne!(state.availability(), Availability::Available);
    }
}
