// @file products/panther/localization/src/message-catalog.rs
// @description Loads and shares the Fluent message bundles behind an Arc.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::HashMap;
use std::sync::Arc;

use fluent::FluentValue;
use i18n_embed::LanguageLoader;
use i18n_embed::fluent::FluentLanguageLoader;
use i18n_embed_fl::fl;
use locale::Locale;
use unic_langid::LanguageIdentifier;

use crate::baked_resource_provider::BakedResourceProvider;
use crate::embedded_localizations::EmbeddedLocalizations;
use crate::locale_generation::LocaleGeneration;
use crate::localized_message::LocalizedMessage;
use crate::message_arguments::{MessageArgument, MessageArguments};
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
        let reference = reference_locale();
        let languages = validated_available_languages();
        let loader = new_loader(&languages);

        Self {
            loader: Arc::new(loader),
            reference,
            generation: LocaleGeneration::FIRST,
        }
    }

    /// Loads the reference catalogue transformed into a development pseudolocale.
    ///
    /// A pseudolocale is a development instrument, not a translation. It loads
    /// only the `en` reference catalogue and applies the pseudolocale transform
    /// through the Fluent bundle hook, so the accented, expanded, or mirrored
    /// text runs through the same resolution and bounding path as a real locale.
    /// The catalog stamps the pseudolocale identity, so a mirrored pseudolocale
    /// carries its right-to-left direction. This constructor is compiled only
    /// with debug assertions, so a release build cannot build a pseudolocale.
    #[cfg(debug_assertions)]
    pub fn pseudolocalized(pseudolocale: crate::pseudolocale::Pseudolocale) -> Self {
        let languages = vec![locale::to_language_identifier(&reference_locale())];
        let loader = new_loader(&languages);
        let transform = pseudolocale.transform();
        loader.with_bundles_mut(|bundle| bundle.set_transform(Some(transform)));

        Self {
            loader: Arc::new(loader),
            reference: pseudolocale.locale(),
            generation: LocaleGeneration::FIRST,
        }
    }

    /// Returns a catalog stamped at the given active-locale generation.
    ///
    /// A runtime language change advances the active generation. Every resolved
    /// message must carry the current generation so a generation-aware cache
    /// drops it once the generation advances. The bundles are shared behind an
    /// [`Arc`], so this reuses the parsed data and only replaces the generation.
    pub fn at_generation(&self, generation: LocaleGeneration) -> Self {
        Self {
            loader: Arc::clone(&self.loader),
            reference: self.reference.clone(),
            generation,
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

    /// Resolves a message with named arguments supplied as typed data.
    ///
    /// The arguments are typed values, never preformatted prose, so the message
    /// template controls all wording and no translated text is concatenated. A
    /// numeric argument drives the Fluent plural and select categories.
    /// Interpolated values stay bidi-isolated, because the loader keeps
    /// placeable isolation on. An unknown id falls back to the id itself rather
    /// than crashing.
    pub fn message_with_arguments(
        &self,
        id: &str,
        arguments: &MessageArguments,
    ) -> LocalizedMessage {
        if !self.loader.has(id) {
            return self.bounded(id, id.to_owned());
        }
        let mut values = HashMap::with_capacity(arguments.entries().len());
        for (name, argument) in arguments.entries() {
            let value = match argument {
                MessageArgument::Text(text) => FluentValue::from(text.as_str()),
                MessageArgument::Integer(number) => FluentValue::from(*number),
            };
            values.insert(*name, value);
        }
        self.bounded(id, self.loader.get_args_concrete(id, values))
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

/// Returns the reference locale and ultimate fallback.
fn reference_locale() -> Locale {
    Locale::parse(REFERENCE).expect("the reference locale is valid")
}

/// Builds a loader over the given catalogue languages.
///
/// Placeable isolation stays on so interpolated values keep bidi isolation. The
/// reference language is always the loader fallback, so a missing key resolves
/// against `en`.
fn new_loader(languages: &[LanguageIdentifier]) -> FluentLanguageLoader {
    let fallback = locale::to_language_identifier(&reference_locale());
    let loader = FluentLanguageLoader::new(DOMAIN, fallback);
    loader.set_use_isolating(true);
    let _ = loader.load_languages(&EmbeddedLocalizations, languages);
    loader
}

/// Returns the embedded catalogue languages that hold valid Fluent content.
///
/// A resource that is unavailable, oversized, or not valid Fluent is dropped.
/// The reference language leads and always remains, so the set is never empty
/// and the default catalog resolves against the reference regardless of the
/// order the resources are embedded in.
fn validated_available_languages() -> Vec<LanguageIdentifier> {
    let provider = BakedResourceProvider;
    let reference = locale::to_language_identifier(&reference_locale());
    let mut languages = vec![reference.clone()];
    for candidate in provider.available_locales() {
        let language = locale::to_language_identifier(&candidate);
        if language == reference {
            continue;
        }
        match provider.load(&candidate, RESOURCE_FILE) {
            Ok(bytes) if validate_ftl(bytes.as_ref()).is_ok() => languages.push(language),
            _ => {}
        }
    }
    languages
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{MessageCatalog, validated_available_languages};
    use crate::message_arguments::{MessageArgument, MessageArguments};

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
    fn interpolated_argument_is_wrapped_in_bidi_isolates() {
        let catalog = MessageCatalog::load();
        let message = catalog.permission_camera_title("example.com");

        assert!(message.text().contains("\u{2068}example.com\u{2069}"));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn interpolated_argument_stays_isolated_in_a_right_to_left_message() {
        use locale::TextDirection;

        use crate::pseudolocale::Pseudolocale;

        let catalog = MessageCatalog::pseudolocalized(Pseudolocale::BidiMirrored);
        let message = catalog.permission_camera_title("example.com");

        assert_eq!(message.direction(), TextDirection::RightToLeft);
        assert!(message.text().contains("\u{2068}example.com\u{2069}"));
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

    #[test]
    fn plural_message_selects_by_count() {
        let catalog = MessageCatalog::load();

        let single = catalog.message_with_arguments(
            "tabs-open",
            &MessageArguments::new().with("count", MessageArgument::Integer(1)),
        );
        assert_eq!(single.text(), "One tab is open");

        let several = catalog.message_with_arguments(
            "tabs-open",
            &MessageArguments::new()
                .with("count", MessageArgument::Integer(1000))
                .with("formatted", MessageArgument::Text("1,000".to_owned())),
        );
        assert!(several.text().contains("1,000"));
        assert!(several.text().contains("tabs are open"));
    }

    #[test]
    fn named_arguments_are_placed_by_name() {
        let catalog = MessageCatalog::load();
        let message = catalog.message_with_arguments(
            "permissions-camera-title",
            &MessageArguments::new().with("site", MessageArgument::Text("example.com".to_owned())),
        );
        assert!(message.text().contains("example.com"));
        assert!(message.text().contains("camera"));
    }

    #[test]
    fn missing_key_with_arguments_falls_back_without_crashing() {
        let catalog = MessageCatalog::load();
        let message = catalog.message_with_arguments(
            "does-not-exist",
            &MessageArguments::new().with("count", MessageArgument::Integer(2)),
        );
        assert_eq!(message.text(), "does-not-exist");
    }

    #[test]
    fn messages_carry_the_active_generation() {
        use crate::locale_generation::LocaleGeneration;

        let catalog = MessageCatalog::load();
        assert_eq!(catalog.window_title().generation().value(), 1);

        let advanced = catalog.at_generation(LocaleGeneration::new(5));
        assert_eq!(advanced.window_title().generation().value(), 5);
    }

    #[test]
    fn non_english_plural_category_resolves_via_fixture() {
        use fluent::{FluentArgs, FluentBundle, FluentResource};

        let source = concat!(
            "items = { $count ->\n",
            "    [zero] zero\n",
            "    [one] one\n",
            "    [two] two\n",
            "    [few] few\n",
            "    [many] many\n",
            "   *[other] other\n",
            "}\n",
        );
        let resource = FluentResource::try_new(source.to_owned()).expect("valid fixture");
        let arabic = "ar".parse().expect("valid language identifier");
        let mut bundle: FluentBundle<FluentResource> = FluentBundle::new(vec![arabic]);
        bundle.set_use_isolating(false);
        bundle.add_resource(resource).expect("resource added");

        let message = bundle.get_message("items").expect("message exists");
        let pattern = message.value().expect("message has a value");
        let mut arguments = FluentArgs::new();
        arguments.set("count", 3);
        let mut errors = Vec::new();
        let formatted = bundle.format_pattern(pattern, Some(&arguments), &mut errors);

        assert_eq!(formatted, "few");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn accented_pseudolocale_expands_through_the_real_path() {
        use crate::pseudolocale::Pseudolocale;

        let english = MessageCatalog::load().window_new_tab();
        let accented = MessageCatalog::pseudolocalized(Pseudolocale::AccentedExpanded);
        let message = accented.window_new_tab();

        assert_ne!(message.text(), english.text());
        assert!(message.text().len() > english.text().len());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn mirrored_pseudolocale_carries_right_to_left_direction_through_the_real_path() {
        use locale::TextDirection;

        use crate::pseudolocale::Pseudolocale;

        let mirrored = MessageCatalog::pseudolocalized(Pseudolocale::BidiMirrored);
        let message = mirrored.window_new_tab();

        assert_eq!(message.locale().to_string(), "ar-XB");
        assert_eq!(message.direction(), TextDirection::RightToLeft);
        assert_ne!(message.text(), "New Tab");
    }

    #[test]
    fn pseudolocales_are_absent_from_the_release_locale_set() {
        let available: Vec<String> = validated_available_languages()
            .iter()
            .map(|language| language.to_string())
            .collect();

        assert!(!available.iter().any(|language| language == "en-XA"));
        assert!(!available.iter().any(|language| language == "ar-XB"));
    }
}
