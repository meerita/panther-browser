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
    fn the_run_records_the_bundled_font_handle() {
        let font = BundledFont::load().expect("the bundled font parses");
        let run = shape("A");
        assert_eq!(run.font(), font.handle());
    }
}
