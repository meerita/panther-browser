// @file products/panther/localization/src/resource-validation.rs
// @description Validates untrusted localization resources before they are used.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::localization_error::LocalizationError;

/// Maximum size of a single localization resource, in bytes.
///
/// A resource is untrusted input, so its size is bounded before it is parsed to
/// prevent a hostile or corrupt catalogue from exhausting memory.
pub(crate) const MAX_RESOURCE_BYTES: usize = 256 * 1024;

/// Validates raw resource bytes before the loader trusts them.
///
/// The bytes are rejected when they exceed the size bound, are not valid UTF-8,
/// or do not parse as Fluent. Rejection is recoverable: the caller drops the
/// resource and falls back to the reference locale, and never crashes.
pub(crate) fn validate_ftl(bytes: &[u8]) -> Result<(), LocalizationError> {
    if bytes.len() > MAX_RESOURCE_BYTES {
        return Err(LocalizationError::InvalidResource);
    }

    let text = core::str::from_utf8(bytes).map_err(|_| LocalizationError::InvalidResource)?;

    fluent_syntax::parser::parse(text).map_err(|_| LocalizationError::InvalidResource)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_RESOURCE_BYTES, validate_ftl};
    use crate::localization_error::LocalizationError;

    #[test]
    fn accepts_valid_fluent() {
        assert!(validate_ftl(b"window-title = Panther\n").is_ok());
    }

    #[test]
    fn rejects_invalid_fluent_without_crashing() {
        assert_eq!(
            validate_ftl(b"= not a valid message\n"),
            Err(LocalizationError::InvalidResource)
        );
    }

    #[test]
    fn rejects_oversized_resource() {
        let oversized = vec![b'a'; MAX_RESOURCE_BYTES + 1];
        assert_eq!(
            validate_ftl(&oversized),
            Err(LocalizationError::InvalidResource)
        );
    }
}
