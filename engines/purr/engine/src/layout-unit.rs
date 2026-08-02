// @file engines/purr/engine/src/layout-unit.rs
// @description Defines the fixed-point LayoutUnit and its geometry helpers for layout.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Fixed-point layout arithmetic.
//!
//! All layout geometry uses [`LayoutUnit`], a fixed-point number over `i32` with
//! a fixed 1/64 px fraction (6 fractional bits). Integer-only math makes the
//! result bit-identical across platforms, which the deterministic render test
//! depends on. There is no floating point in layout geometry.
//!
//! Overflow never wraps silently. Each arithmetic operation is offered in a
//! checked form that returns `None` at the `i32` boundary and a saturating form
//! that clamps to it. The layout phases choose per call site.
//!
//! An indefinite size (a size that layout has not resolved yet) is the distinct
//! [`LayoutSize`] type, never a NaN, an infinity, a maximum integer, or a
//! negative sentinel. A definite arithmetic result on `LayoutUnit` can never be
//! mistaken for an indefinite size, because the two are different types.
//!
//! Rounding to a whole device pixel is round-half-away-from-zero. The rule is
//! fixed and documented so the same input always rounds the same way.

// The layout phases (Phase 09 onward) are the first non-test consumers of the
// arithmetic and geometry helpers. This phase adds the type and exercises it
// through the unit tests below, so several methods are otherwise unused in a
// non-test build.
#![allow(dead_code)]

/// The number of fractional bits in a [`LayoutUnit`].
pub const FRACTION_BITS: u32 = 6;

/// The raw value of one whole pixel: 2^6 = 64 sixty-fourths of a pixel.
pub const ONE_PX_RAW: i32 = 1 << FRACTION_BITS;

/// A fixed-point length in 1/64 px.
///
/// The raw `i32` counts sixty-fourths of a pixel, so `LayoutUnit` represents
/// roughly ±33.5 million pixels at 1/64 px precision. Ordering and equality are
/// the ordering and equality of the raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LayoutUnit(i32);

impl LayoutUnit {
    /// The zero length.
    pub const ZERO: Self = Self(0);

    /// A length of exactly `pixels` whole pixels, or `None` when the value does
    /// not fit the fixed-point range.
    pub fn from_px(pixels: i32) -> Option<Self> {
        pixels.checked_mul(ONE_PX_RAW).map(Self)
    }

    /// A length of `pixels` whole pixels, clamped to the fixed-point range.
    pub fn from_px_saturating(pixels: i32) -> Self {
        Self(pixels.saturating_mul(ONE_PX_RAW))
    }

    /// A length of `numerator / denominator` pixels, rounded to 1/64 px
    /// half-away-from-zero, or `None` when the denominator is zero or the value
    /// does not fit the fixed-point range.
    ///
    /// This is the constructor for sub-pixel values such as a third of a pixel,
    /// which the fixed-point grid represents at 1/64 granularity.
    pub fn from_ratio(numerator: i32, denominator: i32) -> Option<Self> {
        let scaled = (numerator as i64).checked_mul(ONE_PX_RAW as i64)?;
        Self::from_raw_ratio(scaled, denominator as i64)
    }

    /// A length from a raw sixty-fourths-of-a-pixel value.
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// The raw sixty-fourths-of-a-pixel value.
    ///
    /// The raw value serializes exactly: `LayoutUnit::from_raw(unit.raw())` is
    /// `unit` for every value.
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// The nearest whole pixel, rounding half away from zero.
    pub fn round_to_px(self) -> i32 {
        let raw = self.0 as i64;
        let half = (ONE_PX_RAW / 2) as i64;
        let rounded = if raw >= 0 {
            (raw + half) / ONE_PX_RAW as i64
        } else {
            (raw - half) / ONE_PX_RAW as i64
        };
        rounded as i32
    }

    /// The sum, or `None` at the `i32` boundary. Never wraps.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// The difference, or `None` at the `i32` boundary. Never wraps.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// The product with an integer scale, or `None` at the `i32` boundary.
    pub fn checked_mul_int(self, scale: i32) -> Option<Self> {
        self.0.checked_mul(scale).map(Self)
    }

    /// The quotient by an integer divisor, truncated toward zero, or `None` when
    /// the divisor is zero.
    pub fn checked_div_int(self, divisor: i32) -> Option<Self> {
        self.0.checked_div(divisor).map(Self)
    }

    /// The sum, clamped to the `i32` boundary. Never wraps.
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// The difference, clamped to the `i32` boundary. Never wraps.
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// The product with an integer scale, clamped to the `i32` boundary.
    pub fn saturating_mul_int(self, scale: i32) -> Self {
        Self(self.0.saturating_mul(scale))
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

/// A layout size that is either a definite length or explicitly indefinite.
///
/// Indefinite is its own variant, not a sentinel value of `LayoutUnit`. Layout
/// carries an unresolved main or cross size as [`LayoutSize::Indefinite`] and can
/// never confuse it with a definite arithmetic result, because a `LayoutUnit`
/// operation only ever yields a `LayoutUnit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutSize {
    Definite(LayoutUnit),
    Indefinite,
}

impl LayoutSize {
    /// A definite size.
    pub const fn definite(value: LayoutUnit) -> Self {
        Self::Definite(value)
    }

