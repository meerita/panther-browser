// @file foundation/capability-system/src/reason.rs
// @description Defines resolution reasons and their diagnostic messages.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Deciding cause of an effective state.
///
/// Each variant names one cause that a resolver layer can assign to a
/// capability. The cause is reported alongside the deciding authority.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    NotCompiledIn,
    PlatformUnsupported,
    MandatorySecurity,
    SafeMode,
    ExperimentGated,
    UserDisabled,
    UserEnabled,
    DependencyUnmet,
    QuarantinedAfterFailure,
    DefaultAvailable,
}

/// Returns a short factual English sentence for a reason.
///
/// The text is static and allocation-free so it stays cheap on diagnostic
/// paths. It describes the cause only and adds no capability-specific context.
pub const fn reason_message(reason: Reason) -> &'static str {
    match reason {
        Reason::NotCompiledIn => "The capability is not included in this build.",
        Reason::PlatformUnsupported => "The platform does not support the capability.",
        Reason::MandatorySecurity => "The capability is mandatory and cannot be turned off.",
        Reason::SafeMode => "Safe mode turned the capability off.",
        Reason::ExperimentGated => "The capability is experimental and no experiment enabled it.",
        Reason::UserDisabled => "The user turned the capability off.",
        Reason::UserEnabled => "The user turned the capability on.",
        Reason::DependencyUnmet => "A required capability is not available.",
        Reason::QuarantinedAfterFailure => {
            "The capability was quarantined after repeated failures."
        }
        Reason::DefaultAvailable => "The capability is available by default.",
    }
}

#[cfg(test)]
mod tests {
    use super::{Reason, reason_message};

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
    fn every_reason_has_a_non_empty_message() {
        for reason in ALL_REASONS {
            assert!(!reason_message(reason).is_empty());
        }
    }
}
