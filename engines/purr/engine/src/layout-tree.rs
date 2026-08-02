// @file engines/purr/engine/src/layout-tree.rs
// @description Builds the logical layout tree from the DOM and computed style, generating anonymous block boxes.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Layout tree construction.
//!
//! The layout tree is the logical box tree. It is derived from the DOM plus the
//! committed computed style, and it is distinct from both the DOM tree and the
//! later fragment tree. A DOM node maps to zero boxes (`display: none`, a comment,
//! a doctype, or collapsible whitespace), one box, or, together with its siblings,
//! an extra anonymous box.
//!
//! Box generation follows the block/inline split. An element box is a block box
//! when its computed `display` is `block`, and an inline box otherwise. A text
//! node with non-whitespace data is an inline box. When a block container holds
//! both block-level and inline-level children, each run of inline-level children
//! is wrapped in an anonymous block box, so the container holds only block-level
//! children. This keeps a block container either an inline formatting context (all
//! inline children) or a block formatting participant (all block children), never a
//! mix.
//!
//! The tree is bounded. An object-count cap and a depth cap are checked with
//! checked arithmetic while the tree is built, and an over-limit document fails
//! closed with a typed [`LayoutError`] instead of a panic. The same error type is
//! shared with the block-layout stage, which owns the fragment-count cap.

// The block-layout stage is the first non-test consumer of the tree builder and
// the box accessors. This phase adds the builder and exercises it through the unit
// tests, so some entry points are otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::computed_style::StyleTree;
use crate::css_parser::PropertyId;
use crate::dom_node::{Dom, NodeId, NodeKind};

/// Upper bound for the number of boxes in one layout tree.
///
/// The builder rejects a document whose box count, including the generated
/// anonymous boxes, would exceed this limit. The bound mirrors the DOM node cap
/// for the M2 slice; a later milestone raises it as the input space grows.
pub const MAX_LAYOUT_OBJECTS: usize = 65_536;

/// Upper bound for the depth of one layout tree.
///
/// The builder and the block-layout recursion both reject a box below this depth.
/// The bound keeps the recursive tree passes terminating on adversarial nesting.
pub const MAX_LAYOUT_DEPTH: usize = 512;

/// Failure the layout stages report to their caller.
///
/// The layout tree owns the object-count and depth caps; the block-layout stage
/// owns the fragment-count cap and the arithmetic-overflow case. The variants are
/// shared so a caller handles one error type across the two stages. Each message
/// is a static, factual, non-secret string.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    #[error("the layout tree reached its maximum object count")]
    TooManyObjects,
    #[error("the layout tree reached its maximum depth")]
    TooDeep,
    #[error("the fragment tree reached its maximum fragment count")]
    TooManyFragments,
    #[error("a layout value exceeded the fixed-point range")]
    Overflow,
}

/// The role of one layout box.
///
/// The set is closed for M2: a block box, an inline box, or an anonymous block box
/// generated to separate inline runs from block siblings. Positioned, float, flex,
/// grid, and table roles arrive in a later milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxKind {
    Block,
    Inline,
    AnonymousBlock,
}

/// One logical box in the layout tree.
///
/// A box carries its role, the DOM node it derives from (absent for an anonymous
/// box), and its child boxes in document order. The box is logical: it holds no
/// geometry. The block-layout stage reads the box together with the style tree and
/// produces the physical fragments.
pub struct LayoutBox {
    kind: BoxKind,
    node: Option<NodeId>,
    children: Vec<LayoutBox>,
}

impl LayoutBox {
    /// A block box for a DOM node.
    pub(crate) fn block(node: NodeId, children: Vec<LayoutBox>) -> Self {
        Self {
            kind: BoxKind::Block,
            node: Some(node),
            children,
        }
    }

    /// An inline box for a DOM node.
    pub(crate) fn inline(node: NodeId, children: Vec<LayoutBox>) -> Self {
        Self {
            kind: BoxKind::Inline,
            node: Some(node),
            children,
        }
    }

    /// An anonymous block box with no DOM node.
    pub(crate) fn anonymous_block(children: Vec<LayoutBox>) -> Self {
        Self {
            kind: BoxKind::AnonymousBlock,
            node: None,
            children,
        }
    }

    /// The role of the box.
    pub(crate) fn kind(&self) -> BoxKind {
        self.kind
    }

    /// The DOM node the box derives from, or `None` for an anonymous box.
    pub(crate) fn node(&self) -> Option<NodeId> {
        self.node
    }

    /// The child boxes in document order.
    pub(crate) fn children(&self) -> &[LayoutBox] {
        &self.children
    }

    /// Whether the box participates in block flow (a block or anonymous block).
    pub(crate) fn is_block_level(&self) -> bool {
        matches!(self.kind, BoxKind::Block | BoxKind::AnonymousBlock)
    }

