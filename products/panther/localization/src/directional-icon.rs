// @file products/panther/localization/src/directional-icon.rs
// @description Flags whether an icon mirrors in right-to-left layout.
// @created Diego Martín Lafuente <meerita@icloud.com>

use locale::TextDirection;

/// Whether an icon mirrors when the layout direction is right to left.
///
/// A directional icon points along the reading direction, such as a back arrow
/// or a forward arrow, so it must mirror in right-to-left layout. A neutral icon
/// carries no direction, such as a camera or a lock, so it must never mirror.
/// The flag records the icon's own property; [`IconDirectionality::mirrors`]
/// applies the active direction to decide.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IconDirectionality {
    Directional,
    Neutral,
}

impl IconDirectionality {
    /// Returns whether the icon mirrors for the given direction.
    ///
    /// Only a directional icon in right-to-left layout mirrors. Every other case
    /// keeps the original orientation.
    pub fn mirrors(self, direction: TextDirection) -> bool {
        matches!(self, IconDirectionality::Directional) && direction == TextDirection::RightToLeft
    }
}

#[cfg(test)]
mod tests {
    use locale::TextDirection;

    use super::IconDirectionality;

    #[test]
    fn directional_icon_mirrors_only_in_right_to_left() {
        assert!(IconDirectionality::Directional.mirrors(TextDirection::RightToLeft));
        assert!(!IconDirectionality::Directional.mirrors(TextDirection::LeftToRight));
    }

    #[test]
    fn neutral_icon_never_mirrors() {
        assert!(!IconDirectionality::Neutral.mirrors(TextDirection::RightToLeft));
        assert!(!IconDirectionality::Neutral.mirrors(TextDirection::LeftToRight));
    }
}
