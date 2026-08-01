// @file foundation/locale/src/negotiate.rs
// @description Resolves a requested locale list against an available set.
// @created Diego Martín Lafuente <meerita@icloud.com>

use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use unic_langid::LanguageIdentifier;

use crate::fluent_langid::to_language_identifier;
use crate::locale::Locale;

/// Resolves the best available locale for an ordered list of requested locales.
///
/// The requested list is ordered from the most to the least preferred locale.
/// Matching uses Fluent Filtering against the available set. The result always
/// resolves to a [`Locale`]: when nothing matches, `fallback` is returned, so
/// the function never yields an unresolved value.
pub fn negotiate(requested: &[Locale], available: &[Locale], fallback: &Locale) -> Locale {
    let requested_ids: Vec<LanguageIdentifier> =
        requested.iter().map(to_language_identifier).collect();
    let available_ids: Vec<LanguageIdentifier> =
        available.iter().map(to_language_identifier).collect();
    let fallback_id = to_language_identifier(fallback);

    let resolved = negotiate_languages(
        &requested_ids,
        &available_ids,
        Some(&fallback_id),
        NegotiationStrategy::Filtering,
    );

    let Some(best) = resolved.first() else {
        return fallback.clone();
    };

    match available_ids.iter().position(|id| id == *best) {
        Some(index) => available[index].clone(),
        None => fallback.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::negotiate;
    use crate::locale::Locale;

    fn locales(identifiers: &[&str]) -> Vec<Locale> {
        identifiers
            .iter()
            .map(|id| Locale::parse(id).expect("valid identifier"))
            .collect()
    }

    #[test]
    fn filtering_picks_the_first_available_match() {
        let requested = locales(&["fr", "es-AR", "en"]);
        let available = locales(&["en", "es", "ja"]);
        let fallback = Locale::parse("en").expect("valid identifier");

        let resolved = negotiate(&requested, &available, &fallback);

        assert_eq!(resolved.to_string(), "es");
    }

    #[test]
    fn no_match_returns_the_fallback() {
        let requested = locales(&["de"]);
        let available = locales(&["ja"]);
        let fallback = Locale::parse("en").expect("valid identifier");

        let resolved = negotiate(&requested, &available, &fallback);

        assert_eq!(resolved, fallback);
    }
}
