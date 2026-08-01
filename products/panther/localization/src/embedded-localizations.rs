// @file products/panther/localization/src/embedded-localizations.rs
// @description Embeds the localization catalogues into the binary.
// @created Diego Martín Lafuente <meerita@icloud.com>

use rust_embed::RustEmbed;

/// The localization catalogues baked into the binary.
///
/// The `i18n` directory holds one catalogue per locale, named after the crate
/// domain. Embedding keeps the resources inside the binary so that the baked
/// provider serves them without filesystem access at run time.
#[derive(RustEmbed)]
#[folder = "i18n/"]
pub(crate) struct EmbeddedLocalizations;