    /// Whether the box is inline-level.
    pub(crate) fn is_inline_level(&self) -> bool {
        matches!(self.kind, BoxKind::Inline)
    }
}

/// Builds the layout tree for one document, or `None` when it generates no root.
///
/// The root box is the box of the document's first element child (the root
/// element). A document without a rendered root element, for example one whose
/// root element is `display: none`, produces no root box. Fails closed with a
/// typed error when the object or depth cap is exceeded.
pub(crate) fn build_layout_tree(
    dom: &Dom,
    styles: &StyleTree,
) -> Result<Option<LayoutBox>, LayoutError> {
    let Some(root_element) = root_element(dom) else {
        return Ok(None);
    };

    let mut object_count = 0usize;
    build_box(dom, styles, root_element, 0, &mut object_count)
}

/// The document's first element child, or `None` when it has none.
fn root_element(dom: &Dom) -> Option<NodeId> {
    dom.children(dom.root())?
        .iter()
        .copied()
        .find(|&node| dom.kind(node) == Some(NodeKind::Element))
}

/// Builds the box for one DOM node and its descendants.
///
/// Returns `None` when the node generates no box (`display: none`, a comment, a
/// doctype, or collapsible whitespace). A block box anonymizes its children so it
/// holds only block-level children.
fn build_box(
    dom: &Dom,
    styles: &StyleTree,
    node: NodeId,
    depth: usize,
    object_count: &mut usize,
) -> Result<Option<LayoutBox>, LayoutError> {
    if depth > MAX_LAYOUT_DEPTH {
        return Err(LayoutError::TooDeep);
    }

    let Some(kind) = box_kind(dom, styles, node) else {
        return Ok(None);
    };
    account_box(object_count)?;

    let mut children = Vec::new();
    if let Some(dom_children) = dom.children(node) {
        for &child in dom_children {
            let child_depth = depth.checked_add(1).ok_or(LayoutError::TooDeep)?;
            if let Some(child_box) = build_box(dom, styles, child, child_depth, object_count)? {
                children.push(child_box);
            }
        }
    }

    if kind == BoxKind::Block {
        children = anonymize(children, object_count)?;
    }

    let layout_box = match kind {
        BoxKind::Block => LayoutBox::block(node, children),
        BoxKind::Inline => LayoutBox::inline(node, children),
        BoxKind::AnonymousBlock => unreachable!("box_kind never returns an anonymous role"),
    };
    Ok(Some(layout_box))
}

/// The box role for a DOM node, or `None` when it generates no box.
///
/// An element uses its computed `display`: `none` generates no box, `block`
/// generates a block box, and every other value generates an inline box (the
/// initial `display` is inline). A non-whitespace text node generates an inline
/// box. Every other node kind generates no box.
fn box_kind(dom: &Dom, styles: &StyleTree, node: NodeId) -> Option<BoxKind> {
    match dom.kind(node)? {
        NodeKind::Element => {
            let display = styles.get(node)?.get(PropertyId::Display);
            match display {
                "none" => None,
                "block" => Some(BoxKind::Block),
                _ => Some(BoxKind::Inline),
            }
        }
        NodeKind::Text => {
            let data = dom.text_data(node)?;
            if is_collapsible_whitespace(data) {
                None
            } else {
                Some(BoxKind::Inline)
            }
        }
        NodeKind::Document | NodeKind::DocumentType | NodeKind::Comment => None,
    }
}

/// Whether text data is only whitespace and generates no box here.
///
/// Inline layout arrives in a later phase, so a run of whitespace between block
/// siblings is dropped now rather than producing a spurious anonymous box. This is
/// a deliberate M2 simplification of full white-space processing.
fn is_collapsible_whitespace(data: &str) -> bool {
    data.chars()
        .all(|character| character.is_ascii_whitespace())
}

/// Wraps runs of inline-level children in anonymous block boxes.
///
/// When the children hold both block-level and inline-level boxes, each maximal
/// run of inline-level boxes becomes one anonymous block box, so the returned
/// children are all block-level. When the children are all block-level or all
/// inline-level, they are returned unchanged: an all-inline block container is an
/// inline formatting context and needs no anonymous wrapper.
fn anonymize(
    children: Vec<LayoutBox>,
    object_count: &mut usize,
) -> Result<Vec<LayoutBox>, LayoutError> {
    let has_block = children.iter().any(LayoutBox::is_block_level);
    let has_inline = children.iter().any(LayoutBox::is_inline_level);
    if !(has_block && has_inline) {
        return Ok(children);
    }

    let mut wrapped = Vec::new();
    let mut inline_run: Vec<LayoutBox> = Vec::new();
    for child in children {
        if child.is_inline_level() {
            inline_run.push(child);
            continue;
        }
        flush_inline_run(&mut inline_run, &mut wrapped, object_count)?;
        wrapped.push(child);
    }
    flush_inline_run(&mut inline_run, &mut wrapped, object_count)?;
    Ok(wrapped)
}

