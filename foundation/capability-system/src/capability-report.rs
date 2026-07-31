// @file foundation/capability-system/src/capability-report.rs
// @description Defines the read-only per-capability diagnostics report.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::availability::Availability;
use crate::capability_id::CapabilityId;
use crate::category::Category;
use crate::deciding_authority::DecidingAuthority;
use crate::effective_state::EffectiveState;
use crate::lifecycle::{FailureCategory, Lifecycle};
use crate::maturity::Maturity;
use crate::owner::Owner;
use crate::policy_inputs::UserPreference;
use crate::reason::{Reason, reason_message};

/// Read-only diagnostic view of one capability.
///
/// A report captures the declaration metadata, the resolved effective state, the
/// preference the user requested, and the dependencies that are not available.
/// [`crate::Manager`] builds it from already resolved state, so it runs no
/// resolution and keeps no reference to the manager. Availability, reason,
/// authority, and lifecycle come from one [`EffectiveState`], which keeps the two
/// state axes orthogonal, so the report cannot present a lifecycle for an
/// unavailable capability. The struct stays extensible for later fields such as
/// retained resources or host-process attribution without reshaping the API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityReport {
    id: CapabilityId,
    owner: Owner,
    category: Category,
    maturity: Maturity,
    requested_preference: Option<UserPreference>,
    state: EffectiveState,
    unmet_dependencies: Vec<CapabilityId>,
}

impl CapabilityReport {
    pub(crate) fn new(
        id: CapabilityId,
        owner: Owner,
        category: Category,
        maturity: Maturity,
        requested_preference: Option<UserPreference>,
        state: EffectiveState,
        unmet_dependencies: Vec<CapabilityId>,
    ) -> Self {
        Self {
            id,
            owner,
            category,
            maturity,
            requested_preference,
            state,
            unmet_dependencies,
        }
    }

    pub fn id(&self) -> CapabilityId {
        self.id
    }

    pub fn owner(&self) -> Owner {
        self.owner
    }

    pub fn category(&self) -> Category {
        self.category
    }

    pub fn maturity(&self) -> Maturity {
        self.maturity
    }

    /// Returns the preference the user requested, or `None` when the user
    /// expressed none.
    pub fn requested_preference(&self) -> Option<UserPreference> {
        self.requested_preference
    }

    pub fn availability(&self) -> Availability {
        self.state.availability()
    }

    pub fn reason(&self) -> Reason {
        self.state.reason()
    }

    pub fn authority(&self) -> DecidingAuthority {
        self.state.authority()
    }

    /// Returns the runtime lifecycle when the capability is available, otherwise
    /// `None`.
    pub fn lifecycle(&self) -> Option<Lifecycle> {
        self.state.lifecycle()
    }

    /// Returns the runtime failure classification when the capability failed to
    /// activate. A quarantine that removes availability appears instead as a
    /// `Prohibited` availability with the `QuarantinedAfterFailure` reason.
    pub fn runtime_failure(&self) -> Option<FailureCategory> {
        match self.state.lifecycle() {
            Some(Lifecycle::Failed(category)) => Some(category),
            _ => None,
        }
    }

    /// Returns the dependencies that are not available.
    pub fn unmet_dependencies(&self) -> &[CapabilityId] {
        &self.unmet_dependencies
    }

    /// Returns the derived, human-readable explanation of the deciding reason.
    pub fn message(&self) -> &'static str {
        reason_message(self.state.reason())
    }
}
