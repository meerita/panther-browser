// @file products/panther/localization/src/pseudolocale.rs
// @description Development pseudolocales built from the reference catalogue.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::borrow::Cow;

use fluent_pseudo::transform;
use locale::Locale;

/// A development pseudolocale.
///
/// A pseudolocale is not a translation. It transforms the `en` reference text to
/// expose hard-coded strings, clipping, text expansion, and right-to-left layout
/// before real translations exist. Pseudolocales are development instruments
/// only: they never appear in the stable user language settings, and an end user
/// can never select one in a release build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pseudolocale {
    /// `en-XA`. Accents each letter and expands the text about thirty percent to
    /// reveal clipping and hard-coded strings.
    AccentedExpanded,
    /// `ar-XB`. Mirrors each letter and carries right-to-left direction to reveal
    /// bidirectional and layout problems.
    BidiMirrored,
}

impl Pseudolocale {
    /// Every development pseudolocale.
    pub const ALL: [Pseudolocale; 2] = [Self::AccentedExpanded, Self::BidiMirrored];

    /// Returns the locale identity of the pseudolocale.
    ///
    /// The mirrored pseudolocale uses the `ar` language, so a resolved message
    /// stamped with this identity carries right-to-left direction.
    pub fn locale(self) -> Locale {
        let identifier = match self {
            Self::AccentedExpanded => "en-XA",
            Self::BidiMirrored => "ar-XB",
        };
        Locale::parse(identifier).expect("the pseudolocale identifier is valid")
    }

    /// Returns the Fluent bundle transform hook for the pseudolocale.
    ///
    /// A Fluent transform is a plain function pointer, so it cannot capture
    /// state. Each pseudolocale maps to one dedicated transform function.
    pub(crate) fn transform(self) -> fn(&str) -> Cow<'_, str> {
        match self {
            Self::AccentedExpanded => accent_and_expand,
            Self::BidiMirrored => mirror,
        }
    }
}

/// Accents and expands the reference text for `en-XA`.
fn accent_and_expand(source: &str) -> Cow<'_, str> {
    transform(source, false, true)
}

/// Mirrors the reference text for the bidi pseudolocale `ar-XB`.
fn mirror(source: &str) -> Cow<'_, str> {
    transform(source, true, false)
}

/// Returns the development pseudolocales when the developer flag is set.
///
/// The pseudolocales appear only when the caller enables the developer flag, and
/// only in a development build. A release build never lists them, so they stay
/// out of the stable user language settings.
pub fn development_pseudolocales(developer_flag_enabled: bool) -> Vec<Locale> {
    if !developer_flag_enabled {
        return Vec::new();
    }
    pseudolocale_identifiers()
}

/// Returns the pseudolocale identities in a development build.
#[cfg(debug_assertions)]
fn pseudolocale_identifiers() -> Vec<Locale> {
    Pseudolocale::ALL
        .iter()
        .map(|pseudolocale| pseudolocale.locale())
        .collect()
}

/// Returns no pseudolocale identities in a release build.
#[cfg(not(debug_assertions))]
fn pseudolocale_identifiers() -> Vec<Locale> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use locale::TextDirection;

    use super::{Pseudolocale, development_pseudolocales};

    #[test]
    fn accented_pseudolocale_is_left_to_right_english() {
        let locale = Pseudolocale::AccentedExpanded.locale();
        assert_eq!(locale.to_string(), "en-XA");
        assert_eq!(locale.direction(), TextDirection::LeftToRight);
    }

    #[test]
    fn mirrored_pseudolocale_carries_right_to_left_direction() {
        let locale = Pseudolocale::BidiMirrored.locale();
        assert_eq!(locale.to_string(), "ar-XB");
        assert_eq!(locale.direction(), TextDirection::RightToLeft);
    }

    #[test]
    fn accent_transform_differs_and_expands() {
        let transform = Pseudolocale::AccentedExpanded.transform();
        let output = transform("New Tab");
        assert_ne!(output.as_ref(), "New Tab");
        assert!(output.len() > "New Tab".len());
    }

    #[test]
    fn mirror_transform_differs_from_the_source() {
        let transform = Pseudolocale::BidiMirrored.transform();
        let output = transform("New Tab");
        assert_ne!(output.as_ref(), "New Tab");
    }

    #[test]
    fn the_developer_flag_gates_availability() {
        assert!(development_pseudolocales(false).is_empty());
    }
}
