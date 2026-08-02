// @file engines/purr/engine/src/computed-style.rs
// @description Resolves cascaded values into an immutable computed style per element, committed per style generation.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Computed style and the style tree.
//!
//! This stage turns the cascaded specified values (from `style_cascade`) into a
//! full computed style for each element. For every property it takes the cascaded
//! winner when one exists, inherits the parent's computed value for an inherited
//! property, or falls back to the initial value. Resolution runs ancestor-first
//! over the tree, so a parent's computed style is committed before its children.
//!
//! A `ComputedStyle` holds only logical values: no percentage is resolved to
//! geometry, no `auto` is resolved, and no GPU or native type appears. Those
//! resolutions belong to layout.
//!
//! The commit is atomic and per generation. Resolving a document produces one
//! immutable `StyleTree` for a single `StyleGeneration`, mapping each element to
//! its computed style. Style identity is separate from DOM identity: the DOM owns
//! the nodes, the style tree owns the computed styles keyed by node identity, and
//! an element never mutates its computed style.

// Layout is the first consumer of the computed style and the style tree. Some
// entry points are otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::css_parser::{PropertyId, Stylesheet};
use crate::dom_node::{Dom, NodeId, NodeKind};
use crate::style_cascade::cascade_element;
use std::collections::HashMap;

const PROPERTY_COUNT: usize = PropertyId::ALL.len();

/// Marks one atomic style resolution over a document.
///
/// A style tree carries the generation it was committed for, so a later stage
/// (layout, fragment, paint) records which style generation it consumed. The
/// value is monotonic and opaque; the resolver commits one tree per generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleGeneration(u64);

impl StyleGeneration {
    /// The generation of the first style commit.
    pub const FIRST: Self = Self(1);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// The next generation.
    ///
    /// Uses checked arithmetic and fails safe. A `u64` generation cannot overflow
    /// in practice, so `None` is unreachable, but the resolver does not wrap a
    /// generation back to an earlier value.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// The immutable computed style of one element.
///
/// It stores one resolved logical value per M2 property, indexed by
/// `PropertyId::index`. The values are logical only. An element references its
/// computed style by identity and never mutates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedStyle {
    values: [String; PROPERTY_COUNT],
}

impl ComputedStyle {
    /// The resolved logical value of a property.
    pub fn get(&self, property: PropertyId) -> &str {
        &self.values[property.index()]
    }
}

/// The committed computed styles of one document for one style generation.
///
/// The tree is built atomically and is not mutated afterward. It maps each
/// element to its immutable computed style; non-element nodes carry no entry and
/// read their parent element's style at layout time.
pub struct StyleTree {
    generation: StyleGeneration,
    styles: HashMap<NodeId, ComputedStyle>,
}

impl StyleTree {
    /// The generation this tree was committed for.
    pub fn generation(&self) -> StyleGeneration {
        self.generation
    }

    /// The computed style of an element, or `None` for a non-element or an
    /// element without an entry.
    pub fn get(&self, element: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(&element)
    }

    /// The number of elements with a committed computed style.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }
}

/// Resolves the computed style of every element in the document.
///
/// Matches and cascades each element through `style_cascade`, then resolves
/// inheritance and initial values ancestor-first. Returns one immutable style
/// tree committed for `generation`.
pub fn resolve_document_style(
    dom: &Dom,
    user_agent: &Stylesheet,
    author: &Stylesheet,
    generation: StyleGeneration,
) -> StyleTree {
    let mut styles = HashMap::new();
    resolve_elements(dom, user_agent, author, &mut styles);
    StyleTree { generation, styles }
}

/// Walks the tree ancestor-first, committing one computed style per element.
///
/// The walk is an explicit pre-order stack, so a deeply nested document does not
/// recurse. Each frame carries the nearest element ancestor, whose committed
/// style supplies the inherited values. Ancestor-first order guarantees that
/// style is present before a child reads it.
fn resolve_elements(
    dom: &Dom,
    user_agent: &Stylesheet,
    author: &Stylesheet,
    styles: &mut HashMap<NodeId, ComputedStyle>,
) {
    let mut stack: Vec<(NodeId, Option<NodeId>)> = vec![(dom.root(), None)];
    while let Some((node, element_parent)) = stack.pop() {
        let is_element = dom.kind(node) == Some(NodeKind::Element);
        let child_parent = if is_element {
            let parent_style = element_parent.and_then(|parent| styles.get(&parent));
            let style = compute_style(dom, node, user_agent, author, parent_style);
            styles.insert(node, style);
            Some(node)
        } else {
            element_parent
        };

        if let Some(children) = dom.children(node) {
            for &child in children.iter().rev() {
                stack.push((child, child_parent));
            }
        }
    }
}

