// @file products/panther/browser/src/lib.rs
// @description Library root for the Panther browser product.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther browser product.
//!
//! This crate is the product core. It uses the Purr embedding layer and must not
//! reach the engine core directly.
//!
//! It declares the `panther.*` capabilities and their mock providers, supplies
//! the policy inputs, merges the engine declarations from the embedding boundary
//! into a validated catalogue, owns the capability
//! [`Manager`](capability_system::Manager), and pushes the effective
//! engine-policy snapshot to the embedding holder. [`bootstrap`] performs the
//! whole assembly and returns the product-owned result.
//!
//! It also owns the tab model: the ordered tab collection, the active-tab
//! selector, and one shared document seam. A consumer opens tabs, attaches a
//! document to a tab, activates a tab, and produces the active tab's frame
//! through [`TabModel`], naming a tab only by its opaque [`TabId`].

#[path = "address.rs"]
mod address;
#[path = "capability-assembly.rs"]
mod capability_assembly;
#[path = "core-error.rs"]
mod core_error;
#[path = "product-capabilities.rs"]
mod product_capabilities;
#[path = "product-policy.rs"]
mod product_policy;
#[path = "product-providers.rs"]
mod product_providers;
#[path = "tab.rs"]
mod tab;
#[path = "tab-model.rs"]
mod tab_model;

pub use capability_assembly::{AssemblyError, BootstrapResult, bootstrap, bootstrap_with};
pub use capability_system::Availability;
pub use core_error::CoreError;
pub use product_capabilities::{
    DEVELOPER_MODE, DEVELOPER_TOOLS, REMOTE_DEBUGGING, product_capabilities,
};
pub use product_policy::BootstrapConfig;
pub use tab::{Tab, TabId};
pub use tab_model::{AddressOutcome, TabModel};

// The bundled M2 demonstration source, surfaced so the composition root can
// attach it to the first tab without reaching the engine core or a filesystem.
pub use purr_embedding::m2_demonstration_fixture;
