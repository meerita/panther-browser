// @file products/panther/localization/src/baked-resource-provider.rs
// @description Serves the resources baked into the binary behind the seam.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::borrow::Cow;

use locale::Locale;

use crate::embedded_localizations::EmbeddedLocalizations;
use crate::localization_error::LocalizationError;
use crate::resource_provider::ResourceProvider;

/// The provider that serves resources baked into the binary.
///
/// It reads the embedded catalogues by locale and resource name. A resource
/// that is not embedded is a recoverable error, never a crash, which keeps the
/// fail-safe contract.
#[derive(Clone, Copy, Default, Debug)]
pub struct BakedResourceProvider;

impl ResourceProvider for BakedResourceProvider {
    fn load(
        &self,
        locale: &Locale,
        resource: &str,
    ) -> Result<Cow<'static, [u8]>, LocalizationError> {
        let path = format!("{locale}/{resource}");
        EmbeddedLocalizations::get(&path)
            .map(|file| file.data)
            .ok_or(LocalizationError::ResourceUnavailable)
    }

    fn available_locales(&self) -> Vec<Locale> {
        let mut locales = Vec::new();
        for path in EmbeddedLocalizations::iter() {
            let Some((language, _)) = path.split_once('/') else {
                continue;
            };
            let Ok(locale) = Locale::parse(language) else {
                continue;
            };
            if !locales.contains(&locale) {
                locales.push(locale);
            }
        }
        locales
    }
}
