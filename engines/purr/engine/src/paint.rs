// @file engines/purr/engine/src/paint.rs
// @description Lowers the immutable fragment tree to a physical display list of draw commands and the glyph atlas upload.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Paint and display-list lowering.
//!
//! This stage walks the committed, immutable fragment tree and lowers it to a
//! physical display list: a document background fill, block and inline box
//! backgrounds as [`DrawCommand::FillRect`], and one [`DrawCommand::TexturedQuad`]
//! per glyph sampling the glyph atlas, plus the single glyph-atlas
//! [`ResourceUpload`]. The commands and the upload use the engine producer
//! namespace and document-local coordinates with the origin at `(0, 0)`.
//!
//! Layout geometry is fixed-point [`LayoutUnit`] in CSS pixels; this is the
//! boundary that converts it to the `f32` device-pixel `Rect` the graphics commands
//! use. The device pixel ratio from the viewport is applied here (to paint and
//! glyph rasterization sizing), never in layout, per the seam contract. The product
//! still owns the translate-to-viewport-origin and the clip.
//!
//! The display list is a full rebuild per generation: the stage holds no retained
//! state and the returned [`PaintOutput`] is an owned, immutable value. The logical
//! glyph run stays internal; only the physical quads and the atlas cross into the
//! frame.

// The document store is the only in-crate consumer of the paint output. This phase
// adds the stage and exercises it through the unit tests, so a couple of items are
// otherwise unused in a non-test build.
#![allow(dead_code)]

use std::collections::HashMap;

use crate::bundled_font::BundledFont;
use crate::computed_style::StyleTree;
use crate::css_parser::PropertyId;
use crate::dom_node::NodeId;
use crate::fragment_tree::{BoxContents, BoxFragment, FragmentTree, LineFragment, LineItem};
use crate::glyph_atlas::{
    GlyphAtlas, GlyphAtlasError, GlyphKey, GlyphPlacement, build_glyph_atlas,
};
use crate::layout_unit::{LayoutUnit, LogicalRect, ONE_PX_RAW};
use crate::text_shaping::GlyphRunSlice;
use purr_graphics::{
    Color, DeviceGeneration, DrawCommand, Extent2d, Rect, ResourceGeneration, ResourceUpload,
};

/// The opaque white document background painted behind all content.
const CANVAS_BACKGROUND: Color = Color::new(1.0, 1.0, 1.0, 1.0);

/// The owned, immutable result of lowering one fragment tree.
///
/// The commands paint the frame in order, and the uploads carry the single glyph
/// atlas the glyph quads sample. Both are values, so the frame holds no borrow into
/// engine state.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintOutput {
    pub commands: Vec<DrawCommand>,
    pub uploads: Vec<ResourceUpload>,
}

/// Failure the paint stage reports.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PaintError {
    #[error("the glyph atlas could not be built")]
    Atlas(#[from] GlyphAtlasError),
}

/// Lowers a committed fragment tree to a physical display list and the atlas upload.
///
/// Rasterizes every glyph the tree references into one atlas, then walks the tree in
/// paint order emitting the document background, the block and inline backgrounds,
/// and one textured quad per glyph. The device pixel ratio scales paint and glyph
/// rasterization sizing but not layout. Fails closed with a typed error, and never
/// panics, when the atlas cannot be built.
pub fn paint_document(
    tree: &FragmentTree,
    styles: &StyleTree,
    font: &BundledFont,
    content_extent: Extent2d,
    device_pixel_ratio: f32,
    resource_generation: ResourceGeneration,
    device_generation: DeviceGeneration,
) -> Result<PaintOutput, PaintError> {
    let mut keys = Vec::new();
    if let Some(root) = tree.root() {
        collect_glyph_keys(root, device_pixel_ratio, &mut keys);
    }
    let atlas = build_glyph_atlas(font, &keys, resource_generation, device_generation)?;
    let lookup: HashMap<GlyphKey, GlyphPlacement> = atlas
        .placements()
        .iter()
        .map(|placement| (placement.key(), *placement))
        .collect();

    let mut commands = Vec::new();
    commands.push(canvas_fill(content_extent, device_pixel_ratio));
    if let Some(root) = tree.root() {
        paint_box(
            root,
            styles,
            &atlas,
            &lookup,
            device_pixel_ratio,
            &mut commands,
        );
    }

    Ok(PaintOutput {
        commands,
        uploads: vec![atlas.upload().clone()],
    })
}