/// Moves a pending inline run into an anonymous block box.
fn flush_inline_run(
    inline_run: &mut Vec<LayoutBox>,
    wrapped: &mut Vec<LayoutBox>,
    object_count: &mut usize,
) -> Result<(), LayoutError> {
    if inline_run.is_empty() {
        return Ok(());
    }
    account_box(object_count)?;
    wrapped.push(LayoutBox::anonymous_block(std::mem::take(inline_run)));
    Ok(())
}

/// Counts one box against the object cap with checked arithmetic.
fn account_box(object_count: &mut usize) -> Result<(), LayoutError> {
    let next = object_count
        .checked_add(1)
        .ok_or(LayoutError::TooManyObjects)?;
    if next > MAX_LAYOUT_OBJECTS {
        return Err(LayoutError::TooManyObjects);
    }
    *object_count = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computed_style::{StyleGeneration, resolve_document_style};
    use crate::css_parser::{Origin, Stylesheet, parse_stylesheet};
    use crate::user_agent_styles::parse_user_agent_stylesheet;

    fn author(source: &str) -> Stylesheet {
        parse_stylesheet(source, Origin::Author)
    }

    fn tree(dom: &Dom, author_sheet: &Stylesheet) -> StyleTree {
        resolve_document_style(
            dom,
            &parse_user_agent_stylesheet(),
            author_sheet,
            StyleGeneration::FIRST,
        )
    }

    #[test]
    fn a_block_element_generates_a_block_box() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let div = dom.create_element("div").expect("under the node cap");
        dom.append_child(html, div).expect("within depth");

        let styles = tree(&dom, &author(""));
        let root = build_layout_tree(&dom, &styles)
            .expect("within caps")
            .expect("a root box");

        assert_eq!(root.kind(), BoxKind::Block);
        assert_eq!(root.node(), Some(html));
        assert_eq!(root.children().len(), 1);
        assert_eq!(root.children()[0].kind(), BoxKind::Block);
    }

    #[test]
    fn a_display_none_root_generates_no_box() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");

        let styles = tree(&dom, &author("html { display: none; }"));
        let root = build_layout_tree(&dom, &styles).expect("within caps");

        assert!(root.is_none());
    }

    #[test]
    fn mixed_inline_and_block_children_generate_an_anonymous_wrapper() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let container = dom.create_element("div").expect("under the node cap");
        dom.append_child(html, container).expect("within depth");
        dom.append_text(container, "hello").expect("appends text");
        let block_child = dom.create_element("div").expect("under the node cap");
        dom.append_child(container, block_child)
            .expect("within depth");

        let styles = tree(&dom, &author(""));
        let root = build_layout_tree(&dom, &styles)
            .expect("within caps")
            .expect("a root box");
        let container_box = &root.children()[0];

        assert_eq!(container_box.children().len(), 2);
        let anonymous = &container_box.children()[0];
        assert_eq!(anonymous.kind(), BoxKind::AnonymousBlock);
        assert_eq!(anonymous.node(), None);
        assert_eq!(anonymous.children().len(), 1);
        assert_eq!(anonymous.children()[0].kind(), BoxKind::Inline);
        assert_eq!(container_box.children()[1].kind(), BoxKind::Block);
    }

    #[test]
    fn all_inline_children_are_not_anonymized() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let paragraph = dom.create_element("p").expect("under the node cap");
        dom.append_child(html, paragraph).expect("within depth");
        dom.append_text(paragraph, "hello ").expect("appends text");
        let emphasis = dom.create_element("span").expect("under the node cap");
        dom.append_child(paragraph, emphasis).expect("within depth");

        let styles = tree(&dom, &author(""));
        let root = build_layout_tree(&dom, &styles)
            .expect("within caps")
            .expect("a root box");
        let paragraph_box = &root.children()[0];

        assert_eq!(paragraph_box.children().len(), 2);
        assert!(
            paragraph_box
                .children()
                .iter()
                .all(LayoutBox::is_inline_level)
        );
    }

    #[test]
    fn collapsible_whitespace_between_blocks_generates_no_box() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let first = dom.create_element("div").expect("under the node cap");
        dom.append_child(html, first).expect("within depth");
        dom.append_text(html, "\n   ").expect("appends whitespace");
        let second = dom.create_element("div").expect("under the node cap");
        dom.append_child(html, second).expect("within depth");

        let styles = tree(&dom, &author(""));
        let root = build_layout_tree(&dom, &styles)
            .expect("within caps")
            .expect("a root box");

        assert_eq!(root.children().len(), 2);
        assert!(root.children().iter().all(LayoutBox::is_block_level));
    }
}
