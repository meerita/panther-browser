// @file foundation/capability-system/src/availability.rs
// @description Defines the effective availability axis and its unavailable subset.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Effective availability of a capability.
///
/// This is the first of the two orthogonal axes. It answers whether a
/// capability can be used at all. Only `Available` permits a runtime lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Availability {
    NotBuilt,
    Unsupported,
    Prohibited,
    Disabled,
    Available,
}

/// Availability restricted to the values that are not `Available`.
///
/// This subset lets the unavailable shape of an effective state hold a status
/// while making the `Available` value unrepresentable in that shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnavailableStatus {
    NotBuilt,
    Unsupported,
    Prohibited,
    Disabled,
}

impl UnavailableStatus {
    pub const fn availability(self) -> Availability {
        match self {
            Self::NotBuilt => Availability::NotBuilt,
            Self::Unsupported => Availability::Unsupported,
            Self::Prohibited => Availability::Prohibited,
            Self::Disabled => Availability::Disabled,
        }
    }
}