/// Collects the glyph keys (glyph plus device size) the tree references.
///
/// The device size folds the device pixel ratio into the shaped font size, so the
/// atlas rasterizes each glyph at the resolution it will paint at.
fn collect_glyph_keys(box_fragment: &BoxFragment, dpr: f32, keys: &mut Vec<GlyphKey>) {
    match box_fragment.contents() {
        BoxContents::Blocks(children) => {
            for child in children {
                collect_glyph_keys(child, dpr, keys);
            }
        }
        BoxContents::Lines(lines) => {
            for line in lines {
                for item in line.items() {
                    if let LineItem::Text(text) = item {
                        collect_slice_keys(text.slice(), dpr, keys);
                    }
                }
            }
        }
    }
}

/// Collects the glyph keys of one text-fragment slice.
fn collect_slice_keys(slice: &GlyphRunSlice, dpr: f32, keys: &mut Vec<GlyphKey>) {
    let size = device_size(slice.size(), dpr);
    for glyph in slice.glyphs() {
        keys.push(GlyphKey::new(glyph.glyph(), size));
    }
}

/// Paints one box fragment: its background, then its contents, in paint order.
fn paint_box(
    box_fragment: &BoxFragment,
    styles: &StyleTree,
    atlas: &GlyphAtlas,
    lookup: &HashMap<GlyphKey, GlyphPlacement>,
    dpr: f32,
    commands: &mut Vec<DrawCommand>,
) {
    push_background(
        box_fragment.node(),
        box_fragment.rect(),
        styles,
        dpr,
        commands,
    );

    match box_fragment.contents() {
        BoxContents::Blocks(children) => {
            for child in children {
                paint_box(child, styles, atlas, lookup, dpr, commands);
            }
        }
        BoxContents::Lines(lines) => {
            for line in lines {
                paint_line(line, styles, atlas, lookup, dpr, commands);
            }
        }
    }
}

/// Paints one line: inline-box backgrounds first, then the glyph quads over them.
fn paint_line(
    line: &LineFragment,
    styles: &StyleTree,
    atlas: &GlyphAtlas,
    lookup: &HashMap<GlyphKey, GlyphPlacement>,
    dpr: f32,
    commands: &mut Vec<DrawCommand>,
) {
    let baseline = line
        .rect()
        .origin
        .y
        .checked_add(line.baseline())
        .unwrap_or(line.rect().origin.y);

    for item in line.items() {
        match item {
            LineItem::InlineBox(inline_box) => {
                push_background(inline_box.node(), inline_box.rect(), styles, dpr, commands);
            }
            LineItem::Text(text) => {
                paint_text(
                    text.slice(),
                    text.position().x,
                    baseline,
                    atlas,
                    lookup,
                    dpr,
                    commands,
                );
            }
        }
    }
}

/// Paints the glyphs of one text-fragment slice as textured quads.
///
/// The pen starts at the fragment inline position and advances by each glyph
/// advance. A blank glyph (a space) has a zero-area source rectangle and paints no
/// quad. Every quad samples the glyph atlas at the glyph's physical source rect.
#[allow(clippy::too_many_arguments)]
fn paint_text(
    slice: &GlyphRunSlice,
    inline_start: LayoutUnit,
    baseline: LayoutUnit,
    atlas: &GlyphAtlas,
    lookup: &HashMap<GlyphKey, GlyphPlacement>,
    dpr: f32,
    commands: &mut Vec<DrawCommand>,
) {
    let size = device_size(slice.size(), dpr);
    let baseline_px = to_device_px(baseline, dpr);
    let mut pen = inline_start;

    for glyph in slice.glyphs() {
        if let Some(placement) = lookup.get(&GlyphKey::new(glyph.glyph(), size)) {
            let source = placement.source();
            if source.width > 0 && source.height > 0 {
                let x = to_device_px(pen, dpr) + placement.left() as f32;
                let y = baseline_px + placement.top() as f32;
                commands.push(DrawCommand::TexturedQuad {
                    rect: Rect::new(x, y, source.width as f32, source.height as f32),
                    texture: atlas.resource(),
                    source: Rect::new(
                        source.x as f32,
                        source.y as f32,
                        source.width as f32,
                        source.height as f32,
                    ),
                });
            }
        }
        pen = pen.saturating_add(glyph.advance());
    }
}

/// Pushes a background fill for a node whose style resolves an opaque color.
///
/// A node with no style, or a transparent or unrecognized background color, paints
/// nothing.
fn push_background(
    node: Option<NodeId>,
    rect: LogicalRect,
    styles: &StyleTree,
    dpr: f32,
    commands: &mut Vec<DrawCommand>,
) {
    let Some(color) = node
        .and_then(|node| styles.get(node))
        .and_then(|style| parse_color(style.get(PropertyId::BackgroundColor)))
    else {
        return;
    };
    commands.push(DrawCommand::FillRect {
        rect: rect_to_device(rect, dpr),
        color,
    });
}

