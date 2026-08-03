// @file engines/purr/text/src/placed-run.rs
// @description Combines a shaped glyph run and its atlas into a neutral placed run a consumer paints as textured quads.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Placed glyph run.
//!
//! A shaped [`GlyphRun`] carries glyph indices and advances; a [`GlyphAtlas`]
//! carries where each glyph's pixels live. A consumer that paints text needs both
//! at once. [`PlacedGlyphRun`] pre-combines them into one neutral value: per glyph
//! the atlas source rectangle, the mask bearings, and the pen advance; and for the
//! whole run the total advance, the vertical metrics for baseline placement, and
//! the atlas resource identity. The consumer builds textured quads from this value
//! without depending on the atlas type.
//!
//! A blank glyph (for example a space) has no visible placement. The constructor
//! keeps such a glyph out of the placed list but folds its advance into the
//! previous placed glyph, so the pen still moves and inter-glyph spacing is
//! preserved. The run total advance always equals the shaped run total advance.

// The chrome producer and the shell (later phases) are the first non-test
// consumers of the placed run and its accessors. This phase adds the type and
// exercises it through the unit tests below, so the accessors are otherwise
// unused in a non-test build.
#![allow(dead_code)]

use crate::bundled_font::FontMetrics;
use crate::glyph_atlas::{GlyphAtlas, GlyphKey, TexelRect};
use crate::pixel_unit::TextUnit;
use crate::text_shaping::GlyphRun;
use purr_graphics::GpuResourceIdentity;

/// One glyph placed for painting: where its pixels are, how the mask offsets from
/// the pen, and how far the pen moves to the next placed glyph.
///
/// The advance is a paint advance, not the raw font advance: it includes the
/// advance of any blank glyph the constructor folded into this one, so a consumer
/// that walks the placed glyphs reproduces the shaped spacing.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedGlyph {
    source: TexelRect,
    left: i32,
    top: i32,
    advance: TextUnit,
}

impl PlacedGlyph {
    /// The atlas source rectangle of the glyph mask.
    pub fn source(&self) -> TexelRect {
        self.source
    }

    /// The horizontal offset of the mask from the pen origin, in device pixels.
    pub fn left(&self) -> i32 {
        self.left
    }

    /// The vertical offset of the mask from the baseline, in device pixels.
    pub fn top(&self) -> i32 {
        self.top
    }

    /// The pen advance to the next placed glyph.
    pub fn advance(&self) -> TextUnit {
        self.advance
    }
}

/// A shaped run combined with its atlas: the paint-ready glyphs, the total
/// advance, the vertical metrics, and the atlas identity the glyphs sample.
///
/// The value owns its glyphs and compares by value, so a consumer can track a
/// change to a rendered label without holding the atlas or the shaped run.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedGlyphRun {
    glyphs: Vec<PlacedGlyph>,
    total_advance: TextUnit,
    ascent: TextUnit,
    descent: TextUnit,
    atlas: GpuResourceIdentity,
}

impl PlacedGlyphRun {
    /// Combines a shaped run, the atlas that packs its glyphs, and the font
    /// metrics into a placed run.
    ///
    /// Each glyph is looked up in the atlas by its key at the run size. A glyph
    /// with a visible placement becomes one [`PlacedGlyph`]. A glyph with no
    /// visible placement (a blank glyph such as a space) is skipped, and its
    /// advance is folded into the previous placed glyph so the pen keeps moving.
    /// The total advance is carried from the shaped run, so it counts every glyph.
    pub fn from_shaped_run(run: &GlyphRun, atlas: &GlyphAtlas, metrics: FontMetrics) -> Self {
        let mut glyphs: Vec<PlacedGlyph> = Vec::with_capacity(run.len());
        for positioned in run.glyphs() {
            let advance = positioned.advance();
            let key = GlyphKey::new(positioned.glyph(), run.size());
            let visible = atlas
                .placement(key)
                .filter(|placement| placement.source().width > 0 && placement.source().height > 0);

            let Some(placement) = visible else {
                if let Some(previous) = glyphs.last_mut() {
                    previous.advance = previous.advance.saturating_add(advance);
                }
                continue;
            };

            glyphs.push(PlacedGlyph {
                source: placement.source(),
                left: placement.left(),
                top: placement.top(),
                advance,
            });
        }

        Self {
            glyphs,
            total_advance: run.total_advance(),
            ascent: metrics.ascent(),
            descent: metrics.descent(),
            atlas: atlas.resource(),
        }
    }

    /// The placed glyphs in visual order.
    pub fn glyphs(&self) -> &[PlacedGlyph] {
        &self.glyphs
    }

    /// The total advance of the run, including glyphs with no visible placement.
    pub fn total_advance(&self) -> TextUnit {
        self.total_advance
    }

