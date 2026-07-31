// @file foundation/capability-system/src/lib.rs
// @description Library root for the shared capability mechanism and vocabulary.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Shared capability mechanism and vocabulary.
//!
//! This crate holds the generic capability mechanism and the vocabulary that
//! both the Purr engine and the Panther product share. It holds mechanism and
//! vocabulary only. It must not hold product policy or engine policy, and it
//! depends on neither side.
//!
//! The state model uses two orthogonal axes. Effective availability answers
//! whether a capability can be used at all. Runtime lifecycle answers, only
//! when a capability is available, where it sits in its runtime cycle. The
//! [`EffectiveState`] type keeps these axes orthogonal so that a lifecycle can
//! never attach to an unavailable capability.

#[path = "availability.rs"]
mod availability;
#[path = "capability-id.rs"]
mod capability_id;
#[path = "category.rs"]
mod category;
#[path = "deciding-authority.rs"]
mod deciding_authority;
#[path = "effective-state.rs"]
mod effective_state;
#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "maturity.rs"]
mod maturity;
#[path = "owner.rs"]
mod owner;
#[path = "reason.rs"]
mod reason;

pub use availability::{Availability, UnavailableStatus};
pub use capability_id::CapabilityId;
pub use category::Category;
pub use deciding_authority::DecidingAuthority;
pub use effective_state::EffectiveState;
pub use lifecycle::{FailureCategory, Lifecycle};
pub use maturity::Maturity;
pub use owner::Owner;
pub use reason::{Reason, reason_message};
