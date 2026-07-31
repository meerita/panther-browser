// @file foundation/capability-system/src/capability-definition.rs
// @description Defines the static schema of one capability.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::{CapabilityId, Category, Maturity, Owner};

/// Static description of one capability.
///
/// A definition is declaration metadata. It is declared locally as a constant
/// and carries no runtime state. The owner is stored next to the identifier so
/// the catalogue builder can reject an identifier whose namespace does not match
/// its owner. Dependencies reference other capabilities by identifier; the
/// builder validates that every referenced identifier exists and that the
/// dependency graph is acyclic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityDefinition {
    pub id: CapabilityId,
    pub owner: Owner,
    pub category: Category,
    pub maturity: Maturity,
    pub dependencies: &'static [CapabilityId],
    /// A mandatory capability is non-disableable. A disable attempt from a
    /// lower authority is rejected during resolution.
    pub is_mandatory: bool,
    /// Build availability metadata. A capability that is not built is absent
    /// because its code was not compiled in. This is a metadata flag, not a
    /// Cargo feature.
    pub is_built: bool,
}
