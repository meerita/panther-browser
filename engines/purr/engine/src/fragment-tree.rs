// @file engines/purr/engine/src/fragment-tree.rs
// @description Defines the immutable physical fragment tree committed per layout generation.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Fragment tree.
//!
//! The fragment tree is the physical output of layout. It is distinct from the
//! logical box tree: a box tree holds logical Layout Objects, while the fragment
//! tree holds their placed geometry in document-local coordinates. A block box
//! produces a [`BoxFragment`]; the inline content of a block produces
//! [`LineFragment`]s that hold [`TextFragment`]s (a slice of a shaped run at a
//! position) and inline-box background fragments.
//!
//! Every geometry value is a [`LayoutUnit`]; the tree owns no pixels and no glyph
//! texture coordinate. A text fragment carries a [`GlyphRunSlice`], which keeps the
//! glyph-to-source cluster map, but never a rasterized mask or an atlas position.
//!
//! The tree is immutable after the atomic commit. A [`FragmentTree`] records both
//! its own layout generation and the style generation it consumed, so it extends
//! the Style to Layout to Fragment generation chain and a later stage rejects a
//! tree built from a superseded generation.

// The paint stage is the first non-test consumer of the fragment tree. This phase
// builds and commits the tree and exercises it through the unit tests, so some
// entry points are otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::computed_style::StyleGeneration;
use crate::dom_node::NodeId;
use crate::layout_unit::{LayoutUnit, LogicalPoint, LogicalRect};
use crate::text_shaping::GlyphRunSlice;

/// Marks one atomic layout commit.
///
/// A fragment tree carries the generation it was committed for. The value is
/// monotonic and opaque; a new layout produces a new generation, so a stale tree
/// is detectable. It is distinct from the style generation the layout consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutGeneration(u64);

impl LayoutGeneration {
    /// The generation of the first layout commit.
    pub const FIRST: Self = Self(1);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// The next generation, or `None` at the `u64` boundary.
    ///
    /// A `u64` generation cannot overflow in practice, so `None` is unreachable,
    /// but the layout stage never wraps a generation back to an earlier value.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// One immutable text fragment: a slice of a shaped run at a position.
///
/// The slice keeps the glyph advances and the cluster map, so the fragment maps
/// each glyph back to its source text byte offset. The position is the top-left of
/// the fragment in document-local coordinates; the vertical baseline comes from the
/// [`LineFragment`] that owns the fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFragment {
    slice: GlyphRunSlice,
    position: LogicalPoint,
}

impl TextFragment {
    pub fn new(slice: GlyphRunSlice, position: LogicalPoint) -> Self {
        Self { slice, position }
    }

    pub fn slice(&self) -> &GlyphRunSlice {
        &self.slice
    }

    pub fn position(&self) -> LogicalPoint {
        self.position
    }

    /// The same fragment moved by `offset`, or `None` at the `i32` boundary.
    pub(crate) fn translated(self, offset: LogicalPoint) -> Option<Self> {
        Some(Self {
            slice: self.slice,
            position: offset_point(self.position, offset)?,
        })
    }
}

/// One item placed on a line: text, or an inline-box background.
///
/// The items paint in order, so a background item precedes the text it sits behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineItem {
    Text(TextFragment),
    InlineBox(BoxFragment),
}

impl LineItem {
    /// The same item moved by `offset`, or `None` at the `i32` boundary.
    pub(crate) fn translated(self, offset: LogicalPoint) -> Option<Self> {
        match self {
            LineItem::Text(text) => Some(LineItem::Text(text.translated(offset)?)),
            LineItem::InlineBox(inline_box) => {
                Some(LineItem::InlineBox(inline_box.translated(offset)?))
            }
        }
    }
}

/// One immutable line box produced by the inline formatting context.
///
/// The rectangle is the line box in document-local coordinates: its inline extent
/// is the used content width and its block extent is the line height. The baseline
/// is the distance from the top of the line box down to the shared baseline. The
/// items are the placed inline content in paint order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineFragment {
    rect: LogicalRect,
    baseline: LayoutUnit,
    items: Vec<LineItem>,
}

impl LineFragment {
    pub fn new(rect: LogicalRect, baseline: LayoutUnit, items: Vec<LineItem>) -> Self {
        Self {
            rect,
            baseline,
            items,
        }
    }

    pub fn rect(&self) -> LogicalRect {
        self.rect
    }

    pub fn baseline(&self) -> LayoutUnit {
        self.baseline
    }

    pub fn items(&self) -> &[LineItem] {
        &self.items
    }

