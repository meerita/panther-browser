// @file products/panther/localization/src/locale-resolver.rs
// @description Resolves the active user-interface and region locales.
// @created Diego Martín Lafuente <meerita@icloud.com>

use icu_locale::LocaleExpander;
use locale::{Locale, negotiate};

use crate::active_locales::ActiveLocales;
use crate::locale_request::LocaleRequest;
use crate::system_locales::detect_system_locales;

/// The reference locale identifier and ultimate fallback.
const REFERENCE: &str = "en";

/// Resolves the active user-interface and region locales for a profile.
///
/// The resolver owns the available catalogue locales, the ordered operating
/// system preference list, and the reference fallback. It applies the
/// user-interface precedence (explicit user, profile, operating system, then the
/// reference) through catalogue negotiation, and it resolves the region on its
/// own precedence so that the region can differ from the user-interface
/// language.
pub struct LocaleResolver {
    available: Vec<Locale>,
    system: Vec<Locale>,
    fallback: Locale,
}

impl LocaleResolver {
    /// Creates a resolver with an explicit operating-system list.
    ///
    /// The system list is data, so a test can supply a fixed ordered list
    /// instead of reading the host.
    pub fn new(available: Vec<Locale>, system: Vec<Locale>) -> Self {
        let fallback = Locale::parse(REFERENCE).expect("the reference locale is valid");
        Self {
            available,
            system,
            fallback,
        }
    }

    /// Creates a resolver that reads the operating-system preference list.
    pub fn with_system_detection(available: Vec<Locale>) -> Self {
        Self::new(available, detect_system_locales())
    }

    /// Resolves the active locales for a main profile.
    pub fn resolve(&self, request: &LocaleRequest) -> ActiveLocales {
        let ui = self.resolve_ui(request);
        let region = self.resolve_region(request, &ui);
        ActiveLocales::new(ui, region)
    }

    /// Resolves the active locales for a private profile.
    ///
    /// A private profile inherits the main profile's active locales and adds no
    /// separate detection signal, so a private session carries no distinguishing
    /// locale. The function reads only the main result, so it cannot introduce a
    /// new signal.
    pub fn resolve_private(main: &ActiveLocales) -> ActiveLocales {
        main.clone()
    }

    fn resolve_ui(&self, request: &LocaleRequest) -> Locale {
        let mut requested = Vec::new();
        if let Some(language) = request.user_language() {
            requested.push(language.clone());
        }
        if let Some(language) = request.profile_language() {
            requested.push(language.clone());
        }
        requested.extend(self.system.iter().cloned());
        negotiate(&requested, &self.available, &self.fallback)
    }

    fn resolve_region(&self, request: &LocaleRequest, ui: &Locale) -> Locale {
        if let Some(region) = request.user_region() {
            return region.clone();
        }
        if let Some(region) = request.profile_region() {
            return region.clone();
        }
        if let Some(region) = self.system_region() {
            return region;
        }
        derive_region(ui)
    }

    fn system_region(&self) -> Option<Locale> {
        self.system
            .iter()
            .find(|locale| locale.as_icu().id.region.is_some())
            .cloned()
    }
}

/// Derives a region locale from the user-interface locale.
///
/// The identifier is maximized with likely subtags to obtain the region that
/// the language implies, then reduced to a language and region locale. A
/// language without a likely region keeps the user-interface locale, so the
/// result is always a valid locale.
fn derive_region(ui: &Locale) -> Locale {
    let mut identifier = ui.as_icu().id.clone();
    LocaleExpander::new_common().maximize(&mut identifier);
    match identifier.region {
        Some(region) => Locale::parse(&format!("{}-{region}", identifier.language))
            .unwrap_or_else(|_| ui.clone()),
        None => ui.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::LocaleResolver;
    use crate::locale_request::LocaleRequest;
    use locale::Locale;

    fn locale(identifier: &str) -> Locale {
        Locale::parse(identifier).expect("valid identifier")
    }

    fn locales(identifiers: &[&str]) -> Vec<Locale> {
        identifiers.iter().map(|id| locale(id)).collect()
    }

    fn available() -> Vec<Locale> {
        locales(&["en", "es", "ja"])
    }

    #[test]
    fn ordered_os_list_selects_the_first_available_match() {
        let resolver = LocaleResolver::new(available(), locales(&["fr", "es-AR", "en"]));
        let active = resolver.resolve(&LocaleRequest::new());
        assert_eq!(active.ui_locale().to_string(), "es");
    }

    #[test]
    fn explicit_user_language_overrides_the_os_list() {
        let resolver = LocaleResolver::new(available(), locales(&["es", "en"]));
        let request = LocaleRequest::new().with_user_language(locale("ja"));
        let active = resolver.resolve(&request);
        assert_eq!(active.ui_locale().to_string(), "ja");
    }

    #[test]
    fn profile_language_overrides_the_os_list() {
        let resolver = LocaleResolver::new(available(), locales(&["es"]));
        let request = LocaleRequest::new().with_profile_language(locale("ja"));
        let active = resolver.resolve(&request);
        assert_eq!(active.ui_locale().to_string(), "ja");
    }

    #[test]
    fn user_language_takes_precedence_over_profile_language() {
        let resolver = LocaleResolver::new(available(), Vec::new());
        let request = LocaleRequest::new()
            .with_user_language(locale("ja"))
            .with_profile_language(locale("es"));
        let active = resolver.resolve(&request);
        assert_eq!(active.ui_locale().to_string(), "ja");
    }

    #[test]
    fn unmatched_preferences_fall_back_to_the_reference() {
        let resolver = LocaleResolver::new(available(), locales(&["de", "fr"]));
        let active = resolver.resolve(&LocaleRequest::new());
        assert_eq!(active.ui_locale().to_string(), "en");
    }

    #[test]
    fn explicit_user_region_wins_and_is_independent_of_the_ui() {
        let resolver = LocaleResolver::new(available(), Vec::new());
        let request = LocaleRequest::new()
            .with_user_language(locale("ja"))
            .with_user_region(locale("de-DE"));
        let active = resolver.resolve(&request);
        assert_eq!(active.ui_locale().to_string(), "ja");
        assert_eq!(active.region_locale().to_string(), "de-DE");
    }

    #[test]
    fn region_is_derived_from_the_ui_via_likely_subtags() {
        let resolver = LocaleResolver::new(available(), locales(&["es"]));
        let active = resolver.resolve(&LocaleRequest::new());
        assert_eq!(active.ui_locale().to_string(), "es");
        assert_eq!(active.region_locale().to_string(), "es-ES");
    }

    #[test]
    fn os_region_is_separate_from_the_ui_language() {
        let resolver = LocaleResolver::new(locales(&["en"]), locales(&["es-AR"]));
        let active = resolver.resolve(&LocaleRequest::new());
        assert_eq!(active.ui_locale().to_string(), "en");
        assert_eq!(active.region_locale().to_string(), "es-AR");
    }

    #[test]
    fn private_profile_inherits_the_main_locale() {
        let resolver = LocaleResolver::new(available(), locales(&["es"]));
        let main = resolver.resolve(&LocaleRequest::new());
        let private = LocaleResolver::resolve_private(&main);
        assert_eq!(private, main);
    }
}
