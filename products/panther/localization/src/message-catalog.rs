// @file products/panther/localization/src/message-catalog.rs
// @description Loads and shares the Fluent message bundles behind an Arc.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::sync::Arc;

use i18n_embed::LanguageLoader;
use i18n_embed::fluent::FluentLanguageLoader;
use i18n_embed_fl::fl;
use locale::Locale;

use crate::baked_resource_provider::BakedResourceProvider;
use crate::embedded_localizations::EmbeddedLocalizations;
use crate::locale_generation::LocaleGeneration;
use crate::localized_message::LocalizedMessage;
use crate::resource_provider::ResourceProvider;
use crate::resource_validation::validate_ftl;

/// The Fluent domain of the crate. It names the catalogue file per locale.
const DOMAIN: &str = "panther-localization";

/// The catalogue file name for the domain.
const RESOURCE_FILE: &str = "panther-localization.ftl";

/// Maximum length of a resolved message, in bytes.
///
/// A resolved message is bounded so that an over-expanded translation cannot
/// reach a text sink. A message that exceeds the bound falls back to its id.
const MAX_MESSAGE_BYTES: usize = 8 * 1024;

/// The reference locale identifier.
const REFERENCE: &str = "en";

/// The parsed, shared message bundles.
///
/// The catalogue parses each locale once into the Fluent loader and shares the
/// result behind an [`Arc`], so cloning the catalogue never reparses. Argument
/// isolation stays enabled to prepare bidi isolation of interpolated values.
/// The reference locale `en` is the source and the ultimate fallback.
#[derive(Clone)]
pub struct MessageCatalog {
    loader: Arc<FluentLanguageLoader>,
    reference: Locale,
    generation: LocaleGeneration,
}

impl MessageCatalog {
    /// Loads the baked catalogues into a shared set of bundles.
    ///
    /// Loading is fail-safe: a resource that is unavailable, oversized, or not
    /// valid Fluent is dropped, the loader tolerates malformed content, and the
    /// reference locale always remains, so construction never crashes.
    pub fn load() -> Self {
        let reference = Locale::parse(REFERENCE).expect("the reference locale is valid");
        let fallback = locale::to_language_identifier(&reference);

        let loader = FluentLanguageLoader::new(DOMAIN, fallback.clone());
        loader.set_use_isolating(true);

        let provider = BakedResourceProvider;
        let mut languages = Vec::new();
        for candidate in provider.available_locales() {
            match provider.load(&candidate, RESOURCE_FILE) {
                Ok(bytes) if validate_ftl(bytes.as_ref()).is_ok() => {
                    languages.push(locale::to_language_identifier(&candidate));
                }
                _ => {}
            }
        }
        if languages.is_empty() {
            languages.push(fallback);
        }

        let _ = loader.load_languages(&EmbeddedLocalizations, &languages);

        Self {
            loader: Arc::new(loader),
            reference,
            generation: LocaleGeneration::new(1),
        }
    }

    /// Resolves an arbitrary message id.
    ///
    /// An unknown id falls back to the id itself rather than crashing, so a
    /// missing key can never take down a caller.
    pub fn message(&self, id: &str) -> LocalizedMessage {
        if !self.loader.has(id) {
            return self.bounded(id, id.to_owned());
        }
        self.bounded(id, self.loader.get(id))
    }

    /// Resolves the window title.
    pub fn window_title(&self) -> LocalizedMessage {
        self.bounded("window-title", fl!(self.loader, "window-title"))
    }

    /// Resolves the new-tab action label.
    pub fn window_new_tab(&self) -> LocalizedMessage {
        self.bounded("window-new-tab", fl!(self.loader, "window-new-tab"))
    }

    /// Resolves the camera permission prompt title for a requesting site.
    pub fn permission_camera_title(&self, site: &str) -> LocalizedMessage {
        self.bounded(
            "permissions-camera-title",
            fl!(self.loader, "permissions-camera-title", site = site),
        )
    }

    /// Resolves the permission grant button label.
    pub fn permission_allow(&self) -> LocalizedMessage {
        self.bounded("permissions-allow", fl!(self.loader, "permissions-allow"))
    }

    fn bounded(&self, id: &str, text: String) -> LocalizedMessage {
        if text.len() > MAX_MESSAGE_BYTES {
            return LocalizedMessage::resolved(
                id.to_owned(),
                self.reference.clone(),
                self.generation,
            );
        }
        LocalizedMessage::resolved(text, self.reference.clone(), self.generation)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::MessageCatalog;

    #[test]
    fn seed_key_resolves_under_en() {
        let catalog = MessageCatalog::load();
        assert_eq!(catalog.window_title().text(), "Panther");
        assert_eq!(catalog.window_new_tab().text(), "New Tab");
    }

    #[test]
    fn argument_is_interpolated_and_isolated() {
        let catalog = MessageCatalog::load();
        let message = catalog.permission_camera_title("example.com");
        assert!(message.text().contains("example.com"));
    }

    #[test]
    fn missing_key_falls_back_without_crashing() {
        let catalog = MessageCatalog::load();
        assert_eq!(catalog.message("does-not-exist").text(), "does-not-exist");
    }

    #[test]
    fn bundles_are_parsed_once_and_reused() {
        let catalog = MessageCatalog::load();
        let shared = catalog.clone();
        assert!(Arc::ptr_eq(&catalog.loader, &shared.loader));
    }
}
