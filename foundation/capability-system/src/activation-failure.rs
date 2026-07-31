// @file foundation/capability-system/src/activation-failure.rs
// @description Defines the typed failure a provider returns from activation.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::lifecycle::FailureCategory;

/// Failure a [`crate::CapabilityProvider`] reports when activation cannot
/// complete.
///
/// The message is a static, safe, factual string. A provider translates its
/// internal failure into one of these messages at its own boundary, so no raw
/// dependency error and no secret ever reaches this type.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ActivationFailure {
    category: FailureCategory,
    message: &'static str,
}

impl ActivationFailure {
    pub const fn new(category: FailureCategory, message: &'static str) -> Self {
        Self { category, message }
    }

    pub const fn category(&self) -> FailureCategory {
        self.category
    }

    pub const fn message(&self) -> &'static str {
        self.message
    }
}
