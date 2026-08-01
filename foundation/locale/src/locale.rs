// @file foundation/locale/src/locale.rs
// @description Defines the canonical locale identity value type.
// @created Diego Martín Lafuente <meerita@icloud.com>

use core::fmt;

use icu_locale::{Direction, LocaleCanonicalizer, LocaleDirectionality};
use icu_locale_core::Locale as IcuLocale;

use crate::locale_parse_error::LocaleParseError;
use crate::text_direction::TextDirection;

/// A parsed and canonicalized Unicode locale identifier.
///
/// The inner value is always well-formed and canonical, so the type is the
/// neutral locale vocabulary that the product and future engines share. The
/// inner value stays private so that construction always goes through parsing
/// and canonicalization and can never hold an unchecked identifier.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Locale {
    inner: IcuLocale,
}

impl Locale {
    /// Parses and canonicalizes a Unicode locale identifier.
    ///
    /// Canonicalization replaces deprecated subtags and normalizes case, so
    /// `EN-us` becomes `en-US`. Malformed input is rejected, never repaired
    /// into a default locale.
    pub fn parse(input: &str) -> Result<Self, LocaleParseError> {
        let mut inner: IcuLocale = input.parse().map_err(|_| LocaleParseError)?;
        LocaleCanonicalizer::new_common().canonicalize(&mut inner);
        Ok(Self { inner })
    }

    /// Derives the writing direction from the locale script.
    ///
    /// The direction comes from the script, expanding likely subtags when the
    /// identifier omits an explicit script. An unknown direction is treated as
    /// left to right, the safe default for a script without directional data.
    pub fn direction(&self) -> TextDirection {
        match LocaleDirectionality::new_common().get(&self.inner.id) {
            Some(Direction::RightToLeft) => TextDirection::RightToLeft,
            _ => TextDirection::LeftToRight,
        }
    }

    /// Returns the underlying canonical identifier for negotiation and data
    /// lookups performed by dependent crates.
    pub fn as_icu(&self) -> &IcuLocale {
        &self.inner
    }

    /// Wraps an identifier that is already canonical.
    ///
    /// The crate uses this for identifiers produced by locale fallback. Those
    /// identifiers are already well-formed and canonical, so they bypass
    /// parsing and canonicalization.
    pub(crate) fn from_icu(inner: IcuLocale) -> Self {
        Self { inner }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::Locale;
    use crate::text_direction::TextDirection;

    #[test]
    fn parses_and_normalizes_case() {
        let locale = Locale::parse("EN-us").expect("valid identifier");
        assert_eq!(locale.to_string(), "en-US");
    }

    #[test]
    fn rejects_invalid_input_without_defaulting() {
        assert!(Locale::parse("not a locale!").is_err());
        assert!(Locale::parse("").is_err());
    }

    #[test]
    fn derives_right_to_left_for_arabic() {
        let locale = Locale::parse("ar").expect("valid identifier");
        assert_eq!(locale.direction(), TextDirection::RightToLeft);
    }

    #[test]
    fn derives_left_to_right_for_english_and_japanese() {
        let english = Locale::parse("en").expect("valid identifier");
        let japanese = Locale::parse("ja").expect("valid identifier");
        assert_eq!(english.direction(), TextDirection::LeftToRight);
        assert_eq!(japanese.direction(), TextDirection::LeftToRight);
    }
}