    /// The indefinite size.
    pub const fn indefinite() -> Self {
        Self::Indefinite
    }

    /// Whether the size is indefinite.
    pub fn is_indefinite(self) -> bool {
        matches!(self, Self::Indefinite)
    }

    /// The definite length, or `None` when the size is indefinite.
    pub fn definite_value(self) -> Option<LayoutUnit> {
        match self {
            Self::Definite(value) => Some(value),
            Self::Indefinite => None,
        }
    }
}

/// A point in document-local logical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogicalPoint {
    pub x: LayoutUnit,
    pub y: LayoutUnit,
}

impl LogicalPoint {
    pub const fn new(x: LayoutUnit, y: LayoutUnit) -> Self {
        Self { x, y }
    }
}

/// A width and height in document-local logical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogicalSize {
    pub width: LayoutUnit,
    pub height: LayoutUnit,
}

impl LogicalSize {
    pub const fn new(width: LayoutUnit, height: LayoutUnit) -> Self {
        Self { width, height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sequence_yields_the_same_raw_value() {
        let compute = || {
            let a = LayoutUnit::from_px(10).expect("in range");
            let b = LayoutUnit::from_ratio(1, 3).expect("in range");
            a.checked_add(b)
                .and_then(|v| v.checked_mul_int(3))
                .and_then(|v| v.checked_sub(LayoutUnit::from_px(1).expect("in range")))
                .expect("in range")
                .raw()
        };
        assert_eq!(compute(), compute());
        // 10 px = 640; 1/3 px rounds to 21; (640 + 21) * 3 - 64 = 1919.
        assert_eq!(compute(), 1919);
    }

    #[test]
    fn checked_arithmetic_returns_none_at_the_boundary_and_never_wraps() {
        assert_eq!(LayoutUnit::from_px(i32::MAX), None);
        let max = LayoutUnit::from_raw(i32::MAX);
        assert_eq!(max.checked_add(LayoutUnit::from_raw(1)), None);
        let min = LayoutUnit::from_raw(i32::MIN);
        assert_eq!(min.checked_sub(LayoutUnit::from_raw(1)), None);
        assert_eq!(max.checked_mul_int(2), None);
        assert_eq!(LayoutUnit::from_raw(1).checked_div_int(0), None);
    }

    #[test]
    fn saturating_arithmetic_clamps_to_the_boundary() {
        assert_eq!(
            LayoutUnit::from_px_saturating(i32::MAX),
            LayoutUnit::from_raw(i32::MAX)
        );
        let max = LayoutUnit::from_raw(i32::MAX);
        assert_eq!(max.saturating_add(LayoutUnit::from_raw(1)), max);
        let min = LayoutUnit::from_raw(i32::MIN);
        assert_eq!(min.saturating_sub(LayoutUnit::from_raw(1)), min);
        assert_eq!(max.saturating_mul_int(2), max);
    }

    #[test]
    fn sub_pixel_values_use_one_sixty_fourth_granularity() {
        let third = LayoutUnit::from_ratio(1, 3).expect("in range");
        assert_eq!(third.raw(), 21);
        assert_eq!(third.round_to_px(), 0);

        let two_thirds = LayoutUnit::from_ratio(2, 3).expect("in range");
        assert_eq!(two_thirds.raw(), 43);
        assert_eq!(two_thirds.round_to_px(), 1);

        assert_eq!(LayoutUnit::from_ratio(1, 0), None);
    }

    #[test]
    fn rounding_is_half_away_from_zero_and_symmetric() {
        let half = LayoutUnit::from_raw(ONE_PX_RAW / 2);
        assert_eq!(half.round_to_px(), 1);
        let negative_half = LayoutUnit::from_raw(-ONE_PX_RAW / 2);
        assert_eq!(negative_half.round_to_px(), -1);
    }

    #[test]
    fn fixed_inputs_produce_exact_raw_values() {
        assert_eq!(LayoutUnit::from_px(10).expect("in range").raw(), 640);
        assert_eq!(LayoutUnit::from_px(0).expect("in range").raw(), 0);
        assert_eq!(LayoutUnit::from_px(-5).expect("in range").raw(), -320);
    }

    #[test]
    fn raw_value_round_trips_without_loss() {
        for raw in [i32::MIN, -1000, 0, 1, 63, 64, 1024, i32::MAX] {
            let unit = LayoutUnit::from_raw(raw);
            assert_eq!(unit.raw(), raw);
            assert_eq!(LayoutUnit::from_raw(unit.raw()), unit);
        }
    }

    #[test]
    fn indefinite_size_is_a_distinct_representation() {
        let definite = LayoutSize::definite(LayoutUnit::from_px(20).expect("in range"));
        let indefinite = LayoutSize::indefinite();

        assert!(!definite.is_indefinite());
        assert!(indefinite.is_indefinite());
        assert_eq!(
            definite.definite_value(),
            Some(LayoutUnit::from_px(20).expect("in range"))
        );
        assert_eq!(indefinite.definite_value(), None);

        // A definite arithmetic result is always a definite size; no operation on
        // `LayoutUnit` can produce the indefinite variant.
        let sum = LayoutUnit::from_px(1)
            .and_then(|a| a.checked_add(LayoutUnit::from_px(2)?))
            .expect("in range");
        assert!(!LayoutSize::definite(sum).is_indefinite());
    }
}
