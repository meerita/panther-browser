// @file engines/purr/engine/src/style-cascade.rs
// @description Matches selectors against elements and cascades the matched declarations into per-property winners.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Selector matching and the cascade.
//!
//! Matching and cascade are two separate stages. The matcher collects, for one
//! element, the declarations of every rule whose selector matches (a brute-force
//! full match, sanctioned for the minimal prototype). The cascade then orders the
//! matched candidates by origin, then specificity, then source order, with
//! importance folded into the origin layer, and selects one deterministic winner
//! per property.
//!
//! Winner selection is independent of hash-map or iteration order: the cascade
//! keeps the candidate with the strictly greater cascade key, and the key is a
//! total order with no ties across the two M2 origins, so the result is
//! deterministic.
//!
//! This stage yields only the winning specified value per property. The
//! computed-style stage adds inheritance and initial values on top.

// The computed-style stage is the first consumer of this stage; layout consumes
// the computed style it produces. Some entry points are otherwise unused in a
// non-test build.
#![allow(dead_code)]

use crate::css_parser::{Origin, PropertyId, Selector, Specificity, Stylesheet};
use crate::dom_node::{Dom, NodeId};

const PROPERTY_COUNT: usize = PropertyId::ALL.len();

/// The cascaded specified values for one element.
///
/// Each entry is the winning declaration's specified value, or `None` when no
/// rule set the property. The computed-style stage turns this into a full
/// immutable computed style by resolving inheritance and initial values.
pub struct CascadedValues<'sheet> {
    values: [Option<&'sheet str>; PROPERTY_COUNT],
}

impl<'sheet> CascadedValues<'sheet> {
    /// The winning specified value for a property, or `None` when unset.
    pub fn get(&self, property: PropertyId) -> Option<&'sheet str> {
        self.values[property.index()]
    }
}

/// Matches and cascades the two M2 origins for one element.
///
/// The user-agent origin is collected before the author origin, but order does
/// not affect the winner: the cascade compares full keys and keeps the strictly
/// greater one.
pub fn cascade_element<'sheet>(
    dom: &Dom,
    element: NodeId,
    user_agent: &'sheet Stylesheet,
    author: &'sheet Stylesheet,
) -> CascadedValues<'sheet> {
    let mut winners: [Option<Candidate<'sheet>>; PROPERTY_COUNT] = [None; PROPERTY_COUNT];
    collect_sheet(dom, element, user_agent, &mut winners);
    collect_sheet(dom, element, author, &mut winners);

    let mut values: [Option<&'sheet str>; PROPERTY_COUNT] = [None; PROPERTY_COUNT];
    for (slot, winner) in values.iter_mut().zip(winners.iter()) {
        *slot = winner.map(|candidate| candidate.value);
    }
    CascadedValues { values }
}

/// One matched declaration competing in the cascade.
///
/// The key is `(layer, specificity, source_order)`. The layer folds origin and
/// importance; specificity and source order break the remaining ties. The key is
/// a total order with no ties across the two M2 origins.
#[derive(Debug, Clone, Copy)]
struct Candidate<'sheet> {
    layer: u8,
    specificity: Specificity,
    source_order: u32,
    value: &'sheet str,
}

impl Candidate<'_> {
    fn key(&self) -> (u8, Specificity, u32) {
        (self.layer, self.specificity, self.source_order)
    }
}

/// Collects the matched declarations of one sheet into the running winners.
fn collect_sheet<'sheet>(
    dom: &Dom,
    element: NodeId,
    sheet: &'sheet Stylesheet,
    winners: &mut [Option<Candidate<'sheet>>; PROPERTY_COUNT],
) {
    let origin = sheet.origin();
    for rule in sheet.rules() {
        let Some(specificity) = best_match_specificity(dom, element, rule.selectors()) else {
            continue;
        };

        for declaration in rule.declarations() {
            if !declaration.is_valid() {
                continue;
            }
            let candidate = Candidate {
                layer: cascade_layer(origin, declaration.important()),
                specificity,
                source_order: declaration.source_order(),
                value: declaration.value(),
            };
            consider(&mut winners[declaration.property().index()], candidate);
        }
    }
}

/// Keeps the candidate with the strictly greater cascade key.
fn consider<'sheet>(slot: &mut Option<Candidate<'sheet>>, candidate: Candidate<'sheet>) {
    let wins = match slot {
        None => true,
        Some(current) => candidate.key() > current.key(),
    };
    if wins {
        *slot = Some(candidate);
    }
}

/// The highest specificity among the selectors that match, or `None`.
fn best_match_specificity(
    dom: &Dom,
    element: NodeId,
    selectors: &[Selector],
) -> Option<Specificity> {
    selectors
        .iter()
        .filter(|selector| selector_matches(dom, element, selector))
        .map(Selector::specificity)
        .max()
}

/// Whether a compound selector matches an element.
///
/// The type name matches by exact local name (HTML tag names are lowercased on
/// tokenization); the id and each class match the element's `id` and `class`
/// attribute values, which are case-sensitive.
fn selector_matches(dom: &Dom, element: NodeId, selector: &Selector) -> bool {
    if let Some(type_name) = selector.type_name()
        && dom.local_name(element) != Some(type_name)
    {
        return false;
    }

    if let Some(id) = selector.id()
        && element_id(dom, element) != Some(id)
    {
        return false;
    }

    selector
        .classes()
        .iter()
        .all(|class| element_has_class(dom, element, class))
}

/// The value of the element's `id` attribute, or `None`.
fn element_id(dom: &Dom, element: NodeId) -> Option<&str> {
    dom.attributes(element)?
        .iter()
        .find(|attribute| attribute.name == "id")
        .map(|attribute| attribute.value.as_str())
}

