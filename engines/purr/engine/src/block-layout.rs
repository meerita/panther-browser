// @file engines/purr/engine/src/block-layout.rs
// @description Lays out block boxes into an immutable fragment tree with box sizing and margin collapsing.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Block formatting context.
//!
//! This stage consumes the logical layout tree and the committed computed style
//! and produces an immutable fragment tree of block box fragments. It runs the
//! block formatting context: in-flow block boxes stack top to bottom, each box is
//! sized from `width`, `height`, `margin`, and `padding`, and adjacent margins
//! collapse.
//!
//! The box tree stays logical and separate from the fragment tree, which is
//! physical. Writing mode is horizontal top-to-bottom and direction is
//! left-to-right for M2, so a logical coordinate equals its physical coordinate
//! and no axis conversion is needed. A box fragment carries a border-box rectangle
//! in [`LayoutUnit`], in document-local coordinates with the origin at `(0, 0)`.
//! The box model is content-box: `width` and `height` size the content, and
//! padding extends the border box around it.
//!
//! Inline content is deferred to the inline formatting context. A block box that
//! holds only inline content contributes no in-flow block child here, so its
//! auto content height is zero until the inline stage measures its lines.
//!
//! All arithmetic is checked. A value that leaves the fixed-point range and a tree
//! that exceeds the fragment cap or the depth cap abort with a typed
//! [`LayoutError`] instead of wrapping or panicking. The stage owns no pixels: it
//! produces geometry only. The layout result records the style generation it
//! consumed, extending the style-to-layout generation chain.

// The paint stage is the first non-test consumer of the fragment tree and the
// layout result. This phase adds them and exercises them through the unit tests,
// so some entry points are otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::computed_style::{ComputedStyle, StyleGeneration, StyleTree};
use crate::css_parser::PropertyId;
use crate::dom_node::{Dom, NodeId};
use crate::layout_tree::{LayoutBox, LayoutError, MAX_LAYOUT_DEPTH, build_layout_tree};
use crate::layout_unit::{LayoutSize, LayoutUnit, LogicalPoint, LogicalRect, LogicalSize};

/// Upper bound for the number of fragments in one fragment tree.
///
/// The stage rejects a layout that would produce more fragments than this. The
/// bound mirrors the layout-object cap for the M2 slice.
pub const MAX_FRAGMENTS: usize = 65_536;

/// The immutable input to a block layout algorithm.
///
/// It carries the available inline size (the containing-block content width) and
/// the available block size (definite for a fixed viewport, indefinite when the
/// block axis grows with content). It has no mutable parent pointer and is not
/// changed once built, so a child algorithm reads a stable, explicit constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstraintSpace {
    available_inline_size: LayoutUnit,
    available_block_size: LayoutSize,
}

impl ConstraintSpace {
    pub fn new(available_inline_size: LayoutUnit, available_block_size: LayoutSize) -> Self {
        Self {
            available_inline_size,
            available_block_size,
        }
    }

    pub fn available_inline_size(&self) -> LayoutUnit {
        self.available_inline_size
    }

    pub fn available_block_size(&self) -> LayoutSize {
        self.available_block_size
    }
}

/// One immutable box fragment in document-local physical coordinates.
///
/// A fragment carries the border-box rectangle and its child fragments. It records
/// the DOM node it derives from, or `None` for an anonymous box, so the paint
/// stage reads the box background and text style from the style tree by node
/// identity. A fragment is not mutated after the layout result commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxFragment {
    node: Option<NodeId>,
    rect: LogicalRect,
    children: Vec<BoxFragment>,
}

impl BoxFragment {
    pub fn node(&self) -> Option<NodeId> {
        self.node
    }

    pub fn rect(&self) -> LogicalRect {
        self.rect
    }

    pub fn children(&self) -> &[BoxFragment] {
        &self.children
    }
}

/// The immutable result of laying out one document.
///
/// It carries the root box fragment, absent when the document generates no box,
/// and the style generation the layout consumed. The generation extends the
/// style-to-layout chain, so a later stage rejects a fragment tree built from a
/// superseded style generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResult {
    style_generation: StyleGeneration,
    root: Option<BoxFragment>,
}

impl LayoutResult {
    pub fn style_generation(&self) -> StyleGeneration {
        self.style_generation
    }

    pub fn root(&self) -> Option<&BoxFragment> {
        self.root.as_ref()
    }
}

