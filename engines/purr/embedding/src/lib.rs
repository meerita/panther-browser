// @file engines/purr/embedding/src/lib.rs
// @description Library root for the Purr embedding layer.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Purr embedding layer.
//!
//! This crate hosts the Purr engine and exposes it to a product. It depends on
//! the engine core and must not depend on any Panther product package.
//!
//! It is the narrow two-way capability boundary. Upward it surfaces the engine
//! capability offers (declaration, platform support, and provider) as shared
//! vocabulary types. Downward it holds the effective engine-policy snapshot the
//! product resolves and exposes it for engine diagnostics. Only shared
//! `capability-system` types cross the boundary.

#[path = "engine-capability-offer.rs"]
mod engine_capability_offer;
#[path = "engine-policy-holder.rs"]
mod engine_policy_holder;

pub use engine_capability_offer::{EngineCapabilityOffer, engine_capability_offers};
pub use engine_policy_holder::EnginePolicyHolder;
