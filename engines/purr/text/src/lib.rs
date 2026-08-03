// @file engines/purr/text/src/lib.rs
// @description Library root for the neutral Purr text primitive.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Neutral Purr text primitive.
//!
//! This crate holds engine-neutral text capabilities (font, shaping, glyph
//! atlas, and raster) shared across Purr consumers. It depends on no engine or
//! product policy and carries no user-facing prose.
//!
//! It owns a neutral fixed-point pixel unit, [`TextUnit`], whose representation
//! matches the engine layout unit exactly so a caller can convert at the
//! boundary without changing any raw value.

#[path = "pixel-unit.rs"]
mod pixel_unit;

#[path = "bundled-font.rs"]
mod bundled_font;

#[path = "glyph-raster.rs"]
mod glyph_raster;

#[path = "text-shaping.rs"]
mod text_shaping;

#[path = "glyph-atlas.rs"]
mod glyph_atlas;

pub use pixel_unit::{FRACTION_BITS, ONE_PX_RAW, TextUnit};

pub use bundled_font::{
    BundledFont, FontError, FontHandle, FontMetrics, FontVisibility, GlyphIndex, GlyphOutline,
    OutlinePoint,
};

pub use glyph_raster::{GlyphMask, MAX_GLYPH_EXTENT, RasterError, rasterize_glyph};

pub use text_shaping::{
    CmapOneToOneAdapter, GlyphRun, GlyphRunGeneration, GlyphRunId, GlyphRunSlice,
    MAX_SHAPED_SCALARS, PositionedGlyph, ShapingError, ShapingRequest, TextShapingAdapter,
};

pub use glyph_atlas::{
    GlyphAtlas, GlyphAtlasError, GlyphKey, GlyphPlacement, TexelRect, build_glyph_atlas,
};