/// Lays out one document into an immutable layout result.
///
/// Builds the layout tree, lays out the root block box against `constraint`, and
/// commits the fragment tree together with the consumed style generation. Fails
/// closed with a typed error when a cap is exceeded or a value leaves the
/// fixed-point range.
pub fn layout_document(
    dom: &Dom,
    styles: &StyleTree,
    constraint: &ConstraintSpace,
) -> Result<LayoutResult, LayoutError> {
    let generation = styles.generation();
    let Some(root_box) = build_layout_tree(dom, styles)? else {
        return Ok(LayoutResult {
            style_generation: generation,
            root: None,
        });
    };

    let root = layout_root(&root_box, styles, constraint)?;
    Ok(LayoutResult {
        style_generation: generation,
        root: Some(root),
    })
}

/// Lays out a root box and flattens its subtree into absolute coordinates.
///
/// The root box is positioned in the initial containing block: its inline margin
/// offsets it along the inline axis, and its collapsed top margin offsets it along
/// the block axis, so a margin that collapses through to the top of the document
/// appears once above the root.
pub(crate) fn layout_root(
    root_box: &LayoutBox,
    styles: &StyleTree,
    constraint: &ConstraintSpace,
) -> Result<BoxFragment, LayoutError> {
    let mut fragment_count = 0usize;
    let laid = layout_block(
        root_box,
        styles,
        constraint.available_inline_size,
        0,
        &mut fragment_count,
    )?;

    let origin = LogicalPoint::new(laid.margin.left, laid.top_collapse.solve()?);
    flatten(laid.node, origin)
}

/// A block box laid out in coordinates relative to its own border-box origin.
///
/// The stage returns this from a child so the parent can position it. It carries
/// the fragment subtree (with the root offset still unset), the box border-box
/// size, the box own margins, and the margins that adjoin the top and bottom edges
/// for collapsing with an ancestor or a sibling.
struct LaidOutBox {
    node: FragmentNode,
    border_box_size: LogicalSize,
    margin: EdgeSizes,
    top_collapse: CollapsibleMargins,
    bottom_collapse: CollapsibleMargins,
}

/// A mutable fragment node during layout, offset relative to its parent.
///
/// The offset is set by the parent when it places the child, and is relative to
/// the parent border-box origin. The flatten pass turns the relative offsets into
/// absolute document-local coordinates and freezes the immutable fragment tree.
struct FragmentNode {
    node_id: Option<NodeId>,
    offset: LogicalPoint,
    size: LogicalSize,
    children: Vec<FragmentNode>,
}

/// The four edge lengths of a box in the physical axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EdgeSizes {
    top: LayoutUnit,
    right: LayoutUnit,
    bottom: LayoutUnit,
    left: LayoutUnit,
}

impl EdgeSizes {
    const ZERO: Self = Self {
        top: LayoutUnit::ZERO,
        right: LayoutUnit::ZERO,
        bottom: LayoutUnit::ZERO,
        left: LayoutUnit::ZERO,
    };
}

/// The resolved box-model inputs of one box.
struct ResolvedBox {
    margin: EdgeSizes,
    padding: EdgeSizes,
    inline_size: LayoutSize,
    block_size: LayoutSize,
}

/// The set of margins that adjoin one edge and may collapse.
///
/// Collapsing margins do not add: a set collapses to the sum of its largest
/// positive margin and its most negative margin. The set keeps those two extremes
/// so several adjoining margins collapse to one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CollapsibleMargins {
    max_positive: LayoutUnit,
    min_negative: LayoutUnit,
}

impl CollapsibleMargins {
    const ZERO: Self = Self {
        max_positive: LayoutUnit::ZERO,
        min_negative: LayoutUnit::ZERO,
    };

    /// The set of a single margin.
    fn from_margin(margin: LayoutUnit) -> Self {
        if margin >= LayoutUnit::ZERO {
            Self {
                max_positive: margin,
                min_negative: LayoutUnit::ZERO,
            }
        } else {
            Self {
                max_positive: LayoutUnit::ZERO,
                min_negative: margin,
            }
        }
    }

    /// The union of two adjoining sets.
    fn combine(self, other: Self) -> Self {
        Self {
            max_positive: self.max_positive.max(other.max_positive),
            min_negative: self.min_negative.min(other.min_negative),
        }
    }

    /// The single collapsed margin of the set.
    fn solve(self) -> Result<LayoutUnit, LayoutError> {
        self.max_positive
            .checked_add(self.min_negative)
            .ok_or(LayoutError::Overflow)
    }
}

