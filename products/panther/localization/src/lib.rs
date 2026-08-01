// @file products/panther/localization/src/lib.rs
// @description Library root for the Panther localization crate.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther localization.
//!
//! This crate owns all Panther user-facing prose and localization policy. It
//! sits above `foundation/locale` in the Panther chain and turns typed states
//! into localized text at the presentation boundary. Low-level crates stay
//! prose-free; only this crate maps a typed state to a message.
//!
//! Two contracts anchor the design. The [`ResourceProvider`] seam hides how
//! message and formatting resources are loaded, so baked embedding now and
//! external packs later share one contract. The [`LocalizedMessage`] type is
//! the only text a user interface sink accepts, which keeps raw strings out of
//! text sinks; a typed state becomes a message through the [`MessageAdapter`]
//! pattern. The [`MessageCatalog`] parses the baked Fluent catalogues once into
//! shared bundles and resolves seed keys into localized text. Later phases add
//! formatting, resolution, and runtime switching.

#[path = "baked-resource-provider.rs"]
mod baked_resource_provider;
#[path = "embedded-localizations.rs"]
mod embedded_localizations;
#[path = "locale-generation.rs"]
mod locale_generation;
#[path = "localization-error.rs"]
mod localization_error;
#[path = "localized-message.rs"]
mod localized_message;
#[path = "message-adapter.rs"]
mod message_adapter;
#[path = "message-arguments.rs"]
mod message_arguments;
#[path = "message-catalog.rs"]
mod message_catalog;
#[path = "resource-provider.rs"]
mod resource_provider;
#[path = "resource-validation.rs"]
mod resource_validation;

pub use baked_resource_provider::BakedResourceProvider;
pub use locale_generation::LocaleGeneration;
pub use localization_error::LocalizationError;
pub use localized_message::LocalizedMessage;
pub use message_adapter::MessageAdapter;
pub use message_arguments::{MessageArgument, MessageArguments};
pub use message_catalog::MessageCatalog;
pub use resource_provider::ResourceProvider;
