// @file engines/purr/engine/src/text-shaping.rs
// @description Defines the text shaping seam: the TextShapingAdapter trait, a 1:1 cmap adapter, and the immutable GlyphRun.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Text shaping seam.
//!
//! Shaping turns a run of text in one font at one size into an immutable
//! [`GlyphRun`]: an ordered list of glyphs with advances, an explicit cluster map
//! from each glyph back to its source text position, and an id plus generation.
//! Inline layout consumes the advances and the cluster map; a later phase turns
//! the glyph indices into masks.
//!
//! [`TextShapingAdapter`] is the durable seam. A real cross-platform shaper
//! replaces only the adapter implementation; the trait, the `GlyphRun` shape, and
//! the rule that a glyph index is never a text index stay fixed. The M2
//! implementation is [`CmapOneToOneAdapter`], which maps each Unicode scalar to
//! one glyph through the font cmap. It is a swappable leaf, not the contract.
//!
//! Shaping is deterministic: the same text, font, and size always produce the same
//! glyph run, with no per-call variation.

// Inline layout (a later phase) is the first non-test consumer of the glyph run
// and its accessors. This phase adds the seam and exercises it through the unit
// tests below, so several accessors are otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::bundled_font::{BundledFont, FontHandle, GlyphIndex};
use crate::layout_unit::LayoutUnit;

/// Upper bound for the number of scalars one shaping call accepts.
///
/// Shaping is linear in the input, so an unbounded input would be unbounded work.
/// The bound rejects an over-long run rather than shaping it.
pub const MAX_SHAPED_SCALARS: usize = 65_536;

/// Stable identity of one glyph run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphRunId(u32);

impl GlyphRunId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

/// Generation of one glyph run.
///
/// A glyph run is valid only while the identities and generations it references
/// stay valid. The caller advances the generation with the pipeline generation
/// chain, so a stale run is detectable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphRunGeneration(u64);

impl GlyphRunGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// One glyph placed in a run: which glyph, and how far it advances the pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionedGlyph {
    glyph: GlyphIndex,
    advance: LayoutUnit,
}

impl PositionedGlyph {
    pub fn glyph(self) -> GlyphIndex {
        self.glyph
    }

    pub fn advance(self) -> LayoutUnit {
        self.advance
    }
}

/// Failure the shaping adapter reports.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShapingError {
    #[error("the text run exceeds the maximum shaped length")]
    TextTooLong,
    #[error("a glyph advance does not fit the fixed-point range")]
    AdvanceOverflow,
}

/// An immutable shaped run of glyphs.
///
/// A glyph run holds its glyphs in visual order, a parallel cluster map from each
/// glyph position to the byte offset of its source scalar, and its identity. The
/// fields are private and set once at construction: a run is never mutated. The
/// cluster map stores byte offsets (`usize`), a different type from the glyph
/// index, so a glyph index and a text index cannot be interchanged.
pub struct GlyphRun {
    id: GlyphRunId,
    generation: GlyphRunGeneration,
    font: FontHandle,
    size: LayoutUnit,
    glyphs: Vec<PositionedGlyph>,
    cluster_map: Vec<usize>,
}

impl GlyphRun {
    /// The run identity.
    pub fn id(&self) -> GlyphRunId {
        self.id
    }

    /// The run generation.
    pub fn generation(&self) -> GlyphRunGeneration {
        self.generation
    }

    /// The font this run was shaped with.
    pub fn font(&self) -> FontHandle {
        self.font
    }

    /// The pixel size this run was shaped at.
    ///
    /// The size is a shaping input, not a texture coordinate: it records the font
    /// size the advances were measured with, so a later stage can rasterize the
    /// glyphs at the matching size. It is not an atlas position.
    pub fn size(&self) -> LayoutUnit {
        self.size
    }

    /// The number of glyphs.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// Whether the run has no glyphs.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The glyphs in visual order.
    pub fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    /// The glyph at a position, or `None` when the position is out of range.
    pub fn glyph(&self, position: usize) -> Option<PositionedGlyph> {
        self.glyphs.get(position).copied()
    }

