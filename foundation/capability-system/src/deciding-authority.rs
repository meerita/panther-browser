// @file foundation/capability-system/src/deciding-authority.rs
// @description Defines the resolver layer that decided an effective state.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Resolver layer that decided the effective state of a capability.
///
/// Each variant names one layer of the ordered resolver pipeline, plus the
/// dependency check that runs across the pipeline result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecidingAuthority {
    BuildAvailability,
    PlatformSupport,
    MandatorySecurity,
    SafeMode,
    MaturityGating,
    UserPreference,
    RuntimeHealth,
    DependencyCheck,
}
