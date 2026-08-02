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

#[path = "block-layout.rs"]
mod block_layout;
#[path = "bundled-font.rs"]
mod bundled_font;
#[path = "capability-declarations.rs"]
mod capability_declarations;
#[path = "computed-style.rs"]
mod computed_style;
#[path = "css-parser.rs"]
mod css_parser;
#[path = "css-tokenizer.rs"]
mod css_tokenizer;
#[path = "document-store.rs"]
mod document_store;
#[path = "dom-node.rs"]
mod dom_node;
#[path = "fragment-tree.rs"]
mod fragment_tree;
#[path = "html-tokenizer.rs"]
mod html_tokenizer;
#[path = "html-tree-builder.rs"]
mod html_tree_builder;
#[path = "inline-layout.rs"]
mod inline_layout;
#[path = "layout-tree.rs"]
mod layout_tree;
#[path = "layout-unit.rs"]
mod layout_unit;
#[path = "mock-providers.rs"]
mod mock_providers;
#[path = "platform-support.rs"]
mod platform_support;
#[path = "style-cascade.rs"]
mod style_cascade;
#[path = "text-shaping.rs"]
mod text_shaping;
#[path = "user-agent-styles.rs"]
mod user_agent_styles;

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
