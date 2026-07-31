// @file products/panther/browser/src/lib.rs
// @description Library root for the Panther browser product.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther browser product.
//!
//! This crate holds the browser product logic. It uses the Purr embedding
//! layer and must not reach the engine core directly.
//!
//! It declares the `panther.*` capabilities and their mock providers, supplies
//! the policy inputs, merges the engine declarations from the embedding boundary
//! into a validated catalogue, owns the capability
//! [`Manager`](capability_system::Manager), and pushes the effective
//! engine-policy snapshot to the embedding holder. [`bootstrap`] performs the
//! whole assembly and returns the product-owned result.

#[path = "capability-assembly.rs"]
mod capability_assembly;
#[path = "product-capabilities.rs"]
mod product_capabilities;
#[path = "product-policy.rs"]
mod product_policy;
#[path = "product-providers.rs"]
mod product_providers;

pub use capability_assembly::{AssemblyError, BootstrapResult, bootstrap, bootstrap_with};
pub use capability_system::Availability;
pub use product_capabilities::{
    DEVELOPER_MODE, DEVELOPER_TOOLS, REMOTE_DEBUGGING, product_capabilities,
};
pub use product_policy::BootstrapConfig;
