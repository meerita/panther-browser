// @file products/panther/chrome-text/src/lib.rs
// @description Library root for the Panther chrome text producer crate.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther chrome text producer.
//!
//! This product crate is the sole translator for the shell chrome. It owns the
//! map from a chrome region to its catalogue key and resolves each key to a
//! localized string through `panther-localization` at the active locale and
//! generation, so the shell stays prose-free. Resolution fails safe to `en` and
//! runs displayed text through bidi neutralization, so a spoofing control can
//! never reach a text sink. The resolved strings stay inside the crate: a later
//! phase shapes them into placed glyph runs, and only that neutral geometry
//! reaches the shell.

#[path = "label-set.rs"]
mod label_set;

#[path = "producer.rs"]
mod producer;

pub use label_set::{ChromeLabels, resolve_labels};
pub use producer::{ChromeRefresh, ChromeText, ChromeTextError, ChromeTextView};
