// @file foundation/capability-system/src/policy-inputs.rs
// @description Defines the in-memory policy inputs the resolver reads.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::{HashMap, HashSet};

use crate::CapabilityId;

/// User or profile preference for one capability.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserPreference {
    Enable,
    Disable,
}

/// In-memory, program-controlled inputs for one resolution.
///
/// The resolver reads these inputs together with each definition. Build
/// availability is not held here; it comes from the definition `is_built` flag.
///
/// Platform support is fail closed: a capability counts as supported only when a
/// support probe recorded it in [`PolicyInputs::mark_supported`]. An absent
/// support result means unsupported, so a missing probe never makes a capability
/// available.
#[derive(Debug, Default)]
pub struct PolicyInputs {
    supported: HashSet<CapabilityId>,
    safe_mode: bool,
    experiments_enabled: HashSet<CapabilityId>,
    preferences: HashMap<CapabilityId, UserPreference>,
    quarantined: HashSet<CapabilityId>,
}

impl PolicyInputs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_supported(&mut self, id: CapabilityId) -> &mut Self {
        self.supported.insert(id);
        self
    }

    pub fn set_safe_mode(&mut self, value: bool) -> &mut Self {
        self.safe_mode = value;
        self
    }

    pub fn enable_experiment(&mut self, id: CapabilityId) -> &mut Self {
        self.experiments_enabled.insert(id);
        self
    }

    pub fn set_preference(&mut self, id: CapabilityId, preference: UserPreference) -> &mut Self {
        self.preferences.insert(id, preference);
        self
    }

    pub fn quarantine(&mut self, id: CapabilityId) -> &mut Self {
        self.quarantined.insert(id);
        self
    }

    pub fn is_supported(&self, id: CapabilityId) -> bool {
        self.supported.contains(&id)
    }

    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    pub fn is_experiment_enabled(&self, id: CapabilityId) -> bool {
        self.experiments_enabled.contains(&id)
    }

    pub fn preference(&self, id: CapabilityId) -> Option<UserPreference> {
        self.preferences.get(&id).copied()
    }

    pub fn is_quarantined(&self, id: CapabilityId) -> bool {
        self.quarantined.contains(&id)
    }
}
