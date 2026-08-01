// @file products/panther/localization/src/locale-snapshots.rs
// @description Snapshots formatted output and plural behavior per locale.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Per-locale snapshots for the shipped locales.
//!
//! Formatting output (numbers, dates, lists, file sizes) is snapshotted exactly,
//! because it is ICU4X output and not translated prose. Plural, direction, and
//! isolation behavior is asserted by structure, not by the translated sentence,
//! so a wording change in a catalogue does not force a test edit and the tests
//! never restate the FTL source.

use std::collections::HashMap;

use fluent::FluentValue;
use i18n_embed::LanguageLoader;
use i18n_embed::fluent::FluentLanguageLoader;
use icu::plurals::{PluralCategory, PluralRules};
use locale::{Locale, TextDirection};
use unic_langid::LanguageIdentifier;

use crate::embedded_localizations::EmbeddedLocalizations;
use crate::regional_formatter::RegionalFormatter;

/// The bidi first-strong isolate that wraps an interpolated argument.
const ISOLATE_START: char = '\u{2068}';

/// The bidi pop-directional-isolate that closes an interpolated argument.
const ISOLATE_END: char = '\u{2069}';

fn region_formatter(identifier: &str) -> RegionalFormatter {
    RegionalFormatter::new(&Locale::parse(identifier).expect("valid identifier"))
        .expect("compiled formatting data")
}

/// Builds a message loader whose primary language is the given locale.
///
/// The reference locale is the fallback, so a key a partial locale does not
/// translate resolves against `en` without failing. Placeable isolation stays
/// on so an interpolated argument keeps its bidi isolates.
fn message_loader(language: &str) -> FluentLanguageLoader {
    let fallback: LanguageIdentifier = "en".parse().expect("valid identifier");
    let loader = FluentLanguageLoader::new("panther-localization", fallback);
    loader.set_use_isolating(true);
    let requested: LanguageIdentifier = language.parse().expect("valid identifier");
    loader
        .load_languages(&EmbeddedLocalizations, &[requested])
        .expect("loaded language");
    loader
}

/// Resolves the open-tab count message for a language and count.
///
/// The count drives the plural category. The formatted number is supplied as a
/// separate isolated argument, so no translated text is concatenated.
fn tabs_open(language: &str, count: i64) -> String {
    let mut arguments: HashMap<&str, FluentValue> = HashMap::new();
    arguments.insert("count", FluentValue::from(count));
    arguments.insert("formatted", FluentValue::from(count.to_string()));
    message_loader(language).get_args_concrete("tabs-open", arguments)
}

/// Removes the isolated number from a resolved count message.
///
/// Two messages that differ only by their number share one template, which is
/// how the other-only rule is confirmed without restating the translation.
fn without_isolated_number(text: &str, count: i64) -> String {
    text.replace(&format!("{ISOLATE_START}{count}{ISOLATE_END}"), "")
}

fn plural_category(language: &str, count: u64) -> PluralCategory {
    let locale = Locale::parse(language).expect("valid identifier");
    let rules =
        PluralRules::try_new(locale.as_icu().into(), Default::default()).expect("plural data");
    rules.category_for(count)
}

#[test]
fn numbers_format_per_locale() {
    assert_eq!(region_formatter("en").integer(1_234_567), "1,234,567");
    assert_eq!(region_formatter("es").integer(1_234_567), "1.234.567");
    assert_eq!(region_formatter("ar").integer(1_234_567), "1,234,567");
    assert_eq!(region_formatter("ja").integer(1_234_567), "1,234,567");
}

#[test]
fn dates_format_per_locale() {
    assert_eq!(region_formatter("en").date(2025, 1, 15), "Jan 15, 2025");
    assert_eq!(region_formatter("es").date(2025, 1, 15), "15 ene 2025");
    assert_eq!(
        region_formatter("ar").date(2025, 1, 15),
        "15\u{200f}/01\u{200f}/2025"
    );
    assert_eq!(region_formatter("ja").date(2025, 1, 15), "2025/01/15");
}

#[test]
fn lists_format_per_locale() {
    let items = ["a".to_owned(), "b".to_owned(), "c".to_owned()];
    assert_eq!(region_formatter("en").list(&items), "a, b, and c");
    assert_eq!(region_formatter("es").list(&items), "a, b y c");
    assert_eq!(region_formatter("ar").list(&items), "a وb وc");
    assert_eq!(region_formatter("ja").list(&items), "a、b、c");
}

#[test]
fn file_sizes_format_per_locale() {
    assert_eq!(region_formatter("en").file_size(1536), "1.5 KiB");
    assert_eq!(region_formatter("es").file_size(1536), "1,5 KiB");
    assert_eq!(region_formatter("ar").file_size(1536), "1.5 KiB");
    assert_eq!(region_formatter("ja").file_size(1536), "1.5 KiB");
}

#[test]
fn latin_locales_select_singular_and_plural() {
    for language in ["en", "es"] {
        let singular = tabs_open(language, 1);
        let plural = tabs_open(language, 2);
        assert_ne!(
            singular, plural,
            "{language} must distinguish one from other"
        );
        assert!(
            plural.contains(&format!("{ISOLATE_START}2{ISOLATE_END}")),
            "{language} plural must carry the isolated number"
        );
        assert!(
            !singular.contains('2'),
            "{language} singular must not carry a number"
        );
    }
}

#[test]
fn arabic_exercises_the_six_plural_categories() {
    assert_eq!(plural_category("ar", 0), PluralCategory::Zero);
    assert_eq!(plural_category("ar", 1), PluralCategory::One);
    assert_eq!(plural_category("ar", 2), PluralCategory::Two);
    assert_eq!(plural_category("ar", 3), PluralCategory::Few);
    assert_eq!(plural_category("ar", 11), PluralCategory::Many);
    assert_eq!(plural_category("ar", 100), PluralCategory::Other);

    let zero = tabs_open("ar", 0);
    let one = tabs_open("ar", 1);
    let two = tabs_open("ar", 2);
    let few = tabs_open("ar", 3);
    assert_ne!(zero, one);
    assert_ne!(one, two);
    assert_ne!(two, few);
    assert!(few.contains(&format!("{ISOLATE_START}3{ISOLATE_END}")));
}

#[test]
fn japanese_uses_only_the_other_category() {
    for count in [0_u64, 1, 2, 3, 11, 100] {
        assert_eq!(plural_category("ja", count), PluralCategory::Other);
    }

    let one = tabs_open("ja", 1);
    let hundred = tabs_open("ja", 100);
    assert_ne!(one, hundred, "the isolated number still changes");
    assert_eq!(
        without_isolated_number(&one, 1),
        without_isolated_number(&hundred, 100),
        "one template serves every count"
    );
}

#[test]
fn arabic_message_renders_right_to_left_with_isolated_argument() {
    let mut arguments: HashMap<&str, FluentValue> = HashMap::new();
    arguments.insert("site", FluentValue::from("example.com"));
    let message = message_loader("ar").get_args_concrete("permissions-camera-title", arguments);

    assert!(
        message.contains(&format!("{ISOLATE_START}example.com{ISOLATE_END}")),
        "the site argument must stay isolated"
    );
    assert!(
        message
            .chars()
            .any(|character| ('\u{0600}'..='\u{06FF}').contains(&character)),
        "the message must render real Arabic content"
    );
    assert_eq!(
        Locale::parse("ar").expect("valid identifier").direction(),
        TextDirection::RightToLeft
    );
}