/// Resolves one element's computed style from its cascaded values.
fn compute_style(
    dom: &Dom,
    element: NodeId,
    user_agent: &Stylesheet,
    author: &Stylesheet,
    parent: Option<&ComputedStyle>,
) -> ComputedStyle {
    let cascaded = cascade_element(dom, element, user_agent, author);
    let values = std::array::from_fn(|index| {
        let property = PropertyId::ALL[index];
        resolve_value(property, cascaded.get(property), parent)
    });
    ComputedStyle { values }
}

/// Resolves one property: cascaded winner, else inheritance, else initial value.
fn resolve_value(
    property: PropertyId,
    cascaded: Option<&str>,
    parent: Option<&ComputedStyle>,
) -> String {
    if let Some(value) = cascaded {
        return value.to_owned();
    }

    if is_inherited(property)
        && let Some(parent) = parent
    {
        return parent.get(property).to_owned();
    }

    initial_value(property).to_owned()
}

/// Whether a property inherits its parent's computed value when unset.
fn is_inherited(property: PropertyId) -> bool {
    matches!(
        property,
        PropertyId::Color | PropertyId::FontSize | PropertyId::FontFamily | PropertyId::LineHeight
    )
}

/// The initial value of a property for the M2 set.
///
/// The initial `font-size` is the absolute medium size (`16px`); the other
/// initials are the CSS defaults for the property. Each value is logical.
fn initial_value(property: PropertyId) -> &'static str {
    match property {
        PropertyId::Display => "inline",
        PropertyId::Width | PropertyId::Height => "auto",
        PropertyId::MarginTop
        | PropertyId::MarginRight
        | PropertyId::MarginBottom
        | PropertyId::MarginLeft
        | PropertyId::PaddingTop
        | PropertyId::PaddingRight
        | PropertyId::PaddingBottom
        | PropertyId::PaddingLeft => "0",
        PropertyId::BackgroundColor => "transparent",
        PropertyId::Color => "#000000",
        PropertyId::FontSize => "16px",
        PropertyId::FontFamily => "serif",
        PropertyId::LineHeight => "normal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css_parser::{Origin, parse_stylesheet};

    fn ua(source: &str) -> Stylesheet {
        parse_stylesheet(source, Origin::UserAgent)
    }

    fn author(source: &str) -> Stylesheet {
        parse_stylesheet(source, Origin::Author)
    }

    /// Builds `<div><span></span></div>` under the document root and returns the
    /// two elements.
    fn div_with_span() -> (Dom, NodeId, NodeId) {
        let mut dom = Dom::new();
        let div = dom.create_element("div").expect("under the node cap");
        dom.append_child(dom.root(), div).expect("within depth");
        let span = dom.create_element("span").expect("under the node cap");
        dom.append_child(div, span).expect("within depth");
        (dom, div, span)
    }

    #[test]
    fn each_property_resolves_to_its_expected_logical_value() {
        let mut dom = Dom::new();
        let element = dom.create_element("div").expect("under the node cap");
        dom.append_child(dom.root(), element).expect("within depth");
        dom.set_attribute(element, "class", "box")
            .expect("sets class");

        let author = author(
            ".box { display: block; width: 100px; height: 40px; margin: 8px; padding: 4px; \
             background-color: #eee; color: #123456; font-size: 18px; font-family: sans-serif; \
             line-height: 1.5; }",
        );
        let tree = resolve_document_style(&dom, &ua(""), &author, StyleGeneration::FIRST);
        let style = tree.get(element).expect("the element has a computed style");

        assert_eq!(style.get(PropertyId::Display), "block");
        assert_eq!(style.get(PropertyId::Width), "100px");
        assert_eq!(style.get(PropertyId::Height), "40px");
        assert_eq!(style.get(PropertyId::MarginTop), "8px");
        assert_eq!(style.get(PropertyId::MarginLeft), "8px");
        assert_eq!(style.get(PropertyId::PaddingTop), "4px");
        assert_eq!(style.get(PropertyId::PaddingRight), "4px");
        assert_eq!(style.get(PropertyId::BackgroundColor), "#eee");
        assert_eq!(style.get(PropertyId::Color), "#123456");
        assert_eq!(style.get(PropertyId::FontSize), "18px");
        assert_eq!(style.get(PropertyId::FontFamily), "sans-serif");
        assert_eq!(style.get(PropertyId::LineHeight), "1.5");
    }

    #[test]
    fn an_inherited_property_flows_to_a_child() {
        let (dom, _div, span) = div_with_span();
        let author = author("div { color: #123456; }");

        let tree = resolve_document_style(&dom, &ua(""), &author, StyleGeneration::FIRST);
        let span_style = tree.get(span).expect("the span has a computed style");

        assert_eq!(span_style.get(PropertyId::Color), "#123456");
    }

    #[test]
    fn a_non_inherited_property_does_not_flow_to_a_child() {
        let (dom, _div, span) = div_with_span();
        let author = author("div { background-color: #eee; }");

        let tree = resolve_document_style(&dom, &ua(""), &author, StyleGeneration::FIRST);
        let span_style = tree.get(span).expect("the span has a computed style");

        assert_eq!(span_style.get(PropertyId::BackgroundColor), "transparent");
    }

    #[test]
    fn an_unset_inherited_property_falls_back_to_the_initial_value() {
        let (dom, div, _span) = div_with_span();

        let tree = resolve_document_style(&dom, &ua(""), &author(""), StyleGeneration::FIRST);
        let div_style = tree.get(div).expect("the div has a computed style");

        assert_eq!(div_style.get(PropertyId::Color), "#000000");
        assert_eq!(div_style.get(PropertyId::FontSize), "16px");
        assert_eq!(div_style.get(PropertyId::Display), "inline");
    }

    #[test]
    fn resolving_twice_yields_identical_computed_values() {
        let (dom, div, span) = div_with_span();
        let author = author("div { color: #111; } span { width: 5px; }");
        let ua = ua("div { display: block; }");

        let first = resolve_document_style(&dom, &ua, &author, StyleGeneration::FIRST);
        let second = resolve_document_style(&dom, &ua, &author, StyleGeneration::FIRST);

        assert_eq!(first.get(div), second.get(div));
        assert_eq!(first.get(span), second.get(span));
    }

    #[test]
    fn the_winner_is_stable_across_source_orders() {
        let mut dom = Dom::new();
        let element = dom.create_element("p").expect("under the node cap");
        dom.append_child(dom.root(), element).expect("within depth");
        dom.set_attribute(element, "class", "a")
            .expect("sets class");

        let forward = author("p.a { color: #222; } .a { color: #111; }");
        let reversed = author(".a { color: #111; } p.a { color: #222; }");

        let first = resolve_document_style(&dom, &ua(""), &forward, StyleGeneration::FIRST);
        let second = resolve_document_style(&dom, &ua(""), &reversed, StyleGeneration::FIRST);

        assert_eq!(
            first.get(element).map(|style| style.get(PropertyId::Color)),
            Some("#222")
        );
        assert_eq!(first.get(element), second.get(element));
    }

    #[test]
    fn two_generations_commit_independent_immutable_trees() {
        let (dom, div, _span) = div_with_span();
        let author = author("div { color: #111; }");

        let first = resolve_document_style(&dom, &ua(""), &author, StyleGeneration::FIRST);
        let second_generation = StyleGeneration::FIRST.next().expect("does not overflow");
        let second = resolve_document_style(&dom, &ua(""), &author, second_generation);

        assert_eq!(first.generation(), StyleGeneration::FIRST);
        assert_eq!(second.generation(), second_generation);
        assert_ne!(first.generation(), second.generation());
        assert_eq!(first.get(div), second.get(div));
    }

    #[test]
    fn only_elements_receive_a_computed_style() {
        let mut dom = Dom::new();
        let div = dom.create_element("div").expect("under the node cap");
        dom.append_child(dom.root(), div).expect("within depth");
        dom.append_text(div, "hello").expect("appends text");

        let tree = resolve_document_style(&dom, &ua(""), &author(""), StyleGeneration::FIRST);
        let text = dom.children(div).expect("div resolves")[0];

        assert!(tree.get(div).is_some());
        assert!(tree.get(text).is_none());
        assert_eq!(tree.len(), 1);
    }
}
