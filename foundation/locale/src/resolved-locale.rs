// @file foundation/locale/src/resolved-locale.rs
// @description Pairs a requested identifier with its canonical locale.
// @created Diego Martín Lafuente <meerita@icloud.com>

use crate::locale::Locale;
use crate::locale_parse_error::LocaleParseError;

/// A requested locale identifier together with its canonical resolution.
///
/// The requested identifier is kept in its original form so that diagnostics
/// and later re-negotiation can see what the caller actually asked for, while
/// the resolved value is always canonical. The requested string is only stored
/// after a successful parse, so it is a short, well-formed identifier and never
/// unbounded external input.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedLocale {
    requested: String,
    resolved: Locale,
}

impl ResolvedLocale {
    /// Parses the requested identifier and keeps both forms.
    pub fn from_requested(requested: &str) -> Result<Self, LocaleParseError> {
        let resolved = Locale::parse(requested)?;
        Ok(Self {
            requested: requested.to_owned(),
            resolved,
        })
    }

    /// Returns the requested identifier in its original form.
    pub fn requested(&self) -> &str {
        &self.requested
    }

    /// Returns the canonical resolved locale.
    pub fn resolved(&self) -> &Locale {
        &self.resolved
    }
}

#[cfg(test)]
mod tests {
    use super::ResolvedLocale;

    #[test]
    fn keeps_requested_form_through_canonicalization() {
        let resolved = ResolvedLocale::from_requested("EN-us").expect("valid identifier");
        assert_eq!(resolved.requested(), "EN-us");
        assert_eq!(resolved.resolved().to_string(), "en-US");
    }

    #[test]
    fn rejects_invalid_requested_identifier() {
        assert!(ResolvedLocale::from_requested("not a locale!").is_err());
    }
}