/// The document background fill covering the whole content extent.
fn canvas_fill(content_extent: Extent2d, dpr: f32) -> DrawCommand {
    let width = content_extent.width as f32 * dpr;
    let height = content_extent.height as f32 * dpr;
    DrawCommand::FillRect {
        rect: Rect::new(0.0, 0.0, width, height),
        color: CANVAS_BACKGROUND,
    }
}

/// Folds the device pixel ratio into a shaped font size.
///
/// The glyph is rasterized at the device size so it paints crisply at the target
/// resolution. At the M2 device pixel ratio of one this is the identity.
fn device_size(css_size: LayoutUnit, dpr: f32) -> LayoutUnit {
    if dpr == 1.0 {
        return css_size;
    }
    let scaled = css_size.raw() as f32 * dpr;
    LayoutUnit::from_raw(scaled.round() as i32)
}

/// Converts a fixed-point CSS-pixel length to an `f32` device-pixel length.
fn to_device_px(unit: LayoutUnit, dpr: f32) -> f32 {
    (unit.raw() as f32 / ONE_PX_RAW as f32) * dpr
}

/// Converts a document-local logical rectangle to a device-pixel `Rect`.
fn rect_to_device(rect: LogicalRect, dpr: f32) -> Rect {
    Rect::new(
        to_device_px(rect.origin.x, dpr),
        to_device_px(rect.origin.y, dpr),
        to_device_px(rect.size.width, dpr),
        to_device_px(rect.size.height, dpr),
    )
}

/// Parses a CSS color into an opaque graphics color, or `None` when it is absent,
/// transparent, or unrecognized.
///
/// Supports the `#rgb` and `#rrggbb` hex forms and the `white`, `black`, and
/// `transparent` keywords. An unrecognized value paints no background rather than a
/// guessed color.
fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim();
    match value {
        "" | "transparent" => None,
        "white" => Some(Color::new(1.0, 1.0, 1.0, 1.0)),
        "black" => Some(Color::new(0.0, 0.0, 0.0, 1.0)),
        _ => parse_hex_color(value),
    }
}

/// Parses a `#rgb` or `#rrggbb` hex color into an opaque graphics color.
fn parse_hex_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    let (r, g, b) = match hex.len() {
        3 => {
            let r = double_nibble(hex_digit(hex.as_bytes()[0])?);
            let g = double_nibble(hex_digit(hex.as_bytes()[1])?);
            let b = double_nibble(hex_digit(hex.as_bytes()[2])?);
            (r, g, b)
        }
        6 => {
            let bytes = hex.as_bytes();
            let r = hex_pair(bytes[0], bytes[1])?;
            let g = hex_pair(bytes[2], bytes[3])?;
            let b = hex_pair(bytes[4], bytes[5])?;
            (r, g, b)
        }
        _ => return None,
    };
    Some(Color::new(channel(r), channel(g), channel(b), 1.0))
}

/// The value of one hex digit, or `None` when the byte is not a hex digit.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Combines two hex digits into one byte value.
fn hex_pair(high: u8, low: u8) -> Option<u8> {
    Some(hex_digit(high)? * 16 + hex_digit(low)?)
}

/// Expands one hex nibble to a full byte, so `f` becomes `0xff`.
fn double_nibble(nibble: u8) -> u8 {
    nibble * 16 + nibble
}

