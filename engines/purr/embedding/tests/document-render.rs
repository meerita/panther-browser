// @file engines/purr/embedding/tests/document-render.rs
// @description Deterministic headless render test over the M2 fixture and a resize case.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Headless document render assertion (D15).
//!
//! The test drives the document-attachment seam end to end: it attaches the
//! bundled M2 fixture bytes, produces a `DocumentFrame` at a fixed viewport, and
//! asserts the frame structure (the canvas and card background fills, the text
//! quads, the single glyph-atlas upload, and that `validate()` passes). A second
//! case produces at a larger viewport and asserts the canvas fill tracks the
//! content extent while the fixed-width card fill stays put.
//!
//! The output is deterministic: fixed extents, a device pixel ratio of one, the
//! fixed-point layout unit, grayscale antialiasing, and the bundled font. The
//! seam returns an owned value, so the test needs no GPU, no platform handle, and
//! no `unsafe`.

use purr_embedding::{DocumentFrame, DocumentSession, ViewportGeometry, m2_demonstration_fixture};
use purr_graphics::{Color, DrawCommand, Extent2d, Rect, ResourceKind};

/// Base viewport the first case renders into.
///
/// Wide enough that the whole fixture (the card plus its margins) fits inside the
/// content box, so the layout geometry is independent of the extent.
const BASE_EXTENT: Extent2d = Extent2d {
    width: 800,
    height: 600,
};

/// Second viewport the resize case renders into.
///
/// A different extent in both axes, still wide enough to fit the fixture, so only
/// the extent-tracking geometry (the canvas fill) is expected to change.
const RESIZED_EXTENT: Extent2d = Extent2d {
    width: 1200,
    height: 700,
};

/// Opaque background color of the `.card` div (`#eef2ff`).
///
/// The channels use the same integer-over-255 conversion the paint stage applies,
/// so the comparison is exact.
const CARD_BACKGROUND: Color = Color {
    r: 0xee as f32 / 255.0,
    g: 0xf2 as f32 / 255.0,
    b: 0xff as f32 / 255.0,
    a: 1.0,
};

/// The opaque white canvas background painted behind all content.
const CANVAS_BACKGROUND: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

fn geometry(content_extent: Extent2d) -> ViewportGeometry {
    ViewportGeometry {
        content_extent,
        device_pixel_ratio: 1.0,
    }
}

/// Produces a frame for the bundled fixture at the given extent.
fn render(extent: Extent2d) -> DocumentFrame {
    let mut session = DocumentSession::new();
    let handle = session
        .attach(m2_demonstration_fixture())
        .expect("the bundled fixture attaches");
    session
        .produce(&handle, geometry(extent))
        .expect("the fixture produces a frame")
}

/// The solid fills of a frame as (rect, color) pairs, in paint order.
fn fill_rects(frame: &DocumentFrame) -> Vec<(Rect, Color)> {
    frame
        .commands
        .iter()
        .filter_map(|command| match command {
            DrawCommand::FillRect { rect, color } => Some((*rect, *color)),
            _ => None,
        })
        .collect()
}

/// The single fill that covers the whole content extent (the canvas background).
///
/// At a device pixel ratio of one the canvas fill is exactly the extent, so a fill
/// matching both dimensions and the white canvas color is unambiguous.
fn canvas_fill(frame: &DocumentFrame, extent: Extent2d) -> (Rect, Color) {
    fill_rects(frame)
        .into_iter()
        .find(|(rect, color)| {
            rect.x == 0.0
                && rect.y == 0.0
                && rect.width == extent.width as f32
                && rect.height == extent.height as f32
                && *color == CANVAS_BACKGROUND
        })
        .expect("a canvas fill covering the content extent")
}

/// The card background fill (`#eef2ff`) with its document-local geometry.
fn card_fill(frame: &DocumentFrame) -> (Rect, Color) {
    fill_rects(frame)
        .into_iter()
        .find(|(_, color)| *color == CARD_BACKGROUND)
        .expect("a card background fill")
}

fn textured_quad_count(frame: &DocumentFrame) -> usize {
    frame
        .commands
        .iter()
        .filter(|command| matches!(command, DrawCommand::TexturedQuad { .. }))
        .count()
}

#[test]
fn the_fixture_renders_to_a_validated_document_frame() {
    let frame = render(BASE_EXTENT);

    assert_eq!(frame.validate(), Ok(()), "the produced frame validates");

    // Exactly one upload, and it is the glyph atlas the text quads sample.
    assert_eq!(frame.uploads.len(), 1, "one resource upload");
    assert_eq!(
        frame.uploads[0].resource.resource_kind(),
        ResourceKind::GlyphAtlas,
        "the upload is the glyph atlas"
    );

    // The canvas fill and the card background fill are both present.
    let _canvas = canvas_fill(&frame, BASE_EXTENT);
    let card = card_fill(&frame);
    assert_eq!(
        card.0.x, 40.0,
        "the card sits at the body plus card left margins (16 + 24)"
    );
    assert_eq!(
        card.0.width, 392.0,
        "the card padding box is its fixed width plus horizontal padding (360 + 2 * 16)"
    );

    // The wrapping paragraph text produces glyph quads.
    assert!(
        textured_quad_count(&frame) > 0,
        "the fixture text produces at least one glyph quad"
    );
}

#[test]
fn resizing_the_viewport_changes_the_canvas_fill_geometry() {
    let base = render(BASE_EXTENT);
    let resized = render(RESIZED_EXTENT);

    // The canvas fill tracks the content extent: a wider and taller viewport
    // produces a wider and taller canvas fill.
    let base_canvas = canvas_fill(&base, BASE_EXTENT);
    let resized_canvas = canvas_fill(&resized, RESIZED_EXTENT);
    assert_eq!(base_canvas.0.width, BASE_EXTENT.width as f32);
    assert_eq!(resized_canvas.0.width, RESIZED_EXTENT.width as f32);
    assert_ne!(
        base_canvas.0.width, resized_canvas.0.width,
        "the canvas fill width follows the resized extent"
    );
    assert_ne!(
        base_canvas.0.height, resized_canvas.0.height,
        "the canvas fill height follows the resized extent"
    );

    // The card has a fixed width and fixed margins, so its geometry does not move
    // when only the viewport extent changes.
    assert_eq!(
        card_fill(&base).0,
        card_fill(&resized).0,
        "the fixed-width card geometry is stable across the resize"
    );
}
