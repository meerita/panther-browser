// @file foundation/capability-system/src/reason.rs
// @description Defines resolution reasons and their stable diagnostic codes.
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

/// Returns the stable, language-neutral code for a reason.
///
/// The code is a deliberate cross-boundary contract, not the Rust variant name.
/// It stays canonical for logs, telemetry, and support, and it is never
/// localized. Presentation code turns the code and its typed reason into text at
/// the localization boundary. The function is `const` and allocation-free so it
/// stays cheap on diagnostic paths.
pub const fn reason_code(reason: Reason) -> &'static str {
    match reason {
        Reason::NotCompiledIn => "CAP_REASON_NOT_COMPILED_IN",
        Reason::PlatformUnsupported => "CAP_REASON_PLATFORM_UNSUPPORTED",
        Reason::MandatorySecurity => "CAP_REASON_MANDATORY_SECURITY",
        Reason::SafeMode => "CAP_REASON_SAFE_MODE",
        Reason::ExperimentGated => "CAP_REASON_EXPERIMENT_GATED",
        Reason::UserDisabled => "CAP_REASON_USER_DISABLED",
        Reason::UserEnabled => "CAP_REASON_USER_ENABLED",
        Reason::DependencyUnmet => "CAP_REASON_DEPENDENCY_UNMET",
        Reason::QuarantinedAfterFailure => "CAP_REASON_QUARANTINED_AFTER_FAILURE",
        Reason::DefaultAvailable => "CAP_REASON_DEFAULT_AVAILABLE",
    }
}

#[cfg(test)]
mod tests {
    use super::{Reason, reason_code};

    const CODES: [(Reason, &str); 10] = [
        (Reason::NotCompiledIn, "CAP_REASON_NOT_COMPILED_IN"),
        (
            Reason::PlatformUnsupported,
            "CAP_REASON_PLATFORM_UNSUPPORTED",
        ),
        (Reason::MandatorySecurity, "CAP_REASON_MANDATORY_SECURITY"),
        (Reason::SafeMode, "CAP_REASON_SAFE_MODE"),
        (Reason::ExperimentGated, "CAP_REASON_EXPERIMENT_GATED"),
        (Reason::UserDisabled, "CAP_REASON_USER_DISABLED"),
        (Reason::UserEnabled, "CAP_REASON_USER_ENABLED"),
        (Reason::DependencyUnmet, "CAP_REASON_DEPENDENCY_UNMET"),
        (
            Reason::QuarantinedAfterFailure,
            "CAP_REASON_QUARANTINED_AFTER_FAILURE",
        ),
        (Reason::DefaultAvailable, "CAP_REASON_DEFAULT_AVAILABLE"),
    ];

    #[test]
    fn every_reason_maps_to_its_stable_code() {
        for (reason, code) in CODES {
            assert_eq!(reason_code(reason), code);
        }
    }
}
