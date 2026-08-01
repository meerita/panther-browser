// @file products/panther/localization/src/bidi-spoof.rs
// @description Detects and neutralizes bidi spoofing controls in displayed text.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::borrow::Cow;

/// The marker shown in place of a neutralized control character.
///
/// U+FFFD keeps the substitution visible, so a neutralized string is never
/// mistaken for its original spoofed form.
const REPLACEMENT: char = '\u{FFFD}';

/// Returns whether the text holds a bidirectional control character.
///
/// A displayed URL or filename is checked before it reaches a text sink. A
/// bidirectional override or isolate can reorder characters so that a hostile
/// value looks like a safe one, so its presence is a spoofing signal.
pub fn contains_bidi_control(text: &str) -> bool {
    text.chars().any(is_bidi_control)
}

/// Returns the text with every bidirectional control character neutralized.
///
/// Each control is replaced with a visible marker instead of being dropped, so
/// tampering stays evident and the displayed string can never silently reorder.
/// Text without a control is returned borrowed and unchanged.
pub fn neutralize_bidi_controls(text: &str) -> Cow<'_, str> {
    if !contains_bidi_control(text) {
        return Cow::Borrowed(text);
    }
    let neutralized = text
        .chars()
        .map(|character| {
            if is_bidi_control(character) {
                REPLACEMENT
            } else {
                character
            }
        })
        .collect();
    Cow::Owned(neutralized)
}

/// Returns whether the character is a bidirectional control.
///
/// The set covers the embedding, override, and isolate controls together with
/// the directional marks. These are the characters that can reorder displayed
/// text and enable a spoofed URL or filename.
fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{202A}'
            | '\u{202B}'
            | '\u{202C}'
            | '\u{202D}'
            | '\u{202E}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}'
    )
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{contains_bidi_control, neutralize_bidi_controls};

    #[test]
    fn clean_text_is_borrowed_and_unchanged() {
        let url = "https://example.com/report.pdf";
        assert!(!contains_bidi_control(url));
        assert!(matches!(neutralize_bidi_controls(url), Cow::Borrowed(_)));
    }

    #[test]
    fn override_in_a_filename_is_detected_and_neutralized() {
        let spoofed = "report\u{202E}fdp.exe";
        assert!(contains_bidi_control(spoofed));

        let neutralized = neutralize_bidi_controls(spoofed);
        assert!(matches!(neutralized, Cow::Owned(_)));
        assert!(!contains_bidi_control(&neutralized));
        assert!(neutralized.contains('\u{FFFD}'));
    }

    #[test]
    fn override_in_a_url_is_detected_and_neutralized() {
        let spoofed = "https://example.com/\u{202E}gpj.exe";
        assert!(contains_bidi_control(spoofed));

        let neutralized = neutralize_bidi_controls(spoofed);
        assert!(!contains_bidi_control(&neutralized));
        assert!(neutralized.contains('\u{FFFD}'));
        assert!(neutralized.contains("example.com"));
    }
}
