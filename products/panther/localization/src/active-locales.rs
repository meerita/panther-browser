// @file products/panther/localization/src/active-locales.rs
// @description Holds the resolved active user-interface and region locales.
// @created Diego Martín Lafuente <meerita@icloud.com>

use locale::Locale;

/// The resolved locales that drive presentation for a profile.
///
/// The user-interface locale selects the message catalogue. The region locale
/// is resolved on its own precedence and drives regional formatting, so it can
/// differ from the user-interface language. Later phases read the region for
/// formatting and update both locales together on a language switch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ActiveLocales {
    ui: Locale,
    region: Locale,
}

impl ActiveLocales {
    pub(crate) fn new(ui: Locale, region: Locale) -> Self {
        Self { ui, region }
    }

    /// Returns the active user-interface locale.
    pub fn ui_locale(&self) -> &Locale {
        &self.ui
    }

    /// Returns the active region locale.
    pub fn region_locale(&self) -> &Locale {
        &self.region
    }
}
