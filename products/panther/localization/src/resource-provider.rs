// @file products/panther/localization/src/resource-provider.rs
// @description Defines the seam that hides how localization resources are loaded.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::borrow::Cow;

use locale::Locale;

use crate::localization_error::LocalizationError;

/// The seam that hides how message and formatting resources are loaded.
///
/// Baked embedding is the only implementation now, but external resource packs
/// must fit behind the same contract later without a rewrite, so the contract
/// stays small and free of any loading detail. A resource is returned as raw
/// bytes because a provider serves both message catalogues and other baked
/// data; the caller validates and decodes the bytes and treats them as
/// untrusted input.
pub trait ResourceProvider {
    /// Loads the raw bytes of a named resource for a locale.
    fn load(
        &self,
        locale: &Locale,
        resource: &str,
    ) -> Result<Cow<'static, [u8]>, LocalizationError>;

    /// Lists the locales this provider can serve.
    fn available_locales(&self) -> Vec<Locale>;
}