    /// The source text byte offset the glyph at `position` came from, or `None`
    /// when the position is out of range.
    pub fn source_index(&self, position: usize) -> Option<usize> {
        self.cluster_map.get(position).copied()
    }

    /// The total advance of the run, clamped to the fixed-point range.
    pub fn total_advance(&self) -> LayoutUnit {
        self.glyphs.iter().fold(LayoutUnit::ZERO, |sum, glyph| {
            sum.saturating_add(glyph.advance)
        })
    }

    /// A slice of the run over the glyph range `[start, end)`, or `None` when the
    /// range is inverted or out of bounds.
    ///
    /// The slice copies the glyphs and the parallel cluster map for the range, so
    /// it keeps the glyph-to-source mapping. Slicing is how inline layout splits a
    /// run at a line break without losing the cluster map.
    pub fn slice(&self, start: usize, end: usize) -> Option<GlyphRunSlice> {
        if start > end || end > self.glyphs.len() {
            return None;
        }
        Some(GlyphRunSlice {
            run_id: self.id,
            generation: self.generation,
            font: self.font,
            size: self.size,
            glyphs: self.glyphs[start..end].to_vec(),
            cluster_map: self.cluster_map[start..end].to_vec(),
        })
    }
}

/// An immutable slice of a shaped glyph run.
///
/// Inline layout slices a run at a line break and keeps the slice in a text
/// fragment. The slice owns the glyphs and the parallel cluster map for its range,
/// and records the source run identity and generation, so it still maps each glyph
/// back to the source text byte offset without borrowing the run. The cluster map
/// stores byte offsets (`usize`), a different type from the glyph index, so a glyph
/// index and a text index cannot be interchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphRunSlice {
    run_id: GlyphRunId,
    generation: GlyphRunGeneration,
    font: FontHandle,
    size: LayoutUnit,
    glyphs: Vec<PositionedGlyph>,
    cluster_map: Vec<usize>,
}

impl GlyphRunSlice {
    /// The identity of the run this slice comes from.
    pub fn run_id(&self) -> GlyphRunId {
        self.run_id
    }

    /// The generation of the run this slice comes from.
    pub fn generation(&self) -> GlyphRunGeneration {
        self.generation
    }

    /// The font the run was shaped with.
    pub fn font(&self) -> FontHandle {
        self.font
    }

    /// The pixel size the run was shaped at.
    ///
    /// Paint rasterizes each glyph at this size; it is a shaping input, never a
    /// texture coordinate.
    pub fn size(&self) -> LayoutUnit {
        self.size
    }

    /// The number of glyphs in the slice.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// Whether the slice has no glyphs.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The glyphs of the slice in visual order.
    pub fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    /// The source text byte offset the glyph at `position` came from, or `None`
    /// when the position is out of range.
    pub fn source_index(&self, position: usize) -> Option<usize> {
        self.cluster_map.get(position).copied()
    }

    /// The total advance of the slice, clamped to the fixed-point range.
    pub fn total_advance(&self) -> LayoutUnit {
        self.glyphs.iter().fold(LayoutUnit::ZERO, |sum, glyph| {
            sum.saturating_add(glyph.advance)
        })
    }
}

/// A request to shape one text run.
pub struct ShapingRequest<'a> {
    pub font: &'a BundledFont,
    pub text: &'a str,
    pub size: LayoutUnit,
    pub run_id: GlyphRunId,
    pub generation: GlyphRunGeneration,
}

