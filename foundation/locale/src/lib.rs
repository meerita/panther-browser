// @file foundation/locale/src/lib.rs
// @description Library root for the shared locale identity vocabulary.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Shared locale identity vocabulary.
//!
//! This crate holds the neutral locale vocabulary that the Panther product and
//! future engines share. It holds locale identity and its directly derived
//! properties only. It must not hold catalogues, messages, formatting, or any
//! user-facing prose, and it depends on neither side of the product and engine
//! chain.
//!
//! A [`Locale`] is always a parsed and canonical Unicode locale identifier. A
//! [`ResolvedLocale`] keeps the original requested identifier next to its
//! canonical resolution so that diagnostics and re-negotiation stay possible.
//! Text direction is derived from the locale script through [`TextDirection`].
//!
//! The crate also resolves a [`Locale`] from a requested list against an
//! available set through [`negotiate`], and produces the data-fallback chain of
//! a locale through [`fallback_chain`]. Conversion to the Fluent `unic-langid`
//! identifier lives in one place, the single seam for the Fluent boundary.

#[path = "fallback-chain.rs"]
mod fallback_chain;
#[path = "fluent-langid.rs"]
mod fluent_langid;
#[path = "locale.rs"]
mod locale;
#[path = "locale-parse-error.rs"]
mod locale_parse_error;
#[path = "negotiate.rs"]
mod negotiate;
#[path = "resolved-locale.rs"]
mod resolved_locale;
#[path = "text-direction.rs"]
mod text_direction;

pub use fallback_chain::fallback_chain;
pub use fluent_langid::{from_language_identifier, to_language_identifier};
pub use locale::Locale;
pub use locale_parse_error::LocaleParseError;
pub use negotiate::negotiate;
pub use resolved_locale::ResolvedLocale;
pub use text_direction::TextDirection;
