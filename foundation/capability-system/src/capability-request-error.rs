// @file foundation/capability-system/src/capability-request-error.rs
// @description Defines the typed error an explicit capability request can return.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::capability_id::CapabilityId;

/// Reason an explicit request to the manager is rejected.
///
/// Resolution itself never returns an error; only an explicit request does. The
/// identifier is a static, non-secret capability name, so it is safe to carry
/// for diagnosis.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityRequestError {
    #[error("unknown capability: {}", .0.as_str())]
    UnknownCapability(CapabilityId),
    #[error("capability is not available: {}", .0.as_str())]
    NotAvailable(CapabilityId),
    #[error("mandatory capability cannot be disabled: {}", .0.as_str())]
    MandatoryCapabilityNotDisableable(CapabilityId),
}
