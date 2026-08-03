// @file engines/purr/text/src/pixel-unit.rs
// @description Defines the neutral fixed-point pixel unit for text geometry.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Fixed-point pixel arithmetic for text geometry.
//!
//! Text geometry uses [`TextUnit`], a fixed-point number over `i32` with a fixed
//! 1/64 px fraction (6 fractional bits). Integer-only math makes the result
//! bit-identical across platforms. There is no floating point.
//!
//! The representation matches the engine layout unit exactly, so a caller can
//! convert between the two without changing any raw value. Overflow never wraps
//! silently: the checked constructors return `None` at the `i32` boundary and the
//! saturating form clamps to it.

// The font scaling path is the only consumer of the sub-pixel ratio
// constructor. In a build without that consumer the method is exercised only by
// the tests below.
#![allow(dead_code)]

/// The number of fractional bits in a [`TextUnit`].
pub const FRACTION_BITS: u32 = 6;

/// The raw value of one whole pixel: 2^6 = 64 sixty-fourths of a pixel.
pub const ONE_PX_RAW: i32 = 1 << FRACTION_BITS;

/// A fixed-point length in 1/64 px.
///
/// The raw `i32` counts sixty-fourths of a pixel. Ordering and equality are the
/// ordering and equality of the raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TextUnit(i32);

impl TextUnit {
    /// The zero length.
    pub const ZERO: Self = Self(0);

    /// A length of exactly `pixels` whole pixels, or `None` when the value does
    /// not fit the fixed-point range.
    pub fn from_px(pixels: i32) -> Option<Self> {
        pixels.checked_mul(ONE_PX_RAW).map(Self)
    }

    /// A length from a raw sixty-fourths-of-a-pixel value.
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// The raw sixty-fourths-of-a-pixel value.
    ///
    /// The raw value serializes exactly: `TextUnit::from_raw(unit.raw())` is
    /// `unit` for every value.
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// The sum, clamped to the `i32` boundary. Never wraps.
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// A length from `numerator / denominator` counted directly in raw
    /// sixty-fourths of a pixel, rounded half away from zero.
    ///
    /// The font scaling path uses this to turn a font-unit value at a given pixel
    /// size into a fixed-point length without an intermediate float. Returns
    /// `None` when the denominator is zero or the value does not fit `i32`.
    pub(crate) fn from_raw_ratio(numerator: i64, denominator: i64) -> Option<Self> {
        let raw = round_div(numerator, denominator)?;
        i32::try_from(raw).ok().map(Self)
    }
}

/// Rounds an integer division half away from zero.
///
/// Returns `None` when the divisor is zero. Integer-only, so the result is
/// identical on every platform.
fn round_div(numerator: i64, denominator: i64) -> Option<i64> {
    if denominator == 0 {
        return None;
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder == 0 {
        return Some(quotient);
    }

    let twice_remainder = remainder.checked_mul(2)?.unsigned_abs();
    if twice_remainder >= denominator.unsigned_abs() {
        let away_from_zero = if (numerator < 0) ^ (denominator < 0) {
            -1
        } else {
            1
        };
        Some(quotient + away_from_zero)
    } else {
        Some(quotient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_has_a_raw_value_of_zero() {
        assert_eq!(TextUnit::ZERO.raw(), 0);
    }

    #[test]
    fn from_px_converts_pixels_to_raw_units() {
        assert_eq!(TextUnit::from_px(10).expect("in range").raw(), 640);
        assert_eq!(TextUnit::from_px(0).expect("in range").raw(), 0);
        assert_eq!(TextUnit::from_px(-5).expect("in range").raw(), -320);
        assert_eq!(TextUnit::from_px(i32::MAX), None);
    }

    #[test]
    fn raw_value_round_trips_without_loss() {
        for raw in [i32::MIN, -1000, 0, 1, 63, 64, 1024, i32::MAX] {
            let unit = TextUnit::from_raw(raw);
            assert_eq!(unit.raw(), raw);
            assert_eq!(TextUnit::from_raw(unit.raw()), unit);
        }
    }

    #[test]
    fn saturating_add_clamps_to_the_boundary() {
        let max = TextUnit::from_raw(i32::MAX);
        assert_eq!(max.saturating_add(TextUnit::from_raw(1)), max);
        let min = TextUnit::from_raw(i32::MIN);
        assert_eq!(min.saturating_add(TextUnit::from_raw(-1)), min);
    }

    #[test]
    fn from_raw_ratio_rounds_half_away_from_zero() {
        // 1/3 px = ONE_PX_RAW / 3 = 64 / 3 rounds to 21.
        let third = TextUnit::from_raw_ratio(ONE_PX_RAW as i64, 3).expect("in range");
        assert_eq!(third.raw(), 21);

        // 2/3 px = 128 / 3 rounds to 43.
        let two_thirds = TextUnit::from_raw_ratio(2 * ONE_PX_RAW as i64, 3).expect("in range");
        assert_eq!(two_thirds.raw(), 43);

        assert_eq!(TextUnit::from_raw_ratio(1, 0), None);
    }
}
