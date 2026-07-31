// @file foundation/capability-system/src/effective-state.rs
// @description Defines the two-axis effective state and its accessors.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::availability::{Availability, UnavailableStatus};
use crate::deciding_authority::DecidingAuthority;
use crate::lifecycle::Lifecycle;
use crate::reason::Reason;

/// Resolved state of a capability across both orthogonal axes.
///
/// The two shapes keep the axes orthogonal. The unavailable shape holds an
/// [`UnavailableStatus`], so an unavailable capability cannot carry a
/// lifecycle. The available shape always carries a lifecycle. This makes an
/// illegal axis combination unrepresentable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectiveState {
    Unavailable {
        status: UnavailableStatus,
        reason: Reason,
        authority: DecidingAuthority,
    },
    Available {
        reason: Reason,
        authority: DecidingAuthority,
        lifecycle: Lifecycle,
    },
}

impl EffectiveState {
    pub fn availability(&self) -> Availability {
        match self {
            Self::Unavailable { status, .. } => status.availability(),
            Self::Available { .. } => Availability::Available,
        }
    }

    pub fn reason(&self) -> Reason {
        match self {
            Self::Unavailable { reason, .. } | Self::Available { reason, .. } => *reason,
        }
    }

    pub fn authority(&self) -> DecidingAuthority {
        match self {
            Self::Unavailable { authority, .. } | Self::Available { authority, .. } => *authority,
        }
    }

    /// Returns the lifecycle when the capability is available, otherwise `None`.
    pub fn lifecycle(&self) -> Option<Lifecycle> {
        match self {
            Self::Available { lifecycle, .. } => Some(*lifecycle),
            Self::Unavailable { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EffectiveState;
    use crate::availability::{Availability, UnavailableStatus};
    use crate::deciding_authority::DecidingAuthority;
    use crate::lifecycle::Lifecycle;
    use crate::reason::Reason;

    #[test]
    fn unavailable_state_never_carries_a_lifecycle() {
        let state = EffectiveState::Unavailable {
            status: UnavailableStatus::NotBuilt,
            reason: Reason::NotCompiledIn,
            authority: DecidingAuthority::BuildAvailability,
        };
        assert_eq!(state.availability(), Availability::NotBuilt);
        assert_eq!(state.reason(), Reason::NotCompiledIn);
        assert_eq!(state.authority(), DecidingAuthority::BuildAvailability);
        assert_eq!(state.lifecycle(), None);
    }

    #[test]
    fn available_state_always_carries_a_lifecycle() {
        let state = EffectiveState::Available {
            reason: Reason::DefaultAvailable,
            authority: DecidingAuthority::UserPreference,
            lifecycle: Lifecycle::Dormant,
        };
        assert_eq!(state.availability(), Availability::Available);
        assert_eq!(state.reason(), Reason::DefaultAvailable);
        assert_eq!(state.authority(), DecidingAuthority::UserPreference);
        assert_eq!(state.lifecycle(), Some(Lifecycle::Dormant));
    }
}