/// The shaping seam.
///
/// An implementation turns a text run into a glyph run. The trait is the stable
/// boundary; the implementation is replaceable.
pub trait TextShapingAdapter {
    fn shape(&self, request: ShapingRequest<'_>) -> Result<GlyphRun, ShapingError>;
}

/// The M2 shaping adapter: one glyph per Unicode scalar through the font cmap.
///
/// This adapter handles the M2 page (left-to-right Latin, one font), where
/// itemization, fallback, and bidi collapse to a single 1:1 run. It performs no
/// ligature, no reordering, and no complex-script shaping.
pub struct CmapOneToOneAdapter;

impl TextShapingAdapter for CmapOneToOneAdapter {
    fn shape(&self, request: ShapingRequest<'_>) -> Result<GlyphRun, ShapingError> {
        let scalar_count = request.text.chars().count();
        if scalar_count > MAX_SHAPED_SCALARS {
            return Err(ShapingError::TextTooLong);
        }

        let mut glyphs = Vec::with_capacity(scalar_count);
        let mut cluster_map = Vec::with_capacity(scalar_count);
        for (byte_offset, character) in request.text.char_indices() {
            let glyph = request.font.glyph_for(character);
            let advance = request
                .font
                .advance(glyph, request.size)
                .ok_or(ShapingError::AdvanceOverflow)?;
            glyphs.push(PositionedGlyph { glyph, advance });
            cluster_map.push(byte_offset);
        }

        Ok(GlyphRun {
            id: request.run_id,
            generation: request.generation,
            font: request.font.handle(),
            size: request.size,
            glyphs,
            cluster_map,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(text: &str) -> GlyphRun {
        let font = BundledFont::load().expect("the bundled font parses");
        CmapOneToOneAdapter
            .shape(ShapingRequest {
                font: &font,
                text,
                size: LayoutUnit::from_px(16).expect("in range"),
                run_id: GlyphRunId::new(1),
                generation: GlyphRunGeneration::new(1),
            })
            .expect("the Latin run shapes")
    }

    #[test]
    fn latin_run_yields_one_glyph_per_scalar_with_a_cluster_map() {
        let run = shape("Aa 0");
        assert_eq!(run.len(), 4);

        let glyph_ids: Vec<u16> = run.glyphs().iter().map(|g| g.glyph().value()).collect();
        assert_eq!(glyph_ids, [36, 68, 3, 19]);

        let advances: Vec<i32> = run.glyphs().iter().map(|g| g.advance().raw()).collect();
        assert_eq!(advances, [538, 538, 538, 538]);

        let source: Vec<usize> = (0..run.len())
            .map(|i| run.source_index(i).expect("mapped"))
            .collect();
        assert_eq!(source, [0, 1, 2, 3]);

        // The run records the size it was shaped at, so a later stage rasterizes
        // the glyphs at the matching size.
        assert_eq!(run.size(), LayoutUnit::from_px(16).expect("in range"));
    }

    #[test]
    fn glyph_index_is_not_the_source_text_index() {
        let run = shape("A");
        let positioned = run.glyph(0).expect("one glyph");
        // The glyph index (36) is the font's glyph for 'A'; the source index (0) is
        // the byte offset in the text. They are different values and different
        // types: a glyph index can never be read as a text index.
        assert_eq!(positioned.glyph().value(), 36);
        assert_eq!(run.source_index(0), Some(0));
        assert_ne!(u32::from(positioned.glyph().value()), 'A' as u32);
    }

    #[test]
    fn empty_text_yields_an_empty_run() {
        let run = shape("");
        assert!(run.is_empty());
        assert_eq!(run.total_advance(), LayoutUnit::ZERO);
    }

    #[test]
    fn total_advance_sums_the_glyph_advances() {
        let run = shape("Aa 0");
        assert_eq!(run.total_advance().raw(), 4 * 538);
    }

    #[test]
    fn a_slice_preserves_the_cluster_map_of_its_range() {
        let run = shape("aaa bbb");
        // Slice the second word "bbb", glyph positions 4..7.
        let slice = run.slice(4, 7).expect("in range");
        assert_eq!(slice.len(), 3);
        assert_eq!(slice.source_index(0), Some(4));
        assert_eq!(slice.source_index(2), Some(6));
        assert_eq!(slice.total_advance().raw(), 3 * 538);
        assert_eq!(slice.run_id(), run.id());
        assert_eq!(slice.generation(), run.generation());
        assert_eq!(slice.size(), run.size());
    }

    #[test]
    fn an_out_of_range_slice_returns_none() {
        let run = shape("aa");
        assert_eq!(run.slice(0, 3), None);
        assert_eq!(run.slice(2, 1), None);
    }

    #[test]
    fn the_run_records_the_bundled_font_handle() {
        let font = BundledFont::load().expect("the bundled font parses");
        let run = shape("A");
        assert_eq!(run.font(), font.handle());
    }
}
