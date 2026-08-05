// @file products/panther/browser/src/address.rs
// @description Parses submitted address text and resolves the panther-scheme allow-list.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Address value type and the panther-scheme route resolver.
//!
//! Submitted address text is untrusted input. It is parsed with `url::Url` before
//! any use, never by string slicing. An address that fails to parse is reported as
//! absence, so a caller treats it as not committed. A parsed address resolves only
//! against an explicit allow-list of `panther:` targets; every other scheme and
//! path is unsupported and fails closed.

use url::Url;

/// A parsed, committed address.
///
/// Constructed only through [`Address::parse`], so the wrapped value is always a
/// URL the parser accepted. The display string is the parser's serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Address {
    url: Url,
}

/// The resolved target of a parsed address.
///
/// Only the two approved `panther:` targets resolve; every other scheme and path
/// is [`Unsupported`](AddressRoute::Unsupported), so an unexpected scheme fails
/// closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddressRoute {
    Blank,
    Demo,
    Unsupported,
}

impl Address {
    /// Parses untrusted address text into an address.
    ///
    /// Returns `None` when the text is not a valid absolute URL, so a caller
    /// treats an unparseable address as not committed.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        Url::parse(text).ok().map(|url| Self { url })
    }

    /// The display string for the committed address.
    pub(crate) fn as_str(&self) -> &str {
        self.url.as_str()
    }

    /// Resolves the address against the explicit `panther:` allow-list.
    ///
    /// The match is an allow-list: any scheme other than `panther`, and any
    /// `panther:` path other than `blank` or `demo`, resolves to `Unsupported`.
    pub(crate) fn route(&self) -> AddressRoute {
        if self.url.scheme() != "panther" {
            return AddressRoute::Unsupported;
        }

        match self.url.path() {
            "blank" => AddressRoute::Blank,
            "demo" => AddressRoute::Demo,
            _ => AddressRoute::Unsupported,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_a_well_formed_absolute_url_and_reports_the_display_string() {
        let address = Address::parse("panther:demo").expect("a parsed address");
        assert_eq!(address.as_str(), "panther:demo");
    }

    #[test]
    fn parse_rejects_an_unparseable_input() {
        assert!(Address::parse("not a url").is_none());
    }

    #[test]
    fn route_maps_the_approved_panther_targets() {
        assert_eq!(
            Address::parse("panther:blank").expect("parsed").route(),
            AddressRoute::Blank
        );
        assert_eq!(
            Address::parse("panther:demo").expect("parsed").route(),
            AddressRoute::Demo
        );
    }

    #[test]
    fn route_treats_every_other_scheme_and_path_as_unsupported() {
        let unsupported = [
            "https://example.com",
            "file:///etc/hosts",
            "javascript:alert(1)",
            "data:text/plain,hello",
            "panther:settings",
        ];

        for text in unsupported {
            let address = Address::parse(text).expect("a parsed address");
            assert_eq!(
                address.route(),
                AddressRoute::Unsupported,
                "expected {text} to be unsupported"
            );
        }
    }
}
