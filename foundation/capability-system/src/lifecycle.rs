// @file foundation/capability-system/src/lifecycle.rs
// @description Defines the runtime lifecycle axis and its failure classification.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Reason a capability entered the `Failed` lifecycle state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailureCategory {
    ActivationError,
    Quarantined,
}

/// Runtime lifecycle of an available capability.
///
/// This is the second of the two orthogonal axes. It exists only for an
/// available capability and describes where the capability sits in its runtime
/// cycle. `Failed` carries the classification of the failure.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lifecycle {
    Dormant,
    Starting,
    Active,
    Deactivating,
    Failed(FailureCategory),
}
