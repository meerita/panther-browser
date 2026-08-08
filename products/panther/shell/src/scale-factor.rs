// @file products/panther/shell/src/scale-factor.rs
// @description Defines the display scale factor and the canonical chrome font size.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::{Extent2d, MAX_TEXTURE_EXTENT, Rect};
use purr_text::{ONE_PX_RAW, TextUnit};

/// Canonical chrome font size in logical (density-independent) pixels.
///
/// The shell owns the one chrome font size so the shell and the chrome text
/// producer never diverge. The value is logical: the paint path multiplies it by
/// the active [`ScaleFactor`] to reach the physical size the glyph atlas
/// rasterizes at.
pub const CHROME_FONT_SIZE: TextUnit = TextUnit::from_raw(15 * ONE_PX_RAW);

/// Smallest display scale the shell accepts.
const SCALE_MIN: f32 = 0.25;

/// Largest display scale the shell accepts.
const SCALE_MAX: f32 = 8.0;

/// A validated display scale factor.
///
/// The shell lays out entirely in logical pixels and converts to physical pixels
/// at the paint seam by multiplying through this type. A value of
/// [`ScaleFactor::ONE`] is the identity: every conversion returns its input
/// unchanged, so the logical and physical pixel spaces coincide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleFactor(f32);

impl ScaleFactor {
    /// The identity scale, one physical pixel per logical pixel.
    pub const ONE: ScaleFactor = ScaleFactor(1.0);

    /// A scale factor from the raw `winit` value.
    ///
    /// A non-finite or non-positive input fails safe to [`ScaleFactor::ONE`]. A
    /// valid input clamps to the accepted range and narrows to `f32`. The narrow
    /// is a deliberate, bounded seam conversion: the clamped value is well within
    /// the exactly representable `f32` range.
    pub fn from_winit(scale: f64) -> ScaleFactor {
        if !scale.is_finite() || scale <= 0.0 {
            return ScaleFactor::ONE;
        }

        let clamped = (scale as f32).clamp(SCALE_MIN, SCALE_MAX);
        ScaleFactor(clamped)
    }

    /// The raw scale value.
    pub fn get(self) -> f32 {
        self.0
    }

    /// A logical length converted to physical pixels.
    pub fn scale_length(self, length: f32) -> f32 {
        length * self.0
    }

    /// A logical rectangle converted to physical pixels.
    pub fn scale_rect(self, rect: Rect) -> Rect {
        Rect::new(
            rect.x * self.0,
            rect.y * self.0,
            rect.width * self.0,
            rect.height * self.0,
        )
    }

    /// A physical surface extent converted to a logical extent.
    ///
    /// Each dimension divides by the scale, rounds to the nearest whole pixel, and
    /// clamps to at least one and at most `MAX_TEXTURE_EXTENT`. The `u32` narrow
    /// happens only after the clamp, so it cannot lose range or produce a zero
    /// dimension.
    pub fn to_logical_extent(self, physical: Extent2d) -> Extent2d {
        Extent2d::new(
            logical_dimension(physical.width, self.0),
            logical_dimension(physical.height, self.0),
        )
    }

    /// A logical fixed-point length converted to physical pixels.
    ///
    /// The raw fixed-point value multiplies by the scale in `f64`, rounds half
    /// away from zero, and clamps to the `i32` range before it rebuilds a
    /// [`TextUnit`]. The checked clamp keeps a large size from wrapping.
    pub fn scale_text_unit(self, unit: TextUnit) -> TextUnit {
        let scaled = (f64::from(unit.raw()) * f64::from(self.0)).round();
        let clamped = scaled.clamp(f64::from(i32::MIN), f64::from(i32::MAX));
        TextUnit::from_raw(clamped as i32)
    }
}

/// One physical dimension divided by the scale into a bounded logical dimension.
///
/// The result rounds to the nearest whole pixel and clamps to the valid extent
/// range before the `u32` narrow, so it is never zero and never above the texture
/// bound.
fn logical_dimension(physical: u32, scale: f32) -> u32 {
    let logical = (physical as f32 / scale).round();
    logical.clamp(1.0, MAX_TEXTURE_EXTENT as f32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_winit_keeps_a_valid_scale() {
        assert_eq!(ScaleFactor::from_winit(2.0).get(), 2.0);
        assert_eq!(ScaleFactor::from_winit(1.0), ScaleFactor::ONE);
    }

    #[test]
    fn from_winit_fails_safe_on_invalid_input() {
        assert_eq!(ScaleFactor::from_winit(0.0), ScaleFactor::ONE);
        assert_eq!(ScaleFactor::from_winit(-2.0), ScaleFactor::ONE);
        assert_eq!(ScaleFactor::from_winit(f64::NAN), ScaleFactor::ONE);
        assert_eq!(ScaleFactor::from_winit(f64::INFINITY), ScaleFactor::ONE);
    }

    #[test]
    fn from_winit_clamps_out_of_range_input() {
        assert_eq!(ScaleFactor::from_winit(0.1).get(), SCALE_MIN);
        assert_eq!(ScaleFactor::from_winit(100.0).get(), SCALE_MAX);
    }

    #[test]
    fn scale_rect_multiplies_every_field() {
        let scale = ScaleFactor::from_winit(2.0);
        let scaled = scale.scale_rect(Rect::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(scaled, Rect::new(2.0, 4.0, 6.0, 8.0));
    }

    #[test]
    fn scale_rect_at_one_returns_the_input() {
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(ScaleFactor::ONE.scale_rect(rect), rect);
    }

    #[test]
    fn to_logical_extent_halves_a_two_scale() {
        let scale = ScaleFactor::from_winit(2.0);
        assert_eq!(
            scale.to_logical_extent(Extent2d::new(800, 600)),
            Extent2d::new(400, 300)
        );
    }

    #[test]
    fn to_logical_extent_never_returns_zero() {
        let scale = ScaleFactor::from_winit(8.0);
        assert_eq!(
            scale.to_logical_extent(Extent2d::new(1, 1)),
            Extent2d::new(1, 1)
        );
    }

    #[test]
    fn to_logical_extent_at_one_returns_the_input() {
        let extent = Extent2d::new(1280, 720);
        assert_eq!(ScaleFactor::ONE.to_logical_extent(extent), extent);
    }

    #[test]
    fn scale_text_unit_doubles_at_two() {
        let scale = ScaleFactor::from_winit(2.0);
        assert_eq!(
            scale.scale_text_unit(CHROME_FONT_SIZE).raw(),
            CHROME_FONT_SIZE.raw() * 2
        );
    }

    #[test]
    fn scale_text_unit_at_one_returns_the_input() {
        assert_eq!(
            ScaleFactor::ONE.scale_text_unit(CHROME_FONT_SIZE),
            CHROME_FONT_SIZE
        );
    }

    #[test]
    fn scale_text_unit_rounds_a_fractional_scale() {
        let scale = ScaleFactor::from_winit(1.5);
        // 15 px = 960 raw; 960 * 1.5 = 1440 raw exactly.
        assert_eq!(scale.scale_text_unit(CHROME_FONT_SIZE).raw(), 1440);
        // An odd raw value forces a rounding decision: 1 * 1.5 = 1.5 rounds to 2.
        assert_eq!(scale.scale_text_unit(TextUnit::from_raw(1)).raw(), 2);
    }
}
