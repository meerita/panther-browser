// @file engines/purr/engine/src/text-unit-conversion.rs
// @description Converts the engine layout unit to and from the neutral text unit at the purr-text boundary.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Raw-preserving conversion between the engine [`LayoutUnit`] and the neutral
//! [`TextUnit`].
//!
//! Both units share the same fixed-point representation (1/64 px in `i32`), so each
//! conversion is a raw round-trip that changes no value. The engine crosses this
//! boundary when it builds a shaping request or a glyph key, and when it reads
//! advances, sizes, and metrics back into layout and paint.

use crate::layout_unit::LayoutUnit;
use purr_text::TextUnit;

pub(crate) fn to_text_unit(unit: LayoutUnit) -> TextUnit {
    TextUnit::from_raw(unit.raw())
}

pub(crate) fn to_layout_unit(unit: TextUnit) -> LayoutUnit {
    LayoutUnit::from_raw(unit.raw())
}
