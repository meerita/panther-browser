// @file engines/purr/engine/src/lib.rs
// @description Library root for the Purr web engine core.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Purr web engine core.
//!
//! This crate holds the engine layer. It has no dependency on the Panther
//! product and must remain independent of it.
//!
//! It declares the engine `purr.*` capabilities, reports platform support for
//! them, and supplies the mock providers that attach behavior. The product
//! merges these declarations into its catalogue; the engine holds no product
//! policy.

#[path = "capability-declarations.rs"]
mod capability_declarations;
#[path = "mock-providers.rs"]
mod mock_providers;
#[path = "platform-support.rs"]
mod platform_support;

pub use capability_declarations::{
    AUTHOR_STYLES, SERVICE_WORKERS, USER_AGENT_STYLES, WEBGPU, engine_capabilities,
};
pub use mock_providers::{SucceedingProvider, WebGpuProvider};
pub use platform_support::platform_supports;
