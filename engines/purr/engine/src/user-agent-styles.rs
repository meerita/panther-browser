// @file engines/purr/engine/src/user-agent-styles.rs
// @description Holds the built-in user-agent stylesheet and parses it into the UA origin.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Built-in user-agent stylesheet.
//!
//! The engine ships a small baseline sheet for the M2 element set. It sets the
//! block or inline `display` default for each element and the base typographic
//! defaults, so a document renders predictably before any author style applies.
//! The sheet is parsed into the mandatory user-agent origin.

// The cascade (a later phase) is the first consumer of the parsed sheet, so the
// entry point is otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::css_parser::{Origin, Stylesheet, parse_stylesheet};

/// The built-in user-agent stylesheet for the M2 element set.
///
/// It covers the rendered elements (`html`, `body`, `div`, `p`, `span`, `h1`,
/// `h2`, `a`) and hides the non-rendered head elements (`head`, `title`). The
/// values are logical defaults, not final geometry.
const USER_AGENT_STYLESHEET: &str = "\
html { display: block; }
head { display: none; }
title { display: none; }
body { display: block; margin: 8px; color: #000000; font-size: 16px; font-family: serif; line-height: 1.2; }
div { display: block; }
p { display: block; margin: 16px; }
span { display: inline; }
a { display: inline; color: #0000ee; }
h1 { display: block; font-size: 32px; margin: 21px; }
h2 { display: block; font-size: 24px; margin: 19px; }
";

/// Parses the built-in user-agent stylesheet into the user-agent origin.
pub fn parse_user_agent_stylesheet() -> Stylesheet {
    parse_stylesheet(USER_AGENT_STYLESHEET, Origin::UserAgent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css_parser::{PropertyId, StyleRule};

    fn type_rule<'a>(sheet: &'a Stylesheet, type_name: &str) -> Option<&'a StyleRule> {
        sheet.rules().iter().find(|rule| {
            rule.selectors()
                .iter()
                .any(|selector| selector.type_name() == Some(type_name))
        })
    }

    fn display_of(sheet: &Stylesheet, type_name: &str) -> Option<String> {
        type_rule(sheet, type_name)?
            .declarations()
            .iter()
            .find(|declaration| declaration.property() == PropertyId::Display)
            .map(|declaration| declaration.value().to_owned())
    }

    #[test]
    fn the_sheet_parses_into_the_user_agent_origin() {
        let sheet = parse_user_agent_stylesheet();

        assert_eq!(sheet.origin(), Origin::UserAgent);
        assert!(!sheet.rules().is_empty());
    }

    #[test]
    fn block_and_inline_elements_get_their_default_display() {
        let sheet = parse_user_agent_stylesheet();

        assert_eq!(display_of(&sheet, "body").as_deref(), Some("block"));
        assert_eq!(display_of(&sheet, "div").as_deref(), Some("block"));
        assert_eq!(display_of(&sheet, "p").as_deref(), Some("block"));
        assert_eq!(display_of(&sheet, "span").as_deref(), Some("inline"));
        assert_eq!(display_of(&sheet, "a").as_deref(), Some("inline"));
        assert_eq!(display_of(&sheet, "head").as_deref(), Some("none"));
    }
}