/// Lays out one block box and its in-flow block descendants.
///
/// Returns the box in coordinates relative to its own border-box origin, plus the
/// collapsing metadata its parent needs. Only block-level children participate:
/// inline children contribute no block geometry in this phase.
fn layout_block(
    layout_box: &LayoutBox,
    styles: &StyleTree,
    available_inline: LayoutUnit,
    depth: usize,
    fragment_count: &mut usize,
) -> Result<LaidOutBox, LayoutError> {
    if depth > MAX_LAYOUT_DEPTH {
        return Err(LayoutError::TooDeep);
    }
    account_fragment(fragment_count)?;

    let resolved = resolve_box(layout_box, styles);
    let content_width = resolve_content_width(&resolved, available_inline)?;
    let border_box_width = sum3(content_width, resolved.padding.left, resolved.padding.right)?;

    let flow = layout_children(
        layout_box,
        styles,
        &resolved,
        content_width,
        depth,
        fragment_count,
    )?;

    let content_height = match resolved.block_size {
        LayoutSize::Definite(height) => height.max(LayoutUnit::ZERO),
        LayoutSize::Indefinite => flow.content_height,
    };
    let border_box_height = sum3(
        content_height,
        resolved.padding.top,
        resolved.padding.bottom,
    )?;

    let bottom_collapse = match resolved.block_size {
        LayoutSize::Definite(_) => CollapsibleMargins::from_margin(resolved.margin.bottom),
        LayoutSize::Indefinite => flow.bottom_collapse,
    };

    let node = FragmentNode {
        node_id: layout_box.node(),
        offset: LogicalPoint::default(),
        size: LogicalSize::new(border_box_width, border_box_height),
        children: flow.children,
    };
    Ok(LaidOutBox {
        node,
        border_box_size: LogicalSize::new(border_box_width, border_box_height),
        margin: resolved.margin,
        top_collapse: flow.top_collapse,
        bottom_collapse,
    })
}

/// The result of laying out the in-flow block children of one box.
struct ChildFlow {
    children: Vec<FragmentNode>,
    content_height: LayoutUnit,
    top_collapse: CollapsibleMargins,
    bottom_collapse: CollapsibleMargins,
}

/// Places the in-flow block children top to bottom with margin collapsing.
///
/// Each child is positioned in coordinates relative to the parent border-box
/// origin. Adjacent sibling margins collapse. The parent top margin collapses with
/// the first child top margin when no top padding separates them, and the parent
/// bottom margin collapses with the last child bottom margin when no bottom padding
/// separates them and the parent block size is auto.
fn layout_children(
    parent: &LayoutBox,
    styles: &StyleTree,
    parent_box: &ResolvedBox,
    content_width: LayoutUnit,
    depth: usize,
    fragment_count: &mut usize,
) -> Result<ChildFlow, LayoutError> {
    let padding_top_zero = parent_box.padding.top == LayoutUnit::ZERO;
    let padding_bottom_zero = parent_box.padding.bottom == LayoutUnit::ZERO;
    let block_size_auto = parent_box.block_size.is_indefinite();

    let mut children = Vec::new();
    let mut flow_y = LayoutUnit::ZERO;
    let mut prev_bottom = CollapsibleMargins::ZERO;
    let mut placed_any = false;
    let mut top_collapse = CollapsibleMargins::from_margin(parent_box.margin.top);

    for child in parent
        .children()
        .iter()
        .filter(|child| child.is_block_level())
    {
        let child_depth = depth.checked_add(1).ok_or(LayoutError::TooDeep)?;
        let laid = layout_block(child, styles, content_width, child_depth, fragment_count)?;

        let child_border_top = if !placed_any {
            if padding_top_zero {
                top_collapse = CollapsibleMargins::from_margin(parent_box.margin.top)
                    .combine(laid.top_collapse);
                LayoutUnit::ZERO
            } else {
                laid.top_collapse.solve()?
            }
        } else {
            let gap = prev_bottom.combine(laid.top_collapse).solve()?;
            flow_y.checked_add(gap).ok_or(LayoutError::Overflow)?
        };

        let inline_offset = parent_box
            .padding
            .left
            .checked_add(laid.margin.left)
            .ok_or(LayoutError::Overflow)?;
        let block_offset = parent_box
            .padding
            .top
            .checked_add(child_border_top)
            .ok_or(LayoutError::Overflow)?;

        let mut node = laid.node;
        node.offset = LogicalPoint::new(inline_offset, block_offset);
        children.push(node);

        flow_y = child_border_top
            .checked_add(laid.border_box_size.height)
            .ok_or(LayoutError::Overflow)?;
        prev_bottom = laid.bottom_collapse;
        placed_any = true;
    }

    let (content_height, bottom_collapse) = if !placed_any {
        (
            LayoutUnit::ZERO,
            CollapsibleMargins::from_margin(parent_box.margin.bottom),
        )
    } else if block_size_auto && padding_bottom_zero {
        (
            flow_y,
            prev_bottom.combine(CollapsibleMargins::from_margin(parent_box.margin.bottom)),
        )
    } else {
        let height = flow_y
            .checked_add(prev_bottom.solve()?)
            .ok_or(LayoutError::Overflow)?;
        (
            height,
            CollapsibleMargins::from_margin(parent_box.margin.bottom),
        )
    };

    Ok(ChildFlow {
        children,
        content_height,
        top_collapse,
        bottom_collapse,
    })
}

