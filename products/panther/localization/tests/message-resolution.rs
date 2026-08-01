// @file products/panther/localization/tests/message-resolution.rs
// @description Integration tests for missing-translation fallback and bidi RTL messages.
// @created Diego Martín Lafuente <meerita@icloud.com>

use fluent::{FluentArgs, FluentBundle, FluentResource};
use locale::{Locale, TextDirection};
use panther_localization::{BakedResourceProvider, ResourceProvider};

/// The catalogue file name embedded for every locale.
const RESOURCE_FILE: &str = "panther-localization.ftl";

/// The reference locale and ultimate fallback.
const REFERENCE: &str = "en";

/// The bidi first-strong isolate that opens an interpolated argument.
const ISOLATE_START: char = '\u{2068}';

/// The bidi pop-directional-isolate that closes an interpolated argument.
const ISOLATE_END: char = '\u{2069}';

/// Builds a message bundle from the baked catalogue of a locale.
///
/// The bytes come through the public resource seam and are treated as untrusted
/// input: an absent catalogue yields `None` rather than a panic. Placeable
/// isolation stays on so an interpolated argument keeps its bidi isolates.
fn bundle(identifier: &str) -> Option<FluentBundle<FluentResource>> {
    let locale = Locale::parse(identifier).expect("valid identifier");
    let bytes = BakedResourceProvider.load(&locale, RESOURCE_FILE).ok()?;
    let source = String::from_utf8(bytes.into_owned()).expect("catalogue is valid utf8");
    let resource = FluentResource::try_new(source).expect("catalogue is valid fluent");
    let language = identifier.parse().expect("valid language identifier");
    let mut bundle = FluentBundle::<FluentResource>::new(vec![language]);
    bundle.set_use_isolating(true);
    bundle.add_resource(resource).expect("resource added");
    Some(bundle)
}

/// Resolves a message with the same fallback contract the crate applies.
///
/// The primary locale is tried first; a message it does not carry falls back to
/// the reference locale. An identifier absent everywhere returns itself rather
/// than crashing.
fn resolve(identifier: &str, primary: &str, arguments: Option<&FluentArgs>) -> String {
    let format = |bundle: &FluentBundle<FluentResource>| -> Option<String> {
        let message = bundle.get_message(identifier)?;
        let pattern = message.value()?;
        let mut errors = Vec::new();
        Some(
            bundle
                .format_pattern(pattern, arguments, &mut errors)
                .into_owned(),
        )
    };

    if let Some(primary_bundle) = bundle(primary)
        && let Some(text) = format(&primary_bundle)
    {
        return text;
    }
    bundle(REFERENCE)
        .as_ref()
        .and_then(format)
        .unwrap_or_else(|| identifier.to_owned())
}

/// A missing translation falls back to the reference locale without a crash (D6, Invariant 7).
///
/// Every shipped locale resolves the key, and a locale with no catalogue at all
/// falls back to the reference. The fallback is asserted by structural equality
/// with the reference resolution, not by a copied sentence.
#[test]
fn missing_translation_falls_back_to_the_reference() {
    for identifier in ["en", "es", "ar", "ja"] {
        let text = resolve("window-new-tab", identifier, None);
        assert!(
            !text.is_empty(),
            "{identifier} must resolve the seed key without a crash"
        );
    }

    let reference = resolve("window-new-tab", REFERENCE, None);
    let unshipped = resolve("window-new-tab", "de", None);
    assert_eq!(
        unshipped, reference,
        "a locale without a catalogue resolves through the reference"
    );
}

/// A real Arabic message renders right to left and isolates its argument (D12, Invariant 6).
///
/// The interpolated site stays wrapped in bidi isolates, the resolved text
/// carries real Arabic content, and the locale reports right-to-left direction.
/// The Arabic prose is asserted by script range, not by a copied sentence.
#[test]
fn arabic_message_is_right_to_left_and_isolates_the_argument() {
    let mut arguments = FluentArgs::new();
    arguments.set("site", "example.com");
    let text = resolve("permissions-camera-title", "ar", Some(&arguments));

    assert!(
        text.contains(&format!("{ISOLATE_START}example.com{ISOLATE_END}")),
        "the left-to-right site argument must stay isolated"
    );
    assert!(
        text.chars()
            .any(|character| ('\u{0600}'..='\u{06FF}').contains(&character)),
        "the message must render real Arabic content"
    );
    assert_eq!(
        Locale::parse("ar").expect("valid identifier").direction(),
        TextDirection::RightToLeft
    );
}
