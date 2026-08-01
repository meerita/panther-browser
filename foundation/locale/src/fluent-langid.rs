// @file foundation/locale/src/fluent-langid.rs
// @description Converts between the locale identity and the Fluent language identifier.
// @created Diego Martín Lafuente <meerita@icloud.com>

use unic_langid::LanguageIdentifier;

use crate::locale::Locale;
use crate::locale_parse_error::LocaleParseError;

/// Converts a [`Locale`] into the `unic-langid` identifier that Fluent uses.
///
/// This is the single conversion point for the Fluent boundary. The conversion
/// goes through the canonical identifier string and drops locale extensions,
/// which Fluent negotiation does not use. A [`Locale`] is always well-formed, so
/// the parse cannot fail; the unknown identifier `und` is the safe closed value.
pub fn to_language_identifier(locale: &Locale) -> LanguageIdentifier {
    locale.as_icu().id.to_string().parse().unwrap_or_default()
}

/// Converts a `unic-langid` identifier back into a canonical [`Locale`].
///
/// This is the single conversion point for the Fluent boundary. The identifier
/// is validated by the standard locale parser, so a malformed identifier is
/// rejected, never repaired into a default locale.
pub fn from_language_identifier(langid: &LanguageIdentifier) -> Result<Locale, LocaleParseError> {
    Locale::parse(&langid.to_string())
}

#[cfg(test)]
mod tests {
    use super::{from_language_identifier, to_language_identifier};
    use crate::locale::Locale;

    #[test]
    fn round_trips_through_unic_langid() {
        for identifier in ["en", "es-AR", "ar", "ja"] {
            let locale = Locale::parse(identifier).expect("valid identifier");
            let langid = to_language_identifier(&locale);
            let restored = from_language_identifier(&langid).expect("valid identifier");
            assert_eq!(restored, locale);
        }
    }
}