    /// The same line moved by `offset`, or `None` at the `i32` boundary.
    pub(crate) fn translated(self, offset: LogicalPoint) -> Option<Self> {
        let mut items = Vec::with_capacity(self.items.len());
        for item in self.items {
            items.push(item.translated(offset)?);
        }
        Some(Self {
            rect: offset_rect(self.rect, offset)?,
            baseline: self.baseline,
            items,
        })
    }
}

/// The content of one box fragment.
///
/// A block box either establishes a block formatting context and holds child box
/// fragments, or establishes an inline formatting context and holds line
/// fragments. The two are never mixed, because the layout tree wraps a run of
/// inline content in an anonymous block before layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxContents {
    Blocks(Vec<BoxFragment>),
    Lines(Vec<LineFragment>),
}

/// One immutable box fragment in document-local physical coordinates.
///
/// A fragment carries a border-box rectangle, its content (block children or
/// lines), and the DOM node it derives from, or `None` for an anonymous box. A
/// fragment is never mutated after the layout result commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxFragment {
    node: Option<NodeId>,
    rect: LogicalRect,
    contents: BoxContents,
}

impl BoxFragment {
    pub fn new(node: Option<NodeId>, rect: LogicalRect, contents: BoxContents) -> Self {
        Self {
            node,
            rect,
            contents,
        }
    }

    /// A box fragment with no content, used for an inline-box background.
    pub fn leaf(node: Option<NodeId>, rect: LogicalRect) -> Self {
        Self::new(node, rect, BoxContents::Blocks(Vec::new()))
    }

    pub fn node(&self) -> Option<NodeId> {
        self.node
    }

    pub fn rect(&self) -> LogicalRect {
        self.rect
    }

    pub fn contents(&self) -> &BoxContents {
        &self.contents
    }

    /// The child box fragments, or an empty slice when the box holds lines.
    pub fn children(&self) -> &[BoxFragment] {
        match &self.contents {
            BoxContents::Blocks(children) => children,
            BoxContents::Lines(_) => &[],
        }
    }

    /// The line fragments, or an empty slice when the box holds block children.
    pub fn lines(&self) -> &[LineFragment] {
        match &self.contents {
            BoxContents::Lines(lines) => lines,
            BoxContents::Blocks(_) => &[],
        }
    }

    /// The same fragment moved by `offset`, or `None` at the `i32` boundary.
    ///
    /// An inline-box background fragment has no content, so this offsets its
    /// rectangle only. The block subtree is placed by the layout flatten pass, not
    /// by this method.
    pub(crate) fn translated(self, offset: LogicalPoint) -> Option<Self> {
        let contents = match self.contents {
            BoxContents::Blocks(children) => {
                let mut moved = Vec::with_capacity(children.len());
                for child in children {
                    moved.push(child.translated(offset)?);
                }
                BoxContents::Blocks(moved)
            }
            BoxContents::Lines(lines) => {
                let mut moved = Vec::with_capacity(lines.len());
                for line in lines {
                    moved.push(line.translated(offset)?);
                }
                BoxContents::Lines(moved)
            }
        };
        Some(Self {
            node: self.node,
            rect: offset_rect(self.rect, offset)?,
            contents,
        })
    }
}

/// The immutable result of laying out one document.
///
/// It carries the root box fragment, absent when the document generates no box, its
/// own layout generation, and the style generation the layout consumed. The two
/// generations extend the Style to Layout to Fragment chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentTree {
    layout_generation: LayoutGeneration,
    style_generation: StyleGeneration,
    root: Option<BoxFragment>,
}

impl FragmentTree {
    pub fn new(
        layout_generation: LayoutGeneration,
        style_generation: StyleGeneration,
        root: Option<BoxFragment>,
    ) -> Self {
        Self {
            layout_generation,
            style_generation,
            root,
        }
    }

    pub fn layout_generation(&self) -> LayoutGeneration {
        self.layout_generation
    }

    pub fn style_generation(&self) -> StyleGeneration {
        self.style_generation
    }

    pub fn root(&self) -> Option<&BoxFragment> {
        self.root.as_ref()
    }
}

/// Moves a point by an offset, or `None` at the `i32` boundary.
fn offset_point(point: LogicalPoint, offset: LogicalPoint) -> Option<LogicalPoint> {
    Some(LogicalPoint::new(
        point.x.checked_add(offset.x)?,
        point.y.checked_add(offset.y)?,
    ))
}

/// Moves a rectangle origin by an offset, or `None` at the `i32` boundary.
fn offset_rect(rect: LogicalRect, offset: LogicalPoint) -> Option<LogicalRect> {
    Some(LogicalRect::new(
        offset_point(rect.origin, offset)?,
        rect.size,
    ))
}