/// Resolves the box-model inputs of one box from its computed style.
///
/// An anonymous box, or a box whose element has no computed style, uses the
/// defaults: zero margins and padding and auto sizes. Padding is clamped to be
/// non-negative; a margin keeps its sign so a negative margin collapses correctly.
fn resolve_box(layout_box: &LayoutBox, styles: &StyleTree) -> ResolvedBox {
    let Some(style) = layout_box.node().and_then(|node| styles.get(node)) else {
        return ResolvedBox {
            margin: EdgeSizes::ZERO,
            padding: EdgeSizes::ZERO,
            inline_size: LayoutSize::Indefinite,
            block_size: LayoutSize::Indefinite,
        };
    };

    ResolvedBox {
        margin: EdgeSizes {
            top: margin_length(style, PropertyId::MarginTop),
            right: margin_length(style, PropertyId::MarginRight),
            bottom: margin_length(style, PropertyId::MarginBottom),
            left: margin_length(style, PropertyId::MarginLeft),
        },
        padding: EdgeSizes {
            top: padding_length(style, PropertyId::PaddingTop),
            right: padding_length(style, PropertyId::PaddingRight),
            bottom: padding_length(style, PropertyId::PaddingBottom),
            left: padding_length(style, PropertyId::PaddingLeft),
        },
        inline_size: dimension(style, PropertyId::Width),
        block_size: dimension(style, PropertyId::Height),
    }
}

/// Resolves the used content width of a box.
///
/// A definite `width` is the content width. An auto `width` fills the available
/// inline size after the box own margins and padding, and never goes below zero.
fn resolve_content_width(
    resolved: &ResolvedBox,
    available_inline: LayoutUnit,
) -> Result<LayoutUnit, LayoutError> {
    match resolved.inline_size {
        LayoutSize::Definite(width) => Ok(width.max(LayoutUnit::ZERO)),
        LayoutSize::Indefinite => {
            let used = available_inline
                .checked_sub(resolved.margin.left)
                .and_then(|value| value.checked_sub(resolved.margin.right))
                .and_then(|value| value.checked_sub(resolved.padding.left))
                .and_then(|value| value.checked_sub(resolved.padding.right))
                .ok_or(LayoutError::Overflow)?;
            Ok(used.max(LayoutUnit::ZERO))
        }
    }
}

/// A margin length, or zero when the value is `auto` or not a pixel length.
fn margin_length(style: &ComputedStyle, property: PropertyId) -> LayoutUnit {
    parse_px_length(style.get(property)).unwrap_or(LayoutUnit::ZERO)
}

/// A padding length clamped to be non-negative, defaulting to zero.
fn padding_length(style: &ComputedStyle, property: PropertyId) -> LayoutUnit {
    parse_px_length(style.get(property))
        .unwrap_or(LayoutUnit::ZERO)
        .max(LayoutUnit::ZERO)
}

/// A width or height, definite for a pixel length and indefinite for `auto`.
fn dimension(style: &ComputedStyle, property: PropertyId) -> LayoutSize {
    match parse_px_length(style.get(property)) {
        Some(length) => LayoutSize::Definite(length.max(LayoutUnit::ZERO)),
        None => LayoutSize::Indefinite,
    }
}

