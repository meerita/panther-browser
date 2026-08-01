// @file foundation/locale/src/text-direction.rs
// @description Defines the writing direction of a locale.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Writing direction of a locale.
///
/// The direction is a property of the locale script, not of the language name.
/// It drives the logical `start` and `end` layout contract that the product
/// uses instead of physical left and right.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}
