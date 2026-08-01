// @file products/panther/localization/src/system-locales.rs
// @description Detects the ordered operating-system preferred-language list.
// @created Diego Martín Lafuente <meerita@icloud.com>

use locale::Locale;
use sys_locale::get_locales;

/// Detects the operating-system preferred-language list, most preferred first.
///
/// This is the single impure boundary of locale resolution: it reads the host
/// preference list and returns canonical [`Locale`] values. A tag the operating
/// system reports that does not parse is dropped rather than repaired, so a
/// malformed host value can never enter resolution. Duplicates are removed while
/// the preference order is kept, so the result feeds negotiation directly.
pub fn detect_system_locales() -> Vec<Locale> {
    let mut detected = Vec::new();
    for tag in get_locales() {
        let Ok(locale) = Locale::parse(&tag) else {
            continue;
        };
        if !detected.contains(&locale) {
            detected.push(locale);
        }
    }
    detected
}

#[cfg(test)]
mod tests {
    use super::detect_system_locales;

    #[test]
    fn detected_locales_are_canonical_and_unique() {
        let detected = detect_system_locales();
        for locale in &detected {
            assert_eq!(detected.iter().filter(|other| *other == locale).count(), 1);
        }
    }
}
