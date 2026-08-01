// @file products/panther/localization/tests/locale-resolution.rs
// @description Integration tests for locale precedence and separate-region formatting.
// @created Diego Martín Lafuente <meerita@icloud.com>

use locale::Locale;
use panther_localization::{
    LocaleRequest, LocaleResolver, RegionalFormatter, detect_system_locales,
};

fn locale(identifier: &str) -> Locale {
    Locale::parse(identifier).expect("valid identifier")
}

fn locales(identifiers: &[&str]) -> Vec<Locale> {
    identifiers.iter().map(|id| locale(id)).collect()
}

/// Operating-system detection and the user-interface precedence (D6).
///
/// The operating-system list is supplied as data, so the precedence is asserted
/// against a fixed ordered list rather than the host. The result is compared by
/// resolved identifier, not by any translated text.
#[test]
fn os_detection_and_ui_precedence() {
    // Real host detection must run and negotiate without a panic. The host list
    // can be empty in a headless environment, so only the absence of a panic is
    // asserted here.
    let _ = detect_system_locales();
    let detected = LocaleResolver::with_system_detection(locales(&["en"]));
    let _ = detected.resolve(&LocaleRequest::new());

    let available = locales(&["en", "es", "ja"]);

    // The ordered operating-system list selects the first available match.
    let resolver = LocaleResolver::new(available.clone(), locales(&["fr", "es-AR", "en"]));
    let active = resolver.resolve(&LocaleRequest::new());
    assert_eq!(active.ui_locale().to_string(), "es");

    // An explicit user choice wins over the operating-system list.
    let request = LocaleRequest::new().with_user_language(locale("ja"));
    assert_eq!(resolver.resolve(&request).ui_locale().to_string(), "ja");

    // A profile override wins over the operating-system list.
    let request = LocaleRequest::new().with_profile_language(locale("ja"));
    assert_eq!(resolver.resolve(&request).ui_locale().to_string(), "ja");

    // The explicit user choice outranks the profile override.
    let request = LocaleRequest::new()
        .with_user_language(locale("es"))
        .with_profile_language(locale("ja"));
    assert_eq!(resolver.resolve(&request).ui_locale().to_string(), "es");

    // No preference matches, so resolution falls back to the reference locale.
    let resolver = LocaleResolver::new(available, locales(&["de", "fr"]));
    assert_eq!(
        resolver
            .resolve(&LocaleRequest::new())
            .ui_locale()
            .to_string(),
        "en"
    );
}

/// A region locale independent of the user-interface language drives formatting (D9).
///
/// The region, not the user-interface language, decides grouping. The assertion
/// compares formatted output by structure: the region output differs from the
/// user-interface output for the same value.
#[test]
fn separate_region_drives_formatting() {
    let resolver = LocaleResolver::new(locales(&["en"]), locales(&["es-AR"]));
    let active = resolver.resolve(&LocaleRequest::new());

    assert_eq!(active.ui_locale().to_string(), "en");
    assert_eq!(active.region_locale().to_string(), "es-AR");

    let region = RegionalFormatter::new(active.region_locale()).expect("region formatting data");
    let ui = RegionalFormatter::new(active.ui_locale()).expect("reference formatting data");

    // The region locale drives the grouping, so the region and the
    // user-interface outputs differ for the same number.
    assert_ne!(region.integer(1_234_567), ui.integer(1_234_567));
    // The region locale also drives the date, independent of the language.
    assert_ne!(region.date(2025, 1, 15), ui.date(2025, 1, 15));
}
