// @file products/panther/localization/src/locale-request.rs
// @description Holds the user and profile locale inputs for a resolution.
// @created Diego Martín Lafuente <meerita@icloud.com>

use locale::Locale;

/// The user and profile inputs that a resolution considers.
///
/// The request holds the higher-precedence inputs only. The operating-system
/// preference list and the reference fallback belong to the resolver, not to a
/// single request. Every field is optional: an absent input is skipped and the
/// next precedence source decides. The request is kept next to the resolved
/// result so that a later re-negotiation can see what the caller asked for.
#[derive(Clone, Debug, Default)]
pub struct LocaleRequest {
    user_language: Option<Locale>,
    profile_language: Option<Locale>,
    user_region: Option<Locale>,
    profile_region: Option<Locale>,
}

impl LocaleRequest {
    /// Creates a request with no user or profile input.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the explicit user-selected user-interface language.
    pub fn with_user_language(mut self, language: Locale) -> Self {
        self.user_language = Some(language);
        self
    }

    /// Sets the profile user-interface language override.
    pub fn with_profile_language(mut self, language: Locale) -> Self {
        self.profile_language = Some(language);
        self
    }

    /// Sets the explicit user-selected region.
    pub fn with_user_region(mut self, region: Locale) -> Self {
        self.user_region = Some(region);
        self
    }

    /// Sets the profile region override.
    pub fn with_profile_region(mut self, region: Locale) -> Self {
        self.profile_region = Some(region);
        self
    }

    pub(crate) fn user_language(&self) -> Option<&Locale> {
        self.user_language.as_ref()
    }

    pub(crate) fn profile_language(&self) -> Option<&Locale> {
        self.profile_language.as_ref()
    }

    pub(crate) fn user_region(&self) -> Option<&Locale> {
        self.user_region.as_ref()
    }

    pub(crate) fn profile_region(&self) -> Option<&Locale> {
        self.profile_region.as_ref()
    }
}
