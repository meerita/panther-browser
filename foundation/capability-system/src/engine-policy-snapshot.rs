// @file foundation/capability-system/src/engine-policy-snapshot.rs
// @description Defines the downward effective engine-policy snapshot.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::availability::Availability;
use crate::capability_id::CapabilityId;
use crate::deciding_authority::DecidingAuthority;
use crate::reason::Reason;

/// Effective policy of one engine capability in the downward snapshot.
///
/// The entry carries the resolved availability plus the reason and authority
/// that decided it, all shared vocabulary types. It holds no runtime lifecycle,
/// because the snapshot describes the policy decision the embedder passes down,
/// not the runtime cycle the owner manages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineCapabilityState {
    id: CapabilityId,
    availability: Availability,
    reason: Reason,
    authority: DecidingAuthority,
}

impl EngineCapabilityState {
    pub(crate) fn new(
        id: CapabilityId,
        availability: Availability,
        reason: Reason,
        authority: DecidingAuthority,
    ) -> Self {
        Self {
            id,
            availability,
            reason,
            authority,
        }
    }

    pub fn id(&self) -> CapabilityId {
        self.id
    }

    pub fn availability(&self) -> Availability {
        self.availability
    }

    pub fn reason(&self) -> Reason {
        self.reason
    }

    pub fn authority(&self) -> DecidingAuthority {
        self.authority
    }
}

/// Effective engine-policy snapshot the embedder passes down to the engine.
///
/// The snapshot lists every `purr.*` capability with its resolved availability
/// and the reason and authority that decided it. It is built from already
/// resolved state and uses shared vocabulary types only, so no product or engine
/// policy type crosses the boundary. Entries keep catalogue insertion order for
/// deterministic diagnostics. The shape is a plain list of records, so a future
/// out-of-process split can serialize it without reshaping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnginePolicySnapshot {
    entries: Vec<EngineCapabilityState>,
}

impl EnginePolicySnapshot {
    pub(crate) fn new(entries: Vec<EngineCapabilityState>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[EngineCapabilityState] {
        &self.entries
    }

    /// Returns the entry for an identifier, or `None` when the snapshot does not
    /// hold it.
    pub fn get(&self, id: CapabilityId) -> Option<&EngineCapabilityState> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}
