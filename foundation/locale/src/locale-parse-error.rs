// @file foundation/locale/src/locale-parse-error.rs
// @description Defines the typed error for invalid locale identifiers.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Rejection of a malformed locale identifier.
///
/// The parser never repairs an invalid identifier into a default locale, so
/// this error is the only outcome for input that is not a well-formed Unicode
/// locale identifier. The message carries canonical technical English only and
/// never copies the rejected input, because that input is untrusted and can be
/// arbitrarily large.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[error("the locale identifier is not a valid Unicode locale identifier")]
pub struct LocaleParseError;
