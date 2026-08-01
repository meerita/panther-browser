// @file products/panther/localization/src/capability-reason-adapter.rs
// @description Maps a capability reason to its localized message key.
// @created Diego Martín Lafuente <meerita@icloud.com>

use capability_system::Reason;

use crate::message_adapter::MessageAdapter;

/// The capability system reports a typed [`Reason`] and a stable code, never
/// prose. This adapter is the presentation boundary that turns a reason into a
/// `capability-reason-*` message id. The match is exhaustive with no wildcard so
/// a new [`Reason`] variant forces this mapping to change. The messages take no
/// arguments, so no capability-specific or sensitive value is interpolated.
impl MessageAdapter for Reason {
    fn message_id(&self) -> &'static str {
        match self {
            Reason::NotCompiledIn => "capability-reason-not-compiled-in",
            Reason::PlatformUnsupported => "capability-reason-platform-unsupported",
            Reason::MandatorySecurity => "capability-reason-mandatory-security",
            Reason::SafeMode => "capability-reason-safe-mode",
            Reason::ExperimentGated => "capability-reason-experiment-gated",
            Reason::UserDisabled => "capability-reason-user-disabled",
            Reason::UserEnabled => "capability-reason-user-enabled",
            Reason::DependencyUnmet => "capability-reason-dependency-unmet",
            Reason::QuarantinedAfterFailure => "capability-reason-quarantined-after-failure",
            Reason::DefaultAvailable => "capability-reason-default-available",
        }
    }
}

#[cfg(test)]
mod tests {
    use capability_system::Reason;

    use crate::message_adapter::MessageAdapter;
    use crate::message_catalog::MessageCatalog;

    const ALL_REASONS: [Reason; 10] = [
        Reason::NotCompiledIn,
        Reason::PlatformUnsupported,
        Reason::MandatorySecurity,
        Reason::SafeMode,
        Reason::ExperimentGated,
        Reason::UserDisabled,
        Reason::UserEnabled,
        Reason::DependencyUnmet,
        Reason::QuarantinedAfterFailure,
        Reason::DefaultAvailable,
    ];

    #[test]
    fn every_reason_maps_to_a_capability_reason_key() {
        for reason in ALL_REASONS {
            let id = reason.message_id();
            assert!(id.starts_with("capability-reason-"));
            assert!(reason.arguments().entries().is_empty());
        }
    }

    #[test]
    fn every_reason_resolves_to_a_localized_message() {
        let catalog = MessageCatalog::load();
        for reason in ALL_REASONS {
            let id = reason.message_id();
            let message = catalog.message_with_arguments(id, &reason.arguments());
            assert_ne!(
                message.text(),
                id,
                "reason {reason:?} must resolve to prose, not fall back to its id"
            );
            assert!(!message.text().is_empty());
        }
    }
}