    /// The ascent above the baseline for baseline placement.
    pub fn ascent(&self) -> TextUnit {
        self.ascent
    }

    /// The descent below the baseline for baseline placement.
    pub fn descent(&self) -> TextUnit {
        self.descent
    }

    /// The identity of the atlas the placed glyphs sample.
    pub fn atlas(&self) -> GpuResourceIdentity {
        self.atlas
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundled_font::BundledFont;
    use crate::glyph_atlas::build_glyph_atlas;
    use crate::text_shaping::{
        CmapOneToOneAdapter, GlyphRunGeneration, GlyphRunId, ShapingRequest, TextShapingAdapter,
    };
    use purr_graphics::{DeviceGeneration, ProducerNamespace, ResourceGeneration, ResourceId};

    const MONOSPACE_ADVANCE: i32 = 538;

    fn font() -> BundledFont {
        BundledFont::load().expect("the bundled font parses")
    }

    fn size() -> TextUnit {
        TextUnit::from_px(16).expect("in range")
    }

    fn shape(font: &BundledFont, text: &str) -> GlyphRun {
        CmapOneToOneAdapter
            .shape(ShapingRequest {
                font,
                text,
                size: size(),
                run_id: GlyphRunId::new(1),
                generation: GlyphRunGeneration::new(1),
            })
            .expect("the Latin run shapes")
    }

    fn atlas_for(font: &BundledFont, run: &GlyphRun) -> GlyphAtlas {
        let keys: Vec<GlyphKey> = run
            .glyphs()
            .iter()
            .map(|glyph| GlyphKey::new(glyph.glyph(), run.size()))
            .collect();
        build_glyph_atlas(
            font,
            &keys,
            ProducerNamespace::new(7),
            ResourceId::new(42),
            ResourceGeneration::new(1),
            DeviceGeneration::new(1),
        )
        .expect("the atlas builds")
    }

    #[test]
    fn each_visible_glyph_pairs_with_its_atlas_placement() {
        let font = font();
        let run = shape(&font, "Ab");
        let atlas = atlas_for(&font, &run);
        let metrics = font.metrics(size()).expect("metrics scale in range");

        let placed = PlacedGlyphRun::from_shaped_run(&run, &atlas, metrics);

        assert_eq!(placed.glyphs().len(), 2);
        for (position, positioned) in run.glyphs().iter().enumerate() {
            let expected = atlas
                .placement(GlyphKey::new(positioned.glyph(), run.size()))
                .expect("the glyph is packed");
            let glyph = &placed.glyphs()[position];
            assert_eq!(glyph.source(), expected.source());
            assert!(glyph.source().width > 0 && glyph.source().height > 0);
            assert_eq!(glyph.left(), expected.left());
            assert_eq!(glyph.top(), expected.top());
            assert_eq!(glyph.advance(), positioned.advance());
        }
    }

    #[test]
    fn the_run_total_advance_equals_the_shaped_run_total_advance() {
        let font = font();
        let run = shape(&font, "Ab");
        let atlas = atlas_for(&font, &run);
        let metrics = font.metrics(size()).expect("metrics scale in range");

        let placed = PlacedGlyphRun::from_shaped_run(&run, &atlas, metrics);

        assert_eq!(placed.total_advance(), run.total_advance());
    }

    #[test]
    fn a_blank_glyph_advances_the_pen_but_adds_no_placement() {
        let font = font();
        let run = shape(&font, "a b");
        let atlas = atlas_for(&font, &run);
        let metrics = font.metrics(size()).expect("metrics scale in range");

        let placed = PlacedGlyphRun::from_shaped_run(&run, &atlas, metrics);

        // The space is not a placed glyph, so only "a" and "b" appear.
        assert_eq!(placed.glyphs().len(), 2);
        // The run total advance still counts the space.
        assert_eq!(placed.total_advance().raw(), 3 * MONOSPACE_ADVANCE);
        // The space advance folds into "a", so the pen reaches "b" at the shaped
        // position of two advances.
        assert_eq!(placed.glyphs()[0].advance().raw(), 2 * MONOSPACE_ADVANCE);
        assert_eq!(placed.glyphs()[1].advance().raw(), MONOSPACE_ADVANCE);
    }

    #[test]
    fn the_run_carries_the_atlas_identity_and_the_font_metrics() {
        let font = font();
        let run = shape(&font, "Ab");
        let atlas = atlas_for(&font, &run);
        let metrics = font.metrics(size()).expect("metrics scale in range");

        let placed = PlacedGlyphRun::from_shaped_run(&run, &atlas, metrics);

        assert_eq!(placed.atlas(), atlas.resource());
        assert_eq!(placed.ascent(), metrics.ascent());
        assert_eq!(placed.descent(), metrics.descent());
    }
}