/// Whether the element's `class` attribute lists the given class token.
fn element_has_class(dom: &Dom, element: NodeId, class: &str) -> bool {
    let Some(attributes) = dom.attributes(element) else {
        return false;
    };
    attributes
        .iter()
        .filter(|attribute| attribute.name == "class")
        .any(|attribute| {
            attribute
                .value
                .split_whitespace()
                .any(|token| token == class)
        })
}

/// Folds origin and importance into one cascade layer.
///
/// Higher wins. Importance reverses the origin precedence, so an important
/// user-agent declaration outranks an important author one, matching the cascade
/// order for the two M2 origins.
fn cascade_layer(origin: Origin, important: bool) -> u8 {
    match (origin, important) {
        (Origin::UserAgent, false) => 0,
        (Origin::Author, false) => 1,
        (Origin::Author, true) => 2,
        (Origin::UserAgent, true) => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css_parser::parse_stylesheet;
    use crate::dom_node::Dom;

    fn element_with(local_name: &str, attributes: &[(&str, &str)]) -> (Dom, NodeId) {
        let mut dom = Dom::new();
        let element = dom.create_element(local_name).expect("under the node cap");
        dom.append_child(dom.root(), element)
            .expect("within the depth cap");
        for (name, value) in attributes {
            dom.set_attribute(element, name, value)
                .expect("sets the attribute");
        }
        (dom, element)
    }

    #[test]
    fn a_type_selector_matches_by_local_name() {
        let (dom, element) = element_with("p", &[]);
        let author = parse_stylesheet("p { color: #111; } div { color: #222; }", Origin::Author);
        let ua = parse_stylesheet("", Origin::UserAgent);

        let cascaded = cascade_element(&dom, element, &ua, &author);
        assert_eq!(cascaded.get(PropertyId::Color), Some("#111"));
    }

    #[test]
    fn a_class_and_id_selector_match_the_attributes() {
        let (dom, element) = element_with("div", &[("class", "card lead"), ("id", "main")]);
        let author = parse_stylesheet(
            ".card { width: 10px; } #main { height: 20px; } .missing { color: #111; }",
            Origin::Author,
        );
        let ua = parse_stylesheet("", Origin::UserAgent);

        let cascaded = cascade_element(&dom, element, &ua, &author);
        assert_eq!(cascaded.get(PropertyId::Width), Some("10px"));
        assert_eq!(cascaded.get(PropertyId::Height), Some("20px"));
        assert_eq!(cascaded.get(PropertyId::Color), None);
    }

    #[test]
    fn higher_specificity_author_rule_wins() {
        let (dom, element) = element_with("div", &[("class", "card")]);
        let author = parse_stylesheet(
            "div.card { color: #222; } .card { color: #111; }",
            Origin::Author,
        );
        let ua = parse_stylesheet("", Origin::UserAgent);

        let cascaded = cascade_element(&dom, element, &ua, &author);
        assert_eq!(cascaded.get(PropertyId::Color), Some("#222"));
    }

    #[test]
    fn author_wins_over_user_agent_at_equal_specificity() {
        let (dom, element) = element_with("div", &[]);
        let ua = parse_stylesheet("div { color: #000; }", Origin::UserAgent);
        let author = parse_stylesheet("div { color: #222; }", Origin::Author);

        let cascaded = cascade_element(&dom, element, &ua, &author);
        assert_eq!(cascaded.get(PropertyId::Color), Some("#222"));
    }

    #[test]
    fn important_wins_over_normal_in_the_same_origin() {
        let (dom, element) = element_with("div", &[("class", "x")]);
        let author = parse_stylesheet(
            ".x { color: #222 !important; } .x { color: #111; }",
            Origin::Author,
        );
        let ua = parse_stylesheet("", Origin::UserAgent);

        let cascaded = cascade_element(&dom, element, &ua, &author);
        assert_eq!(cascaded.get(PropertyId::Color), Some("#222"));
    }

    #[test]
    fn important_user_agent_wins_over_normal_author() {
        let (dom, element) = element_with("div", &[]);
        let ua = parse_stylesheet("div { color: #000 !important; }", Origin::UserAgent);
        let author = parse_stylesheet("div { color: #fff; }", Origin::Author);

        let cascaded = cascade_element(&dom, element, &ua, &author);
        assert_eq!(cascaded.get(PropertyId::Color), Some("#000"));
    }

    #[test]
    fn the_winner_is_independent_of_source_order() {
        let (dom, element) = element_with("p", &[("class", "a")]);
        let ua = parse_stylesheet("", Origin::UserAgent);

        let forward = parse_stylesheet(".a { color: #111; } p.a { color: #222; }", Origin::Author);
        let reversed = parse_stylesheet("p.a { color: #222; } .a { color: #111; }", Origin::Author);

        let first = cascade_element(&dom, element, &ua, &forward);
        let second = cascade_element(&dom, element, &ua, &reversed);
        assert_eq!(first.get(PropertyId::Color), Some("#222"));
        assert_eq!(second.get(PropertyId::Color), Some("#222"));
    }

    #[test]
    fn a_non_matching_element_gets_no_values() {
        let (dom, element) = element_with("span", &[]);
        let author = parse_stylesheet("div { color: #111; }", Origin::Author);
        let ua = parse_stylesheet("", Origin::UserAgent);

        let cascaded = cascade_element(&dom, element, &ua, &author);
        assert_eq!(cascaded.get(PropertyId::Color), None);
    }
}
