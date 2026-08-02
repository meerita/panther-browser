// @file engines/purr/engine/src/demonstration-fixture.rs
// @description Exposes the bundled M2 demonstration document as source bytes.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Bundled M2 demonstration document.
//!
//! The engine embeds one static HTML document used to prove the render path end
//! to end. It exercises the confirmed M2 element and property subset (headings, a
//! styled card, wrapping paragraphs, an inline tag and link, and margin collapse).
//! The bytes are embedded so the product and the render test reach the same source
//! without a filesystem dependency.

/// Bytes of the bundled M2 demonstration document.
///
/// The source is treated as untrusted input by the document store like any other
/// source, so no assumption is made here beyond returning the embedded bytes.
pub fn m2_demonstration_fixture() -> &'static [u8] {
    include_bytes!("../resources/m2-fixture.html")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_is_non_empty_and_within_the_source_bound() {
        let bytes = m2_demonstration_fixture();

        assert!(!bytes.is_empty());
        assert!(bytes.len() <= crate::MAX_SOURCE_BYTES);
    }
}
