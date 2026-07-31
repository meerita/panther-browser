// @file foundation/capability-system/src/capability-provider.rs
// @description Defines the provider contract that attaches behavior to a capability.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::activation_failure::ActivationFailure;

/// Behavior attached to one capability.
///
/// The manager owns the lifecycle; a provider owns resource acquisition and
/// release. Both methods are synchronous. `activate` is the only fallible one;
/// its `Result` and the separate `Starting` lifecycle state model the shape an
/// asynchronous activation would take later without committing to async now.
pub trait CapabilityProvider {
    /// Acquires the resources the capability needs. The manager calls this on
    /// demand only while the capability is available. A failure moves the
    /// capability to the failed lifecycle state and, once the failure threshold
    /// is reached, to quarantine.
    fn activate(&mut self) -> Result<(), ActivationFailure>;

    /// Releases the resources acquired by `activate`. The manager calls this
    /// when it deactivates an active capability. Release does not fail.
    fn deactivate(&mut self);
}
