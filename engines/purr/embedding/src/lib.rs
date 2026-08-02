// @file engines/purr/embedding/src/lib.rs
// @description Library root for the Purr embedding layer.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Purr embedding layer.
//!
//! This crate hosts the Purr engine and exposes it to a product. It depends on
//! the engine core and must not depend on any Panther product package.
//!
//! It is the narrow two-way capability boundary. Upward it surfaces the engine
//! capability offers (declaration, platform support, and provider) and the
//! engine capability identifiers as shared vocabulary types. Downward it holds
//! the effective engine-policy snapshot the product resolves and exposes it for
//! engine diagnostics. Only shared `capability-system` types cross the boundary.
//!
//! It also hosts the document-attachment seam: the typed, prose-free,
//! generation-tagged boundary that carries a document into the engine and a
//! renderable frame back. The product drives it through `DocumentSession`.

#[path = "document-attachment.rs"]
mod document_attachment;
#[path = "engine-capability-offer.rs"]
mod engine_capability_offer;
#[path = "engine-policy-holder.rs"]
mod engine_policy_holder;

pub use document_attachment::{
    DocumentFrame, DocumentHandle, DocumentSession, SeamError, ViewportGeometry,
};
pub use engine_capability_offer::{EngineCapabilityOffer, engine_capability_offers};
pub use engine_policy_holder::EnginePolicyHolder;

// Opaque handle components surfaced from the engine so the product can name a
// document without reaching the engine core.
pub use purr_engine::{DocumentGeneration, DocumentId};

// The bundled M2 demonstration document, surfaced so the product can attach it
// without reaching the engine core or a filesystem.
pub use purr_engine::m2_demonstration_fixture;

// Engine capability identifiers surfaced as shared `CapabilityId` values so the
// product can name an engine capability in its policy without reaching the
// engine core.
pub use purr_engine::{AUTHOR_STYLES, SERVICE_WORKERS, USER_AGENT_STYLES, WEBGPU};