/// Parses a computed length to a fixed-point unit.
///
/// Accepts a bare `0` and an integer pixel length such as `16px`. A non-length
/// value, including `auto` and a percentage, returns `None` so the caller applies
/// the property default. M2 has no fractional or non-pixel lengths in the box
/// model.
fn parse_px_length(value: &str) -> Option<LayoutUnit> {
    let value = value.trim();
    if value == "0" {
        return Some(LayoutUnit::ZERO);
    }
    let digits = value.strip_suffix("px")?;
    let pixels: i32 = digits.trim().parse().ok()?;
    LayoutUnit::from_px(pixels)
}

/// Flattens a relative fragment subtree into absolute document-local coordinates.
///
/// The recursion depth is bounded by the same depth cap the layout enforces, so a
/// well-formed subtree terminates.
fn flatten(node: FragmentNode, origin: LogicalPoint) -> Result<BoxFragment, LayoutError> {
    let x = origin
        .x
        .checked_add(node.offset.x)
        .ok_or(LayoutError::Overflow)?;
    let y = origin
        .y
        .checked_add(node.offset.y)
        .ok_or(LayoutError::Overflow)?;
    let absolute = LogicalPoint::new(x, y);

    let mut children = Vec::with_capacity(node.children.len());
    for child in node.children {
        children.push(flatten(child, absolute)?);
    }

    Ok(BoxFragment {
        node: node.node_id,
        rect: LogicalRect::new(absolute, node.size),
        children,
    })
}

/// Sums three lengths with checked arithmetic.
fn sum3(a: LayoutUnit, b: LayoutUnit, c: LayoutUnit) -> Result<LayoutUnit, LayoutError> {
    a.checked_add(b)
        .and_then(|value| value.checked_add(c))
        .ok_or(LayoutError::Overflow)
}

