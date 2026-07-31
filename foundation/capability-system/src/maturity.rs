// @file foundation/capability-system/src/maturity.rs
// @description Defines the capability maturity level.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Maturity level of a capability.
///
/// Only the distinction between `Experimental` and `Stable` drives behaviour in
/// M0: an experimental capability is gated unless an experiment enables it. The
/// other levels are metadata for reports and future policy.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Maturity {
    Experimental,
    Preview,
    Stable,
    Deprecated,
}
