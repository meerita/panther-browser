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
//!
//! It also owns the document store: the arena of attached documents and the raw
//! pipeline output the embedding seam wraps. The store surfaces opaque document
//! identities the seam re-exports as handle components.

#[path = "capability-declarations.rs"]
mod capability_declarations;
#[path = "document-store.rs"]
mod document_store;
#[path = "dom-node.rs"]
mod dom_node;
#[path = "html-tokenizer.rs"]
mod html_tokenizer;
#[path = "html-tree-builder.rs"]
mod html_tree_builder;
#[path = "mock-providers.rs"]
mod mock_providers;
#[path = "platform-support.rs"]
mod platform_support;

pub use capability_declarations::{
    AUTHOR_STYLES, GPU_ACCELERATION, SERVICE_WORKERS, USER_AGENT_STYLES, WEBGPU,
    engine_capabilities,
};
pub use document_store::{
    DocumentError, DocumentGeneration, DocumentId, DocumentStore, EngineFrame, MAX_SOURCE_BYTES,
    engine_producer_namespace,
};
pub use mock_providers::{SucceedingProvider, WebGpuProvider};
pub use platform_support::platform_supports;
