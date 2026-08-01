// @file products/panther/localization/src/localized-message.rs
// @description Defines the only text type that user interface sinks accept.
// @created Diego Martín Lafuente <meerita@icloud.com>

use locale::{Locale, TextDirection};

use crate::locale_generation::LocaleGeneration;

/// Resolved user-facing text together with its language metadata.
///
/// This is the only type a user interface text sink accepts, for both visible
/// text and accessible names and descriptions, so a raw `&str` or `String` can
/// never reach a text sink. It carries the resolved locale and its writing
/// direction, and records the active-locale generation it was produced under so
/// that generation-aware caches drop stale values.
///
/// A value comes either from the localization path inside this crate or from
/// the narrow escape hatch [`LocalizedMessage::from_verbatim`]. The fields stay
/// private so that construction always goes through one of those paths.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LocalizedMessage {
    text: String,
    locale: Locale,
    direction: TextDirection,
    generation: LocaleGeneration,
}

impl LocalizedMessage {
    /// Builds a message from text the localization path resolved.
    ///
    /// The localization path (message bundles and formatting) constructs every
    /// resolved message through this constructor.
    pub(crate) fn resolved(text: String, locale: Locale, generation: LocaleGeneration) -> Self {
        let direction = locale.direction();
        Self {
            text,
            locale,
            direction,
            generation,
        }
    }

    /// Wraps text that did not go through localization.
    ///
    /// This is the narrow escape hatch for text that is intentionally not
    /// localized: test fixtures, log lines, protocol and IPC payloads, and
    /// verbatim web content. It carries [`LocaleGeneration::DETACHED`] so it
    /// never matches a real generation boundary. Do not use it for user-facing
    /// product prose.
    pub fn from_verbatim(text: String, locale: Locale) -> Self {
        let direction = locale.direction();
        Self {
            text,
            locale,
            direction,
            generation: LocaleGeneration::DETACHED,
        }
    }

    /// Returns the resolved text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the locale the text was resolved for.
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// Returns the writing direction of the text.
    pub fn direction(&self) -> TextDirection {
        self.direction
    }

    /// Returns the active-locale generation the text was produced under.
    pub fn generation(&self) -> LocaleGeneration {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use locale::{Locale, TextDirection};

    use super::LocalizedMessage;
    use crate::locale_generation::LocaleGeneration;

    #[test]
    fn resolved_message_carries_direction_and_generation() {
        let locale = Locale::parse("ar").expect("valid identifier");

        let message =
            LocalizedMessage::resolved("سلام".to_owned(), locale, LocaleGeneration::new(3));

        assert_eq!(message.text(), "سلام");
        assert_eq!(message.direction(), TextDirection::RightToLeft);
        assert_eq!(message.generation().value(), 3);
    }

    #[test]
    fn verbatim_escape_hatch_is_detached() {
        let locale = Locale::parse("en").expect("valid identifier");

        let message = LocalizedMessage::from_verbatim("raw log line".to_owned(), locale);

        assert_eq!(message.text(), "raw log line");
        assert_eq!(message.direction(), TextDirection::LeftToRight);
        assert_eq!(message.generation(), LocaleGeneration::DETACHED);
    }
}
