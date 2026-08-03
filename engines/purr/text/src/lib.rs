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

pub use pixel_unit::{FRACTION_BITS, ONE_PX_RAW, TextUnit};
