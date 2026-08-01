// @file products/panther/localization/src/active-locale-state.rs
// @description Owns the active locales and advances the generation on a change.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::rc::Rc;

use locale::Locale;

use crate::active_locales::ActiveLocales;
use crate::locale_broadcast::{LocaleBroadcast, LocaleChangeListener};
use crate::locale_generation::LocaleGeneration;
use crate::locale_request::LocaleRequest;
use crate::locale_resolver::LocaleResolver;

/// The single source of truth for the active locales and their generation.
///
/// The state owns the resolver and the current request, so a language or region
/// change re-resolves from one place and cannot leave two views disagreeing. A
/// change advances the generation and notifies subscribers, which then re-pull
/// their text. No localized or formatted value is cached across a generation
/// boundary: callers read the current generation here and pass it to the
/// generation-aware message and formatter caches, which drop any value from an
/// earlier generation.
///
/// Platform-native surfaces are rebuilt on change, not mutated in place, and
/// accessibility must re-announce the change. Those surfaces arrive in a later
/// milestone and are not implemented here.
pub struct ActiveLocaleState {
    resolver: LocaleResolver,
    request: LocaleRequest,
    locales: ActiveLocales,
    generation: LocaleGeneration,
    broadcast: LocaleBroadcast,
}

impl ActiveLocaleState {
    /// Resolves the initial locales and starts at the first generation.
    pub fn new(resolver: LocaleResolver, request: LocaleRequest) -> Self {
        let locales = resolver.resolve(&request);
        Self {
            resolver,
            request,
            locales,
            generation: LocaleGeneration::FIRST,
            broadcast: LocaleBroadcast::new(),
        }
    }

    /// Returns the current active locales.
    pub fn current(&self) -> &ActiveLocales {
        &self.locales
    }

    /// Returns the current active-locale generation.
    pub fn generation(&self) -> LocaleGeneration {
        self.generation
    }

    /// Registers a subscriber for later change notifications.
    pub fn subscribe(&mut self, listener: &Rc<dyn LocaleChangeListener>) {
        self.broadcast.subscribe(listener);
    }

    /// Replaces the request, re-resolves, and advances the generation.
    pub fn apply(&mut self, request: LocaleRequest) {
        self.request = request;
        self.advance();
    }

    /// Changes the user-interface language and advances the generation.
    pub fn change_language(&mut self, language: Locale) {
        self.request = self.request.clone().with_user_language(language);
        self.advance();
    }

    /// Changes the region independently of the user-interface language.
    pub fn change_region(&mut self, region: Locale) {
        self.request = self.request.clone().with_user_region(region);
        self.advance();
    }

    fn advance(&mut self) {
        self.locales = self.resolver.resolve(&self.request);
        self.generation = self.generation.next();
        self.broadcast.broadcast(self.generation);
    }
}

#[cfg(test)]
mod tests {
    use super::ActiveLocaleState;
    use crate::locale_request::LocaleRequest;
    use crate::locale_resolver::LocaleResolver;
    use locale::Locale;

    fn locale(identifier: &str) -> Locale {
        Locale::parse(identifier).expect("valid identifier")
    }

    fn resolver() -> LocaleResolver {
        LocaleResolver::new(
            vec![locale("en"), locale("es"), locale("ja")],
            vec![locale("en")],
        )
    }

    #[test]
    fn change_advances_the_generation() {
        let mut state = ActiveLocaleState::new(resolver(), LocaleRequest::new());
        assert_eq!(state.generation().value(), 1);

        state.change_language(locale("es"));

        assert_eq!(state.generation().value(), 2);
    }

    #[test]
    fn reresolve_after_change_yields_the_new_locale() {
        let mut state = ActiveLocaleState::new(resolver(), LocaleRequest::new());
        assert_eq!(state.current().ui_locale().to_string(), "en");

        state.change_language(locale("es"));

        assert_eq!(state.current().ui_locale().to_string(), "es");
    }

    #[test]
    fn region_changes_independently_of_the_ui_language() {
        let request = LocaleRequest::new().with_user_language(locale("ja"));
        let mut state = ActiveLocaleState::new(resolver(), request);
        assert_eq!(state.current().ui_locale().to_string(), "ja");

        state.change_region(locale("de-DE"));

        assert_eq!(state.current().ui_locale().to_string(), "ja");
        assert_eq!(state.current().region_locale().to_string(), "de-DE");
        assert_eq!(state.generation().value(), 2);
    }
}
