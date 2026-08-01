// @file products/panther/localization/tests/pseudolocalization.rs
// @description Integration test for pseudolocale expansion and mirror through the real path.
// @created Diego Martín Lafuente <meerita@icloud.com>

// The pseudolocale constructor is a development instrument, so it compiles only
// with debug assertions. The whole test is gated the same way and is absent from
// a release build.
#![cfg(debug_assertions)]

use locale::TextDirection;
use panther_localization::{MessageCatalog, Pseudolocale};

/// The pseudolocales expand and mirror the reference through the real path (D13).
///
/// Both pseudolocales run through the same resolution and bounding path as a real
/// locale. Expansion is asserted by length growth and inequality, and the mirror
/// by locale identity and right-to-left direction, never by a copied string.
#[test]
fn pseudolocales_expand_and_mirror() {
    let english = MessageCatalog::load().window_new_tab();

    let expanded = MessageCatalog::pseudolocalized(Pseudolocale::AccentedExpanded).window_new_tab();
    assert_eq!(expanded.locale().to_string(), "en-XA");
    assert_eq!(expanded.direction(), TextDirection::LeftToRight);
    assert_ne!(expanded.text(), english.text());
    assert!(
        expanded.text().len() > english.text().len(),
        "the accented pseudolocale must expand the reference text"
    );

    let mirrored = MessageCatalog::pseudolocalized(Pseudolocale::BidiMirrored).window_new_tab();
    assert_eq!(mirrored.locale().to_string(), "ar-XB");
    assert_eq!(mirrored.direction(), TextDirection::RightToLeft);
    assert_ne!(mirrored.text(), english.text());
}