/// Normalizes an 8-bit channel to the `[0.0, 1.0]` graphics range.
fn channel(value: u8) -> f32 {
    value as f32 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_layout::{ConstraintSpace, layout_document};
    use crate::computed_style::{StyleGeneration, resolve_document_style};
    use crate::css_parser::{Origin, parse_stylesheet};
    use crate::dom_node::Dom;
    use crate::fragment_tree::LayoutGeneration;
    use crate::layout_unit::LayoutSize;
    use crate::user_agent_styles::parse_user_agent_stylesheet;
    use purr_graphics::ResourceKind;

    fn px(value: i32) -> LayoutUnit {
        LayoutUnit::from_px(value).expect("in range")
    }

    fn paint(source_dom: &Dom, author_css: &str, extent: Extent2d, dpr: f32) -> PaintOutput {
        let styles = resolve_document_style(
            source_dom,
            &parse_user_agent_stylesheet(),
            &parse_stylesheet(author_css, Origin::Author),
            StyleGeneration::FIRST,
        );
        let constraint = ConstraintSpace::new(px(extent.width as i32), LayoutSize::Indefinite);
        let tree = layout_document(source_dom, &styles, &constraint, LayoutGeneration::FIRST)
            .expect("layout within caps");
        let font = BundledFont::load().expect("the bundled font parses");
        paint_document(
            &tree,
            &styles,
            &font,
            extent,
            dpr,
            ResourceGeneration::new(1),
            DeviceGeneration::new(1),
        )
        .expect("paint succeeds")
    }

    fn card_dom() -> (Dom, NodeId) {
        let mut dom = Dom::new();
        let html = dom.create_element("html").expect("under the node cap");
        dom.append_child(dom.root(), html).expect("within depth");
        let card = dom.create_element("div").expect("under the node cap");
        dom.set_attribute(card, "class", "card")
            .expect("sets class");
        dom.append_child(html, card).expect("within depth");
        dom.append_text(card, "hello world").expect("appends text");
        (dom, card)
    }

    fn author() -> &'static str {
        ".card { width: 200px; height: 50px; margin: 0; padding: 0; \
         background-color: #eef; line-height: 20px; }"
    }

    fn fill_rects(output: &PaintOutput) -> Vec<(Rect, Color)> {
        output
            .commands
            .iter()
            .filter_map(|command| match command {
                DrawCommand::FillRect { rect, color } => Some((*rect, *color)),
                _ => None,
            })
            .collect()
    }

    fn textured_quads(output: &PaintOutput) -> Vec<DrawCommand> {
        output
            .commands
            .iter()
            .copied()
            .filter(|command| matches!(command, DrawCommand::TexturedQuad { .. }))
            .collect()
    }

    #[test]
    fn the_output_has_a_card_background_text_quads_and_one_atlas_upload() {
        let (dom, _card) = card_dom();
        let output = paint(&dom, author(), Extent2d::new(800, 600), 1.0);

        // The canvas fill plus the card background are both present.
        let fills = fill_rects(&output);
        assert!(fills.len() >= 2);
        assert!(
            fills.iter().any(|(_, color)| *color
                == Color::new(0xee as f32 / 255.0, 0xee as f32 / 255.0, 1.0, 1.0))
        );

        // "hello world" produces at least one glyph quad.
        assert!(!textured_quads(&output).is_empty());

        // Exactly one upload, and it is the glyph atlas.
        assert_eq!(output.uploads.len(), 1);
        assert_eq!(
            output.uploads[0].resource.resource_kind(),
            ResourceKind::GlyphAtlas
        );
    }

    #[test]
    fn the_card_background_rect_matches_the_fragment_geometry() {
        let (dom, _card) = card_dom();
        let output = paint(&dom, author(), Extent2d::new(800, 600), 1.0);

        // The card is 200x50 at the document origin (no margin), so its background
        // fill is that rectangle after the fixed-point to f32 conversion.
        let card_fill = fill_rects(&output)
            .into_iter()
            .find(|(rect, _)| rect.width == 200.0 && rect.height == 50.0)
            .expect("a card-sized fill");
        assert_eq!(card_fill.0.x, 0.0);
        assert_eq!(card_fill.0.y, 0.0);
    }

    #[test]
    fn each_glyph_quad_samples_the_atlas_within_its_extent() {
        let (dom, _card) = card_dom();
        let output = paint(&dom, author(), Extent2d::new(800, 600), 1.0);
        let atlas_resource = output.uploads[0].resource;
        let extent = output.uploads[0].descriptor.extent;

        for quad in textured_quads(&output) {
            let DrawCommand::TexturedQuad {
                texture, source, ..
            } = quad
            else {
                unreachable!("filtered to textured quads");
            };
            assert_eq!(texture, atlas_resource);
            assert!(source.x >= 0.0 && source.y >= 0.0);
            assert!(source.x + source.width <= extent.width as f32);
            assert!(source.y + source.height <= extent.height as f32);
        }
    }

    #[test]
    fn an_empty_document_still_paints_a_canvas_and_one_atlas_upload() {
        let dom = Dom::new();
        let output = paint(&dom, "", Extent2d::new(400, 300), 1.0);

        assert_eq!(output.uploads.len(), 1);
        assert!(
            output
                .commands
                .iter()
                .any(|command| matches!(command, DrawCommand::FillRect { .. }))
        );
        assert!(textured_quads(&output).is_empty());
    }

    #[test]
    fn a_hex_color_parses_to_the_expected_channels() {
        assert_eq!(parse_color("#fff"), Some(Color::new(1.0, 1.0, 1.0, 1.0)));
        assert_eq!(parse_color("#000000"), Some(Color::new(0.0, 0.0, 0.0, 1.0)));
        assert_eq!(parse_color("#ff0000"), Some(Color::new(1.0, 0.0, 0.0, 1.0)));
        assert_eq!(parse_color("transparent"), None);
        assert_eq!(parse_color(""), None);
        assert_eq!(parse_color("not-a-color"), None);
    }
}
