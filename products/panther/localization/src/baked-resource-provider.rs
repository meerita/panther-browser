// @file products/panther/localization/src/baked-resource-provider.rs
// @description Provides the baked resource provider filled in a later phase.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::borrow::Cow;

use locale::Locale;

use crate::localization_error::LocalizationError;
use crate::resource_provider::ResourceProvider;

/// The provider that serves resources baked into the binary.
///
/// The embedding pipeline is added in a later phase. Until then this provider
/// serves no resources and reports none, which keeps the fail-safe contract: a
/// missing resource is a recoverable error, never a crash.
#[derive(Clone, Copy, Default, Debug)]
pub struct BakedResourceProvider;

impl ResourceProvider for BakedResourceProvider {
    fn load(
        &self,
        _locale: &Locale,
        _resource: &str,
    ) -> Result<Cow<'static, [u8]>, LocalizationError> {
        Err(LocalizationError::ResourceUnavailable)
    }

    fn available_locales(&self) -> Vec<Locale> {
        Vec::new()
    }
}
