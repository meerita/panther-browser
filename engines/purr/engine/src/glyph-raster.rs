// @file engines/purr/engine/src/glyph-raster.rs
// @description Rasterizes a glyph outline into a deterministic grayscale coverage mask in pure safe Rust.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Glyph rasterization.
//!
//! The rasterizer turns a glyph index and a pixel size into a [`GlyphMask`]: a
//! single-channel grayscale coverage buffer plus the physical placement offset of
//! the mask relative to the pen origin and baseline. It is pure safe Rust with no
//! `unsafe`, no dependency on a native rasterizer, and no filesystem access.
//!
//! The output is deterministic: the same font, glyph, and size always produce the
//! same bytes, with no per-call variation (privacy invariant I7). Coverage comes
//! from a fixed scheme, analytic horizontal coverage combined with a fixed number
//! of vertical sub-rows and the non-zero winding rule, so the result is stable and
//! carries no configuration entropy.
//!
//! The rasterizer owns pixels; the logical [`crate::text_shaping::GlyphRun`] never
//! does. A text fragment keeps a glyph index and an advance; the mask a glyph index
//! rasterizes to, and the atlas position it later occupies, are physical data that
//! stay out of the logical run.

// The atlas builder and, from a later phase, paint are the consumers of the mask
// and its accessors. This phase adds the rasterizer and exercises it through the
// unit tests below and the atlas, so a few accessors are otherwise unused in a
// non-test build.
#![allow(dead_code)]

use crate::bundled_font::{BundledFont, FontError, GlyphIndex, GlyphOutline};
use crate::layout_unit::LayoutUnit;

/// Number of vertical sub-rows sampled per pixel row.
///
/// Each pixel row is sampled at this many evenly spaced sub-rows, and the analytic
/// horizontal coverage of each sub-row is averaged. A power of two keeps the
/// per-sub-row weight exact in the accumulation.
const VERTICAL_SUBSAMPLES: u32 = 4;

/// Number of line segments one quadratic curve is flattened into.
///
/// The count is fixed, so flattening is deterministic and independent of the
/// curve size. It is enough for the small glyph sizes the M2 page uses.
const QUADRATIC_SEGMENTS: u32 = 8;

/// Upper bound for a rasterized glyph dimension, in texels.
///
/// A single glyph never approaches this at the M2 sizes. The bound rejects a glyph
/// whose scaled bounding box is implausibly large before any allocation, so a
/// malformed metric or an extreme size fails closed instead of allocating a huge
/// mask.
pub const MAX_GLYPH_EXTENT: u32 = 1_024;

/// Failure the rasterizer reports.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RasterError {
    #[error("the glyph outline is malformed")]
    MalformedOutline,
    #[error("the glyph size is out of range")]
    InvalidSize,
    #[error("the rasterized glyph exceeds the maximum extent")]
    GlyphTooLarge,
}

impl From<FontError> for RasterError {
    fn from(_error: FontError) -> Self {
        RasterError::MalformedOutline
    }
}

/// A rasterized glyph: a grayscale coverage mask and its physical placement.
///
/// The coverage buffer holds one byte per texel, row-major, top to bottom, with no
/// padding: `coverage.len() == width * height`. A value of `0` is fully outside the
/// glyph and `255` fully inside. The buffer is empty for a blank glyph (a space),
/// where both dimensions are zero.
///
/// `left` and `top` are the offset of the mask's top-left texel from the pen
/// origin on the baseline, in whole device pixels: `left` is positive to the right
/// of the origin, `top` is negative above the baseline. They are physical values a
/// later paint stage uses to place the mask; the logical run never carries them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphMask {
    width: u32,
    height: u32,
    left: i32,
    top: i32,
    coverage: Vec<u8>,
}

