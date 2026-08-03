// @file products/panther/browser/src/core-error.rs
// @description Defines the typed failures the product core can return.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Product-core error type.
//!
//! The tab model owns these variants. They are typed, prose-free, and factual for
//! developer diagnostics only. A seam failure is preserved as the source without
//! exposing an engine-internal or dependency error type. No variant carries a
//! secret or a localized message.

use purr_embedding::SeamError;

/// Failure the product core reports to a consumer.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    #[error("tab identifier does not name a known tab")]
    UnknownTab,
    #[error("the document seam failed")]
    Seam(#[from] SeamError),
}
