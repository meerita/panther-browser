// @file foundation/capability-system/src/catalogue-build-error.rs
// @description Defines the typed errors that reject a malformed catalogue.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::CapabilityId;

/// Reason a catalogue failed validation during construction.
///
/// Each variant carries only local capability identifiers, never raw external
/// data. The builder returns the first violation it finds and never produces a
/// partially accepted catalogue.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CatalogueBuildError {
    #[error("duplicate capability id: {}", .0.as_str())]
    DuplicateId(CapabilityId),

    #[error("capability id namespace does not match its owner: {}", .0.as_str())]
    NamespaceOwnerMismatch(CapabilityId),

    #[error(
        "capability {} depends on unknown capability {}",
        .dependent.as_str(),
        .dependency.as_str()
    )]
    UnknownDependency {
        dependent: CapabilityId,
        dependency: CapabilityId,
    },

    #[error("dependency cycle involving capability: {}", .0.as_str())]
    DependencyCycle(CapabilityId),
}