impl GlyphMask {
    /// A blank mask with no coverage, used for an empty glyph.
    fn blank() -> Self {
        Self {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            coverage: Vec::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn left(&self) -> i32 {
        self.left
    }

    pub fn top(&self) -> i32 {
        self.top
    }

    /// The coverage buffer, one byte per texel, row-major top to bottom.
    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }

    /// Whether the mask has no coverage (a blank glyph).
    pub fn is_blank(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Rasterizes one glyph of a font at a pixel size into a coverage mask.
///
/// A blank glyph (an empty outline) rasterizes to an empty mask. A malformed
/// outline or an out-of-range size fails closed with a typed error and never
/// panics. The result is deterministic.
pub fn rasterize_glyph(
    font: &BundledFont,
    glyph: GlyphIndex,
    size: LayoutUnit,
) -> Result<GlyphMask, RasterError> {
    if size.raw() <= 0 {
        return Err(RasterError::InvalidSize);
    }
    let outline = font.outline(glyph)?;
    if outline.is_empty() {
        return Ok(GlyphMask::blank());
    }

    let scale = pixel_scale(size, font.units_per_em());
    let contours = flatten_outline(&outline, scale);
    rasterize_contours(&contours)
}

/// The font-unit-to-pixel scale factor for a pixel size.
///
/// The pixel size is the fixed-point `LayoutUnit` read as a pixel count; the scale
/// converts a font design unit to a device pixel. It is `f32`, but the whole path
/// is a pure function of its inputs, so the output stays deterministic.
fn pixel_scale(size: LayoutUnit, units_per_em: u16) -> f32 {
    let size_px = size.raw() as f32 / crate::layout_unit::ONE_PX_RAW as f32;
    size_px / units_per_em as f32
}

/// A point in device-pixel space, y increasing downward.
#[derive(Debug, Clone, Copy)]
struct Vertex {
    x: f32,
    y: f32,
}

/// Flattens a glyph outline into closed polygons in device-pixel space.
///
/// Each contour becomes one closed polygon. Off-curve (quadratic control) points
/// are expanded, including the implied on-curve midpoints between two consecutive
/// off-curve points, and each quadratic is subdivided into a fixed number of
/// segments. The y axis is flipped so the baseline is at `y = 0` and ascenders have
/// negative y, matching a top-down device coordinate space.
fn flatten_outline(outline: &GlyphOutline, scale: f32) -> Vec<Vec<Vertex>> {
    let mut polygons = Vec::with_capacity(outline.contours().len());
    for contour in outline.contours() {
        if contour.len() < 2 {
            continue;
        }
        polygons.push(flatten_contour(contour, scale));
    }
    polygons
}

/// Flattens one contour into a closed polygon of vertices.
fn flatten_contour(contour: &[crate::bundled_font::OutlinePoint], scale: f32) -> Vec<Vertex> {
    let point_count = contour.len();
    let to_vertex = |x: f32, y: f32| Vertex {
        x: x * scale,
        y: -y * scale,
    };

    // The walk needs to start on the curve. When the first point is off-curve,
    // start from the midpoint between the last and first points, which is always
    // on the curve in the TrueType quadratic model.
    let start = if contour[0].on_curve {
        (contour[0].x as f32, contour[0].y as f32)
    } else if contour[point_count - 1].on_curve {
        (
            contour[point_count - 1].x as f32,
            contour[point_count - 1].y as f32,
        )
    } else {
        (
            (contour[point_count - 1].x as f32 + contour[0].x as f32) / 2.0,
            (contour[point_count - 1].y as f32 + contour[0].y as f32) / 2.0,
        )
    };

    let mut vertices = vec![to_vertex(start.0, start.1)];
    let mut current = start;
    let mut pending_control: Option<(f32, f32)> = None;

    for step in 0..point_count {
        let index = if contour[0].on_curve {
            (step + 1) % point_count
        } else {
            step % point_count
        };
        let point = contour[index];
        let coordinate = (point.x as f32, point.y as f32);

        if point.on_curve {
            match pending_control.take() {
                Some(control) => {
                    push_quadratic(&mut vertices, current, control, coordinate, to_vertex);
                }
                None => vertices.push(to_vertex(coordinate.0, coordinate.1)),
            }
            current = coordinate;
        } else {
            if let Some(control) = pending_control.take() {
                let midpoint = (
                    (control.0 + coordinate.0) / 2.0,
                    (control.1 + coordinate.1) / 2.0,
                );
                push_quadratic(&mut vertices, current, control, midpoint, to_vertex);
                current = midpoint;
            }
            pending_control = Some(coordinate);
        }
    }

    if let Some(control) = pending_control.take() {
        push_quadratic(&mut vertices, current, control, start, to_vertex);
    }
    vertices
}

/// Appends the flattened segments of one quadratic curve to a polygon.
///
/// The start vertex is assumed to be already present. The curve from `start`
/// through `control` to `end` is subdivided into a fixed number of segments.
fn push_quadratic(
    vertices: &mut Vec<Vertex>,
    start: (f32, f32),
    control: (f32, f32),
    end: (f32, f32),
    to_vertex: impl Fn(f32, f32) -> Vertex,
) {
    for step in 1..=QUADRATIC_SEGMENTS {
        let t = step as f32 / QUADRATIC_SEGMENTS as f32;
        let inverse = 1.0 - t;
        let x = inverse * inverse * start.0 + 2.0 * inverse * t * control.0 + t * t * end.0;
        let y = inverse * inverse * start.1 + 2.0 * inverse * t * control.1 + t * t * end.1;
        vertices.push(to_vertex(x, y));
    }
}

/// One edge of a polygon in device-pixel space.
struct Edge {
    top_x: f32,
    top_y: f32,
    bottom_y: f32,
    inverse_slope: f32,
    winding: i32,
}

/// Rasterizes flattened polygons into a coverage mask.
///
/// The bounding box of the polygons defines the mask extent, capped at
/// `MAX_GLYPH_EXTENT`. Each pixel row is sampled at `VERTICAL_SUBSAMPLES` sub-rows;
/// each sub-row's spans are computed with the non-zero winding rule and their
/// analytic horizontal coverage is accumulated, so the result has grayscale
/// anti-aliasing on both axes.
fn rasterize_contours(polygons: &[Vec<Vertex>]) -> Result<GlyphMask, RasterError> {
    let Some(bounds) = Bounds::of(polygons) else {
        return Ok(GlyphMask::blank());
    };

    let left = bounds.min_x.floor();
    let top = bounds.min_y.floor();
    let width = (bounds.max_x.ceil() - left) as i64;
    let height = (bounds.max_y.ceil() - top) as i64;
    if width <= 0 || height <= 0 {
        return Ok(GlyphMask::blank());
    }
    if width > MAX_GLYPH_EXTENT as i64 || height > MAX_GLYPH_EXTENT as i64 {
        return Err(RasterError::GlyphTooLarge);
    }
    let width = width as u32;
    let height = height as u32;

    let edges = build_edges(polygons, left, top);
    let mut coverage = vec![0f32; (width as usize) * (height as usize)];
    let sub_weight = 1.0 / VERTICAL_SUBSAMPLES as f32;

    for row in 0..height {
        for sub in 0..VERTICAL_SUBSAMPLES {
            let sample_y = row as f32 + (sub as f32 + 0.5) / VERTICAL_SUBSAMPLES as f32;
            accumulate_row(&edges, sample_y, row, width, sub_weight, &mut coverage);
        }
    }

    let pixels = coverage
        .into_iter()
        .map(|value| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect();

    Ok(GlyphMask {
        width,
        height,
        left: left as i32,
        top: top as i32,
        coverage: pixels,
    })
}

/// The axis-aligned bounds of a set of polygons.
struct Bounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl Bounds {
    fn of(polygons: &[Vec<Vertex>]) -> Option<Self> {
        let mut bounds: Option<Bounds> = None;
        for polygon in polygons {
            for vertex in polygon {
                bounds = Some(match bounds {
                    None => Bounds {
                        min_x: vertex.x,
                        min_y: vertex.y,
                        max_x: vertex.x,
                        max_y: vertex.y,
                    },
                    Some(current) => Bounds {
                        min_x: current.min_x.min(vertex.x),
                        min_y: current.min_y.min(vertex.y),
                        max_x: current.max_x.max(vertex.x),
                        max_y: current.max_y.max(vertex.y),
                    },
                });
            }
        }
        bounds
    }
}

/// Builds the non-horizontal edges of the polygons in mask-local coordinates.
///
/// Each edge records its top vertex, its vertical span, its inverse slope, and its
/// winding direction (`+1` downward, `-1` upward). Horizontal edges carry no
/// crossing and are dropped.
fn build_edges(polygons: &[Vec<Vertex>], left: f32, top: f32) -> Vec<Edge> {
    let mut edges = Vec::new();
    for polygon in polygons {
        for pair in 0..polygon.len() {
            let start = polygon[pair];
            let end = polygon[(pair + 1) % polygon.len()];
            let (top_vertex, bottom_vertex, winding) = if start.y < end.y {
                (start, end, 1)
            } else if start.y > end.y {
                (end, start, -1)
            } else {
                continue;
            };
            let height = bottom_vertex.y - top_vertex.y;
            edges.push(Edge {
                top_x: top_vertex.x - left,
                top_y: top_vertex.y - top,
                bottom_y: bottom_vertex.y - top,
                inverse_slope: (bottom_vertex.x - top_vertex.x) / height,
                winding,
            });
        }
    }
    edges
}

/// Accumulates the analytic horizontal coverage of one sub-row into a pixel row.
fn accumulate_row(
    edges: &[Edge],
    sample_y: f32,
    row: u32,
    width: u32,
    weight: f32,
    coverage: &mut [f32],
) {
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    for edge in edges {
        if sample_y < edge.top_y || sample_y >= edge.bottom_y {
            continue;
        }
        let x = edge.top_x + (sample_y - edge.top_y) * edge.inverse_slope;
        crossings.push((x, edge.winding));
    }
    if crossings.len() < 2 {
        return;
    }
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));

    let row_start = (row as usize) * (width as usize);
    let row_pixels = &mut coverage[row_start..row_start + width as usize];
    let mut winding = 0;
    for window in crossings.windows(2) {
        winding += window[0].1;
        if winding == 0 {
            continue;
        }
        add_span(row_pixels, window[0].0, window[1].0, width, weight);
    }
}

/// Adds the horizontal coverage of one inside span to a pixel row.
///
/// The span `[start, end)` is clamped to the row, then each covered pixel column
/// accumulates the fraction of its width the span covers, scaled by the sub-row
/// weight. This gives exact horizontal anti-aliasing at the span edges.
fn add_span(row: &mut [f32], start: f32, end: f32, width: u32, weight: f32) {
    let clamped_start = start.max(0.0);
    let clamped_end = end.min(width as f32);
    if clamped_end <= clamped_start {
        return;
    }

    let first = clamped_start.floor() as u32;
    let last = (clamped_end.ceil() as u32).min(width);
    for column in first..last {
        let pixel_left = column as f32;
        let pixel_right = pixel_left + 1.0;
        let covered = clamped_end.min(pixel_right) - clamped_start.max(pixel_left);
        if covered > 0.0 {
            row[column as usize] += covered * weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> BundledFont {
        BundledFont::load().expect("the bundled font parses")
    }

    fn size() -> LayoutUnit {
        LayoutUnit::from_px(16).expect("in range")
    }

    #[test]
    fn a_letter_rasterizes_to_a_mask_with_nonzero_coverage() {
        let font = font();
        let mask = rasterize_glyph(&font, font.glyph_for('A'), size()).expect("rasterizes");

        assert!(!mask.is_blank());
        assert_eq!(
            mask.coverage().len(),
            (mask.width() * mask.height()) as usize
        );
        assert!(mask.coverage().iter().any(|&value| value > 0));
        assert!(mask.coverage().contains(&255));
    }

    #[test]
    fn rasterization_is_deterministic_across_runs() {
        let font = font();
        let first = rasterize_glyph(&font, font.glyph_for('g'), size()).expect("rasterizes");
        let second = rasterize_glyph(&font, font.glyph_for('g'), size()).expect("rasterizes");

        assert_eq!(first, second);
    }

    #[test]
    fn a_space_rasterizes_to_a_blank_mask() {
        let font = font();
        let mask = rasterize_glyph(&font, font.glyph_for(' '), size()).expect("rasterizes");

        assert!(mask.is_blank());
        assert!(mask.coverage().is_empty());
    }

    #[test]
    fn a_zero_size_fails_closed() {
        let font = font();
        assert_eq!(
            rasterize_glyph(&font, font.glyph_for('A'), LayoutUnit::ZERO),
            Err(RasterError::InvalidSize)
        );
    }

    #[test]
    fn an_extreme_size_fails_closed_without_panic() {
        let font = font();
        let huge = LayoutUnit::from_px(4_000).expect("in range");
        assert_eq!(
            rasterize_glyph(&font, font.glyph_for('A'), huge),
            Err(RasterError::GlyphTooLarge)
        );
    }
}