/// Counts one fragment against the fragment cap with checked arithmetic.
fn account_fragment(fragment_count: &mut usize) -> Result<(), LayoutError> {
    let next = fragment_count
        .checked_add(1)
        .ok_or(LayoutError::TooManyFragments)?;
    if next > MAX_FRAGMENTS {
        return Err(LayoutError::TooManyFragments);
    }
    *fragment_count = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computed_style::{StyleGeneration, resolve_document_style};
    use crate::css_parser::{Origin, Stylesheet, parse_stylesheet};
    use crate::user_agent_styles::parse_user_agent_stylesheet;

    fn px(value: i32) -> LayoutUnit {
        LayoutUnit::from_px(value).expect("in range")
    }

    fn author(source: &str) -> Stylesheet {
        parse_stylesheet(source, Origin::Author)
    }

    fn style_tree(dom: &Dom, author_sheet: &Stylesheet) -> StyleTree {
        resolve_document_style(
            dom,
            &parse_user_agent_stylesheet(),
            author_sheet,
            StyleGeneration::FIRST,
        )
    }

    fn constraint(inline_px: i32) -> ConstraintSpace {
        ConstraintSpace::new(px(inline_px), LayoutSize::Indefinite)
    }

    /// Finds the first fragment for a DOM node in the tree.
    fn find(fragment: &BoxFragment, node: NodeId) -> Option<&BoxFragment> {
        if fragment.node() == Some(node) {
            return Some(fragment);
        }
        fragment
            .children()
            .iter()
            .find_map(|child| find(child, node))
    }

    #[test]
    fn two_block_siblings_stack_vertically() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let container = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(container, "class", "container")
            .expect("sets class");
        dom.append_child(html, container).expect("within depth");
        let first = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(first, "class", "a").expect("sets class");
        dom.append_child(container, first).expect("within depth");
        let second = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(second, "class", "b").expect("sets class");
        dom.append_child(container, second).expect("within depth");

        let styles = style_tree(
            &dom,
            &author(
                ".container { width: 200px; padding: 10px; } \
                 .a { height: 30px; } .b { height: 40px; }",
            ),
        );
        let result = layout_document(&dom, &styles, &constraint(800)).expect("within caps");
        let root = result.root().expect("a root fragment");

        let container_fragment = find(root, container).expect("container fragment");
        assert_eq!(
            container_fragment.rect().origin,
            LogicalPoint::new(px(0), px(0))
        );
        assert_eq!(
            container_fragment.rect().size,
            LogicalSize::new(px(220), px(90))
        );

        let first_fragment = find(root, first).expect("first fragment");
        assert_eq!(
            first_fragment.rect().origin,
            LogicalPoint::new(px(10), px(10))
        );
        assert_eq!(
            first_fragment.rect().size,
            LogicalSize::new(px(200), px(30))
        );

        let second_fragment = find(root, second).expect("second fragment");
        assert_eq!(
            second_fragment.rect().origin,
            LogicalPoint::new(px(10), px(40))
        );
        assert_eq!(
            second_fragment.rect().size,
            LogicalSize::new(px(200), px(40))
        );
    }

    #[test]
    fn adjacent_margins_collapse_to_the_larger_margin() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let wrap = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(wrap, "class", "wrap")
            .expect("sets class");
        dom.append_child(html, wrap).expect("within depth");
        let heading = dom.create_element("h2").expect("under the node cap");
        dom.append_child(wrap, heading).expect("within depth");
        let card = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(card, "class", "card")
            .expect("sets class");
        dom.append_child(wrap, card).expect("within depth");

        let styles = style_tree(
            &dom,
            &author(
                ".wrap { width: 400px; padding: 50px; } \
                 h2 { margin: 20px; height: 24px; } \
                 .card { margin-top: 30px; height: 40px; width: 200px; }",
            ),
        );
        let result = layout_document(&dom, &styles, &constraint(800)).expect("within caps");
        let root = result.root().expect("a root fragment");

        let heading_fragment = find(root, heading).expect("heading fragment");
        let card_fragment = find(root, card).expect("card fragment");

        let heading_bottom = heading_fragment
            .rect()
            .origin
            .y
            .checked_add(heading_fragment.rect().size.height)
            .expect("in range");
        let gap = card_fragment
            .rect()
            .origin
            .y
            .checked_sub(heading_bottom)
            .expect("in range");

        // The 20px bottom margin and the 30px top margin collapse to 30px, not 50px.
        assert_eq!(gap, px(30));
    }

    #[test]
    fn box_sizing_produces_the_expected_rectangle() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let box_element = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(box_element, "class", "sized")
            .expect("sets class");
        dom.append_child(html, box_element).expect("within depth");

        let styles = style_tree(
            &dom,
            &author(".sized { width: 100px; height: 40px; margin: 8px; padding: 4px; }"),
        );
        let result = layout_document(&dom, &styles, &constraint(800)).expect("within caps");
        let root = result.root().expect("a root fragment");
        let box_fragment = find(root, box_element).expect("box fragment");

        // Content 100x40 plus 4px padding on every side is a 108x48 border box.
        assert_eq!(box_fragment.rect().size, LogicalSize::new(px(108), px(48)));
        // The 8px margin offsets the border box in both axes.
        assert_eq!(box_fragment.rect().origin, LogicalPoint::new(px(8), px(8)));
    }

    #[test]
    fn an_anonymous_block_produces_a_fragment_without_a_node() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let container = dom.create_element("div").expect("under the node cap");
        dom.append_child(html, container).expect("within depth");
        dom.append_text(container, "hello").expect("appends text");
        let block_child = dom.create_element("div").expect("under the node cap");
        dom.append_child(container, block_child)
            .expect("within depth");

        let styles = style_tree(&dom, &author(""));
        let result = layout_document(&dom, &styles, &constraint(800)).expect("within caps");
        let root = result.root().expect("a root fragment");
        let container_fragment = find(root, container).expect("container fragment");

        // The mixed children generate an anonymous block fragment (no node) before
        // the real block child fragment.
        assert_eq!(container_fragment.children().len(), 2);
        assert_eq!(container_fragment.children()[0].node(), None);
        assert_eq!(container_fragment.children()[1].node(), Some(block_child));
    }

    #[test]
    fn the_depth_cap_rejects_an_over_deep_tree_without_panicking() {
        let mut root = LayoutBox::anonymous_block(Vec::new());
        for _ in 0..(MAX_LAYOUT_DEPTH + 2) {
            root = LayoutBox::anonymous_block(vec![root]);
        }

        let styles = style_tree(&Dom::new(), &author(""));
        let result = layout_root(&root, &styles, &constraint(800));

        assert_eq!(result, Err(LayoutError::TooDeep));
    }

    #[test]
    fn the_layout_result_records_the_consumed_style_generation() {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");

        let generation = StyleGeneration::new(7);
        let styles = resolve_document_style(
            &dom,
            &parse_user_agent_stylesheet(),
            &author(""),
            generation,
        );
        let result = layout_document(&dom, &styles, &constraint(800)).expect("within caps");

        assert_eq!(result.style_generation(), generation);
    }
}
